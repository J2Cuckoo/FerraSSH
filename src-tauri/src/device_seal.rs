//! Install-dir device seal: bind a packed build to the machine that ran the installer.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const SEAL_NAME: &str = env!("FERRA_SEAL_NAME");
const SEAL_SALT: &str = env!("FERRA_SEAL_SALT");

pub fn bind_install_seal() -> Result<(), String> {
    let path = seal_path()?;
    let hash = expected_hash()?;
    write_hidden(&path, hash.as_bytes()).map_err(|e| e.to_string())
}

pub fn unbind_install_seal() -> Result<(), String> {
    let path = seal_path()?;
    let _ = fs::remove_file(path);
    Ok(())
}

pub fn verify_install_seal() -> bool {
    if cfg!(debug_assertions) {
        return true;
    }
    match (seal_path(), expected_hash()) {
        (Ok(path), Ok(want)) => match read_seal(&path) {
            Ok(got) => got == want,
            Err(_) => false,
        },
        _ => false,
    }
}

fn install_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    exe.parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "no install dir".into())
}

fn seal_path() -> Result<PathBuf, String> {
    Ok(install_dir()?.join(SEAL_NAME))
}

fn expected_hash() -> Result<String, String> {
    Ok(hash_fingerprint(&hardware_fingerprint()))
}

fn hash_fingerprint(fp: &str) -> String {
    let mut h = Sha256::new();
    h.update(SEAL_SALT.as_bytes());
    h.update(&[0x1f]);
    h.update(fp.as_bytes());
    hex_encode(&h.finalize())
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

fn hardware_fingerprint() -> String {
    let mut parts = Vec::new();
    parts.push(format!("os={}", std::env::consts::OS));
    parts.push(format!("arch={}", std::env::consts::ARCH));
    #[cfg(windows)]
    {
        if let Some(guid) = windows_machine_guid() {
            parts.push(format!("guid={guid}"));
        }
        if let Some(serial) = windows_volume_serial() {
            parts.push(format!("vol={serial}"));
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(id) = fs::read_to_string("/etc/machine-id") {
            parts.push(format!("machine={}", id.trim()));
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(id) = macos_platform_uuid() {
            parts.push(format!("uuid={id}"));
        }
    }
    parts.join("\n")
}

#[cfg(windows)]
fn windows_machine_guid() -> Option<String> {
    use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
    use winreg::RegKey;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey_with_flags("SOFTWARE\\Microsoft\\Cryptography", KEY_READ | KEY_WOW64_64KEY)
        .ok()?;
    key.get_value::<String, _>("MachineGuid").ok()
}

#[cfg(windows)]
fn windows_volume_serial() -> Option<String> {
    let root = std::env::var("SystemDrive").unwrap_or_else(|_| "C:".into());
    let root = if root.ends_with('\\') {
        root
    } else {
        format!("{root}\\")
    };
    let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
    let mut serial = 0u32;
    let ok = unsafe {
        GetVolumeInformationW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            0,
            &mut serial,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
        )
    };
    if ok == 0 {
        None
    } else {
        Some(format!("{serial:08x}"))
    }
}

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GetVolumeInformationW(
        lp_root_path_name: *const u16,
        lp_volume_name_buffer: *mut u16,
        n_volume_name_size: u32,
        lp_volume_serial_number: *mut u32,
        lp_maximum_component_length: *mut u32,
        lp_file_system_flags: *mut u32,
        lp_file_system_name_buffer: *mut u16,
        n_file_system_name_size: u32,
    ) -> i32;
    fn SetFileAttributesW(lp_file_name: *const u16, dw_file_attributes: u32) -> i32;
}

#[cfg(windows)]
const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;

#[cfg(target_os = "macos")]
fn macos_platform_uuid() -> Option<String> {
    let out = std::process::Command::new("ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some(rest) = line.split("IOPlatformUUID").nth(1) {
            let id = rest
                .trim_start_matches(|c: char| !c.is_alphanumeric() && c != '-')
                .split(|c: char| c == '"' || c.is_whitespace())
                .find(|s| s.len() > 8)?;
            return Some(id.to_string());
        }
    }
    None
}

fn write_hidden(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut f = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    f.write_all(bytes)?;
    f.flush()?;
    drop(f);
    #[cfg(windows)]
    {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        unsafe {
            SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_HIDDEN);
        }
        // Installer runs elevated; grant Users read so a normal launch can verify.
        let _ = std::process::Command::new("icacls")
            .arg(path)
            .args(["/grant", "*S-1-5-32-545:(R)", "/Q"])
            .status();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o400));
    }
    Ok(())
}

fn read_seal(path: &Path) -> std::io::Result<String> {
    let mut f = fs::File::open(path)?;
    let mut buf = String::new();
    f.read_to_string(&mut buf)?;
    Ok(buf.trim().to_string())
}

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_is_stable_64_hex() {
        let a = hash_fingerprint("sample");
        let b = hash_fingerprint("sample");
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(hash_fingerprint("sample"), hash_fingerprint("other"));
    }
}
