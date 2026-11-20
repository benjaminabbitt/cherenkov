#!/usr/bin/env bash
# Run a command while measuring what a dogfood record needs of it:
#   wall time, max RSS (GNU time), the peak extra disk of the watched paths,
#   and the load average before and after.
#
# usage: scripts/measure-run.sh WATCH COMMAND [ARGS...]
#   WATCH   colon-separated directories or globs whose size is sampled, e.g.
#           'target:/tmp/cargo-mutants-*'. Quote globs so they are expanded
#           at each sample (a cargo-mutants scratch copy appears mid-run).
#           Use '' to watch nothing.
#   env SAMPLE_SECS  sampling interval, default 1
#   env MEASURE_OUT  a file to write the JSON to, default stdout. The
#                    command's own output is not captured.
#
# One JSON object, for a record's "Machine and load" and "Timings" sections:
#   wall_s, max_rss_kib, exit, peak_extra_disk_kib (the highest total of the
#   watched paths during the run minus their total before it), per-path
#   start and peak, the number of samples, load before and after, cores.
# "Extra" is growth during the run, not the paths' absolute size. A path
# shared with another process, such as /tmp, counts that process's writes
# too: watch narrow globs. Disk is sampled, so a peak shorter than the
# interval can be missed; the figure is a lower bound.
# Needs bash, du and awk; max_rss_kib needs GNU time (/usr/bin/time) and is
# null without it.
set -euo pipefail

if [ "$#" -lt 2 ]; then
    sed -n '2,/^# Needs/p' "$0" | sed 's/^# \{0,1\}//' >&2
    exit 2
fi
watch=$1; shift
interval=${SAMPLE_SECS:-1}
gnutime=/usr/bin/time
timer=("$gnutime" -f '%M' -o)
if [ ! -x "$gnutime" ]; then
    echo "measure-run: $gnutime (GNU time) is absent: max_rss_kib will be null" >&2
    timer=()
fi

work=$(mktemp -d)
trap 'rm -rf -- "$work"' EXIT
IFS=: read -r -a paths <<<"$watch"

# KiB used by one watched spec (a glob is expanded now); 0 when nothing matches.
size_kib() {
    local spec=$1 total=0 p k
    for p in $spec; do   # unquoted on purpose: expands a glob
        [ -e "$p" ] || continue
        k=$(du -sk "$p" 2>/dev/null | awk '{print $1}')
        total=$((total + ${k:-0}))
    done
    echo "$total"
}

load() { awk '{print "["$1","$2","$3"]"}' /proc/loadavg; }

declare -a start=() peak=()
start_total=0
for i in "${!paths[@]}"; do
    start[i]=$(size_kib "${paths[i]}")
    peak[i]=${start[i]}
    start_total=$((start_total + start[i]))
done
peak_total=$start_total
samples=0
load_before=$(load)
t0=$(date +%s.%N)

if [ "${#timer[@]}" -gt 0 ]; then "${timer[@]}" "$work/rss" "$@" & else "$@" & fi
pid=$!
while kill -0 "$pid" 2>/dev/null; do
    total=0
    for i in "${!paths[@]}"; do
        s=$(size_kib "${paths[i]}")
        [ "$s" -gt "${peak[i]}" ] && peak[i]=$s
        total=$((total + s))
    done
    [ "$total" -gt "$peak_total" ] && peak_total=$total
    samples=$((samples + 1))
    sleep "$interval"
done
set +e; wait "$pid"; status=$?; set -e
t1=$(date +%s.%N)
load_after=$(load)
rss=$(tail -n1 "$work/rss" 2>/dev/null || true)
case $rss in ''|*[!0-9]*) rss=null ;; esac

per_path=""
for i in "${!paths[@]}"; do
    per_path+="${per_path:+,}{\"path\":\"${paths[i]}\",\"start_kib\":${start[i]},\"peak_kib\":${peak[i]}}"
done
json=$(awk -v t0="$t0" -v t1="$t1" -v rss="$rss" -v st="$status" -v ex=$((peak_total - start_total)) \
    -v n="$samples" -v lb="$load_before" -v la="$load_after" -v cores="$(nproc)" -v per="$per_path" \
    'BEGIN { printf "{\"wall_s\":%.2f,\"max_rss_kib\":%s,\"exit\":%d,\"peak_extra_disk_kib\":%d,\"disk_samples\":%d,\"watched\":[%s],\"load_before\":%s,\"load_after\":%s,\"cores\":%d}\n", t1 - t0, rss, st, ex, n, per, lb, la, cores }')
if [ -n "${MEASURE_OUT:-}" ]; then printf '%s\n' "$json" > "$MEASURE_OUT"; else printf '%s\n' "$json"; fi
exit "$status"
