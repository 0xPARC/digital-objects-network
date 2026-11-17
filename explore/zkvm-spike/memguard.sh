#!/usr/bin/env bash
# Runs a command in its own process group and kills the whole group if its
# summed RSS exceeds MAX_GB, or if the system's MemAvailable drops below
# FLOOR_GB. Prints the group's peak RSS and wall time when the command ends.
#
# usage: memguard.sh MAX_GB [FLOOR_GB] -- command args...
set -uo pipefail

max_gb="$1"; shift
floor_gb=3
if [[ "$1" != "--" ]]; then floor_gb="$1"; shift; fi
[[ "$1" == "--" ]] && shift

max_kb=$(( max_gb * 1024 * 1024 ))
floor_kb=$(( floor_gb * 1024 * 1024 ))

setsid "$@" &
leader=$!
start_ns=$(date +%s%N)
peak_kb=0
killed_reason=""

while kill -0 "$leader" 2>/dev/null; do
  group_kb=$(ps -o rss= -g "$leader" 2>/dev/null | awk '{s+=$1} END {print s+0}')
  (( group_kb > peak_kb )) && peak_kb=$group_kb
  avail_kb=$(awk '/MemAvailable/ {print $2}' /proc/meminfo)
  if (( group_kb > max_kb )); then
    killed_reason="group RSS $(( group_kb / 1024 )) MiB exceeded ${max_gb} GiB"
  elif (( avail_kb < floor_kb )); then
    killed_reason="MemAvailable $(( avail_kb / 1024 )) MiB fell below ${floor_gb} GiB"
  fi
  if [[ -n "$killed_reason" ]]; then
    kill -KILL -- "-$leader" 2>/dev/null
    break
  fi
  sleep 0.25
done

wait "$leader"
status=$?
elapsed_ms=$(( ($(date +%s%N) - start_ns) / 1000000 ))
if [[ -n "$killed_reason" ]]; then
  echo "memguard: KILLED: $killed_reason" >&2
fi
echo "memguard: peak_rss_mib=$(( peak_kb / 1024 )) wall_ms=$elapsed_ms exit=$status" >&2
exit "$status"
