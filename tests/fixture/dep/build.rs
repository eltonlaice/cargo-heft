// Exercises build script artifacts: an executable plus a generated file.
fn main() {
    let out = std::env::var("OUT_DIR").unwrap();
    std::fs::write(format!("{out}/generated.rs"), "pub const N: u32 = 42;\n").unwrap();
}
