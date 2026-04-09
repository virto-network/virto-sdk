fn main() {
    println!("cargo:rustc-link-search={}", std::env::current_dir().unwrap().display());
}
