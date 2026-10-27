//! Incorpora nell'eseguibile l'icona, le informazioni sulla versione e il
//! manifest (DPI per monitor e controlli di Windows moderni).

fn main() {
    println!("cargo:rerun-if-changed=assets/skylight.ico");
    println!("cargo:rerun-if-changed=assets/skylight.manifest");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut resource = winresource::WindowsResource::new();
    resource
        .set_icon("assets/skylight.ico")
        .set_manifest_file("assets/skylight.manifest")
        .set("ProductName", "Skylight")
        .set(
            "FileDescription",
            "Skylight: un notch vivo in cima allo schermo",
        );
    if let Err(e) = resource.compile() {
        // Senza risorse l'app funziona lo stesso: solo l'exe non ha l'icona.
        println!("cargo:warning=risorse non incorporate: {e}");
    }
}
