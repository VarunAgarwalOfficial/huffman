#!/usr/bin/env bash
# Compares huffman with gzip on files from the Canterbury corpus.
#
# Usage: scripts/bench.sh
# Downloads the corpus once into bench/ (ignored by git), then prints a
# Markdown table: sizes, compressed/original ratios and best-of-five times.
set -euo pipefail

cd "$(dirname "$0")/.."
bin=target/release/huffman
corpus=bench/cantrbry
url=https://corpus.canterbury.ac.nz/resources/cantrbry.tar.gz
files=(alice29.txt lcet10.txt kennedy.xls ptt5 sum)

cargo build --release --quiet
if [ ! -f "$corpus/alice29.txt" ]; then
    mkdir -p "$corpus"
    curl --fail --silent --show-error --location "$url" -o bench/cantrbry.tar.gz
    tar -xzf bench/cantrbry.tar.gz -C "$corpus" --no-same-owner --no-same-permissions
fi
# One file that cannot be compressed: 1 MiB of random bytes.
if [ ! -f bench/random.bin ]; then
    head -c 1048576 /dev/urandom >bench/random.bin
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Best wall time of five runs, in milliseconds.
best_ms() {
    local best=999999999 start end ms
    for _ in 1 2 3 4 5; do
        start=$(date +%s%N)
        "$@" >/dev/null
        end=$(date +%s%N)
        ms=$(((end - start) / 1000000))
        ((ms < best)) && best=$ms
    done
    echo "$best"
}

ratio() { awk -v a="$1" -v b="$2" 'BEGIN { printf "%.3f", a / b }'; }

echo "| File | Bytes | huffman | gzip -6 | gzip -9 | huffman ratio | gzip -6 ratio | huffman enc/dec ms | gzip -6 enc/dec ms |"
echo "|---|---:|---:|---:|---:|---:|---:|---:|---:|"
for name in "${files[@]}" random.bin; do
    path=$corpus/$name
    [ "$name" = random.bin ] && path=bench/random.bin
    size=$(stat -c %s "$path")
    "$bin" encode "$path" "$work/h"
    "$bin" decode "$work/h" "$work/back"
    cmp -s "$path" "$work/back" || { echo "round trip failed for $name" >&2; exit 1; }
    gzip -6 -c "$path" >"$work/g6"
    gzip -9 -c "$path" >"$work/g9"
    h=$(stat -c %s "$work/h")
    g6=$(stat -c %s "$work/g6")
    g9=$(stat -c %s "$work/g9")
    he=$(best_ms "$bin" encode "$path" -)
    hd=$(best_ms "$bin" decode "$work/h" -)
    ge=$(best_ms gzip -6 -c "$path")
    gd=$(best_ms gzip -d -c "$work/g6")
    echo "| $name | $size | $h | $g6 | $g9 | $(ratio "$h" "$size") | $(ratio "$g6" "$size") | $he / $hd | $ge / $gd |"
done
