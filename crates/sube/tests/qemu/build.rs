fn main() {
    println!("cargo:rustc-link-search={}", std::env::current_dir().unwrap().display());
    // Emitted here rather than in .cargo/config.toml so the cortex-m-rt linker
    // script is applied regardless of the directory cargo is invoked from
    // (CI builds with --manifest-path from crates/sube).
    println!("cargo:rustc-link-arg=-Tlink.x");
}
