use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};

const UPDATE_MANIFEST_URL: &str = "https://files.hyrubik.com/updates/ferrassh/latest.json";

#[derive(serde::Deserialize)]
struct LatestFile {
    version: Option<String>,
    tag: Option<String>,
    notes: Option<String>,
    url: Option<String>,
    windows: Option<LatestAsset>,
}

#[derive(serde::Deserialize)]
struct LatestAsset {
    url: Option<String>,
    #[allow(dead_code)]
    sha256: Option<String>,
}

pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub notes: String,
    pub url: String,
    pub newer: bool,
}

#[derive(Clone, Serialize)]
pub struct UpdateProgress {
    pub stage: String,
    pub transferred: u64,
    pub total: u64,
}

fn parse_ver(s: &str) -> Vec<u32> {
    s.trim().trim_start_matches('v').split('.').filter_map(|p| p.parse().ok()).collect()
}

fn is_newer(latest: &str, current: &str) -> bool {
    let a = parse_ver(latest);
    let b = parse_ver(current);
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

pub fn scrub_url(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    let chars: Vec<char> = s.chars().collect();
    while i < chars.len() {
        let rest: String = chars[i..].iter().collect();
        let lower = rest.to_ascii_lowercase();
        if lower.starts_with("https://") || lower.starts_with("http://") {
            while i < chars.len() && !chars[i].is_whitespace() && !"<>\"')]".contains(chars[i]) {
                i += 1;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn installer_name(url: &str) -> String {
    url.rsplit(['/', '\\'])
        .next()
        .filter(|n| !n.is_empty() && n.to_ascii_lowercase().ends_with(".exe"))
        .unwrap_or("FerraSSH-Setup.exe")
        .to_string()
}

pub async fn check_latest(client: &reqwest::Client, current: &str) -> Result<UpdateInfo, String> {
    let body = client
        .get(UPDATE_MANIFEST_URL)
        .send()
        .await
        .map_err(|e| scrub_url(&e.to_string()))?;
    if !body.status().is_success() {
        return Err(format!("更新通道返回 {}", body.status()));
    }
    let parsed: LatestFile = body.json().await.map_err(|e| format!("更新信息无法解析：{}", scrub_url(&e.to_string())))?;
    let latest = parsed
        .version
        .or(parsed.tag)
        .unwrap_or_default()
        .trim()
        .trim_start_matches('v')
        .to_string();
    let url = parsed
        .windows
        .as_ref()
        .and_then(|w| w.url.clone())
        .or(parsed.url)
        .unwrap_or_default();
    let notes = scrub_url(&parsed.notes.unwrap_or_default()).trim().to_string();
    Ok(UpdateInfo {
        newer: !latest.is_empty() && is_newer(&latest, current),
        current: current.to_string(),
        latest,
        notes,
        url,
    })
}

pub async fn download_installer(app: &AppHandle, url: &str) -> Result<PathBuf, String> {
    if url.is_empty() {
        return Err("没有可用的更新包".into());
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?;
    let mut resp = client.get(url).send().await.map_err(|e| format!("下载失败：{}", scrub_url(&e.to_string())))?;
    if !resp.status().is_success() {
        return Err(format!("下载失败：HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);
    let _ = app.emit(
        "app-update-progress",
        UpdateProgress { stage: "download".into(), transferred: 0, total },
    );
    let dir = std::env::temp_dir().join("ferrassh-update");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let dest = dir.join(installer_name(url));
    let mut file = std::fs::File::create(&dest).map_err(|e| format!("无法保存安装包：{e}"))?;
    let mut transferred = 0u64;
    loop {
        let chunk = resp.chunk().await.map_err(|e| format!("下载失败：{}", scrub_url(&e.to_string())))?;
        let Some(chunk) = chunk else { break };
        file.write_all(&chunk).map_err(|e| format!("无法保存安装包：{e}"))?;
        transferred += chunk.len() as u64;
        let _ = app.emit(
            "app-update-progress",
            UpdateProgress { stage: "download".into(), transferred, total },
        );
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    unblock_download(&dest);
    let _ = app.emit(
        "app-update-progress",
        UpdateProgress { stage: "install".into(), transferred, total },
    );
    Ok(dest)
}

fn unblock_download(path: &Path) {
    #[cfg(windows)]
    win::unblock(path);
    #[cfg(not(windows))]
    let _ = path;
}

pub fn launch_silent_install(setup: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        win::run_setup(setup)?;
        std::thread::sleep(Duration::from_millis(600));
        return Ok(());
    }
    #[cfg(not(windows))]
    {
        let _ = setup;
        Err("当前系统请手动安装更新包".into())
    }
}

#[cfg(windows)]
mod win {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    #[link(name = "kernel32")]
    extern "system" {
        fn DeleteFileW(lp_file_name: *const u16) -> i32;
    }

    #[link(name = "shell32")]
    extern "system" {
        fn ShellExecuteW(
            hwnd: *mut core::ffi::c_void,
            lp_operation: *const u16,
            lp_file: *const u16,
            lp_parameters: *const u16,
            lp_directory: *const u16,
            n_show_cmd: i32,
        ) -> isize;
    }

    fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
        s.as_ref().encode_wide().chain(std::iter::once(0)).collect()
    }

    pub fn unblock(path: &Path) {
        let mut ads: Vec<u16> = path.as_os_str().encode_wide().collect();
        ads.extend(":Zone.Identifier".encode_utf16());
        ads.push(0);
        unsafe {
            DeleteFileW(ads.as_ptr());
        }
    }

    pub fn run_setup(setup: &Path) -> Result<(), String> {
        let op = wide("open");
        let file = wide(setup.as_os_str());
        let params = wide("/S /R /UPDATE");
        let ret = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                op.as_ptr(),
                file.as_ptr(),
                params.as_ptr(),
                std::ptr::null(),
                0,
            )
        };
        if ret <= 32 {
            Err("无法启动安装".into())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_compares_semver() {
        assert!(is_newer("0.2.3", "0.2.2"));
        assert!(!is_newer("0.2.2", "0.2.3"));
        assert!(!is_newer("0.2.3", "0.2.3"));
    }

    #[test]
    fn scrub_hides_https() {
        let s = scrub_url("发现新版本 https://files.hyrubik.com/updates/ferrassh/FerraSSH-0.2.3-x64-Setup.exe 请安装");
        assert!(!s.contains("https://"));
        assert!(!s.contains("files.hyrubik.com"));
        assert!(s.contains("发现新版本"));
    }
}
