#!/usr/bin/env bash
# The time, the peak RSS and the peak heap of the tile cut, for each input and zoom (docs/design.md, section 4).
#   tools/measure.sh OUT_DIR [--memory SIZE] IN.osm.pbf:ZOOM[,ZOOM...] ...
# A zoom `rN` is the cut into H3 cells of resolution N (`pbfcut h3 --res N`).
# Two runs for each zoom: the time and the peak RSS from a run without --steps (the count of the heap slows the threads),
# the peak heap from a run with --steps. The tile files go into OUT_DIR/run and are removed after each run. TSV output:
#   input, zoom, memory, tiles, files, seconds, peak RSS (MiB), peak heap (MiB).
set -euo pipefail
out=$1; shift
memory=()
if [ "${1:-}" = --memory ]; then memory=(--memory "$2"); shift 2; fi
bin=target/release/pbfcut
rss() {  # the peak RSS in MiB of a command, from /usr/bin/time
  if [ "$(uname)" = Darwin ]; then
    /usr/bin/time -l "$@" 2> "$out/time.txt" > "$out/report.json"
    echo $(( $(grep 'maximum resident set size' "$out/time.txt" | awk '{print $1}') / 1048576 ))
  else
    /usr/bin/time -v "$@" 2> "$out/time.txt" > "$out/report.json"
    echo $(( $(grep 'Maximum resident set size' "$out/time.txt" | awk '{print $NF}') / 1024 ))
  fi
}
printf 'input\tzoom\tmemory\ttiles\tfiles\tseconds\tpeak_rss_mib\tpeak_heap_mib\n'
for spec in "$@"; do
  pbf=${spec%%:*}
  for z in $(echo "${spec#*:}" | tr ',' ' '); do
    rm -rf "$out/run"
    case $z in r*) cut=(h3 "$pbf" --res "${z#r}") ;; *) cut=(tiles "$pbf" --zoom "$z") ;; esac
    m=$(rss "$bin" "${cut[@]}" ${memory[@]+"${memory[@]}"} -o "$out/run")
    cp "$out/report.json" "$out/plain.json"
    rm -rf "$out/run"
    "$bin" "${cut[@]}" ${memory[@]+"${memory[@]}"} --steps -o "$out/run" > "$out/report.json" 2> "$out/steps-$(basename "$pbf" .osm.pbf)-z$z.txt"
    python3 - "$out/plain.json" "$out/report.json" "$pbf" "$z" "$m" "${memory[1]:-default}" <<'PY'
import json, sys
plain, steps = json.load(open(sys.argv[1])), json.load(open(sys.argv[2]))
print("\t".join(map(str, [sys.argv[3].split("/")[-1], sys.argv[4], sys.argv[6], plain.get("tiles", plain.get("cells")), len(plain["files"]),
                          round(plain["seconds"], 1), sys.argv[5], round(steps["peak_heap_mib"])])))
PY
  done
done
rm -rf "$out/run"
