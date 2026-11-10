//! Embeds the Aether icon in the Windows executable. GPUI loads icon resource 1
//! for its windows, and the Explorer shows it for `aether-desktop.exe`.
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/aether.rc");
    println!("cargo:rerun-if-changed=assets/aether.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("assets/aether.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("cannot embed the Aether icon");
    }
}
