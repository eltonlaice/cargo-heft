# cargo-heft

Shows where the bytes of a Cargo `target/` directory go.

```
cargo install --path .
cargo heft
```

```
target: 313.8 MB on disk

by kind
  rlib                              90.6 MB   28.9%
  object file                       80.2 MB   25.5%
  incremental                       68.8 MB   21.9%
  binary                            31.8 MB   10.1%
  rmeta                             29.8 MB    9.5%
  build script executable           11.4 MB    3.6%
  ...
```

(ripgrep, fresh `dev` build, macOS arm64, cargo 1.99.1.)

## What the numbers mean

- Sizes are on-disk: allocated blocks, not file lengths. The total matches `du -s`.
- Every inode is counted once. rustc hard-links object files and metadata
  between `deps/` and `incremental/`; a shared file is counted as the artifact
  in `deps/`, so `incremental` is what deleting that directory would free.
- Files are classified by name, not by directory.

## Options

- `--target-dir <DIR>`: measure this directory instead of the workspace's.
- `--top <N>`: how many crates to list (default 15).
- `--json`: machine-readable output.

## Status

Early. Works on macOS and Linux. Planned: a breakdown inside `.rlib` files
(metadata, code, debuginfo) and a `--what-if` mode that measures what each
profile setting saves on your project.

## License

MIT or Apache-2.0, at your option.
