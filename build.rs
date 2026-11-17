// Embeds Windows version information (product name, version, copyright)
// into srec.exe. Properly described binaries are treated with more
// trust by Windows SmartScreen and antivirus heuristics.
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/srec.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/srec.ico");
        res.set("ProductName", "SRec");
        res.set("FileDescription", "SRec - AV1 screen recorder");
        res.set("CompanyName", "The SRec Authors");
        res.set("LegalCopyright", "Copyright (c) 2026 The SRec Authors. MIT License.");
        res.set("OriginalFilename", "srec.exe");
        if let Err(e) = res.compile() {
            println!("cargo:warning=could not embed version info: {e}");
        }
    }
}
