//! Randomized trees: every report must still add up and match `du`.
//!
//! The generator is a small deterministic xorshift, so a failure reproduces
//! with the same seed and the test needs no dependencies.

mod common;

use std::path::Path;

use common::*;

const DIRS: [&str; 7] = [
    "debug/deps",
    "debug/build/libfoo-0123456789abcdef",
    "debug/.fingerprint/libfoo-0123456789abcdef",
    "debug/incremental/foo-2k1x9",
    "doc/foo",
    "package",
    "",
];

const EXTENSIONS: [&str; 12] = [
    "rlib", "rmeta", "o", "d", "dylib", "so", "dwo", "dwp", "rlib", "o", "txt", "",
];

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn build_tree(root: &Path, seed: u64) {
    let mut rng = Rng::new(seed);
    for i in 0..40 {
        let dir = DIRS[rng.below(DIRS.len())];
        let ext = EXTENSIONS[rng.below(EXTENSIONS.len())];
        let name = format!("crate{i}-0123456789abcdef");
        let file = if ext.is_empty() {
            name
        } else {
            format!("{name}.{ext}")
        };
        let rel = if dir.is_empty() {
            file
        } else {
            format!("{dir}/{file}")
        };
        let len = rng.below(9) * 512;
        let path = write_sized(root, &rel, len, b'x');
        if ext.is_empty() {
            set_executable(&path);
        }
    }
    // A few files that exist only to exercise the content view, and a hard
    // link between deps and incremental.
    let rlib = write_sized(
        root,
        "debug/deps/liblinked-0123456789abcdef.rlib",
        12_288,
        b'r',
    );
    hard_link(
        &rlib,
        &root.join("debug/incremental/foo-2k1x9/dep-graph.bin"),
    );
    write_sized(
        root,
        "debug/incremental/foo-2k1x9/query-cache.bin",
        4096,
        b'q',
    );
}

#[test]
fn reports_add_up_for_random_trees() {
    for seed in 1..=8 {
        let dir = TempDir::new(&format!("random-{seed}"));
        build_tree(dir.path(), seed);
        let report = report(dir.path(), &["--target-dir", dir.path().to_str().unwrap()]);
        assert_consistent(&report);
        let total = report["total_bytes"].as_u64().unwrap();
        assert!(total > 0, "seed {seed}");
        assert!(
            report["by_crate"]
                .as_object()
                .unwrap()
                .values()
                .all(|b| b.as_u64().unwrap() <= total),
            "seed {seed}: a crate accounts for more than the total"
        );
        #[cfg(unix)]
        assert_matches_du(total, dir.path());
    }
}
