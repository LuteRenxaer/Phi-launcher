//! Download + install + launch manager, plus manually-added local versions.
//!
//! Each download runs on its own thread, streaming bytes to disk while
//! updating a shared progress record. Zip archives are auto-extracted; a
//! launchable `.exe` is then discovered so the version can be started.
//!
//! Manually-added local versions are persisted in `config/local_versions.json`
//! so they survive restarts.

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};


use crate::github::{self, Asset, Release};

/// Progress / lifecycle status of a single download.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum Status {
    Queued,
    Downloading { received: u64, total: u64 },
    Extracting,
    Installed(PathBuf),
    Failed(String),
}

#[derive(Clone)]
pub struct Entry {
    pub status: Status,
}

/// A manually-added local version.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalVersion {
    /// Which category / repo this belongs to (e.g. "phira").
    pub repo: String,
    /// Display tag / version string (e.g. "v0.8.2-local").
    pub tag: String,
    /// Optional display name override.
    #[serde(default)]
    pub name: Option<String>,
    /// Path to the executable.
    pub exe_path: PathBuf,
}

/// Manages downloads keyed by a unique id (`repo|tag|asset`).
pub struct DownloadManager {
    base_dir: PathBuf,
    entries: Arc<Mutex<HashMap<String, Entry>>>,
    local: Vec<LocalVersion>,
    local_config_path: PathBuf,
}

impl DownloadManager {
    pub fn new(base_dir: PathBuf) -> Self {
        let config_dir = base_dir.join("config");
        let local_config_path = config_dir.join("local_versions.json");
        let local = load_local(&local_config_path).unwrap_or_default();
        Self {
            base_dir,
            entries: Arc::new(Mutex::new(HashMap::new())),
            local,
            local_config_path,
        }
    }

    pub fn key(repo: &str, tag: &str, asset: &str) -> String {
        format!("{repo}|{tag}|{asset}")
    }

    /// Directory where a given (repo, tag) is installed.
    pub fn install_dir(&self, repo: &str, tag: &str) -> PathBuf {
        self.base_dir
            .join("versions")
            .join(sanitize(repo))
            .join(sanitize(tag))
    }

    pub fn status(&self, key: &str) -> Option<Status> {
        self.entries.lock().unwrap().get(key).map(|e| e.status.clone())
    }

    /// Is this (repo, tag) already installed on disk (folder exists & non-empty)?
    pub fn is_installed(&self, repo: &str, tag: &str) -> bool {
        let dir = self.install_dir(repo, tag);
        dir.is_dir()
            && fs::read_dir(&dir)
                .map(|mut it| it.next().is_some())
                .unwrap_or(false)
    }

    /// ✅ 新增：检查特定资产是否已安装（文件是否存在）
    pub fn is_asset_installed(&self, repo: &str, tag: &str, asset_name: &str) -> bool {
        let dir = self.install_dir(repo, tag);
        let file_path = dir.join(sanitize(asset_name));
        file_path.exists()
    }

    /// Find an executable inside a (repo, tag) install directory.
    pub fn find_executable(&self, repo: &str, tag: &str) -> Option<PathBuf> {
        find_exe(&self.install_dir(repo, tag))
    }

    /// Begin downloading `url` for (repo, tag, asset_name).
    pub fn start(
        &self,
        repo: &str,
        tag: &str,
        asset_name: &str,
        url: &str,
        ctx: egui::Context,
    ) {
        let key = Self::key(repo, tag, asset_name);
        let install_dir = self.install_dir(repo, tag);
        let file_path = install_dir.join(sanitize(asset_name));

        // ✅ 检查文件是否已存在
        if file_path.exists() {
            let mut map = self.entries.lock().unwrap();
            map.insert(key.clone(), Entry { status: Status::Installed(install_dir) });
            return;
        }

        {
            let mut map = self.entries.lock().unwrap();
            if let Some(e) = map.get(&key) {
                if matches!(
                    e.status,
                    Status::Downloading { .. } | Status::Extracting | Status::Queued
                ) {
                    return;
                }
            }
            map.insert(key.clone(), Entry { status: Status::Queued });
        }

        let entries = Arc::clone(&self.entries);
        let dir = self.install_dir(repo, tag);
        let url = url.to_string();
        let asset_name = asset_name.to_string();

        thread::spawn(move || {
            let result = run_download(&url, &asset_name, &dir, &key, &entries, &ctx);
            let mut map = entries.lock().unwrap();
            let status = match result {
                Ok(path) => Status::Installed(path),
                Err(e) => Status::Failed(e.to_string()),
            };
            map.insert(key.clone(), Entry { status });
            drop(map);
            ctx.request_repaint();
        });
    }

