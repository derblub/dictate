// Copyright (C) 2026 Daniel Kurdoghlian
// Pushing Pixels, Vienna <https://pushingpixels.at>
//
// This program is free software; you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation; either version 3 of the License, or (at your option)
// any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY
// or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU General Public License
// for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program; if not, see <https://www.gnu.org/licenses>.
//
// Additional permission under GNU GPL version 3 section 7
//
// If you modify this Program, or any covered work, by linking or combining
// it with NVIDIA CUDA (the libraries of the NVIDIA CUDA Toolkit, such as
// libcudart, libcublas and libcublasLt, and the CUDA driver library libcuda
// of the NVIDIA driver) (or a modified version of that library), containing
// parts covered by the terms of the NVIDIA CUDA Toolkit End User License
// Agreement or the NVIDIA Driver License Agreement, the licensors of this
// Program grant you additional permission to convey the resulting work.

//! Streaming-Logik: Phrasenpuffer, VAD-Gating, Zwischen-Durchläufe, LocalAgreement-2, Festschreiben.
//! Die öffentlichen Signaturen sind der Vertrag mit dem Daemon (T3) und stehen in PLAN.md.
//!
//! Ablauf je `tick` (VAD und Whisper sehen das Audio mit der Verstärkung aus `gain.rs`, gemessen über die Sitzung):
//! 1. VAD inkrementell über die neuen Samples plus Überlappung (nie über die ganze Phrase); die Überlappung ergänzt
//!    erkannte Sprache nur, sie streicht keine.
//! 2. Führende Stille über 1 s abschneiden (300 ms Vorlauf bleiben).
//! 3. Festschreiben, wenn nach Sprache `commit_silence_ms` Stille folgt, bei langen Phrasen schon nach kürzerer
//!    Pause, spätestens bei `max_phrase_ms` (Schnitt an der letzten VAD-Pause).
//!    Der finale Durchlauf sieht nur die Sprachbereiche (Rand `final_pad_ms`, Pausen unter `span_join_ms`
//!    bleiben erhalten, eigene Schnittkanten mit `edge_fade_ms` weich).
//! 4. Sonst bei neuer Sprache ein Zwischen-Durchlauf (Greedy, `single_segment`, verkleinertes `audio_ctx`) und
//!    LocalAgreement-2 auf Wortebene.
//! 5. Ist die Sprache seit `speculative_ms` vorbei, läuft der finale Durchlauf der Phrase vorab. Kommt keine neue
//!    Sprache, verwendet das Festschreiben bzw. `finish` dieses Ergebnis, die Finalisierung kostet dann nur noch VAD.

use crate::config::Config;
use crate::engine::{DecodeOpts, Transcriber};
use crate::filter;
use crate::gain::{self, InputGain};
use std::ops::Range;
use std::time::Instant;

/// Abtastrate (fest, 16 kHz mono)
pub const RATE: usize = 16_000;
/// Samples pro Encoder-Frame: 1500 Frames decken 30 s ab
const SAMPLES_PER_FRAME: usize = 320;
/// Whisper braucht mindestens etwa 1 s Eingabe, kürzere Puffer werden mit Nullen aufgefüllt
const MIN_DECODE_SAMPLES: usize = RATE;

