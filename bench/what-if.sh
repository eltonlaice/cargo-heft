#!/bin/sh
# Runs `cargo heft --what-if` on projects already cloned by bench/run.sh.
#
#   TOOLCHAIN=1.100.0 bench/what-if.sh <work-dir> <out-dir> <project>...
#
# One JSON file per project in <out-dir>, plus <name>.meta with the commit
# and the toolchain.
set -u
tc=${TOOLCHAIN:-stable}
work=$1 out=$2
shift 2
heft=$(cd "$(dirname "$0")/.." && pwd)/target/release/cargo-heft
mkdir -p "$out"
work=$(cd "$work" && pwd) out=$(cd "$out" && pwd)

for name in "$@"; do
    [ -f "$out/$name.json" ] && continue
    src=$work/$name
    if (cd "$src" && RUSTUP_TOOLCHAIN=$tc "$heft" --what-if --json > "$out/$name.json" 2> "$out/$name.log"); then
        status=ok
    else
        status=failed
        rm -f "$out/$name.json"
    fi
    {
        echo "status=$status"
        echo "commit=$(git -C "$src" rev-parse HEAD)"
        echo "rustc=$(rustc "+$tc" --version)"
        echo "host=$(rustc "+$tc" -vV | sed -n 's/^host: //p')"
    } > "$out/$name.meta"
    echo "$name $status"
done
