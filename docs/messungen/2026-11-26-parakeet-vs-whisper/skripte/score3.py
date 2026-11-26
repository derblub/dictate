"""WER in three variants + latency table for the Parakeet-vs-Whisper measurement.
usage: score3.py OUT.json label=result.json ...   (run with the venv python: `python -I score3.py ...`)
(a) raw: whitespace tokens, nothing normalised (punctuation and case count)
(b) normalised: NFKC, casefold, punctuation removed (hyphen/dash -> space), whitespace collapsed
(c) punctuation removed, case kept
Pooled WER = sum(S+D+I) / sum(reference words) over clips with a reference."""
import json, re, sys, unicodedata
import numpy as np, jiwer

REF = "/home/data/stt-test/audio/testset"
ALL = [f"t{i:02d}" for i in range(1, 19)]
FORMAT = {"t01", "t04", "t10", "t11", "t12"}  # spoken numbers / spoken punctuation commands: reference is the spoken form
# t06 is an English sentence; the Whisper runs force language=de (dictate's setting), large-v3 then translates it into German
SUBS = {"all 18": ALL, "without format clips (13)": [c for c in ALL if c not in FORMAT],
        "German only, without t06 (17)": [c for c in ALL if c != "t06"],
        "German only, without t06 and format clips (12)": [c for c in ALL if c != "t06" and c not in FORMAT]}

def punct(s):
    s = unicodedata.normalize("NFKC", s)
    s = re.sub(r"[-‐-―]", " ", s)
    s = re.sub(r"[^\w\s]|_", "", s)
    return re.sub(r"\s+", " ", s).strip()
VAR = {"a_raw": lambda s: re.sub(r"\s+", " ", unicodedata.normalize("NFKC", s)).strip(),
       "b_norm": lambda s: punct(s).casefold(),
       "c_case": punct}
refs = {c: open(f"{REF}/{c}.txt", encoding="utf-8").read() for c in ALL}

def pooled(rows, ids, f):
    e = n = 0
    for c in ids:
        r = f(refs[c]); h = f(rows[c]["text"]); k = len(r.split())
        if h:
            o = jiwer.process_words(r, h); e += o.substitutions + o.deletions + o.insertions
        else:
            e += k
        n += k
    return e, n

res = {}; out = {}
for a in sys.argv[2:]:
    l, f = a.split("=", 1); res[l] = json.load(open(f))
for l, d in res.items():
    rows = {r["clip"]: r for r in d["rows"]}
    o = {"wer": {}, "lat": {}}
    for sn, ids in SUBS.items():
        for vn, vf in VAR.items():
            e, n = pooled(rows, ids, vf); o["wer"][f"{sn}|{vn}"] = [e, n, e / n]
    ms = np.array([r["ms"] for r in d["rows"]]); au = np.array([r["audio_s"] for r in d["rows"]])
    o["lat"] = {"n": len(ms), "median_ms": float(np.median(ms)), "p90_ms": float(np.percentile(ms, 90)), "mean_ms": float(ms.mean()),
                "rtf_sum": float(ms.sum() / 1000 / au.sum()), "x_realtime": float(au.sum() / (ms.sum() / 1000)),
                "load_ms": d.get("load_ms"), "warmup_ms": d.get("warmup_ms", d.get("warmup_pass_ms"))}
    out[l] = o
json.dump(out, open(sys.argv[1], "w"), indent=1)
print("| candidate | " + " | ".join(f"{s} {v}" for s in SUBS for v in VAR) + " |")
for l, o in out.items():
    print(f"| {l} | " + " | ".join("%.1f %% (%d/%d)" % (o["wer"][f"{s}|{v}"][2] * 100, *o["wer"][f"{s}|{v}"][:2]) for s in SUBS for v in VAR) + " |")
print()
print("| candidate | median ms | p90 ms | mean ms | RTF | x realtime | load s | warm-up |")
for l, o in out.items():
    t = o["lat"]; print(f"| {l} | {t['median_ms']:.0f} | {t['p90_ms']:.0f} | {t['mean_ms']:.0f} | {t['rtf_sum']:.3f} | {t['x_realtime']:.1f} | {(t['load_ms'] or 0)/1000:.2f} | {(t['warmup_ms'] or 0)/1000:.1f} s |")