    /// ✅ 新增：删除单个资产文件（不删除整个版本文件夹）
    pub fn uninstall_asset(&self, repo: &str, tag: &str, asset_name: &str) -> anyhow::Result<()> {
        let dir = self.install_dir(repo, tag);
        let file_path = dir.join(sanitize(asset_name));

        if file_path.exists() {
            fs::remove_file(&file_path)?;
        }

        // 从状态表中移除该资产的记录
        let key = Self::key(repo, tag, asset_name);
        self.entries.lock().unwrap().remove(&key);

        Ok(())
    }

    /// 删除整个版本文件夹
    pub fn uninstall(&self, repo: &str, tag: &str) -> anyhow::Result<()> {
        let dir = self.install_dir(repo, tag);
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        let prefix = format!("{repo}|{tag}|");
        self.entries.lock().unwrap().retain(|k, _| !k.starts_with(&prefix));
        Ok(())
    }

    // ---- local versions ----

    pub fn local_versions(&self, repo: &str) -> Vec<&LocalVersion> {
        self.local.iter().filter(|v| v.repo == repo).collect()
    }

    pub fn add_local(&mut self, v: LocalVersion) -> anyhow::Result<()> {
        self.local.push(v);
        self.save_local()
    }

    pub fn remove_local(&mut self, repo: &str, tag: &str) -> anyhow::Result<()> {
        self.local.retain(|v| !(v.repo == repo && v.tag == tag));
        self.save_local()
    }

    fn save_local(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.local_config_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.local)?;
        fs::write(&self.local_config_path, json)?;
        Ok(())
    }

    // ---- 新增：与 github.rs 集成 ----

    /// 获取指定 Category 的所有 Windows 版本并自动下载最新版
    pub fn fetch_and_download_latest(
        &self,
        cat: &github::Category,
        ctx: egui::Context,
    ) -> anyhow::Result<()> {
        let releases = fetch_releases_sync(cat)?;

        if releases.is_empty() {
            anyhow::bail!("没有找到任何 Release");
        }

        let release = &releases[0];
        let tag = &release.tag_name;
        let repo = cat.repo;

        let windows_assets: Vec<&Asset> = release
            .assets
            .iter()
            .filter(|a| is_windows_asset(&a.name))
            .collect();

        if windows_assets.is_empty() {
            anyhow::bail!("当前版本没有 Windows 资产");
        }

        for asset in windows_assets {
            let asset_name = &asset.name;
            let url = &asset.download_url;

            // ✅ 改为检查具体资产
            if self.is_asset_installed(repo, tag, asset_name) {
                continue;
            }

            self.start(repo, tag, asset_name, url, ctx.clone());
        }

        Ok(())
    }

    /// 手动下载指定 Release 的某个资产
    pub fn download_release_asset(
        &self,
        repo: &str,
        release: &Release,
        asset: &Asset,
        ctx: egui::Context,
    ) {
        let tag = &release.tag_name;
        let asset_name = &asset.name;
        let url = &asset.download_url;

        self.start(repo, tag, asset_name, url, ctx);
    }
}

// ─── 辅助函数 ────────────────────────────────────────────────────────

fn load_local(path: &Path) -> anyhow::Result<Vec<LocalVersion>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let data = fs::read_to_string(path)?;
    let v: Vec<LocalVersion> = serde_json::from_str(&data)?;
    Ok(v)
}

