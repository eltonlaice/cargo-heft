#!/bin/sh
# Builds well-known Rust projects from scratch (profile from $PROFILE,
# default dev; toolchain from $TOOLCHAIN, default stable)
# and measures each target directory with cargo-heft.
#
#   bench/run.sh <work-dir> <out-dir>
#
# Environment:
#   TOOLCHAIN  toolchain to build with (default stable)
#   PROFILE    cargo profile to build with (default dev)
#   ONLY       space-separated project names; the rest are skipped
#   PIN_DIR    a results directory; each project is checked out at the commit
#              recorded in its .meta file there, to repeat an earlier run
#
# One JSON file per project in <out-dir>, plus <name>.meta with the commit,
# the build time and the toolchain. The target directory is removed after
# measuring, so only one is on disk at a time.
set -u
tc=${TOOLCHAIN:-stable}
profile=${PROFILE:-dev}
work=$1 out=$2
heft=$(cd "$(dirname "$0")/.." && pwd)/target/release/cargo-heft
mkdir -p "$work" "$out"
work=$(cd "$work" && pwd) out=$(cd "$out" && pwd)

while read -r name url; do
    [ -z "$name" ] && continue
    case " ${ONLY:-$name} " in *" $name "*) ;; *) continue ;; esac
    [ -f "$out/$name.json" ] && continue
    src=$work/$name target=$work/target-$name
    pin=
    [ -n "${PIN_DIR:-}" ] && pin=$(sed -n 's/^commit=//p' "$PIN_DIR/$name.meta" 2>/dev/null)
    if [ -d "$src" ]; then
        :
    elif [ -n "$pin" ]; then
        git init -q "$src" && git -C "$src" fetch -q --depth 1 "$url" "$pin" && git -C "$src" checkout -q FETCH_HEAD
    else
        git clone -q --depth 1 "$url" "$src"
    fi || { echo "status=clone-failed" > "$out/$name.meta"; continue; }
    start=$(date +%s)
    if (cd "$src" && CARGO_TARGET_DIR=$target cargo "+$tc" build --profile "$profile" >"$out/$name.log" 2>&1); then
        (cd "$work" && "$heft" --json --target-dir "target-$name") > "$out/$name.json"
        status=ok
    else
        status=build-failed
    fi
    {
        echo "status=$status"
        echo "url=$url"
        echo "commit=$(git -C "$src" rev-parse HEAD)"
        echo "build_seconds=$(($(date +%s) - start))"
        echo "profile=$profile"
        echo "rustc=$(rustc "+$tc" --version)"
        echo "host=$(rustc "+$tc" -vV | sed -n 's/^host: //p')"
    } > "$out/$name.meta"
    rm -rf "$target"
    echo "$name $status"
done <<LIST
ripgrep https://github.com/BurntSushi/ripgrep
fd https://github.com/sharkdp/fd
bat https://github.com/sharkdp/bat
hyperfine https://github.com/sharkdp/hyperfine
starship https://github.com/starship/starship
alacritty https://github.com/alacritty/alacritty
zola https://github.com/getzola/zola
tokio https://github.com/tokio-rs/tokio
axum https://github.com/tokio-rs/axum
clap https://github.com/clap-rs/clap
tantivy https://github.com/quickwit-oss/tantivy
cargo https://github.com/rust-lang/cargo
rust-analyzer https://github.com/rust-lang/rust-analyzer
typst https://github.com/typst/typst
nushell https://github.com/nushell/nushell
ruff https://github.com/astral-sh/ruff
uv https://github.com/astral-sh/uv
wasmtime https://github.com/bytecodealliance/wasmtime
LIST