const fn ms(v: u64) -> usize {
    (v as usize) * RATE / 1000
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Partial {
    /// gesamter bisher bestätigter Text der Sitzung
    pub stable: String,
    /// unbestätigter Rest der laufenden Phrase
    pub tail: String,
}

/// Interne Stellschrauben (nicht in der Konfigurationsdatei; Defaults sind ausgemessen, siehe Bericht T2).
#[derive(Debug, Clone, PartialEq)]
pub struct Tuning {
    /// Beam der Zwischen-Durchläufe (1 = Greedy)
    pub partial_beam: usize,
    /// Untergrenze für `audio_ctx` der Zwischen-Durchläufe (Frames, 50 pro Sekunde)
    pub partial_ctx_min: i32,
    /// Rand über der Phrasenlänge für `audio_ctx` (Frames)
    pub partial_ctx_margin: i32,
    /// `audio_ctx` des finalen Durchlaufs als Rand über der Länge (Frames), `None` = voll (1500)
    pub final_ctx_margin: Option<i32>,
    /// Beam des finalen Durchlaufs (Default aus `final_beam_size`)
    pub final_beam: usize,
    /// Überlappung des inkrementellen VAD-Fensters
    pub vad_overlap_ms: u64,
    /// Führende Stille über dieser Länge wird abgeschnitten …
    pub lead_max_ms: u64,
    /// … bis auf diesen Vorlauf
    pub lead_keep_ms: u64,
    /// Rand um die Sprachbereiche im finalen Durchlauf
    pub final_pad_ms: u64,
    /// Kürzere Lücken zwischen zwei gepolsterten Sprachbereichen bleiben im finalen Durchlauf erhalten (kein
    /// Schnitt mitten in eine natürliche Sprechpause)
    pub span_join_ms: u64,
    /// Ein- und Ausblenden an den Schnittkanten der Sprachbereiche im finalen Durchlauf (Hann-Flanke)
    pub edge_fade_ms: u64,
    /// Finalen Durchlauf vorab starten, wenn so lange Stille nach der Sprache herrscht (`None` = aus)
    pub speculative_ms: Option<u64>,
    /// Ab dieser Phrasenlänge reicht schon `soft_silence_ms` Pause zum Festschreiben
    pub soft_phrase_ms: u64,
    pub soft_silence_ms: u64,
    /// Länge des Prompts aus bestätigtem Text (Zeichen)
    pub prompt_chars: usize,
}

impl Tuning {
    pub fn from_config(cfg: &Config) -> Self {
        Tuning {
            partial_beam: 1,
            partial_ctx_min: 384,
            partial_ctx_margin: 64,
            final_ctx_margin: None,
            final_beam: cfg.final_beam_size.max(1),
            vad_overlap_ms: 1000,
            lead_max_ms: 1000,
            lead_keep_ms: 300,
            final_pad_ms: 100,
            edge_fade_ms: 20,
            span_join_ms: 300,
            speculative_ms: Some(150),
            soft_phrase_ms: 8000,
            soft_silence_ms: 300,
            prompt_chars: 200,
        }
    }
}

/// Zähler und Zeiten für Messung und Statistik.
#[derive(Debug, Clone, Default)]
pub struct Stats {
    pub ticks: u64,
    pub vad_calls: u64,
    pub vad_ms: f64,
    pub partial_passes: u64,
    pub partial_ms: f64,
    pub final_passes: u64,
    pub final_ms: f64,
    /// Vorab-Durchläufe insgesamt und davon beim Festschreiben verwendete
    pub speculative_passes: u64,
    pub speculative_hits: u64,
    pub commits: u64,
    /// Zwischen-Hypothesen ohne Anker am bestätigten Text (verworfen)
    pub unaligned: u64,
    /// Bis zu diesem absoluten Sample (seit `reset`) hat der letzte Durchlauf das Audio gesehen
    pub covered: u64,
    /// Samples seit `reset`
    pub pushed: u64,
}

#[derive(Debug, Clone)]
struct Speculative {
    /// Anfang der ersten und Ende der letzten Sprache, über die der Durchlauf lief (Phrasen-Koordinaten)
    first: usize,
    end: usize,
    text: String,
}

pub struct Streamer {
    tun: Tuning,
    blocklist: Vec<String>,
    /// Fester Prompt-Teil (`initial_prompt` + Vokabular-Begriffe)
    initial_prompt: String,
    step: usize,
    commit_silence: usize,
    max_phrase: usize,
    /// Verstärkung leiser Eingänge; gilt für VAD und Whisper, das Rohaudio bleibt unverändert
    gain: InputGain,

    /// Audio seit dem letzten Festschreiben
    phrase: Vec<f32>,
    /// absoluter Index von `phrase[0]`
    phrase_origin: u64,
    /// Sprachbereiche in Phrasen-Koordinaten
    ranges: Vec<Range<usize>>,
    /// so weit ist die Phrase per VAD ausgewertet
    vad_upto: usize,
    /// Samples seit dem letzten `tick`
    since_tick: usize,
    /// so viele Samples nach dem letzten `tick` ist der nächste fällig (normal `step`, kürzer, wenn gerade eine
    /// Pause beginnt und der Vorab-Durchlauf sonst bis zu einem Schritt zu spät käme)
    next_due: usize,

    /// festgeschriebener Text der Sitzung
    committed: String,
    /// bestätigte Wörter der laufenden Phrase (wachsen nur)
    confirmed: Vec<String>,
    /// Wörter der letzten zwei Hypothesen hinter dem bestätigten Teil
    prev_tail: Vec<String>,
    cur_tail: Vec<String>,
    /// so viele Hypothesen in Folge ließen sich nicht am bestätigten Text ausrichten
    unaligned_run: u32,
    /// Phrasenlänge beim letzten Zwischen-Durchlauf
    hyp_covered: usize,
    spec: Option<Speculative>,

    stats: Stats,
    /// Texte dürfen (mit `DICTATE_DEBUG`) ins Debug-Log; aus für die Eingabe der Befehlspalette ([`Streamer::set_log_text`])
    log_text: bool,
}

impl Streamer {
    pub fn new(cfg: &Config) -> Self {
        Self::with_tuning(cfg, Tuning::from_config(cfg))
    }

    pub fn with_tuning(cfg: &Config, tun: Tuning) -> Self {
        Streamer {
            tun,
            blocklist: cfg.blocklist.clone(),
            initial_prompt: crate::pipeline::vocabulary::prompt_base(cfg),
            step: ms(cfg.step_ms).max(ms(20)),
            commit_silence: ms(cfg.commit_silence_ms),
            max_phrase: ms(cfg.max_phrase_ms).max(ms(2000)),
            gain: InputGain::new(cfg.max_gain_db),
            phrase: Vec::new(),
            phrase_origin: 0,
            ranges: Vec::new(),
            vad_upto: 0,
            since_tick: 0,
            next_due: ms(cfg.step_ms).max(ms(20)),
            committed: String::new(),
            confirmed: Vec::new(),
            prev_tail: Vec::new(),
            cur_tail: Vec::new(),
            unaligned_run: 0,
            hyp_covered: 0,
            spec: None,
            stats: Stats::default(),
            log_text: true,
        }
    }

    /// Übernimmt geänderte Einstellungen für die nächsten Durchläufe (Sprache, Modell und Threads gehören
    /// der Engine, nicht dem Streamer). Laufende Phrasen bleiben unberührt.
    /// Texte dieser Sitzung dürfen (mit `DICTATE_DEBUG`) ins Debug-Log; der Daemon schaltet es für die Eingabe der
    /// Befehlspalette ab (W5 Durchsicht N1). Ohne Aufruf: an (Offline-Test `dictate transcribe`).
    pub fn set_log_text(&mut self, on: bool) {
        self.log_text = on;
    }

    pub fn apply_config(&mut self, cfg: &Config) {
        self.blocklist = cfg.blocklist.clone();
        self.initial_prompt = crate::pipeline::vocabulary::prompt_base(cfg);
        self.step = ms(cfg.step_ms).max(ms(20));
        self.commit_silence = ms(cfg.commit_silence_ms);
        self.max_phrase = ms(cfg.max_phrase_ms).max(ms(2000));
        self.tun.final_beam = cfg.final_beam_size.max(1);
        self.gain.set_max_gain_db(cfg.max_gain_db);
    }

    pub fn tuning(&self) -> &Tuning {
        &self.tun
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    /// 16 kHz mono f32
    pub fn push(&mut self, pcm: &[f32]) {
        self.gain.observe(pcm);
        self.phrase.extend_from_slice(pcm);
        self.since_tick += pcm.len();
        self.stats.pushed += pcm.len() as u64;
    }

    /// Lohnt ein Zwischen-Durchlauf jetzt? Wahr, sobald seit dem letzten `tick` mindestens `step_ms` Audio
    /// angekommen ist. Ob es Sprache ist, entscheidet erst `tick` per VAD (das kostet bei Stille nur wenige ms).
    pub fn due(&self) -> bool {
        self.since_tick >= self.next_due.max(1) && !self.phrase.is_empty()
    }

    /// Ein Schritt: VAD, ggf. Festschreiben, ggf. Zwischen-Durchlauf. `Some` nur, wenn sich die Anzeige ändert.
    /// Kann so lange dauern wie ein finaler Durchlauf (Festschreiben bzw. Vorab-Durchlauf).
    pub fn tick(&mut self, t: &mut dyn Transcriber) -> anyhow::Result<Option<Partial>> {
        self.since_tick = 0;
        self.next_due = self.step;
        self.stats.ticks += 1;
        let before = self.partial();
        self.update_vad(t)?;
        self.trim_leading();
        let Some(speech_end) = self.ranges.last().map(|r| r.end) else {
            return Ok(self.changed(before));
        };
        let len = self.phrase.len();
        let silence = len - speech_end;

        // Festschreiben an einer Pause
        let soft = len >= ms(self.tun.soft_phrase_ms) && silence >= ms(self.tun.soft_silence_ms);
        if silence >= self.commit_silence || soft {
            self.commit(t, len)?;
            return Ok(self.changed(before));
        }
        // Festschreiben wegen Überlänge, Schnitt an der letzten Pause
        if len >= self.max_phrase {
            let cut = self.cut_point();
            self.commit(t, cut)?;
            return Ok(self.changed(before));
        }
        // Sprache seit `speculative_ms` vorbei: gleich den finalen Durchlauf vorab (zeigt auch neue Wörter)
        let spec_due = self.tun.speculative_ms.is_some_and(|v| silence >= ms(v));
        if spec_due && !self.spec_valid(len) {
            self.speculative_pass(t)?;
        } else {
            if speech_end > self.hyp_covered {
                self.partial_pass(t, silence)?;
            }
            // Pause hat begonnen: nächsten Tick genau dann, wenn der Vorab-Durchlauf fällig wird
            if let Some(v) = self.tun.speculative_ms.filter(|_| silence > 0 && !spec_due) {
                self.next_due = (ms(v) - silence).clamp(ms(40), self.step);
            }
        }
        Ok(self.changed(before))
    }

    /// Restphrase final festschreiben, Gesamttext zurückgeben. Danach ist die Sitzung leer (Statistik bleibt).
    pub fn finish(&mut self, t: &mut dyn Transcriber) -> anyhow::Result<String> {
        self.update_vad(t)?;
        if !self.ranges.is_empty() {
            let len = self.phrase.len();
            self.commit(t, len)?;
        }
        let text = self.committed.split_whitespace().collect::<Vec<_>>().join(" ");
        let stats = std::mem::take(&mut self.stats);
        self.reset();
        self.stats = stats;
        Ok(text)
    }

    /// Alles verwerfen (Audio, Text, Pegelmessung, Statistik).
    pub fn reset(&mut self) {
        self.phrase.clear();
        self.phrase_origin = 0;
        self.ranges.clear();
        self.vad_upto = 0;
        self.since_tick = 0;
        self.next_due = self.step;
        self.committed.clear();
        self.clear_phrase_state();
        self.gain.reset();
        self.stats = Stats::default();
    }

    /// Wächst die nicht festgeschriebene Phrase weit über `max_phrase_ms` hinaus (Festschreiben scheitert
    /// dauerhaft)? Der Daemon beendet dann die Aufnahme, statt unbegrenzt zu puffern.
    pub fn overrun(&self) -> bool {
        self.phrase.len() > self.max_phrase * 2
    }

    /// Aktuelle Anzeige
    pub fn partial(&self) -> Partial {
        let stable = filter::join(&self.committed, &self.confirmed.join(" "));
        Partial { stable, tail: self.cur_tail.join(" ") }
    }

    fn changed(&self, before: Partial) -> Option<Partial> {
        let now = self.partial();
        (now != before).then_some(now)
    }

    fn clear_phrase_state(&mut self) {
        self.confirmed.clear();
        self.prev_tail.clear();
        self.cur_tail.clear();
        self.unaligned_run = 0;
        self.hyp_covered = 0;
        self.spec = None;
    }

    /// Prompt: der feste Teil (`initial_prompt` + Vokabular-Begriffe, höchstens 200 Zeichen) bleibt vorn, dahinter
    /// das Ende des bisher festgeschriebenen Texts (`prompt_chars`, ab einer Wortgrenze).
    fn prompt(&self) -> String {
        let tail = crate::pipeline::vocabulary::tail_chars(&self.committed, self.tun.prompt_chars);
        filter::join(&self.initial_prompt, &tail)
    }

    /// VAD über die noch nicht ausgewerteten Samples plus Überlappung. Die Überlappung darf Sprache nur
    /// hinzufügen (z. B. einen Anfang, den erst der genauere Verstärkungsfaktor erkennt), nie streichen: beginnt
    /// das Fenster mitten in der Sprache, findet Silero ohne Vorlauf dort oft nichts, auch über eine Sekunde und
    /// länger. Ein Streichen ließe die Phrase dann an einer Scheinpause festschreiben, und der finale Durchlauf sähe
    /// die gestrichenen Wörter nie (t03, 23.10.2026: „Größere Änderungen. Nach Rücksprache …“ ohne „übernehmen wir
    /// erst“, obwohl die Zwischen-Durchläufe sie schon gezeigt hatten). Alte und neue Bereiche werden vereinigt.
    fn update_vad(&mut self, t: &mut dyn Transcriber) -> anyhow::Result<()> {
        let len = self.phrase.len();
        if self.vad_upto >= len {
            return Ok(());
        }
        let start = self.vad_upto.saturating_sub(ms(self.tun.vad_overlap_ms));
        let t0 = Instant::now();
        let fresh = t.speech_ranges(&gain::apply(&self.phrase[start..len], self.gain.factor()))?;
        self.stats.vad_calls += 1;
        self.stats.vad_ms += t0.elapsed().as_secs_f64() * 1000.0;
        let mut all: Vec<Range<usize>> = self.ranges.clone();
        all.extend(fresh.into_iter().map(|r| (r.start + start)..(r.end + start).min(len)).filter(|r| !r.is_empty()));
        all.sort_by_key(|r| r.start);
        let join_gap = ms(100);
        let mut merged: Vec<Range<usize>> = Vec::with_capacity(all.len());
        for r in all {
            match merged.last_mut() {
                Some(last) if r.start <= last.end + join_gap => last.end = last.end.max(r.end),
                _ => merged.push(r),
            }
        }
        self.ranges = merged;
        self.vad_upto = len;
        Ok(())
    }

    /// Phrasenanfang verwerfen
    fn drop_front(&mut self, n: usize) {
        if n == 0 {
            return;
        }
        let n = n.min(self.phrase.len());
        self.phrase.drain(..n);
        self.phrase_origin += n as u64;
        self.ranges = self.ranges.iter().filter(|r| r.end > n).map(|r| r.start.saturating_sub(n)..r.end - n).collect();
        self.vad_upto = self.vad_upto.saturating_sub(n);
        self.hyp_covered = self.hyp_covered.saturating_sub(n);
        if let Some(s) = &mut self.spec {
            // Vorab-Ergebnis bleibt gültig, wenn nur Stille vor der Sprache wegfällt
            if s.first >= n {
                s.first -= n;
                s.end -= n;
            } else {
                self.spec = None;
            }
        }
    }

    fn trim_leading(&mut self) {
        let first = self.ranges.first().map_or(self.phrase.len(), |r| r.start);
        if first > ms(self.tun.lead_max_ms) {
            if self.ranges.is_empty() {
                // keine Sprache: angezeigte Hypothesen sind gegenstandslos
                self.clear_phrase_state();
            }
            self.drop_front(first - ms(self.tun.lead_keep_ms));
        }
    }

    /// Schnittpunkt bei Überlänge: Mitte der letzten Pause zwischen zwei Sprachbereichen, sonst alles.
    fn cut_point(&self) -> usize {
        let min_cut = ms(2000);
        for w in self.ranges.windows(2).rev() {
            let mid = (w[0].end + w[1].start) / 2;
            if mid >= min_cut {
                return mid;
            }
        }
        self.phrase.len()
    }

    /// Eingabe für Whisper: mit der aktuellen Verstärkung, mindestens `MIN_DECODE_SAMPLES` lang
    fn decode_input(&self, pcm: &[f32]) -> Vec<f32> {
        let mut v = gain::apply(pcm, self.gain.factor());
        if v.len() < MIN_DECODE_SAMPLES {
            v.resize(MIN_DECODE_SAMPLES, 0.0);
        }
        v
    }

    fn partial_ctx(&self, samples: usize) -> i32 {
        let frames = samples.div_ceil(SAMPLES_PER_FRAME) as i32 + self.tun.partial_ctx_margin;
        frames.clamp(self.tun.partial_ctx_min, 1500)
    }

    fn partial_pass(&mut self, t: &mut dyn Transcriber, silence: usize) -> anyhow::Result<()> {
        let len = self.phrase.len();
        let pcm = self.decode_input(&self.phrase[..len]);
        let prompt = self.prompt();
        let opts = DecodeOpts { audio_ctx: Some(self.partial_ctx(pcm.len())), beam_size: self.tun.partial_beam, prompt: &prompt, single_segment: true };
        let t0 = Instant::now();
        let segs = t.transcribe(&pcm, &opts)?;
        self.stats.partial_passes += 1;
        self.stats.partial_ms += t0.elapsed().as_secs_f64() * 1000.0;
        self.stats.covered = self.phrase_origin + len as u64;
        let text = filter::segments_to_text(&segs, &self.blocklist);
        // Transkripte nur mit DICTATE_DEBUG, sonst die Wortzahl
        if crate::debug_text() && self.log_text {
            tracing::debug!(len_ms = len / 16, ms = t0.elapsed().as_millis() as u64, "partial pass: {text}");
        } else {
            tracing::debug!(len_ms = len / 16, ms = t0.elapsed().as_millis() as u64, words = text.split_whitespace().count(), "partial pass");
        }
        self.hyp_covered = len;
        // Ist die Sprache vorbei, darf auch das letzte Wort bestätigt werden, sonst kann es abgeschnitten sein
        let speech_over = silence >= ms(200);
        self.agree(text.split_whitespace().map(String::from).collect(), speech_over);
        Ok(())
    }

    /// LocalAgreement-2: längster gemeinsamer Wortpräfix (normalisiert) der letzten zwei Hypothesen.
    /// Jede Hypothese wird zuerst am bestätigten Text ausgerichtet (Zwischen-Durchläufe mit kleinem `audio_ctx`
    /// lassen gelegentlich den Phrasenanfang weg oder erfinden ein Wort davor). Hypothesen ohne Anker werden
    /// verworfen, nach drei in Folge gilt die Position.
    fn agree(&mut self, hyp: Vec<String>, allow_last: bool) {
        let a = match align(&self.confirmed, &hyp) {
            Some(a) => a,
            None if self.unaligned_run >= 2 => self.confirmed.len().min(hyp.len()),
            None => {
                self.unaligned_run += 1;
                self.stats.unaligned += 1;
                return;
            }
        };
        self.unaligned_run = 0;
        let tail = hyp[a..].to_vec();
        self.prev_tail = std::mem::replace(&mut self.cur_tail, tail);
        let mut l = local_agreement(&self.prev_tail, &self.cur_tail);
        if !allow_last && l == self.cur_tail.len() {
            l = l.saturating_sub(1);
        }
        self.confirmed.extend(self.cur_tail.drain(..l));
        self.prev_tail.drain(..l.min(self.prev_tail.len()));
    }

    /// Sprachbereiche in [0, cut) mit Rand, zusammengeführt
    fn speech_spans(&self, cut: usize) -> Vec<Range<usize>> {
        let pad = ms(self.tun.final_pad_ms);
        let join = ms(self.tun.span_join_ms);
        let mut out: Vec<Range<usize>> = Vec::new();
        for r in self.ranges.iter().filter(|r| r.start < cut) {
            let a = r.start.saturating_sub(pad);
            let b = (r.end.min(cut) + pad).min(cut);
            match out.last_mut() {
                Some(last) if a <= last.end + join => last.end = last.end.max(b),
                _ => out.push(a..b),
            }
        }
        out
    }

    /// Anfang der ersten und Ende der letzten Sprache in [0, cut)
    fn speech_bounds(&self, cut: usize) -> Option<(usize, usize)> {
        let first = self.ranges.first().filter(|r| r.start < cut)?.start;
        let end = self.ranges.iter().filter(|r| r.start < cut).map(|r| r.end.min(cut)).max()?;
        Some((first, end))
    }

    /// Gilt das Vorab-Ergebnis noch für [0, cut)? Das VAD-Fenster wertet die Überlappung bei jedem Tick neu aus,
    /// Grenzen wackeln dabei um einige Frames; neue Sprache verschiebt das Ende deutlich.
    fn spec_valid(&self, cut: usize) -> bool {
        let tol = ms(100);
        match (&self.spec, self.speech_bounds(cut)) {
            (Some(s), Some((first, end))) => first.abs_diff(s.first) <= tol && end.abs_diff(s.end) <= tol,
            _ => false,
        }
    }

    /// Finaler Durchlauf über die Sprachbereiche in [0, cut)
    fn final_text(&mut self, t: &mut dyn Transcriber, cut: usize) -> anyhow::Result<String> {
        let mut pcm = Vec::new();
        let fade = ms(self.tun.edge_fade_ms);
        for r in self.speech_spans(cut) {
            // Nur eigene Schnitte weich machen: am Anfang der Aufnahme und am Ende des vorhandenen Audios (Stop)
            // endet das Signal wirklich, ein Ausblenden schnitte dort nur das letzte Phonem an
            let fade_in = if r.start == 0 && self.phrase_origin == 0 { 0 } else { fade };
            let fade_out = if r.end == self.phrase.len() { 0 } else { fade };
            let a = pcm.len();
            pcm.extend_from_slice(&self.phrase[r]);
            fade_edges(&mut pcm[a..], fade_in, fade_out);
        }
        if pcm.is_empty() {
            return Ok(String::new());
        }
        let pcm = self.decode_input(&pcm);
        let prompt = self.prompt();
        let audio_ctx = self.tun.final_ctx_margin.map(|m| (pcm.len().div_ceil(SAMPLES_PER_FRAME) as i32 + m).clamp(self.tun.partial_ctx_min, 1500));
        let opts = DecodeOpts { audio_ctx, beam_size: self.tun.final_beam, prompt: &prompt, single_segment: false };
        let t0 = Instant::now();
        let segs = t.transcribe(&pcm, &opts)?;
        self.stats.final_passes += 1;
        self.stats.final_ms += t0.elapsed().as_secs_f64() * 1000.0;
        let text = filter::segments_to_text(&segs, &self.blocklist);
        // Transkripte nur mit DICTATE_DEBUG=1, sonst die Wortzahl
        let spans_ms = self.speech_spans(cut).iter().map(|r| (r.start / 16, r.end / 16)).collect::<Vec<_>>();
        // Pegel sind keine Inhalte und dürfen immer ins Debug-Log
        let g = self.gain.estimate();
        let level = format!("noise {:.0} dB, speech {}, gain {:.1} dB", g.noise_db, g.speech_db.map_or("-".into(), |d| format!("{d:.0} dB")), g.gain_db);
        if crate::debug_text() && self.log_text {
            tracing::debug!(origin_ms = self.phrase_origin / 16, spans_ms = ?spans_ms, level, ms = t0.elapsed().as_millis() as u64, "final pass: {text}");
        } else {
            tracing::debug!(origin_ms = self.phrase_origin / 16, spans_ms = ?spans_ms, level, ms = t0.elapsed().as_millis() as u64, words = text.split_whitespace().count(), "final pass");
        }
        Ok(text)
    }

    fn speculative_pass(&mut self, t: &mut dyn Transcriber) -> anyhow::Result<()> {
        let len = self.phrase.len();
        let text = self.final_text(t, len)?;
        self.stats.speculative_passes += 1;
        self.stats.covered = self.phrase_origin + len as u64;
        if let Some((first, end)) = self.speech_bounds(len) {
            self.spec = Some(Speculative { first, end, text: text.clone() });
        }
        // Ergebnis wie eine weitere Hypothesen behandeln: was übereinstimmt, wird bestätigt
        self.agree(text.split_whitespace().map(String::from).collect(), true);
        Ok(())
    }

    /// Phrase bis `cut` final transkribieren, an den Gesamttext hängen, Rest behalten.
    fn commit(&mut self, t: &mut dyn Transcriber, cut: usize) -> anyhow::Result<()> {
        let text = if self.spec_valid(cut) {
            self.stats.speculative_hits += 1;
            self.spec.take().map(|s| s.text).unwrap_or_default()
        } else {
            self.final_text(t, cut)?
        };
        self.stats.commits += 1;
        tracing::debug!(origin_ms = self.phrase_origin / 16, cut_ms = cut / 16, "commit");
        self.stats.covered = self.stats.covered.max(self.phrase_origin + cut as u64);
        self.committed = filter::join(&self.committed, &text);
        self.clear_phrase_state();
        self.drop_front(cut);
        Ok(())
    }
}

/// Weiche Kanten (Hann-Flanken über `fade_in` bzw. `fade_out` Samples, je höchstens die halbe Länge) an Anfang
/// und Ende eines ausgeschnittenen Bereichs. Ein harter Schnitt mitten in Sprache oder Rauschen ist ein Sprung im
/// Signal; im Test (leise Brio-Aufnahme, Stop 0–340 ms nach dem Sprachende) kippte daran in 28 von 36 Fällen
/// die Erkennung der ganzen Phrase („Test 1, 2, 3“ → „Test“), mit 20-ms-Flanken in keinem.
pub fn fade_edges(pcm: &mut [f32], fade_in: usize, fade_out: usize) {
    let len = pcm.len();
    let hann = |i: usize, n: usize| 0.5 - 0.5 * (std::f32::consts::PI * (i as f32 + 0.5) / n as f32).cos();
    let n = fade_in.min(len / 2);
    for (i, s) in pcm[..n].iter_mut().enumerate() {
        *s *= hann(i, n);
    }
    let n = fade_out.min(len / 2);
    for (i, s) in pcm[len - n..].iter_mut().rev().enumerate() {
        *s *= hann(i, n);
    }
}

/// Position in `hyp` direkt hinter dem bestätigten Text `conf`: bei gleichem Anfang dessen Länge, sonst das Ende
/// des Vorkommens der letzten (bis zu vier) bestätigten Wörter, das der erwarteten Position am nächsten liegt.
/// Ein Einzelwort-Anker muss mindestens vier Zeichen haben („die“ taugt nicht). `None` = kein Anker.
pub fn align(conf: &[String], hyp: &[String]) -> Option<usize> {
    let c = conf.len();
    if c == 0 || local_agreement(conf, hyp) == c {
        return Some(c.min(hyp.len()));
    }
    let cn: Vec<String> = conf.iter().map(|w| filter::norm_word(w)).collect();
    let hn: Vec<String> = hyp.iter().map(|w| filter::norm_word(w)).collect();
    for m in (1..=c.min(4)).rev() {
        let pat = &cn[c - m..];
        if m == 1 && pat[0].chars().count() < 4 {
            continue;
        }
        let best = (0..hn.len().saturating_sub(m - 1)).filter(|&i| hn[i..i + m] == *pat).map(|i| i + m).min_by_key(|&end| end.abs_diff(c));
        if best.is_some() {
            return best;
        }
    }
    None
}

/// Länge des gemeinsamen Präfixes zweier Wortfolgen (normalisiert verglichen)
pub fn local_agreement(a: &[String], b: &[String]) -> usize {
    a.iter().zip(b).take_while(|(x, y)| filter::norm_word(x) == filter::norm_word(y)).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn agreement_ignores_case_and_punctuation() {
        assert_eq!(local_agreement(&w("Hallo, Welt wie"), &w("hallo Welt. wie geht")), 3);
        assert_eq!(local_agreement(&w("a b c"), &w("a x c")), 1);
        assert_eq!(local_agreement(&w(""), &w("a")), 0);
    }

    #[test]
    fn confirmed_never_shrinks_on_revision() {
        let cfg = Config::default();
        let mut s = Streamer::new(&cfg);
        s.agree(w("eins zwei drei"), false);
        s.agree(w("eins zwei drei vier"), false);
        assert_eq!(s.confirmed, w("eins zwei drei"));
        // Hypothese widerspricht dem Bestätigten: bleibt bestätigt, Ausrichtung am Anker „drei“;
        // „vier“ steht jetzt zweimal hinter dem Anker und wird bestätigt
        s.agree(w("eins zwo drei vier fünf"), false);
        assert_eq!(s.partial().stable, "eins zwei drei vier");
        assert_eq!(s.partial().tail, "fünf");
        // Anfang weggelassen, Ausrichtung am Anker „drei vier“
        s.agree(w("drei vier fünf sechs"), false);
        assert_eq!(s.partial().stable, "eins zwei drei vier fünf");
        assert_eq!(s.partial().tail, "sechs");
        // letztes Wort nur nach Sprachende
        s.agree(w("eins zwei drei vier fünf sechs"), false);
        assert_eq!(s.confirmed.len(), 5);
        s.agree(w("eins zwei drei vier fünf sechs"), true);
        assert_eq!(s.confirmed.len(), 6);
        // ohne Anker verworfen
        s.agree(w("ganz anders"), false);
        assert_eq!(s.partial().tail, "");
        assert_eq!(s.stats.unaligned, 1);
    }

    #[test]
    fn alignment_finds_anchor() {
        assert_eq!(align(&w("a b c"), &w("a b c d")), Some(3));
        assert_eq!(align(&w("das Benziner die"), &w("Belegte sowohl das Benziner die Tief")), Some(5));
        assert_eq!(align(&w("sowohl das Benziner"), &w("benziner, die Tief")), Some(1));
        assert_eq!(align(&w("x als auch"), &w("die Fluorchlor")), None);
        assert_eq!(align(&w("in der"), &w("der Zeit")), None);
        assert_eq!(align(&[], &w("a")), Some(0));
    }

    #[test]
    fn fade_edges_shapes_only_the_requested_edges() {
        let mut v = vec![1.0f32; 100];
        fade_edges(&mut v, 10, 0);
        assert!(v[0] < 0.05 && v[9] > 0.95 && v[10] == 1.0);
        assert!(v[..10].windows(2).all(|w| w[0] < w[1]));
        assert_eq!(v[99], 1.0);
        let mut v = vec![1.0f32; 100];
        fade_edges(&mut v, 0, 10);
        assert_eq!(v[0], 1.0);
        assert!(v[99] < 0.05 && v[90] > 0.95);
        // kurze Bereiche: höchstens die halbe Länge je Flanke
        let mut v = vec![1.0f32; 6];
        fade_edges(&mut v, 10, 10);
        assert!(v.iter().all(|x| *x > 0.0 && *x < 1.0));
        fade_edges(&mut [], 10, 10);
    }

    /// Kurze Sprechpausen bleiben im finalen Durchlauf, lange werden herausgeschnitten.
    #[test]
    fn final_spans_keep_short_pauses() {
        let mut s = Streamer::new(&Config::default());
        s.phrase = vec![0.0; ms(6000)];
        s.ranges = vec![ms(500)..ms(1500), ms(1900)..ms(2500), ms(4000)..ms(5000)];
        // Rand 100 ms: Lücke 1600–1800 (200 ms) bleibt, 2600–3900 (1,3 s) fällt weg
        assert_eq!(s.speech_spans(ms(6000)), vec![ms(400)..ms(2600), ms(3900)..ms(5100)]);
        s.tun.span_join_ms = 0;
        assert_eq!(s.speech_spans(ms(6000)).len(), 3);
    }

    /// VAD und Whisper bekommen leise Sprache verstärkt, `max_gain_db = 0` lässt sie unverändert.
    #[test]
    fn quiet_input_reaches_vad_and_whisper_amplified() {
        struct Peek {
            vad_peak: f32,
            dec_peak: f32,
        }
        impl Transcriber for Peek {
            fn transcribe(&mut self, pcm: &[f32], _: &DecodeOpts) -> anyhow::Result<Vec<crate::engine::Segment>> {
                self.dec_peak = self.dec_peak.max(pcm.iter().fold(0.0, |m, x| m.max(x.abs())));
                Ok(Vec::new())
            }
            fn speech_ranges(&mut self, pcm: &[f32]) -> anyhow::Result<Vec<Range<usize>>> {
                self.vad_peak = self.vad_peak.max(pcm.iter().fold(0.0, |m, x| m.max(x.abs())));
                let half = pcm.len() / 2..pcm.len();
                Ok(std::iter::once(half).collect())
            }
        }
        let run = |max_gain_db: u32| {
            let cfg = Config { max_gain_db, ..Config::default() };
            let mut s = Streamer::new(&cfg);
            let mut t = Peek { vad_peak: 0.0, dec_peak: 0.0 };
            s.push(&vec![0.0002; ms(500)]);
            // −40 dBFS RMS
            s.push(&(0..ms(1000)).map(|i| 0.01 * std::f32::consts::SQRT_2 * (i as f32 * 0.2).sin()).collect::<Vec<_>>());
            s.tick(&mut t).unwrap();
            (t.vad_peak, t.dec_peak)
        };
        let (v0, d0) = run(0);
        assert!((v0 - 0.0141).abs() < 0.001 && (d0 - 0.0141).abs() < 0.001, "{v0} {d0}");
        // Ziel −20 dBFS: Faktor 10
        let (v, d) = run(30);
        assert!((v / v0 - 10.0).abs() < 0.5 && (d / d0 - 10.0).abs() < 0.5, "{v} {d}");
    }

    /// Silero findet ohne Vorlauf mitten in der Sprache oft nichts. Ein VAD-Fenster, das dort beginnt, darf die
    /// schon erkannte Sprache nicht streichen, sonst schreibt der Streamer an einer Scheinpause fest (t03).
    #[test]
    fn vad_overlap_never_erases_detected_speech() {
        /// Sprache = Samples ungleich null; beginnt das Fenster in der Sprache, fehlt die erste Sekunde davon
        struct ColdStart;
        impl Transcriber for ColdStart {
            fn transcribe(&mut self, _: &[f32], _: &DecodeOpts) -> anyhow::Result<Vec<crate::engine::Segment>> {
                Ok(Vec::new())
            }
            fn speech_ranges(&mut self, pcm: &[f32]) -> anyhow::Result<Vec<Range<usize>>> {
                let skip = if pcm.first().is_some_and(|x| *x != 0.0) { ms(1000).min(pcm.len()) } else { 0 };
                let a = pcm[skip..].iter().position(|x| *x != 0.0).map(|a| a + skip);
                let b = pcm.iter().rposition(|x| *x != 0.0);
                Ok(a.zip(b).map(|(a, b)| a..b + 1).into_iter().collect())
            }
        }
        let mut s = Streamer::new(&Config::default());
        let mut t = ColdStart;
        let tone: Vec<f32> = (0..ms(4000)).map(|i| 0.1 * (i as f32 * 0.2).sin().signum()).collect();
        s.push(&vec![0.0; ms(500)]);
        for chunk in tone.chunks(ms(250)) {
            s.push(chunk);
            s.tick(&mut t).unwrap();
            assert_eq!(s.stats.commits, 0, "committed during speech at {} ms", s.phrase.len() / 16);
        }
        assert_eq!(s.ranges, vec![ms(500)..ms(4500)]);
    }

    #[test]
    fn partial_ctx_is_clamped() {
        let s = Streamer::new(&Config::default());
        assert_eq!(s.partial_ctx(RATE), s.tun.partial_ctx_min);
        assert_eq!(s.partial_ctx(RATE * 29), 1500);
        assert_eq!(s.partial_ctx(RATE * 10), 500 + s.tun.partial_ctx_margin);
    }
}
