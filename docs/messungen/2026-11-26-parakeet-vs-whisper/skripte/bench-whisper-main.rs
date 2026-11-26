// Measurement harness (outside the dictate repo): runs dictate's own Engine (whisper-rs/whisper.cpp) over the clips.
// Model is loaded once, one warm-up pass (t01), one measured pass over all clips, then partial-pass latency on prefixes.
use anyhow::Result;
use dictate::config::Config;
use dictate::engine::{DecodeOpts, Engine, Transcriber};
use dictate::pipeline::vocabulary::prompt_base;
use dictate::wav::read_16k_mono;
use serde_json::json;
use std::time::Instant;

fn vm_hwm_mb() -> f64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    for l in s.lines() {
        if let Some(r) = l.strip_prefix("VmHWM:") {
            let kb: f64 = r.trim().trim_end_matches("kB").trim().parse().unwrap_or(0.0);
            return kb / 1024.0;
        }
    }
    0.0
}

fn join(segs: &[dictate::engine::Segment]) -> String {
    segs.iter().map(|s| s.text.trim()).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ")
}

fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_env("DICTATE_LOG").unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))).with_writer(std::io::stderr).init();
    let args: Vec<String> = std::env::args().collect();
    let label = &args[1];
    let out = &args[2];
    let lang_auto = args.get(3).map(|s| s == "auto").unwrap_or(false);
    let dir = "/home/data/stt-test/audio";
    let mut cfg = Config::load(None)?;
    if args.get(4).map(|s| s.as_str()) == Some("threads8") {
        cfg.threads = 8;
    }
    let mut clips: Vec<String> = (1..=18).map(|i| format!("{dir}/testset/t{i:02}.wav")).collect();
    for n in ["de05", "de10", "de30"] {
        clips.push(format!("{dir}/{n}.wav"));
    }
    if let Ok(m) = std::env::var("BENCH_MODEL") { cfg.model = m; }
    let prompt = prompt_base(&cfg);
    eprintln!("cfg: model={} language={} threads={} final_beam={} prompt_len={}", cfg.model, cfg.language, cfg.threads, cfg.final_beam_size, prompt.len());
    let t = Instant::now();
    let mut engine = Engine::load(&cfg)?;
    let load_ms = t.elapsed().as_secs_f64() * 1000.0;
    let hwm_after_load = vm_hwm_mb();
    if lang_auto {
        engine.set_session_language(Some("auto"));
    }
    let fin = DecodeOpts { audio_ctx: None, beam_size: cfg.final_beam_size, prompt: &prompt, single_segment: false };
    // warm-up: one pass over t01
    let pcm = read_16k_mono(std::path::Path::new(&clips[0]))?;
    let t = Instant::now();
    let _ = engine.transcribe(&pcm, &fin)?;
    let warm_ms = t.elapsed().as_secs_f64() * 1000.0;
    if args.get(3).map(|s| s.as_str()) == Some("grow") {
        // streaming simulation: de30 re-decoded with a prefix that grows by 250 ms per pass (every input length is new), 1.0 s .. 15 s
        let pcm = read_16k_mono(std::path::Path::new(&clips[20]))?;
        let mut grow = vec![];
        for k in 4..=60usize {
            let len = k * 4000;
            let frames = len.div_ceil(320) as i32 + 64;
            let ctx = frames.clamp(384, 1500);
            let p = DecodeOpts { audio_ctx: Some(ctx), beam_size: 1, prompt: &prompt, single_segment: true };
            let t = Instant::now();
            let segs = engine.transcribe(&pcm[..len], &p)?;
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            grow.push(json!({"prefix_s": len as f64 / 16000.0, "audio_ctx": ctx, "ms": ms, "text": join(&segs)}));
        }
        let res = json!({"label": label, "mode": "grow", "load_ms": load_ms, "warmup_ms": warm_ms, "grow": grow});
        std::fs::write(out, serde_json::to_string_pretty(&res)?)?;
        return Ok(());
    }
    let sel: Vec<String> = if lang_auto { ["t05", "t06", "t07", "t16"].iter().map(|n| format!("{dir}/testset/{n}.wav")).collect() } else { clips.clone() };
    let mut rows = vec![];
    for c in &sel {
        let pcm = read_16k_mono(std::path::Path::new(c))?;
        let t = Instant::now();
        let segs = engine.transcribe(&pcm, &fin)?;
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let name = std::path::Path::new(c).file_stem().unwrap().to_string_lossy().to_string();
        rows.push(json!({"clip": name, "audio_s": pcm.len() as f64 / 16000.0, "ms": ms, "text": join(&segs)}));
        eprintln!("{name}: {:.0} ms", ms);
    }
    // partial passes on prefixes (dictate's partial settings: greedy, single_segment, audio_ctx = clamp(frames+64, 384, 1500))
    let mut parts = vec![];
    if !lang_auto {
        for n in ["de10", "de30", "t18"] {
            let path = if n == "t18" { format!("{dir}/testset/t18.wav") } else { format!("{dir}/{n}.wav") };
            let pcm = read_16k_mono(std::path::Path::new(&path))?;
            for secs in [1usize, 2, 4, 8] {
                let len = secs * 16000;
                if pcm.len() < len {
                    continue;
                }
                let frames = len.div_ceil(320) as i32 + 64;
                let ctx = frames.clamp(384, 1500);
                let p = DecodeOpts { audio_ctx: Some(ctx), beam_size: 1, prompt: &prompt, single_segment: true };
                // 3 repeats of the same prefix to get a stable number (cheap; ~100 ms each)
                for rep in 0..3 {
                    let t = Instant::now();
                    let segs = engine.transcribe(&pcm[..len], &p)?;
                    let ms = t.elapsed().as_secs_f64() * 1000.0;
                    parts.push(json!({"clip": n, "prefix_s": secs, "rep": rep, "audio_ctx": ctx, "ms": ms, "text": join(&segs)}));
                }
            }
        }
    }
    let res = json!({"label": label, "load_ms": load_ms, "warmup_ms": warm_ms, "vmhwm_after_load_mb": hwm_after_load, "vmhwm_end_mb": vm_hwm_mb(), "threads": cfg.threads, "language": if lang_auto {"auto"} else {cfg.language.as_str()}, "beam": cfg.final_beam_size, "prompt": prompt, "rows": rows, "partials": parts});
    std::fs::write(out, serde_json::to_string_pretty(&res)?)?;
    Ok(())
}
