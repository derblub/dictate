"""Paired bootstrap over clips: difference of pooled WER (b) Parakeet fp32 GPU minus Whisper (positive = Parakeet worse). 10000 resamples, seed 1.
usage: bootstrap.py A.json B.json   (A = Parakeet, B = Whisper)"""
import json, re, sys, unicodedata, random, jiwer
R = "/home/data/stt-test/audio/testset"
def n(s):
    s = unicodedata.normalize("NFKC", s); s = re.sub(r"[-‐-―]", " ", s); s = re.sub(r"[^\w\s]|_", "", s)
    return re.sub(r"\s+", " ", s).strip().casefold()
ids = [f"t{i:02d}" for i in range(1, 19)]
def errs(path):
    rows = {r["clip"]: r["text"] for r in json.load(open(path))["rows"]}
    out = {}
    for c in ids:
        r = n(open(f"{R}/{c}.txt", encoding="utf-8").read()); h = n(rows[c]); k = len(r.split())
        o = jiwer.process_words(r, h) if h else None
        out[c] = ((o.substitutions + o.deletions + o.insertions) if o else k, k)
    return out
a, b = errs(sys.argv[1]), errs(sys.argv[2]); random.seed(1); d = []
for _ in range(10000):
    s = [random.choice(ids) for _ in ids]; w = sum(a[c][1] for c in s)
    d.append((sum(a[c][0] for c in s) - sum(b[c][0] for c in s)) / w)
d.sort(); print("diff mean %.1f pp, 95%% CI [%.1f, %.1f] pp" % (100 * sum(d) / len(d), 100 * d[250], 100 * d[9750]))
