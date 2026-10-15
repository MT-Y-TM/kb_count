fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // Slint's generated window constructor needs more than the default
        // one-megabyte main-thread stack in unoptimized Windows builds.
        println!("cargo:rustc-link-arg=/STACK:8388608");
    }
    let config = slint_build::CompilerConfiguration::new().with_style("fluent".into());
    slint_build::compile_with_config("src/ui/main_window.slint", config)
        .expect("failed to compile Slint UI");
}
