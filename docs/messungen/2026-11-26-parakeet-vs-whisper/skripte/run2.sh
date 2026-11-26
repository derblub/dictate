#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# sequential runs under the host lock; results in res2/
cd /home/data/stt-test/parakeet-bench
B=/home/data/stt-test/target-bench/release/bench-whisper
M=/home/data/stt-test/models
R=res2
run() { flock ~/.cache/blub-heavy.lock nice -n 10 ./measure.sh "$@"; }
run $R/whisper-turbo-q5-gpu env BENCH_MODEL=/home/blub/.local/share/dictate/models/ggml-large-v3-turbo-q5_0.bin $B w-turbo-q5-gpu $R/whisper-turbo-q5-gpu.json de
run $R/whisper-turbo-f16-gpu env BENCH_MODEL=$M/whisper/ggml-large-v3-turbo.bin $B w-turbo-f16-gpu $R/whisper-turbo-f16-gpu.json de
run $R/whisper-large-v3-f16-gpu env BENCH_MODEL=$M/whisper/ggml-large-v3.bin $B w-large-v3-f16-gpu $R/whisper-large-v3-f16-gpu.json de
PY=/home/data/stt-test/parakeet-venv/bin/python
run $R/pk-gpu-fp32 $PY -I pk_bench.py pk-gpu-fp32 $R/pk-gpu-fp32.json gpu fp32 4
run $R/pk-cpu-fp32 $PY -I pk_bench.py pk-cpu-fp32 $R/pk-cpu-fp32.json cpu fp32 4
run $R/pk-cpu-int8 $PY -I pk_bench.py pk-cpu-int8 $R/pk-cpu-int8.json cpu int8 4
echo ALLDONE > $R/done
