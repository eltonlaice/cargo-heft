# cargo heft disk

`cargo heft` measures one target directory. `cargo heft disk` measures what
Rust keeps outside it: the Cargo home and the rustup home. It reads only.
Cleaning comes later, as a separate subcommand, and only if users ask for it
(see #45).

## Where it looks

- The Cargo home: `CARGO_HOME`, or `~/.cargo` when it is not set.
- The rustup home: `RUSTUP_HOME`, or `~/.rustup` when it is not set.

A home that does not exist is reported as not found, not as an error. Sizes are
counted the same way as for a target directory: blocks on disk, every inode
once, so each total matches `du -s`. Symlinks are counted, not followed. A
directory that cannot be read is named on stderr and left out of the totals;
the rest is still measured.

## What each part is, and what removing it costs

| Part | What it is | If removed | Removed by |
|---|---|---|---|
| `registry/index` | the crates.io index Cargo has fetched | downloaded again | `cargo clean gc` |
| `registry/cache` | the downloaded `.crate` files | downloaded again; offline builds fail | `cargo clean gc` |
| `registry/src` | those files unpacked | unpacked again from `registry/cache`, no network | `cargo clean gc` |
| `git/db` | clones of git dependencies | cloned again | `cargo clean gc` |
| `git/checkouts` | checkouts of those clones | checked out again from `git/db`, no network | `cargo clean gc` |
| `bin` | tools installed with `cargo install` | the tools | never recommended |
| `toolchains/<name>` | one rustup toolchain | downloaded again by rustup | `rustup toolchain uninstall <name>` |
| a toolchain in use | the default, a `rustup override`, `RUSTUP_TOOLCHAIN`, or the `rust-toolchain.toml` / `rust-toolchain` of the current directory or one above it | builds that use it | never recommended |
| other | settings, credentials, lock files, anything not listed above | unknown | never recommended |

The report marks each part `rebuilt` (no network needed), `download` (the next
build fetches it again) or `keep`.

`cargo clean gc` is nightly-only for now: `cargo +nightly -Zgc clean gc`. Since
Rust 1.88, Cargo also removes cache files it has not used for a while on its
own. Removing a toolchain is rustup's job; cargo-heft names the command and
never suggests `rm`.

A toolchain in use shows why, such as `(in use: default)` or
`(in use: pinned by /work/app/rust-toolchain.toml)`. Run through Cargo,
`cargo heft disk` sees `RUSTUP_TOOLCHAIN` set to the toolchain running it, so
that one is always kept. Only the current directory and those above it are
searched for `rust-toolchain.toml`: a project elsewhere that pins an old
toolchain is not seen, so check before uninstalling one.

## Not yet

- Target directories of other projects (`--projects`, #49).
- JSON output (#51).
- Third-party caches (sccache, IDE caches), and Windows.
