//! GitHub Releases API client.
//!
//! Fetches release lists for each Phi-simulator repository. Runs on a
//! background thread so the UI never blocks; results are pushed back into a
//! shared [`FetchState`] guarded by a mutex.

use serde::Deserialize;
use std::sync::{Arc, Mutex};
use std::thread;

/// A category shown in the launcher (one GitHub repository).
#[derive(Clone)]
pub struct Category {
    /// Display name shown in the sidebar / tab.
    pub name: &'static str,
    /// Short tagline describing this fork.
    pub tagline: &'static str,
    /// GitHub owner (user / org).
    pub owner: &'static str,
    /// GitHub repository name.
    pub repo: &'static str,
}

impl Category {
    pub fn releases_url(&self) -> String {
        format!(
            "https://api.github.com/repos/{}/{}/releases?per_page=30",
            self.owner, self.repo
        )
    }

    pub fn page_url(&self) -> String {
        format!("https://github.com/{}/{}/releases", self.owner, self.repo)
    }
}

/// The four categorized Phi-simulator sources.
pub fn categories() -> Vec<Category> {
    vec![
        Category {
            name: "Phira",
            tagline: "官方本体 · TeamFlos",
            owner: "TeamFlos",
            repo: "phira",
        },
        Category {
            name: "Phira-Firefly",
            tagline: "Firefly 分支 · tiancra",
            owner: "tiancra",
            repo: "Phira-Firefly",
        },
        Category {
            name: "Phi-Recorder",                               // ✅ 修改
            tagline: "Phi-Recorder 分支 · 2278535805",         // ✅ 修改
            owner: "2278535805",
            repo: "Phi-Recorder",                               // ✅ 修改
        },
        Category {
            name: "PhirLie",
            tagline: "PhirLte 分支 · LuteRenxaer",
            owner: "LuteRenxaer",
            repo: "PhirLte",
        },
    ]
}

/// A single downloadable asset attached to a release.
#[derive(Clone, Debug, Deserialize)]
pub struct Asset {
    pub name: String,
    #[serde(rename = "browser_download_url")]
    pub download_url: String,
    #[serde(default)]
    pub size: u64,
}

/// A GitHub release.
#[derive(Clone, Debug, Deserialize)]
pub struct Release {
    #[serde(default)]
    pub name: Option<String>,
    pub tag_name: String,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

impl Release {
    /// Nice display title for the release.
    pub fn title(&self) -> String {
        match &self.name {
            Some(n) if !n.trim().is_empty() => n.clone(),
            _ => self.tag_name.clone(),
        }
    }

    /// Just the date part of `published_at` (YYYY-MM-DD).
    pub fn date(&self) -> String {
        self.published_at
            .as_deref()
            .and_then(|s| s.split('T').next())
            .unwrap_or("")
            .to_string()
    }
}

/// Loading state for one category's release list.
#[derive(Default)]
pub enum FetchState {
    #[default]
    Idle,
    Loading,
    Loaded(Vec<Release>),
    Error(String),
}

/// Kick off a background fetch, storing the outcome into `state`.
pub fn fetch_releases(cat: &Category, state: Arc<Mutex<FetchState>>, ctx: egui::Context) {
    {
        let mut guard = state.lock().unwrap();
        if matches!(*guard, FetchState::Loading) {
            return;
        }
        *guard = FetchState::Loading;
    }
    let url = cat.releases_url();
    thread::spawn(move || {
        let result = do_fetch(&url);
        let mut guard = state.lock().unwrap();
        *guard = match result {
            Ok(rs) => FetchState::Loaded(rs),
            Err(e) => FetchState::Error(e.to_string()),
        };
        drop(guard);
        ctx.request_repaint();
    });
}

fn do_fetch(url: &str) -> anyhow::Result<Vec<Release>> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("PhiLauncher/0.1 (+https://github.com)")
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let resp = client
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .send()?;
    if !resp.status().is_success() {
        anyhow::bail!("GitHub 返回状态码 {}", resp.status());
    }
    let releases: Vec<Release> = resp.json()?;
    Ok(releases)
}