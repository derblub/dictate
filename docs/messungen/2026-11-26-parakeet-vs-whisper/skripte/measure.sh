#!/usr/bin/env bash
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
# usage: measure.sh OUTPREFIX cmd args...   (run under: flock ~/.cache/blub-heavy.lock nice -n 10 ...)
# Runs the command, samples per-PID VRAM (nvidia-smi compute-apps) and RSS every 0.5 s, records loadavg before/after.
prefix="$1"; shift
echo "loadavg_before: $(cat /proc/loadavg)" > "$prefix.meta"
"$@" > "$prefix.stdout" 2> "$prefix.stderr" &
pid=$!
maxv=0; maxrss=0; maxtotal=0; base=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits | head -1)
while kill -0 $pid 2>/dev/null; do
  v=$(nvidia-smi --query-compute-apps=pid,used_memory --format=csv,noheader,nounits 2>/dev/null | awk -F', ' -v p=$pid '$1==p{s+=$2} END{print s+0}')
  t=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits | head -1)
  r=$(awk '/VmRSS/{print int($2/1024)}' /proc/$pid/status 2>/dev/null || echo 0)
  [ "${v:-0}" -gt "$maxv" ] && maxv=$v
  [ "${t:-0}" -gt "$maxtotal" ] && maxtotal=$t
  [ "${r:-0}" -gt "$maxrss" ] && maxrss=$r
  sleep 0.5
done
wait $pid; rc=$?
{ echo "exit: $rc"; echo "gpu_used_baseline_mib: $base"; echo "gpu_total_used_max_mib: $maxtotal"; echo "pid_vram_max_mib: $maxv"; echo "rss_sampled_max_mib: $maxrss"; echo "loadavg_after: $(cat /proc/loadavg)"; } >> "$prefix.meta"
cat "$prefix.meta"
exit $rc
