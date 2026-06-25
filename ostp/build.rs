fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("..\\ostp-gui\\src-tauri\\icons\\icon.ico");
        res.set("ProductName", "OSTP Core");
        res.set("FileDescription", "OSTP CLI");
        res.set("CompanyName", "Ospab Foundation");
        res.set("LegalCopyright", "Copyright (c) 2026 Ospab Foundation");
        if let Err(e) = res.compile() {
            println!("cargo:warning=Failed to compile Windows resources: {}", e);
        }
    }
}
