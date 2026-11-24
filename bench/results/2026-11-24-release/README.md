# Target directory sizes of 18 Rust projects, release profile

The same 18 projects and commits as [`../2026-11-14-rust-1.100`](../2026-11-14-rust-1.100),
built from scratch with `cargo build --release` (the `release` profile, as
each project configures it) and measured with `cargo heft --json`.

- Toolchain: rustc 1.100.0, host `aarch64-apple-darwin` (macOS, Apple silicon).
- Measured on 24 Nov 2026. The 18 builds took 76 minutes, against 25 for `dev`.
- Reproduce: `PROFILE=release TOOLCHAIN=1.100.0 bench/run.sh <work-dir> <out-dir>`,
  then `python3 bench/table.py <out-dir>`.

| Project | Commit | `target/` | Debuginfo | Incremental | Metadata | of which twice | Code | Other |
|---|---|--:|--:|--:|--:|--:|--:|--:|
| clap | `ec93e71` | 25 MB | 22% | 0% | 28% | 14% | 48% | 1% |
| tokio | `f3e855f` | 97 MB | 0% | 0% | 73% | 30% | 26% | 1% |
| axum | `3ebdbe6` | 131 MB | 0% | 0% | 82% | 38% | 16% | 1% |
| fd | `6cf3302` | 133 MB | 0% | 0% | 73% | 36% | 25% | 2% |
| ripgrep | `178cc02` | 166 MB | 33% | 0% | 41% | 20% | 25% | 1% |
| hyperfine | `c516152` | 203 MB | 0% | 0% | 84% | 42% | 15% | 1% |
| tantivy | `2ad36b4` | 414 MB | 0% | 0% | 73% | 34% | 25% | 2% |
| bat | `8cf33bd` | 485 MB | 0% | 0% | 71% | 35% | 27% | 2% |
| alacritty | `aa5d942` | 517 MB | 11% | 0% | 48% | 23% | 40% | 1% |
| cargo | `382ecb8` | 895 MB | 0% | 0% | 67% | 32% | 30% | 3% |
| starship | `d128ba8` | 904 MB | 0% | 0% | 75% | 36% | 24% | 1% |
| rust-analyzer | `114dcb8` | 982 MB | 0% | 0% | 54% | 23% | 45% | 1% |
| typst | `84cca40` | 1.35 GB | 0% | 0% | 67% | 32% | 32% | 1% |
| zola | `48d4e7a` | 1.54 GB | 0% | 0% | 69% | 34% | 27% | 3% |
| uv | `d5c3f17` | 1.59 GB | 0% | 0% | 57% | 26% | 38% | 5% |
| wasmtime | `c52bc6d` | 1.60 GB | 0% | 0% | 63% | 31% | 34% | 3% |
| nushell | `3e2dc65` | 2.29 GB | 0% | 0% | 55% | 27% | 43% | 2% |
| ruff | `b140af2` | 2.68 GB | 0% | 0% | 35% | 15% | 62% | 2% |
| **all 18** | | **16.0 GB** | **1%** | **0%** | **58%** | **28%** | **39%** | **2%** |

The columns are explained in [`../2026-11-14-rust-1.100`](../2026-11-14-rust-1.100/README.md#reading-the-table).

## Against `dev`

| | `dev` | `release` |
|---|--:|--:|
| All 18 projects | 47.5 GB | 16.0 GB |
| Debuginfo | 31% | 1% |
| Incremental | 21% | 0% |
| Metadata | 17% | 58% |
| of which twice | 8% | 28% |
| Code | 30% | 39% |

- A release build of the same projects takes a third of the space (66% less).
  The saving per project goes from 21% (zola) to 79% (wasmtime).
- Release turns off incremental compilation and, by default, debuginfo, so
  almost all of the difference is those two parts.
- What is left is mostly crate metadata: 58% of the bytes, and 28% is
  metadata stored twice, as `.rmeta` and inside the `.rlib`. In release, the
  duplicated metadata alone (4.4 GB) is larger than all the debuginfo,
  incremental cache and other files together.
- Debuginfo stays only where a project asks for it in its release profile:
  ripgrep and alacritty set `debug = 1` (33% and 11%), clap sets
  `debug = "line-tables-only"` (22%).

## Limits

- One platform, one build per project, default members of each workspace,
  no tests. The same limits as the `dev` runs.
