// Windows builds embed meadow.manifest (via mingw's windres) as the exe's manifest.
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=meadow.rc");
    println!("cargo:rerun-if-changed=meadow.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("meadow-manifest.o");
    let status = Command::new("x86_64-w64-mingw32-windres").args(["meadow.rc", "-O", "coff", "-o"]).arg(&out).status().expect("run x86_64-w64-mingw32-windres");
    assert!(status.success(), "windres failed");
    println!("cargo:rustc-link-arg-bins={}", out.display());
}
