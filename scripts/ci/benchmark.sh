#!/bin/sh
# Measures the time of `shem render` of a PDF with several --jobs, for the
# records of docs/benchmarks/ (docs/adr/0013).
#
#   benchmark.sh <input.pdf> <label>=<shem executable>...
#
# Each executable renders the PDF at 72 dpi to PPM files, with each value of
# JOBS (default "1 2 4"), RUNS times (default 3). The runs of the
# executables and of the values alternate, so that a change of the machine
# during the measurement falls on all of them. The output is a Markdown
# table of the wall-clock times in milliseconds, with their medians, and
# whether the files of every run are the same as the first.
#
# It needs date +%s%N, which GNU date has (the runners of GitHub Actions).
set -eu

[ $# -ge 2 ] || { echo "usage: benchmark.sh <input.pdf> <label>=<shem executable>..." >&2; exit 64; }
pdf=$1
shift
jobs=${JOBS:-1 2 4}
runs=${RUNS:-3}
case $(date +%s%N) in
  *N) echo "benchmark.sh: date +%s%N is not supported" >&2; exit 69 ;;
esac
work=$(mktemp -d "${TMPDIR:-/tmp}/shem-benchmark.XXXXXX")
trap 'rm -rf "$work"' EXIT INT TERM

now() { date +%s%N; }

: > "$work/times"
reference=
run=1
while [ "$run" -le "$runs" ]; do
  for pair in "$@"; do
    label=${pair%%=*}
    exe=${pair#*=}
    for j in $jobs; do
      out="$work/out-$run-$label-$j"
      start=$(now)
      "$exe" render --dpi 72 --format ppm --jobs "$j" "$pdf" "$out"
      end=$(now)
      echo "$label $j $run $(( (end - start) / 1000000 ))" >> "$work/times"
      if [ -z "$reference" ]; then
        reference=$out
      elif diff -r "$reference" "$out" > /dev/null; then
        rm -r "$out"
      else
        echo "benchmark.sh: the files of $label --jobs $j (run $run) differ from the first run" >&2
        exit 1
      fi
    done
  done
  run=$((run + 1))
done

# The table: one row per executable and value, the runs, and the median.
header="| executable | \`--jobs\` |"
rule="|---|---|"
run=1
while [ "$run" -le "$runs" ]; do
  header="$header run $run (ms) |"
  rule="$rule---|"
  run=$((run + 1))
done
echo "$header median (ms) |"
echo "$rule---|"
for pair in "$@"; do
  label=${pair%%=*}
  for j in $jobs; do
    awk -v label="$label" -v j="$j" '
      $1 == label && $2 == j { t[n++] = $4 }
      END {
        row = "| " label " | " j " |"
        for (i = 0; i < n; i++) row = row " " t[i] " |"
        # The median, by sorting the times of the runs.
        for (i = 0; i < n; i++) for (k = i + 1; k < n; k++) if (t[k] < t[i]) { s = t[i]; t[i] = t[k]; t[k] = s }
        m = (n % 2 == 1) ? t[(n - 1) / 2] : int((t[n / 2 - 1] + t[n / 2]) / 2)
        print row " " m " |"
      }' "$work/times"
  done
done
echo
echo "The files of every run were the same."
