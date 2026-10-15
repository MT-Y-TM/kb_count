fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        // Slint's generated window constructor needs more than the default
        // one-megabyte main-thread stack in unoptimized Windows builds.
        println!("cargo:rustc-link-arg=/STACK:8388608");
    }
    slint_build::compile("src/ui/main_window.slint").expect("failed to compile Slint UI");
}
