fn main() {
    slint_build::compile_with_config(
        "ui/main.slint",
        slint_build::CompilerConfiguration::new()
            .embed_resources(slint_build::EmbedResourcesKind::EmbedForSoftwareRenderer),
    )
    .unwrap();

    // rodata-fix.x must come before linkall.x to merge DROM segments
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:rustc-link-search={manifest_dir}");
    println!("cargo:rustc-link-arg=-Trodata-fix.x");

    linker_be_nice();
    println!("cargo:rustc-link-arg=-Tlinkall.x");
}

fn linker_be_nice() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        let kind = &args[1];
        let what = &args[2];

        match kind.as_str() {
            "undefined-symbol" => match what.as_str() {
                what if what.starts_with("_defmt_") => {
                    eprintln!(
                        "\n💡 `defmt` not found - make sure `defmt.x` is added as a linker script\n"
                    );
                }
                "_stack_start" => {
                    eprintln!("\n💡 Is the linker script `linkall.x` missing?\n");
                }
                what if what.starts_with("esp_rtos_") => {
                    eprintln!(
                        "\n💡 `esp-radio` has no scheduler enabled. Initialize `esp-rtos`.\n"
                    );
                }
                "free" | "malloc" | "calloc" => {
                    eprintln!("\n💡 Did you forget the `esp-alloc` dependency?\n");
                }
                _ => (),
            },
            _ => {
                std::process::exit(1);
            }
        }
        std::process::exit(0);
    }

    println!(
        "cargo:rustc-link-arg=-Wl,--error-handling-script={}",
        std::env::current_exe().unwrap().display()
    );
}
