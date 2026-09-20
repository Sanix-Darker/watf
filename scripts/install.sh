#!/bin/sh
# Online provisioning only. watf itself never downloads or executes other tools.
set -eu
umask 077
fail() { printf 'watf install: %s\n' "$*" >&2; exit 1; }
repo=${WATF_REPO:-Sanix-Darker/watf}
version=${WATF_VERSION:-v0.0.1}
flavor=lite
bin_dir=${WATF_BIN_DIR:-${HOME:?HOME is required}/.local/bin}
data_dir=${WATF_DATA_DIR:-${XDG_DATA_HOME:-$HOME/.local/share}/watf}
with_model=0
build_index=1
archive=
expected=
while [ "$#" -gt 0 ]; do
    case "$1" in
        --repo|--version|--flavor|--bin-dir|--data-dir|--local-archive|--sha256)
            [ "$#" -ge 2 ] || fail "$1 needs a value"
            case "$1" in
                --repo) repo=$2;; --version) version=$2;; --flavor) flavor=$2;;
                --bin-dir) bin_dir=$2;; --data-dir) data_dir=$2;;
                --local-archive) archive=$2;; --sha256) expected=$2;;
            esac
            shift 2;;
        --with-model) with_model=1; shift;;
        --no-model) with_model=0; shift;;
        --no-index) build_index=0; shift;;
        --help|-h)
            printf '%s\n' 'Usage: install.sh [--repo OWNER/REPO] [--version v0.0.1] [--flavor lite]' '  --with-model: download the pinned 429 MB Qwen3 model for a compatible local full archive' '  --no-index: install files without building the local index' '  --local-archive FILE --sha256 HEX: install a predownloaded release' 'Online releases currently publish Linux x86_64 lite only.'
            exit 0;;
        *) fail "unknown option $1";;
    esac
done
case "$flavor" in full|lite) ;; *) fail 'flavor must be full or lite';; esac
[ "$flavor" = full ] || [ "$with_model" = 0 ] || fail 'the lite binary does not include inference'
case "$version" in v[0-9]*) ;; *) fail 'version must start with v and a digit';; esac
case "$version" in *[!A-Za-z0-9._-]*) fail 'invalid version';; esac
for dir in "$bin_dir" "$data_dir"; do case "$dir" in /*) ;; *) fail 'install directories must be absolute';; esac; done
for command in uname mktemp mkdir cp mv chmod tar; do command -v "$command" >/dev/null 2>&1 || fail "missing installer tool: $command"; done
sha() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"
    elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1"
    else fail 'sha256sum or shasum is required by the installer'; fi
}
check() {
    case "$2" in *[!0-9a-f]*|'') fail 'invalid expected SHA256';; esac
    [ "${#2}" -eq 64 ] || fail 'SHA256 must contain 64 hexadecimal characters'
    actual=$(sha "$1"); actual=${actual%% *}
    [ "$actual" = "$2" ] || fail "SHA256 mismatch: $1"
}
fetch() {
    command -v curl >/dev/null 2>&1 || fail 'curl is required for online provisioning'
    curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fL --retry 3 --connect-timeout 20 --output "$2" "$1"
}
case "$(uname -s)" in Linux) ;; *) fail 'prebuilt installer currently supports Linux; build from source on other systems';; esac
case "$(uname -m)" in x86_64|amd64) target=x86_64-unknown-linux-gnu;; aarch64|arm64) target=aarch64-unknown-linux-gnu;; *) fail 'unsupported CPU architecture';; esac
tmp=$(mktemp -d "${TMPDIR:-/tmp}/watf-install.XXXXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
asset="watf-${version}-${target}-${flavor}.tar.gz"
if [ -n "$archive" ]; then
    [ -f "$archive" ] || fail 'local archive does not exist'
    [ -n "$expected" ] || fail '--local-archive requires --sha256'
    cp "$archive" "$tmp/release.tar.gz"
else
    [ "$target" = x86_64-unknown-linux-gnu ] || fail 'online releases currently publish Linux x86_64 only; use --local-archive for other packaged targets'
    [ "$flavor" = lite ] || fail 'online releases currently publish the lite flavor only; use --local-archive for a manually packaged full build'
    case "$repo" in *[!A-Za-z0-9_./-]*|'') fail 'set --repo OWNER/REPO after publishing the project';; esac
    case "$repo" in */*) owner=${repo%%/*}; project=${repo#*/};; *) fail 'repository must be OWNER/REPO';; esac
    case "$owner/$project" in */*/*) fail 'repository must contain exactly one slash';; esac
    [ -n "$owner" ] && [ -n "$project" ] || fail 'empty repository component'
    case "$owner" in .|..) fail 'invalid repository owner';; esac
    case "$project" in .|..) fail 'invalid repository name';; esac
    base="https://github.com/$repo/releases/download/$version"
    fetch "$base/$asset" "$tmp/release.tar.gz"
    fetch "$base/$asset.sha256" "$tmp/checksum"
    IFS=' ' read -r expected ignored < "$tmp/checksum" || fail 'invalid checksum file'
fi
check "$tmp/release.tar.gz" "$expected"
mkdir "$tmp/files"
# Extract only known flat regular-file entries. Never extract arbitrary archive paths.
for file in watf catalog.jsonl.gz catalog.manifest.json SKILL.md LICENSE botocore-LICENSE.txt models.json BUILD-INFO.json Cargo.lock; do
    tar -xOzf "$tmp/release.tar.gz" "$file" > "$tmp/files/$file" || fail "release is missing $file"
done
[ -s "$tmp/files/watf" ] || fail 'empty binary'
mkdir -p "$bin_dir" "$data_dir" "$data_dir/skills/watf" "$data_dir/licenses"
cp "$tmp/files/watf" "$bin_dir/.watf.$$"
chmod 755 "$bin_dir/.watf.$$"
mv -f "$bin_dir/.watf.$$" "$bin_dir/watf"
for file in catalog.jsonl.gz catalog.manifest.json models.json BUILD-INFO.json Cargo.lock; do
    cp "$tmp/files/$file" "$data_dir/.$file.$$"
    mv -f "$data_dir/.$file.$$" "$data_dir/$file"
done
cp "$tmp/files/SKILL.md" "$data_dir/skills/watf/SKILL.md"
cp "$tmp/files/LICENSE" "$tmp/files/botocore-LICENSE.txt" "$data_dir/licenses/"
if [ "$with_model" = 1 ]; then
    model=Qwen3-0.6B-Q4_0.gguf
    model_sha=da2572f16c06133561ce56accaa822216f2391ef4d37fba427801cd6736417d4
    model_url=https://huggingface.co/ggml-org/Qwen3-0.6B-GGUF/resolve/b5f37287796e5be0ea3dab2e7430873fb3f73e49/Qwen3-0.6B-Q4_0.gguf
    fetch "$model_url" "$tmp/$model"
    check "$tmp/$model" "$model_sha"
    mkdir -p "$data_dir/models"
    cp "$tmp/$model" "$data_dir/models/.$model.$$"
    mv -f "$data_dir/models/.$model.$$" "$data_dir/models/$model"
    printf 'Model installed. Set WATF_MODEL to: %s\n' "$data_dir/models/$model"
fi
if [ "$build_index" = 1 ]; then
    WATF_DATA_DIR="$data_dir" "$bin_dir/watf" index --catalog "$data_dir/catalog.jsonl.gz"
fi
printf 'Installed: %s\nData: %s\n' "$bin_dir/watf" "$data_dir"
printf '%s\n' 'Add the binary directory to PATH yourself. No shell configuration was modified.' 'Set WATF_DATA_DIR when using a non-default data directory.'