fn run_download(
    url: &str,
    asset_name: &str,
    dir: &Path,
    key: &str,
    entries: &Arc<Mutex<HashMap<String, Entry>>>,
    ctx: &egui::Context,
) -> anyhow::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let file_path = dir.join(sanitize(asset_name));

    let client = reqwest::blocking::Client::builder()
        .user_agent("PhiLauncher/0.1 (+https://github.com)")
        .timeout(std::time::Duration::from_secs(60 * 30))
        .build()?;
    let mut resp = client.get(url).send()?;
    if !resp.status().is_success() {
        anyhow::bail!("下载失败，状态码 {}", resp.status());
    }
    let total = resp.content_length().unwrap_or(0);

    let mut file = fs::File::create(&file_path)?;
    let mut buf = [0u8; 64 * 1024];
    let mut received: u64 = 0;
    let mut last_paint = std::time::Instant::now();
    loop {
        let n = resp.read(&mut buf)?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        received += n as u64;
        if last_paint.elapsed().as_millis() > 60 {
            set_status(entries, key, Status::Downloading { received, total });
            ctx.request_repaint();
            last_paint = std::time::Instant::now();
        }
    }
    file.flush()?;
    drop(file);

    // Auto-extract zip archives.
    if asset_name.to_lowercase().ends_with(".zip") {
        set_status(entries, key, Status::Extracting);
        ctx.request_repaint();
        extract_zip(&file_path, dir)?;
    }

    Ok(dir.to_path_buf())
}

fn set_status(entries: &Arc<Mutex<HashMap<String, Entry>>>, key: &str, status: Status) {
    if let Some(e) = entries.lock().unwrap().get_mut(key) {
        e.status = status;
    }
}

fn extract_zip(zip_path: &Path, dest: &Path) -> anyhow::Result<()> {
    let file = fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let out = match entry.enclosed_name() {
            Some(p) => dest.join(p),
            None => continue,
        };
        if entry.is_dir() {
            fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out_file = fs::File::create(&out)?;
            std::io::copy(&mut entry, &mut out_file)?;
        }
    }
    Ok(())
}

/// Recursively look for the most likely launchable `.exe`.
fn find_exe(dir: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    collect_exe(dir, &mut candidates, 0);
    candidates.sort_by_key(|p| {
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_lowercase();
        let mut score = 100i32;
        if name.contains("phi") {
            score -= 40;
        }
        if name.contains("uninstall") || name.contains("unins") {
            score += 100;
        }
        if name.contains("setup") || name.contains("install") {
            score += 30;
        }
        score
    });
    candidates.into_iter().next()
}

fn collect_exe(dir: &Path, out: &mut Vec<PathBuf>, depth: usize) {
    if depth > 4 {
        return;
    }
    let Ok(rd) = fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_exe(&path, out, depth + 1);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("exe"))
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
}

/// Launch an installed program (or open the folder if no exe found).
pub fn launch(path: &Path) -> anyhow::Result<()> {
    let dir = path.parent().unwrap_or(path);
    let mut cmd = std::process::Command::new(path);
    cmd.current_dir(dir);
    cmd.spawn()?;
    Ok(())
}

/// Replace filesystem-unfriendly characters.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

/// Format a byte count as a human readable string.
pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.1} {}", UNITS[i])
}

/// ─── Windows 资产过滤（宽松模式）────────────────────────────────────
/// 优先匹配带 win/windows 的，如果没有则回退到所有 .exe
pub fn is_windows_asset(name: &str) -> bool {
    let n = name.to_lowercase();

    if n.ends_with(".apk") || n.ends_with(".ipa") || n.ends_with(".dmg") || n.ends_with(".deb") {
        return false;
    }

    if n.ends_with(".exe") {
        return true;
    }

    if n.ends_with(".zip") || n.ends_with(".7z") {
        return n.contains("win") || n.contains("windows");
    }

    false
}

/// 别名，保持与 app.rs 兼容
pub fn is_relevant_asset(name: &str) -> bool {
    is_windows_asset(name)
}

/// ─── 同步获取 Releases（供 DownloadManager 内部使用）─────────────
fn fetch_releases_sync(cat: &github::Category) -> anyhow::Result<Vec<Release>> {
    let url = cat.releases_url();
    let client = reqwest::blocking::Client::builder()
        .user_agent("PhiLauncher/0.1 (+https://github.com)")
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()?;
    if !resp.status().is_success() {
        anyhow::bail!("GitHub API 请求失败，状态码 {}", resp.status());
    }
    let releases: Vec<Release> = resp.json()?;
    Ok(releases)
}