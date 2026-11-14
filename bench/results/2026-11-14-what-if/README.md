# What each profile setting saves, on five projects

`cargo heft --what-if` builds a workspace from scratch once per setting and
measures the target directory each time. These are the results for five of
the benchmark projects, at the same commits as
[`../2026-11-14-rust-1.100`](../2026-11-14-rust-1.100).

- Toolchain: rustc 1.100.0, host `aarch64-apple-darwin` (macOS, Apple silicon).
- Measured on 14 Nov 2026 with cargo-heft 0.1.0.
- Reproduce: `TOOLCHAIN=1.100.0 bench/what-if.sh <work-dir> <out-dir> <project>...`
  after `bench/run.sh` has cloned the projects.

Size of `target/` after a fresh `dev` build, and the change against the
project's own configuration:

| Project | As configured | `debug = "line-tables-only"` | `debug = false` | `incremental = false` | line-tables-only, no incremental |
|---|--:|--:|--:|--:|--:|
| tokio | 269 MB | -22% | -30% | -47% | -55% |
| ripgrep | 308 MB | -20% | -39% | -30% | -44% |
| rust-analyzer | 4.39 GB | -1% | -26% | -31% | -32% |
| ruff | 6.35 GB | -1% | -28% | -34% | -34% |
| nushell | 6.54 GB | -27% | -44% | -30% | -44% |

## What stands out

- Turning incremental off saves 30 to 47% of a fresh build on every one of
  the five. It makes rebuilds after an edit slower, so this is a saving for
  CI and for caches, not for local work.
- `line-tables-only` saves 20 to 27% where the project uses Cargo's default
  debuginfo (tokio, ripgrep, nushell), and nothing on the two that have
  already reduced it: ruff sets `debug = "line-tables-only"` in its own
  `dev` profile, and rust-analyzer sets `debug = 1`.
- The settings do not add up. On nushell, no debuginfo alone and
  line-tables-only without incremental both land at -44%.

## Limits

- One platform, one build per setting.
- The JSON files also hold `build_seconds`. Do not read anything into them:
  other work was running on the machine during this run.
- "As configured" is the project's own `dev` profile, so the columns are
  not the same starting point for every project.
