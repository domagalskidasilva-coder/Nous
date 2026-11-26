use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=nous.rc");
    println!("cargo:rerun-if-changed=../../assets/nous.ico");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let rc_file = manifest_dir.join("nous.rc");
    let ico_file = manifest_dir.join("../../assets/nous.ico");
    if !rc_file.exists() || !ico_file.exists() {
        return;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let res_file = out_dir.join("nous.res");

    let status = Command::new("rc.exe")
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_file)
        .arg(&rc_file)
        .status();

    if let Ok(st) = status
        && st.success()
        && res_file.exists()
    {
        println!("cargo:rustc-link-arg-bin=nous-desktop={}", res_file.display());
    }
}
