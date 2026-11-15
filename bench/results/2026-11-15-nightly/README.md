# Target directory sizes of 18 Rust projects, on nightly

The same 18 projects, at the same commits and on the same machine as the
Rust 1.100 run in [`../2026-11-14-rust-1.100`](../2026-11-14-rust-1.100),
built with a recent nightly. Measured after a suggestion on Zulip that recent
nightlies shrink metadata and the query cache.

- Toolchain: rustc 1.102.0-nightly (5f4effd57 2026-11-13), cargo 1.102.0-nightly
  (ede6cefea 2026-11-11), host `aarch64-apple-darwin` (macOS, Apple silicon).
- Measured on 15 Nov 2026 with cargo-heft from main (1d71bbd).
- Reproduce: `TOOLCHAIN=nightly bench/run.sh <work-dir> <out-dir>`, with the
  clones of the 1.100 run (or `PIN_DIR=bench/results/2026-11-14-rust-1.100`).

| Project | Commit | `target/` | Debuginfo | Incremental | Metadata | of which twice | Code | Other |
|---|---|--:|--:|--:|--:|--:|--:|--:|
| clap | `ec93e71` | 75 MB | 31% | 42% | 3% | 0% | 23% | 0% |
| fd | `6cf3302` | 131 MB | 2% | 12% | 27% | 0% | 57% | 2% |
| hyperfine | `c516152` | 188 MB | 26% | 11% | 26% | 0% | 36% | 1% |
| axum | `3ebdbe6` | 191 MB | 25% | 33% | 19% | 0% | 23% | 1% |
| tokio | `f3e855f` | 193 MB | 27% | 43% | 11% | 0% | 19% | 1% |
| ripgrep | `178cc02` | 271 MB | 33% | 24% | 9% | 0% | 32% | 1% |
| alacritty | `aa5d942` | 574 MB | 31% | 18% | 16% | 0% | 35% | 1% |
| bat | `8cf33bd` | 797 MB | 38% | 5% | 15% | 0% | 41% | 1% |
| tantivy | `2ad36b4` | 916 MB | 36% | 27% | 9% | 0% | 26% | 2% |
| zola | `48d4e7a` | 1.40 GB | 0% | 12% | 28% | 0% | 55% | 5% |
| starship | `d128ba8` | 1.72 GB | 40% | 10% | 14% | 0% | 36% | 1% |
| cargo | `382ecb8` | 2.48 GB | 39% | 17% | 9% | 0% | 34% | 2% |
| rust-analyzer | `114dcb8` | 3.38 GB | 20% | 36% | 5% | 0% | 39% | 0% |
| uv | `d5c3f17` | 4.58 GB | 19% | 33% | 6% | 0% | 40% | 2% |
| typst | `84cca40` | 4.65 GB | 61% | 13% | 8% | 0% | 18% | 0% |
| ruff | `b140af2` | 4.97 GB | 20% | 39% | 6% | 0% | 34% | 1% |
| nushell | `3e2dc65` | 5.82 GB | 41% | 18% | 8% | 0% | 32% | 1% |
| wasmtime | `c52bc6d` | 6.20 GB | 36% | 30% | 5% | 0% | 27% | 1% |
| **all 18** | | **38.5 GB** | **33%** | **25%** | **8%** | **0%** | **32%** | **1%** |

## Against Rust 1.100

Totals over the 18 projects:

| | Rust 1.100 | nightly | change |
|---|--:|--:|--:|
| `target/` | 47.54 GB | 38.54 GB | -18.9% |
| metadata in `.rmeta` files | 3.99 GB | 3.22 GB | -19.4% |
| metadata inside `.rlib` files | 4.24 GB | 0.01 GB | -99.8% |
| incremental cache | 10.14 GB | 9.67 GB | -4.7% |
| debuginfo | 14.65 GB | 12.68 GB | -13.4% |
| code, data and symbols | 14.03 GB | 12.49 GB | -11.0% |

Every project is smaller, by 7% (clap) to 29% (tokio, hyperfine, fd, axum).

- **Metadata is no longer stored twice.** Nightly Cargo passes
  `-Z embed-metadata=no` to rustc (seen with `cargo build -v`). Each `.rlib`
  still has a `lib.rmeta` member, but only a stub: 456 bytes in the test
  fixture, against 2,811 for the standalone `.rmeta`. That accounts for about
  4.2 GB of the 9 GB saved.
- **The `.rmeta` files themselves are 19% smaller.**
- **The incremental cache is 5% smaller.**
- Debuginfo and code are also smaller. This run does not show why; the
  debuginfo level Cargo passes is unchanged (`-C debuginfo=2`).

## Limits

- One platform, one build per project, default members, no tests.
- Nightly changes daily; these numbers are for the versions above.
