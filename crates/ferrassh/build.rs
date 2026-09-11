fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_manifest_file("app.manifest");
        res.set("ProductName", "FerraSSH");
        res.set("FileDescription", "FerraSSH");
        res.set("LegalCopyright", "Copyright 2026 Guizhou Lixian Network Technology Co., Ltd.");
        res.set("FileVersion", "0.2.10.0");
        res.set("ProductVersion", "0.2.10.0");
        let ico = std::path::Path::new("../../src-tauri/icons/icon.ico");
        if ico.exists() {
            res.set_icon("../../src-tauri/icons/icon.ico");
        }
        res.compile().expect("embed windows resources");
    }
}
