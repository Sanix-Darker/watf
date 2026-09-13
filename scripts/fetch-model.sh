#!/bin/sh
# Explicit online model provisioning. Never called by the Rust engine.
set -eu
umask 077
if [ "${1:-}" = --help ]; then printf '%s\n' 'Usage: fetch-model.sh [absolute-model-directory]'; exit 0; fi
dir=${1:-${WATF_DATA_DIR:-${XDG_DATA_HOME:-${HOME:?}/.local/share}/watf}/models}
case "$dir" in /*) ;; *) printf '%s\n' 'model directory must be absolute' >&2; exit 1;; esac
name=Qwen3-0.6B-Q4_0.gguf
expected=da2572f16c06133561ce56accaa822216f2391ef4d37fba427801cd6736417d4
url=https://huggingface.co/ggml-org/Qwen3-0.6B-GGUF/resolve/b5f37287796e5be0ea3dab2e7430873fb3f73e49/Qwen3-0.6B-Q4_0.gguf
sha() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"
    elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1"
    else printf '%s\n' 'sha256sum or shasum is required' >&2; exit 1; fi
}
mkdir -p "$dir"
if [ -f "$dir/$name" ]; then
    digest=$(sha "$dir/$name"); digest=${digest%% *}
    if [ "$digest" = "$expected" ]; then printf 'Verified existing model: %s\n' "$dir/$name"; exit 0; fi
fi
tmp=$(mktemp "$dir/.model.XXXXXXXX")
trap 'rm -f "$tmp"' EXIT HUP INT TERM
curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fL --retry 3 --connect-timeout 20 --output "$tmp" "$url"
digest=$(sha "$tmp"); digest=${digest%% *}
[ "$digest" = "$expected" ] || { printf '%s\n' 'model SHA256 mismatch' >&2; exit 1; }
mv -f "$tmp" "$dir/$name"
printf 'Set WATF_MODEL to: %s\n' "$dir/$name"
