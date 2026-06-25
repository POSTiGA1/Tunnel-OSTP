fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("..\\ostp-gui\\src-tauri\\icons\\icon.ico");
        res.set("ProductName", "OSTP Core");
        res.set("FileDescription", "OSTP Tunnel Helper");
        res.set("CompanyName", "Ospab Foundation");
        res.set("LegalCopyright", "Copyright (c) 2026 Ospab Foundation");
        
        // This manifest explicitly requests administrator privileges, which triggers
        // UAC when the helper is run directly. The GUI launches it as admin anyway,
        // but this ensures it always runs elevated.
        res.set_manifest(r#"
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="requireAdministrator" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#);
        if let Err(e) = res.compile() {
            println!("cargo:warning=Failed to compile Windows resources: {}", e);
        }
    }
}
