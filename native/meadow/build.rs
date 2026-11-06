// Windows builds embed meadow.manifest (via mingw's windres) as the exe's manifest.
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=meadow.rc");
    println!("cargo:rerun-if-changed=meadow.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    // link.exe embeds the manifest itself; windres only exists for the mingw targets.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let manifest = PathBuf::from("meadow.manifest").canonicalize().expect("meadow.manifest");
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("meadow-manifest.o");
    // SPIKE: this host has no mingw windres; the manifest only matters for wallpaper mode.
    let Ok(status) = Command::new("x86_64-w64-mingw32-windres").args(["meadow.rc", "-O", "coff", "-o"]).arg(&out).status() else {
        println!("cargo:warning=no x86_64-w64-mingw32-windres: building without the manifest (spike)");
        return;
    };
    assert!(status.success(), "windres failed");
    println!("cargo:rustc-link-arg-bins={}", out.display());
}
