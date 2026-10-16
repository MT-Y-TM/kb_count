fn main() {
    println!("cargo:rerun-if-changed=assets/kb_count.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_resources();
    }
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

fn embed_windows_resources() {
    use std::{env, fs, path::PathBuf, process::Command};
    assert_eq!(
        env::var("CARGO_CFG_TARGET_ENV").as_deref(),
        Ok("msvc"),
        "kb_count Windows builds require the MSVC toolchain"
    );
    let arch = env::var("CARGO_CFG_TARGET_ARCH").expect("target architecture");
    let sdk = find_msvc_tools::find_windows_sdk(&arch)
        .expect("Windows SDK is required for icon resources");
    let compiler = sdk
        .path()
        .map(|directory| directory.join("rc.exe"))
        .find(|path| path.is_file())
        .expect("Windows SDK rc.exe not found");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("build output directory"));
    let icon = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap())
        .join("assets/kb_count.ico")
        .to_string_lossy()
        .replace('\\', "/");
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let major = env::var("CARGO_PKG_VERSION_MAJOR").unwrap();
    let minor = env::var("CARGO_PKG_VERSION_MINOR").unwrap();
    let patch = env::var("CARGO_PKG_VERSION_PATCH").unwrap();
    let source = format!(
        r#"#pragma code_page(65001)
1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004L
FILETYPE 0x1L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "FileDescription", "kb_count · 本地输入统计\0"
            VALUE "ProductName", "kb_count\0"
            VALUE "OriginalFilename", "kb_count.exe\0"
            VALUE "FileVersion", "{version}\0"
            VALUE "ProductVersion", "{version}\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#
    );
    let rc = out.join("kb_count.rc");
    let res = out.join("kb_count.res");
    fs::write(&rc, source).expect("write Windows resource script");
    assert!(
        Command::new(compiler)
            .arg("/nologo")
            .arg("/fo")
            .arg(&res)
            .arg(&rc)
            .status()
            .expect("run Windows resource compiler")
            .success(),
        "Windows resource compilation failed"
    );
    println!("cargo:rustc-link-arg={}", res.display());
}
