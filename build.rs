// Intègre l'icône dans l'exécutable Windows.
fn main() {
    println!("cargo:rerun-if-changed=assets/icons/jinglestar.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/jinglestar.ico");
        res.set("ProductName", "JingleStar");
        res.set("FileDescription", "JingleStar");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icône Windows non intégrée : {e}");
        }
    }
}
