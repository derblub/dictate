# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
"""Parakeet TDT 0.6B v3 (ONNX, via onnx-asr) measurement: one load, one full warm-up pass, one measured pass, partial prefixes.
usage: pk_bench.py LABEL OUT.json DEVICE(gpu|cpu) QUANT(fp32|int8) THREADS
"""
import json, os, resource, sys, time, wave
import numpy as np
import onnxruntime as ort

label, out, device, quant, threads = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4], int(sys.argv[5])
MODEL_DIR = "/home/data/stt-test/models/parakeet-tdt-0.6b-v3-onnx"
AUDIO = "/home/data/stt-test/audio"

if device == "gpu":
    ort.preload_dlls(cuda=True, cudnn=True)

import onnx_asr

so = ort.SessionOptions()
so.intra_op_num_threads = threads
so.inter_op_num_threads = 1
if device == "gpu":
    # HEURISTIC instead of the default EXHAUSTIVE conv-algo search: clips have varying lengths = new shapes
    providers = [("CUDAExecutionProvider", {"device_id": 0, "cudnn_conv_algo_search": "HEURISTIC"}), "CPUExecutionProvider"]
else:
    providers = ["CPUExecutionProvider"]

t = time.perf_counter()
model = onnx_asr.load_model("nemo-parakeet-tdt-0.6b-v3", MODEL_DIR, quantization=(None if quant == "fp32" else quant), sess_options=so, providers=providers)
load_ms = (time.perf_counter() - t) * 1000
rss_after_load_mb = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1024


def read(path):
    with wave.open(path) as w:
        assert w.getframerate() == 16000 and w.getnchannels() == 1 and w.getsampwidth() == 2
        return np.frombuffer(w.readframes(w.getnframes()), dtype="<i2").astype(np.float32) / 32768.0


clips = [f"{AUDIO}/testset/t{i:02d}.wav" for i in range(1, 19)] + [f"{AUDIO}/{n}.wav" for n in ("de05", "de10", "de30")]
pcm = {c: read(c) for c in clips}


def rec(a):
    r = model.recognize(a, sample_rate=16000)
    return r if isinstance(r, str) else getattr(r, "text", str(r))


# which providers did the sessions really get?
prov = {}
for name in dir(model):
    pass
try:
    inner = model.asr
    prov = {"encoder": inner._encoder.get_providers(), "decoder_joint": inner._decoder_joint.get_providers()}
except Exception as e:  # attribute names differ between onnx-asr versions
    prov = {"error": repr(e)}

# warm-up: one full pass over the clip set (variable shapes), discarded
t = time.perf_counter()
for c in clips:
    rec(pcm[c])
warm_total_ms = (time.perf_counter() - t) * 1000

rows = []
for c in clips:
    a = pcm[c]
    t = time.perf_counter()
    text = rec(a)
    ms = (time.perf_counter() - t) * 1000
    rows.append({"clip": os.path.splitext(os.path.basename(c))[0], "audio_s": len(a) / 16000, "ms": ms, "text": text})
    print(rows[-1]["clip"], f"{ms:.0f} ms", flush=True)

parts = []
for n in ("de10", "de30", "t18"):
    a = pcm[f"{AUDIO}/{n}.wav"] if n != "t18" else pcm[f"{AUDIO}/testset/t18.wav"]
    for secs in (1, 2, 4, 8):
        if len(a) < secs * 16000:
            continue
        for rep in range(3):
            t = time.perf_counter()
            text = rec(a[: secs * 16000].copy())
            ms = (time.perf_counter() - t) * 1000
            parts.append({"clip": n, "prefix_s": secs, "rep": rep, "ms": ms, "text": text})

res = {"label": label, "device": device, "quant": quant, "threads": threads, "load_ms": load_ms, "warmup_pass_ms": warm_total_ms,
       "ru_maxrss_after_load_mb": rss_after_load_mb, "ru_maxrss_end_mb": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1024,
       "ort": ort.__version__, "onnx_asr": getattr(onnx_asr, "__version__", "0.12.0"), "session_providers": prov, "rows": rows, "partials": parts}
json.dump(res, open(out, "w"), indent=1, ensure_ascii=False)
