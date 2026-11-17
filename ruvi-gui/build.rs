#[cfg(windows)]
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/ruvi-studio.ico");

    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/ruvi-studio.ico");
    res.set("FileDescription", "RuviStudio — Pure-Rust Media Studio");
    res.set("ProductName", "RuviStudio");
    res.set("OriginalFilename", "ruvi-studio.exe");
    res.set("LegalCopyright", "Copyright (c) 2026 RuviTools");
    if let Err(e) = res.compile() {
        panic!("Failed to compile Windows resource icon: {e}");
    }
}

#[cfg(not(windows))]
fn main() {}
