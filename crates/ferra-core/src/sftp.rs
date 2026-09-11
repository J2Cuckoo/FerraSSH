use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use futures::{stream, StreamExt, TryStreamExt};
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::{FileAttributes, OpenFlags};
use serde::{Deserialize, Serialize};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::sync::mpsc;
use walkdir::WalkDir;

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransferDirection {
    Upload,
    Download,
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProgress {
    pub job_id: String,
    pub path: String,
    pub transferred: u64,
    pub total: u64,
    pub direction: TransferDirection,
    pub finished: bool,
    pub error: Option<String>,
    #[serde(default)]
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub mode: u32,
    pub mtime: u64,
    pub longname: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferOptions {
    pub resume: bool,
    pub preserve_perms: bool,
    pub follow_symlinks: bool,
    pub chunk_size: usize,
    pub parallel: usize,
    #[serde(default)]
    pub overwrite: bool,
}

impl Default for TransferOptions {
    fn default() -> Self {
        Self {
            resume: true,
            preserve_perms: true,
            follow_symlinks: false,
            chunk_size: 512 * 1024,
            parallel: 4,
            overwrite: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SyncAction {
    Upload,
    Download,
    Skip,
    Link,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncItem {
    pub relative: String,
    pub action: SyncAction,
    pub reason: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncPlan {
    pub items: Vec<SyncItem>,
    pub uploads: u64,
    pub downloads: u64,
    pub skipped: u64,
}

pub async fn list_dir(sftp: &SftpSession, path: &str) -> Result<Vec<RemoteEntry>> {
    let path = if path.is_empty() || path == "~" { "." } else { path };
    let mut out = Vec::new();
    for entry in sftp.read_dir(path).await? {
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let joined = entry.path();
        let attrs = entry.metadata();
        out.push(RemoteEntry {
            name: name.clone(),
            path: joined,
            is_dir: attrs.is_dir(),
            is_symlink: attrs.is_symlink(),
            size: attrs.size.unwrap_or(0),
            mode: attrs.permissions.unwrap_or(0),
            mtime: attrs.mtime.unwrap_or(0) as u64,
            longname: String::new(),
        });
    }
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(out)
}

pub async fn mkdir_p(sftp: &SftpSession, path: &str) -> Result<()> {
    let mut acc = String::new();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        if path.starts_with('/') && acc.is_empty() {
            acc.push('/');
        }
        if !acc.is_empty() && !acc.ends_with('/') {
            acc.push('/');
        }
        acc.push_str(part);
        if !sftp.try_exists(&acc).await.unwrap_or(false) {
            let _ = sftp.create_dir(&acc).await;
        }
    }
    Ok(())
}

pub async fn create_file(sftp: &SftpSession, path: &str) -> Result<()> {
    if sftp.try_exists(path).await.unwrap_or(false) {
        return Err(Error::msg("file already exists"));
    }
    if let Some(parent) = parent_remote(path) {
        mkdir_p(sftp, &parent).await?;
    }
    let _ = sftp
        .open_with_flags(path, OpenFlags::CREATE | OpenFlags::WRITE)
        .await?;
    Ok(())
}

pub async fn remove_entry(sftp: &SftpSession, path: &str, recursive: bool) -> Result<()> {
    remove_entry_progress(sftp, path, recursive, "delete", None).await
}

async fn collect_remote_delete(sftp: &SftpSession, path: &str, recursive: bool) -> Result<Vec<(String, bool)>> {
    let meta = sftp.metadata(path).await?;
    let is_dir = meta.is_dir();
    let mut out = Vec::new();
    if is_dir && recursive {
        for child in list_dir(sftp, path).await? {
            out.extend(Box::pin(collect_remote_delete(sftp, &child.path, true)).await?);
        }
    }
    out.push((path.to_string(), is_dir));
    Ok(out)
}

pub async fn remove_entry_progress(
    sftp: &SftpSession,
    path: &str,
    recursive: bool,
    job_id: &str,
    progress: Option<mpsc::UnboundedSender<TransferProgress>>,
) -> Result<()> {
    let list = collect_remote_delete(sftp, path, recursive).await?;
    let total = list.len().max(1) as u64;
    emit(&progress, job_id, path, 0, total, TransferDirection::Delete, false, None);
    for (i, (p, is_dir)) in list.iter().enumerate() {
        let done = i as u64 + 1;
        let r = if *is_dir {
            sftp.remove_dir(p).await
        } else {
            sftp.remove_file(p).await
        };
        if let Err(e) = r {
            let msg = e.to_string();
            emit(
                &progress,
                job_id,
                p,
                done.saturating_sub(1),
                total,
                TransferDirection::Delete,
                true,
                Some(msg.clone()),
            );
            return Err(Error::from(e));
        }
        emit(
            &progress,
            job_id,
            p,
            done,
            total,
            TransferDirection::Delete,
            done >= total,
            None,
        );
    }
    Ok(())
}

pub fn collect_local_delete(path: &Path) -> std::io::Result<Vec<(PathBuf, bool)>> {
    let meta = std::fs::symlink_metadata(path)?;
    let mut out = Vec::new();
    if meta.is_dir() {
        if let Ok(rd) = std::fs::read_dir(path) {
            for e in rd.flatten() {
                if let Ok(kids) = collect_local_delete(&e.path()) {
                    out.extend(kids);
                }
            }
        }
        out.push((path.to_path_buf(), true));
    } else {
        out.push((path.to_path_buf(), false));
    }
    Ok(out)
}

pub async fn rename(sftp: &SftpSession, from: &str, to: &str) -> Result<()> {
    sftp.rename(from, to).await?;
    Ok(())
}

pub async fn symlink(sftp: &SftpSession, target: &str, link: &str) -> Result<()> {
    sftp.symlink(target, link).await?;
    Ok(())
}

pub async fn readlink(sftp: &SftpSession, path: &str) -> Result<String> {
    Ok(sftp.read_link(path).await?)
}

pub async fn path_exists(sftp: &SftpSession, path: &str) -> bool {
    sftp.metadata(path).await.is_ok()
}

pub async fn path_stat(sftp: &SftpSession, path: &str) -> (bool, bool, u64, u64) {
    match sftp.metadata(path).await {
        Ok(m) => (
            true,
            m.is_dir(),
            m.size.unwrap_or(0),
            m.mtime.unwrap_or(0) as u64,
        ),
        Err(_) => (false, false, 0, 0),
    }
}

pub async fn chmod(sftp: &SftpSession, path: &str, mode: u32) -> Result<()> {
    let mut attrs = FileAttributes::default();
    attrs.permissions = Some(mode);
    sftp.set_metadata(path, attrs).await.map_err(Error::from)?;
    Ok(())
}

pub async fn upload_file(
    sftp: &SftpSession,
    local: &Path,
    remote: &str,
    opts: &TransferOptions,
    job_id: &str,
    progress: Option<mpsc::UnboundedSender<TransferProgress>>,
) -> Result<u64> {
    let meta = fs::metadata(local).await?;
    if meta.is_dir() {
        return Err(Error::msg("upload_file called on a directory"));
    }
    if !opts.follow_symlinks && meta.file_type().is_symlink() {
        let target = fs::read_link(local).await?;
        let _ = sftp.remove_file(remote).await;
        sftp.symlink(target.to_string_lossy().as_ref(), remote).await?;
        emit(
            &progress,
            job_id,
            remote,
            0,
            0,
            TransferDirection::Upload,
            true,
            None,
        );
        return Ok(0);
    }

    let total = meta.len();
    let mut offset = 0u64;
    if let Ok(rm) = sftp.metadata(remote).await {
        if !opts.overwrite {
            emit(
                &progress,
                job_id,
                remote,
                rm.size.unwrap_or(0),
                total,
                TransferDirection::Upload,
                true,
                None,
            );
            return Ok(0);
        }
        if opts.resume && !opts.overwrite {
            if rm.size.unwrap_or(0) < total {
                offset = rm.size.unwrap_or(0);
            } else if rm.size.unwrap_or(0) == total {
                emit(&progress, job_id, remote, total, total, TransferDirection::Upload, true, None);
                return Ok(0);
            }
        }
    }

    if let Some(parent) = parent_remote(remote) {
        mkdir_p(sftp, &parent).await?;
    }

    let flags = if offset > 0 {
        OpenFlags::CREATE | OpenFlags::WRITE | OpenFlags::READ
    } else {
        OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE
    };
    let mut remote_file = sftp.open_with_flags(remote, flags).await?;
    let mut local_file = File::open(local).await?;
    if offset > 0 {
        local_file.seek(std::io::SeekFrom::Start(offset)).await?;
        remote_file.seek(std::io::SeekFrom::Start(offset)).await?;
    }

    let mut buf = vec![0u8; opts.chunk_size.max(32 * 1024)];
    let mut transferred = offset;
    loop {
        let n = local_file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        remote_file.write_all(&buf[..n]).await?;
        transferred += n as u64;
        emit(&progress, job_id, remote, transferred, total, TransferDirection::Upload, false, None);
    }
    remote_file.flush().await.map_err(|e| Error::msg(format!("写入未完成：{e}")))?;
    remote_file.shutdown().await.map_err(|e| Error::msg(format!("写入未完成：{e}")))?;
    drop(remote_file);

    stamp_mtime_now(sftp, remote).await;
    if opts.preserve_perms {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut attrs = FileAttributes::default();
            attrs.permissions = Some(meta.permissions().mode());
            let _ = sftp.set_metadata(remote, attrs).await;
        }
    }

    let got = sftp.metadata(remote).await.map_err(|e| Error::msg(format!("上传后无法核对远端文件：{e}")))?;
    if got.size.unwrap_or(0) != total {
        return Err(Error::msg(format!(
            "上传未成功：远端大小 {}，本地 {}",
            got.size.unwrap_or(0),
            total
        )));
    }
    emit(&progress, job_id, remote, transferred, total, TransferDirection::Upload, true, None);
    Ok(transferred.saturating_sub(offset))
}

async fn stamp_mtime_now(sftp: &SftpSession, path: &str) {
    let now = to_unix(SystemTime::now()) as u32;
    let mut attrs = FileAttributes::default();
    attrs.mtime = Some(now);
    attrs.atime = Some(now);
    let _ = sftp.set_metadata(path, attrs).await;
}

pub async fn download_file(
    sftp: &SftpSession,
    remote: &str,
    local: &Path,
    opts: &TransferOptions,
    job_id: &str,
    progress: Option<mpsc::UnboundedSender<TransferProgress>>,
) -> Result<u64> {
    let meta = sftp.metadata(remote).await?;
    if meta.is_symlink() && !opts.follow_symlinks {
        let target = sftp.read_link(remote).await?;
        if let Some(parent) = local.parent() {
            fs::create_dir_all(parent).await?;
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, local)?;
        #[cfg(not(unix))]
        {
            let _ = fs::write(local, format!("symlink -> {target}")).await;
        }
        emit(&progress, job_id, remote, 0, 0, TransferDirection::Download, true, None);
        return Ok(0);
    }

    let total = meta.size.unwrap_or(0);
    if let Some(parent) = local.parent() {
        fs::create_dir_all(parent).await?;
    }
    let mut offset = 0u64;
    if opts.resume {
        if let Ok(lm) = fs::metadata(local).await {
            if lm.len() < total {
                offset = lm.len();
            } else if lm.len() == total {
                emit(&progress, job_id, remote, total, total, TransferDirection::Download, true, None);
                return Ok(0);
            }
        }
    }

    let mut remote_file = sftp.open(remote).await?;
    let mut local_file = if offset > 0 {
        let mut f = OpenOptions::new().write(true).open(local).await?;
        f.seek(std::io::SeekFrom::Start(offset)).await?;
        remote_file.seek(std::io::SeekFrom::Start(offset)).await?;
        f
    } else {
        File::create(local).await?
    };

    let mut buf = vec![0u8; opts.chunk_size.max(32 * 1024)];
    let mut transferred = offset;
    loop {
        let n = remote_file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        local_file.write_all(&buf[..n]).await?;
        transferred += n as u64;
        emit(&progress, job_id, remote, transferred, total, TransferDirection::Download, false, None);
    }
    local_file.flush().await?;

    if opts.preserve_perms {
        #[cfg(unix)]
        if let Some(mode) = meta.permissions {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = fs::metadata(local).await?.permissions();
            perms.set_mode(mode);
            fs::set_permissions(local, perms).await.ok();
        }
        if let Some(mtime) = meta.mtime {
            let _ = set_mtime(local, mtime as u64);
        }
    }
    emit(&progress, job_id, remote, transferred, total, TransferDirection::Download, true, None);
    Ok(transferred.saturating_sub(offset))
}

pub async fn upload_tree(
    sftp: &SftpSession,
    local_root: &Path,
    remote_root: &str,
    opts: &TransferOptions,
    job_id: &str,
    progress: Option<mpsc::UnboundedSender<TransferProgress>>,
) -> Result<u64> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in WalkDir::new(local_root).follow_links(opts.follow_symlinks) {
        let entry = entry.map_err(|e| Error::msg(e.to_string()))?;
        let rel = entry.path().strip_prefix(local_root).unwrap_or(entry.path());
        let remote = join_remote(remote_root, &rel.to_string_lossy().replace('\\', "/"));
        if entry.file_type().is_dir() {
            dirs.push(remote);
        } else {
            files.push((entry.path().to_path_buf(), remote));
        }
    }
    if files.is_empty() {
        return Err(Error::msg("空文件夹不能上传"));
    }
    dirs.sort_by_key(|d| d.matches('/').count());
    for dir in dirs {
        mkdir_p(sftp, &dir).await?;
    }
    transfer_parallel(files, parallel_slots(opts.parallel), |path, remote| {
        let progress = progress.clone();
        async move { upload_file(sftp, &path, &remote, opts, job_id, progress).await }
    })
    .await
}

pub async fn download_tree(
    sftp: &SftpSession,
    remote_root: &str,
    local_root: &Path,
    opts: &TransferOptions,
    job_id: &str,
    progress: Option<mpsc::UnboundedSender<TransferProgress>>,
) -> Result<u64> {
    download_tree_inner(sftp, remote_root, local_root, opts, job_id, progress).await
}

async fn download_tree_inner(
    sftp: &SftpSession,
    remote_root: &str,
    local_root: &Path,
    opts: &TransferOptions,
    job_id: &str,
    progress: Option<mpsc::UnboundedSender<TransferProgress>>,
) -> Result<u64> {
    let mut files = Vec::new();
    collect_remote_files(sftp, remote_root, local_root, &mut files).await?;
    if files.is_empty() {
        return Err(Error::msg("空文件夹不能下载"));
    }
    fs::create_dir_all(local_root).await?;
    for (_, dest) in &files {
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).await?;
        }
    }
    transfer_parallel(files, parallel_slots(opts.parallel), |remote, dest| {
        let progress = progress.clone();
        async move { download_file(sftp, &remote, &dest, opts, job_id, progress).await }
    })
    .await
}

async fn collect_remote_files(
    sftp: &SftpSession,
    remote_root: &str,
    local_root: &Path,
    files: &mut Vec<(String, PathBuf)>,
) -> Result<()> {
    for entry in list_dir(sftp, remote_root).await? {
        let dest = local_root.join(&entry.name);
        if entry.is_dir {
            Box::pin(collect_remote_files(sftp, &entry.path, &dest, files)).await?;
        } else {
            files.push((entry.path, dest));
        }
    }
    Ok(())
}

async fn transfer_parallel<A, B, F, Fut>(
    items: Vec<(A, B)>,
    parallel: usize,
    f: F,
) -> Result<u64>
where
    F: Fn(A, B) -> Fut,
    Fut: std::future::Future<Output = Result<u64>>,
{
    if items.is_empty() {
        return Ok(0);
    }
    stream::iter(items)
        .map(|(a, b)| f(a, b))
        .buffer_unordered(parallel.max(1))
        .try_fold(0u64, |acc, n| async move { Ok(acc + n) })
        .await
}

/// Bidirectional directory sync. Newer mtime wins; equal size+mtime is skipped.
pub async fn plan_sync(
    sftp: &SftpSession,
    local_root: &Path,
    remote_root: &str,
    bidirectional: bool,
) -> Result<SyncPlan> {
    let local = walk_local(local_root)?;
    let remote = walk_remote(sftp, remote_root, "").await?;
    let mut items = Vec::new();
    let mut uploads = 0u64;
    let mut downloads = 0u64;
    let mut skipped = 0u64;

    for (rel, loc) in &local {
        match remote.get(rel) {
            None => {
                items.push(SyncItem {
                    relative: rel.clone(),
                    action: SyncAction::Upload,
                    reason: "missing on remote".into(),
                    size: loc.size,
                });
                uploads += loc.size;
            }
            Some(rem) if loc.is_dir || rem.is_dir => {
                items.push(SyncItem {
                    relative: rel.clone(),
                    action: SyncAction::Skip,
                    reason: "directory".into(),
                    size: 0,
                });
                skipped += 1;
            }
            Some(_rem) if loc.is_symlink => {
                items.push(SyncItem {
                    relative: rel.clone(),
                    action: SyncAction::Link,
                    reason: "symlink".into(),
                    size: 0,
                });
            }
            Some(rem) if loc.size == rem.size && loc.mtime.abs_diff(rem.mtime) <= 1 => {
                items.push(SyncItem {
                    relative: rel.clone(),
                    action: SyncAction::Skip,
                    reason: "size and mtime match".into(),
                    size: loc.size,
                });
                skipped += 1;
            }
            Some(rem) if loc.mtime >= rem.mtime => {
                items.push(SyncItem {
                    relative: rel.clone(),
                    action: SyncAction::Upload,
                    reason: "local is newer".into(),
                    size: loc.size,
                });
                uploads += loc.size;
            }
            Some(rem) if bidirectional => {
                items.push(SyncItem {
                    relative: rel.clone(),
                    action: SyncAction::Download,
                    reason: "remote is newer".into(),
                    size: rem.size,
                });
                downloads += rem.size;
            }
            Some(_) => {
                items.push(SyncItem {
                    relative: rel.clone(),
                    action: SyncAction::Skip,
                    reason: "remote is newer (one-way sync)".into(),
                    size: loc.size,
                });
                skipped += 1;
            }
        }
    }

    if bidirectional {
        for (rel, rem) in &remote {
            if !local.contains_key(rel) && !rem.is_dir {
                items.push(SyncItem {
                    relative: rel.clone(),
                    action: SyncAction::Download,
                    reason: "missing locally".into(),
                    size: rem.size,
                });
                downloads += rem.size;
            }
        }
    }

    Ok(SyncPlan { items, uploads, downloads, skipped })
}

pub async fn apply_sync(
    sftp: &SftpSession,
    local_root: &Path,
    remote_root: &str,
    plan: &SyncPlan,
    opts: &TransferOptions,
    job_id: &str,
    progress: Option<mpsc::UnboundedSender<TransferProgress>>,
) -> Result<u64> {
    let mut bytes = 0u64;
    for item in &plan.items {
        let local = local_root.join(&item.relative);
        let remote = join_remote(remote_root, &item.relative);
        match item.action {
            SyncAction::Upload => {
                if let Some(parent) = parent_remote(&remote) {
                    mkdir_p(sftp, &parent).await?;
                }
                if local.is_dir() {
                    mkdir_p(sftp, &remote).await?;
                } else {
                    bytes += upload_file(sftp, &local, &remote, opts, job_id, progress.clone()).await?;
                }
            }
            SyncAction::Download => {
                bytes += download_file(sftp, &remote, &local, opts, job_id, progress.clone()).await?;
            }
            SyncAction::Link => {
                if local.is_symlink() {
                    let target = fs::read_link(&local).await?;
                    let _ = sftp.remove_file(&remote).await;
                    sftp.symlink(target.to_string_lossy().as_ref(), &remote).await?;
                }
            }
            SyncAction::Skip => {}
        }
    }
    Ok(bytes)
}

#[derive(Clone)]
struct Node {
    size: u64,
    mtime: u64,
    is_dir: bool,
    is_symlink: bool,
}

fn walk_local(root: &Path) -> Result<std::collections::BTreeMap<String, Node>> {
    let mut map = std::collections::BTreeMap::new();
    if !root.exists() {
        return Ok(map);
    }
    for entry in WalkDir::new(root) {
        let entry = entry.map_err(|e| Error::msg(e.to_string()))?;
        let rel = entry
            .path()
            .strip_prefix(root)
            .unwrap_or(entry.path())
            .to_string_lossy()
            .replace('\\', "/");
        if rel.is_empty() {
            continue;
        }
        let meta = entry.metadata().map_err(|e| Error::msg(e.to_string()))?;
        map.insert(
            rel,
            Node {
                size: meta.len(),
                mtime: meta.modified().ok().map(to_unix).unwrap_or(0),
                is_dir: meta.is_dir(),
                is_symlink: meta.file_type().is_symlink(),
            },
        );
    }
    Ok(map)
}

async fn walk_remote(
    sftp: &SftpSession,
    root: &str,
    prefix: &str,
) -> Result<std::collections::BTreeMap<String, Node>> {
    let mut map = std::collections::BTreeMap::new();
    let entries = match list_dir(sftp, root).await {
        Ok(v) => v,
        Err(_) => return Ok(map),
    };
    for e in entries {
        let rel = if prefix.is_empty() { e.name.clone() } else { format!("{prefix}/{}", e.name) };
        map.insert(
            rel.clone(),
            Node { size: e.size, mtime: e.mtime, is_dir: e.is_dir, is_symlink: e.is_symlink },
        );
        if e.is_dir {
            let nested = Box::pin(walk_remote(sftp, &e.path, &rel)).await?;
            map.extend(nested);
        }
    }
    Ok(map)
}

fn emit(
    tx: &Option<mpsc::UnboundedSender<TransferProgress>>,
    job_id: &str,
    path: &str,
    transferred: u64,
    total: u64,
    direction: TransferDirection,
    finished: bool,
    error: Option<String>,
) {
    if let Some(tx) = tx {
        let _ = tx.send(TransferProgress {
            job_id: job_id.into(),
            path: path.into(),
            transferred,
            total,
            direction,
            finished,
            error,
            session_id: String::new(),
        });
    }
}

fn join_remote(base: &str, child: &str) -> String {
    let child = child.trim_start_matches('/');
    if base.is_empty() || base == "." {
        return child.to_string();
    }
    if base.ends_with('/') {
        format!("{base}{child}")
    } else {
        format!("{base}/{child}")
    }
}

fn parent_remote(path: &str) -> Option<String> {
    let path = path.trim_end_matches('/');
    path.rfind('/').map(|i| path[..i].to_string())
}

fn to_unix(time: SystemTime) -> u64 {
    time.duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn set_mtime(path: &Path, unix: u64) -> std::io::Result<()> {
    let dest = PathBuf::from(path);
    let ft = filetime_from_unix(unix);
    #[cfg(windows)]
    {
        let _ = dest;
        let _ = ft;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = dest;
        let _ = ft;
        Ok(())
    }
}

#[cfg(windows)]
fn filetime_from_unix(_unix: u64) -> std::fs::FileTimes {
    std::fs::FileTimes::new()
}

#[cfg(not(windows))]
fn filetime_from_unix(_unix: u64) -> () {}

pub fn parallel_slots(requested: usize) -> usize {
    requested.clamp(1, 8)
}

/// Open extra SFTP channels on the same SSH session for parallel file jobs.
pub async fn open_pool(
    opener: impl Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Arc<SftpSession>>> + Send>>,
    n: usize,
) -> Result<Vec<Arc<SftpSession>>> {
    let mut pool = Vec::new();
    for _ in 0..parallel_slots(n) {
        pool.push(opener().await?);
    }
    Ok(pool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_and_parent() {
        assert_eq!(join_remote("/var", "log/syslog"), "/var/log/syslog");
        assert_eq!(parent_remote("/var/log/syslog").as_deref(), Some("/var/log"));
    }

    #[test]
    fn local_delete_list_children_first() {
        let dir = std::env::temp_dir().join(format!("ferra-del-{}", std::process::id()));
        let nested = dir.join("a");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("f.txt"), b"x").unwrap();
        let list = collect_local_delete(&dir).unwrap();
        assert!(list.last().is_some_and(|(p, is_dir)| p == &dir && *is_dir));
        assert!(list.iter().any(|(p, is_dir)| !is_dir && p.ends_with("f.txt")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
