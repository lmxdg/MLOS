use std::env;
use std::path::PathBuf;
use std::process::Command;
use std::fs;

fn main() {
    let dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let linker = PathBuf::from(&dir).join("linker.ld");
    println!("cargo:rustc-link-arg=-T{}", linker.display());
    println!("cargo:rerun-if-changed=linker.ld");

    let build_time = Command::new("date")
        .args(["-u", "+%Y-%m-%d %H:%M:%S UTC"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=BUILD_TIME={}", build_time);

    // Embedded disk-boot binaries (assembled by `make boot`)
    let out = env::var("OUT_DIR").unwrap();
    let mbr = PathBuf::from(&dir).join("boot/mbr.bin");
    let stage2 = PathBuf::from(&dir).join("boot/stage2.bin");
    println!("cargo:rerun-if-changed=boot/mbr.bin");
    println!("cargo:rerun-if-changed=boot/stage2.bin");
    let mbr_ok = mbr.exists() && fs::metadata(&mbr).map(|m| m.len() == 512).unwrap_or(false);
    let st2_ok = stage2.exists();
    let gen = if mbr_ok && st2_ok {
        format!(
            "pub static MBR: &[u8] = include_bytes!({:?});\npub static STAGE2: &[u8] = include_bytes!({:?});\n",
            mbr, stage2
        )
    } else {
        "pub static MBR: &[u8] = &[];\npub static STAGE2: &[u8] = &[];\n".to_string()
    };
    fs::write(PathBuf::from(&out).join("bootbins.rs"), gen).unwrap();
}
