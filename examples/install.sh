#!/bin/sh
# install ava mags without ava — plain curl bootstrap.
# usage: ./install.sh linux network healthcheck
# env:   AVA_MAG_REPO (default Shroudmoa/ava), AVA_MAG_BRANCH (default main)
set -eu

if [ "$#" -eq 0 ]; then
    echo "usage: $0 <mag>...   e.g. $0 linux network healthcheck" >&2
    exit 1
fi

repo="${AVA_MAG_REPO:-Shroudmoa/ava}"
branch="${AVA_MAG_BRANCH:-main}"
dir="${XDG_CONFIG_HOME:-$HOME/.config}/ava/sheets"
base="https://raw.githubusercontent.com/$repo/$branch/examples"

mkdir -p "$dir"
for mag in "$@"; do
    curl -fsSL --max-time 30 "$base/$mag.toml" -o "$dir/$mag.toml"
    echo "installed $mag -> $dir/$mag.toml"
done
echo "done. start ava — new commands appear in 999."
