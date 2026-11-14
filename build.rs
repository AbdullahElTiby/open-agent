//! Embeds the app icon and name into the Windows executable, so Task Manager,
//! the taskbar and Explorer show "Open Agent" with its logo, and stamps a build id.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    // A fresh build id whenever the sources change, so the running app can tell
    // a new build apart from the one it is (see src/update.rs).
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=build.rs");
    let id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=OA_BUILD_ID={id}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico")
            .set("ProductName", "Open Agent")
            .set("FileDescription", "Open Agent")
            .set("InternalName", "open-agent")
            .set("OriginalFilename", "open-agent.exe");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }
}
