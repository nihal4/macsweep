#!/bin/bash
# Measures what the menu bar monitor costs while it runs.
#
#   scripts/bench-monitor.sh            # 60 minutes, one sample every 10 s
#   MINUTES=5 scripts/bench-monitor.sh  # a quick check
#
# Turn the monitor on in MacSweep first. Prints a Markdown table you can paste
# into the release notes. Uses only tools that ship with macOS (ps, top).
set -euo pipefail

MINUTES=${MINUTES:-60}
EVERY=10
PID=$(pgrep -x macsweep-monitor || true)
if [[ -z "$PID" ]]; then
    echo "The monitor isn't running. Turn it on in MacSweep → Menu Bar Monitor." >&2
    exit 1
fi

samples=$(( MINUTES * 60 / EVERY ))
echo "Sampling macsweep-monitor (pid $PID) every ${EVERY}s for ${MINUTES} min…" >&2

# ps gives memory; CPU time used is compared at the start and the end, which
# is exact (ps %cpu is a decaying average).
# ps prints "M:SS.hh" or "H:MM:SS.hh".
cpu_time() { ps -o time= -p "$PID" | awk -F'[:.]' '{ if (NF==4) print $1*3600+$2*60+$3+$4/100; else print $1*60+$2+$3/100 }'; }
start_cpu=$(cpu_time)
start=$(date +%s)

# Memory as Activity Monitor shows it (top's MEM, the "footprint").
# ps's RSS is also printed: it counts shared system libraries too, so it's
# always much larger and not what macOS charges to the app.
to_mb() { awk '{ v=$1; u=substr(v,length(v)); n=v+0; if(u=="K")n/=1000; else if(u=="M")n*=1.048576; else if(u=="G")n*=1073.741824; else n/=1e6; printf "%.3f", n }'; }
max_rss=0; sum_rss=0; sum_power=0; n=0; sum_mem=0; max_mem=0
for ((i = 0; i < samples; i++)); do
    sleep "$EVERY"
    rss=$(ps -o rss= -p "$PID" | tr -d ' ')       # KB
    # top's POWER column is the same "Energy Impact" Activity Monitor shows.
    read -r mem power < <(top -l 2 -s 1 -pid "$PID" -stats mem,power | tail -1)
    mem_mb=$(echo "$mem" | to_mb)
    sum_mem=$(echo "$sum_mem + $mem_mb" | bc -l)
    (( $(echo "$mem_mb > $max_mem" | bc -l) )) && max_mem=$mem_mb
    (( rss > max_rss )) && max_rss=$rss
    sum_rss=$(( sum_rss + rss ))
    sum_power=$(echo "$sum_power + ${power:-0}" | bc -l)
    n=$(( n + 1 ))
    printf '\r%d/%d' "$n" "$samples" >&2
done
echo >&2

elapsed=$(( $(date +%s) - start ))
used_cpu=$(echo "$(cpu_time) - $start_cpu" | bc -l)

avg_rss=$(echo "$sum_rss / $n / 1000" | bc -l)
avg_mem=$(echo "$sum_mem / $n" | bc -l)
avg_cpu=$(echo "$used_cpu * 100 / $elapsed" | bc -l)
avg_power=$(echo "$sum_power / $n" | bc -l)

cat <<EOF

| Measure ($(sysctl -n hw.model), macOS $(sw_vers -productVersion), ${MINUTES} min) | Result |
|---|---|
| Memory (Activity Monitor), average | $(printf '%.1f' "$avg_mem") MB |
| Memory (Activity Monitor), highest | $(printf '%.1f' "$max_mem") MB |
| Resident incl. shared system libraries (ps RSS), average | $(printf '%.1f' "$avg_rss") MB |
| CPU, average | $(printf '%.3f' "$avg_cpu") % |
| CPU time used | $(printf '%.2f' "$used_cpu") s in $(( elapsed / 60 )) min |
| Energy Impact, average | $(printf '%.2f' "$avg_power") |
EOF
