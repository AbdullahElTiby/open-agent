//! Embeds the app icon and name into the Windows executable, so Task Manager,
//! the taskbar and Explorer show "Open Agent" with its logo.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
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
