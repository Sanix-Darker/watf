#!/bin/sh
set -eu
umask 077
[ "$#" -eq 3 ] || { printf '%s\n' 'Usage: install-local.sh BINARY ABSOLUTE_BIN_DIR ABSOLUTE_DATA_DIR' >&2; exit 2; }
binary=$1; bin=$2; data=$3
[ -x "$binary" ] || { printf 'No built executable: %s\n' "$binary" >&2; exit 1; }
for dir in "$bin" "$data"; do case "$dir" in /*) ;; *) printf '%s\n' 'directories must be absolute' >&2; exit 2;; esac; done
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
mkdir -p "$bin" "$data" "$data/skills/watf" "$data/licenses"
cp "$binary" "$bin/.watf.$$"; chmod 755 "$bin/.watf.$$"; mv -f "$bin/.watf.$$" "$bin/watf"
for file in catalog.jsonl.gz catalog.manifest.json models.json; do cp "$root/data/$file" "$data/.$file.$$"; mv -f "$data/.$file.$$" "$data/$file"; done
cp "$root/skills/watf/SKILL.md" "$data/skills/watf/SKILL.md"
cp "$root/LICENSE" "$root/third_party/botocore-LICENSE.txt" "$data/licenses/"
printf 'Installed %s\nRun: WATF_DATA_DIR="%s" "%s" index\n' "$bin/watf" "$data" "$bin/watf"
printf '%s\n' 'No model downloaded and no shell or agent configuration modified.'
