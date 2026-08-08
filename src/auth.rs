//! Phira account authentication and user data management.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Phira API base URL.
pub const API_URL: &str = "https://phira.5wyxi.com";

/// Default multiplayer server address.
pub const DEFAULT_MP_ADDRESS: &str = "mp2.phira.cn:12345";

/// A logged-in Phira user profile.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    pub id: i32,
    pub name: String,
    pub email: String,
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub rks: f32,
    #[serde(default)]
    pub badge: Option<String>,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub exp: i64,
    #[serde(default)]
    pub roles: i32,
    #[serde(default)]
    pub joined: Option<String>,
    #[serde(default)]
    pub last_login: Option<String>,
}

/// Token pair returned by the login endpoint.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tokens {
    pub token: String,
    pub refresh_token: String,
}

/// The launcher's persisted auth + settings data.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LauncherData {
    #[serde(default)]
    pub me: Option<User>,
    #[serde(default)]
    pub tokens: Option<Tokens>,
    #[serde(default)]
    pub mp_address: String,
    #[serde(default)]
    pub mp_enabled: bool,
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub theme: i32,
    #[serde(default)]
    pub player_name: String,
    #[serde(default)]
    pub player_rks: f32,
}

impl Default for LauncherData {
    fn default() -> Self {
        Self {
            me: None,
            tokens: None,
            mp_address: DEFAULT_MP_ADDRESS.to_string(),
            mp_enabled: false,
            language: "zh-CN".to_string(),
            theme: 0,
            player_name: String::new(),
            player_rks: 0.0,
        }
    }
}

/// State of an in-flight login request.
#[derive(Clone)]
pub enum LoginState {
    Idle,
    Loading,
    Success,
    Failed(String),
}

/// Manages authentication state and persisted launcher data.
pub struct AuthManager {
    base_dir: PathBuf,
    data_path: PathBuf,
    pub data: LauncherData,
    pub login_state: Arc<Mutex<LoginState>>,
}

impl AuthManager {
    pub fn new(base_dir: PathBuf) -> Self {
        let data_dir = base_dir.join("data");
        let data_path = data_dir.join("data.json");
        let data = load_launcher_data(&data_path).unwrap_or_default();
        Self {
            base_dir,
            data_path,
            data,
            login_state: Arc::new(Mutex::new(LoginState::Idle)),
        }
    }

    pub fn reload(&mut self) -> anyhow::Result<()> {
        self.data = load_launcher_data(&self.data_path)?;
        Ok(())
    }


    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.data_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.data)?;
        fs::write(&self.data_path, json)?;
        Ok(())
    }

    pub fn is_logged_in(&self) -> bool {
        self.data.me.is_some() && self.data.tokens.is_some()
    }

    pub fn start_login(&self, email: String, password: String, ctx: egui::Context) {
        {
            let mut state = self.login_state.lock().unwrap();
            *state = LoginState::Loading;
        }
        let login_state = Arc::clone(&self.login_state);
        let data_path = self.data_path.clone();
        let data = self.data.clone();
        std::thread::spawn(move || {
            let result = do_login(&email, &password);
            let mut state = login_state.lock().unwrap();
            match result {
                Ok((user, tokens)) => {
                    let mut new_data = data;
                    let player_name = user.name.clone();
                    let player_rks = user.rks;
                    new_data.me = Some(user);
                    new_data.tokens = Some(tokens);
                    new_data.player_name = player_name;
                    new_data.player_rks = player_rks;
                    let _ = save_launcher_data(&data_path, &new_data);
                    *state = LoginState::Success;
                }
                Err(e) => {
                    *state = LoginState::Failed(e.to_string());
                }
            }
            drop(state);
            ctx.request_repaint();
        });
    }

    pub fn logout(&mut self) {
        self.data.me = None;
        self.data.tokens = None;
        self.data.player_name = String::new();
        self.data.player_rks = 0.0;
        let _ = self.save();
        *self.login_state.lock().unwrap() = LoginState::Idle;
    }


    pub fn sync_to_versions(&self) -> anyhow::Result<usize> {
        let versions_dir = self.base_dir.join("versions");
        if !versions_dir.is_dir() {
            return Ok(0);
        }

        let payload = build_phira_data_json(&self.data);
        let payload_str = serde_json::to_string_pretty(&payload)?;

        let mut count = 0;
        if let Ok(repos) = fs::read_dir(&versions_dir) {
            for repo_entry in repos.flatten() {
                let repo_path = repo_entry.path();
                if !repo_path.is_dir() {
                    continue;
                }
                if let Ok(tags) = fs::read_dir(&repo_path) {
                    for tag_entry in tags.flatten() {
                        let tag_path = tag_entry.path();
                        if !tag_path.is_dir() {
                            continue;
                        }
                        let data_dir = tag_path.join("data");
                        if let Err(e) = fs::create_dir_all(&data_dir) {
                            eprintln!("failed to create {}: {e}", data_dir.display());
                            continue;
                        }
                        let target = data_dir.join("data.json");
                        if fs::write(&target, &payload_str).is_ok() {
                            count += 1;
                        }
                    }
                }
            }
        }
        Ok(count)
    }
}

fn do_login(email: &str, password: &str) -> anyhow::Result<(User, Tokens)> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("PhiLauncher/1.0")
        .timeout(std::time::Duration::from_secs(30))
        .build()?;

    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct LoginReq<'a> {
        email: &'a str,
        password: &'a str,
        client_version: &'a str,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct LoginResp {
        #[allow(dead_code)]
        id: i32,
        token: String,
        refresh_token: String,
    }

    let resp = client
        .post(format!("{API_URL}/login"))
        .json(&LoginReq {
            email,
            password,
            client_version: "0.1.0",
        })
        .send()?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().unwrap_or_default();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(err) = v["error"].as_str() {
                anyhow::bail!("登录失败 ({status}): {err}");
            }
        }
        anyhow::bail!("登录失败，状态码 {status}");
    }

    let login_resp: LoginResp = resp.json()?;
    let tokens = Tokens {
        token: login_resp.token.clone(),
        refresh_token: login_resp.refresh_token,
    };

    let me_resp = client
        .get(format!("{API_URL}/me"))
        .bearer_auth(&login_resp.token)
        .send()?;

    if !me_resp.status().is_success() {
        anyhow::bail!("获取用户信息失败，状态码 {}", me_resp.status());
    }

    let user: User = me_resp.json()?;
    Ok((user, tokens))
}

fn load_launcher_data(path: &Path) -> anyhow::Result<LauncherData> {
    if !path.exists() {
        return Ok(LauncherData::default());
    }
    let data = fs::read_to_string(path)?;
    let v: LauncherData = serde_json::from_str(&data)?;
    Ok(v)
}


fn save_launcher_data(path: &Path, data: &LauncherData) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(data)?;
    fs::write(path, json)?;
    Ok(())
}


fn build_phira_data_json(data: &LauncherData) -> serde_json::Value {
    let mut map = serde_json::Map::new();

    // me
    map.insert("me".to_string(), serde_json::to_value(&data.me).unwrap_or(serde_json::Value::Null));

    // charts
    map.insert("charts".to_string(), serde_json::Value::Array(vec![]));

    // local_records
    map.insert("local_records".to_string(), serde_json::Value::Object(serde_json::Map::new()));

    // config - 完整的配置对象
    let mut config = serde_json::Map::new();
    config.insert("adjust_time_new".to_string(), serde_json::Value::Bool(false));
    config.insert("aggressive".to_string(), serde_json::Value::Bool(true));
    config.insert("apFcIndicator".to_string(), serde_json::Value::Bool(true));
    config.insert("fullScreenJudge".to_string(), serde_json::Value::Bool(false));
    config.insert("comboTextDebug".to_string(), serde_json::Value::Bool(false));
    config.insert("customComboText".to_string(), serde_json::Value::String("COMBO".to_string()));
    config.insert("customWatermark".to_string(), serde_json::Value::String("phirLte".to_string()));
    config.insert("aspectRatio".to_string(), serde_json::Value::Null);
    config.insert("audioBufferSize".to_string(), serde_json::Value::Null);
    config.insert("chartDebug".to_string(), serde_json::Value::Bool(false));
    config.insert("romanNumerals".to_string(), serde_json::Value::Bool(false));
    config.insert("chineseNumerals".to_string(), serde_json::Value::Bool(false));
    config.insert("autoplayDisplayText".to_string(), serde_json::Value::String("Autoplay".to_string()));
    config.insert("disableEffect".to_string(), serde_json::Value::Bool(false));
    config.insert("doubleClickToPause".to_string(), serde_json::Value::Bool(true));
    config.insert("doubleHint".to_string(), serde_json::Value::Bool(true));
    config.insert("fullscreenMode".to_string(), serde_json::Value::Bool(false));
    config.insert("fxaa".to_string(), serde_json::Value::Bool(false));
    config.insert("interactive".to_string(), serde_json::Value::Bool(true));
    config.insert("mods".to_string(), serde_json::Value::String("".to_string()));
    config.insert("mpAddress".to_string(), serde_json::Value::String(data.mp_address.clone()));
    config.insert("mpEnabled".to_string(), serde_json::Value::Bool(data.mp_enabled));
    config.insert("noteScale".to_string(), serde_json::json!(1.0));
    config.insert("offlineMode".to_string(), serde_json::Value::Bool(false));
    config.insert("offset".to_string(), serde_json::json!(0.0));
    config.insert("particle".to_string(), serde_json::Value::Bool(true));
    config.insert("playerName".to_string(), serde_json::Value::String(data.player_name.clone()));
    config.insert("playerRks".to_string(), serde_json::json!(data.player_rks));
    config.insert("preferredSampleRate".to_string(), serde_json::Value::Null);
    config.insert("resPackPath".to_string(), serde_json::Value::Null);
    config.insert("sampleCount".to_string(), serde_json::Value::Number(1.into()));
    config.insert("showAcc".to_string(), serde_json::Value::Bool(false));
    config.insert("showAvgFps".to_string(), serde_json::Value::Bool(false));
    config.insert("speed".to_string(), serde_json::json!(1.0));
    config.insert("touchDebug".to_string(), serde_json::Value::Bool(false));
    config.insert("useKeyboard".to_string(), serde_json::Value::Bool(false));
    config.insert("volumeBgm".to_string(), serde_json::json!(1.0));
    config.insert("volumeMusic".to_string(), serde_json::json!(1.0));
    config.insert("volumeSfx".to_string(), serde_json::json!(1.0));
    config.insert("autoplay".to_string(), serde_json::Value::Null);
    map.insert("config".to_string(), serde_json::Value::Object(config));

    // message_check_time
    map.insert("message_check_time".to_string(), serde_json::Value::Null);

    // language
    map.insert("language".to_string(), serde_json::Value::String(data.language.clone()));

    // theme
    map.insert("theme".to_string(), serde_json::Value::Number(data.theme.into()));

    // tokens - 与官方一致，设为 null
    map.insert("tokens".to_string(), serde_json::Value::Null);

    // respacks
    map.insert("respacks".to_string(), serde_json::Value::Array(vec![]));

    // respack_id
    map.insert("respack_id".to_string(), serde_json::Value::Number(0.into()));

    // accept_invalid_cert
    map.insert("accept_invalid_cert".to_string(), serde_json::Value::Bool(false));

    // read_tos_and_policy
    map.insert("read_tos_and_policy".to_string(), serde_json::Value::Bool(false));

    // terms_modified
    map.insert("terms_modified".to_string(), serde_json::Value::String("Thu, 05 Dec 2024 09:24:29 GMT".to_string()));

    // ignored_version
    map.insert("ignored_version".to_string(), serde_json::Value::Null);

    // character
    let mut character = serde_json::Map::new();
    character.insert("id".to_string(), serde_json::Value::String("shee".to_string()));
    character.insert("name".to_string(), serde_json::Value::String("夕".to_string()));
    character.insert("intro".to_string(), serde_json::Value::String(
        "自断壁残垣中传来的歌声，被繁复乐章所萦绕的，韵律的形状。仿佛奇迹本身，无法用一切已知定律刻画的谜之少女。\n来自未来，却让人觉着似乎是某种与世间本质关联、恒久的存在。无妨，一切无形和有形的附着已随少女的记忆模糊淡去，不见所踪。\n翩翩然如禽羽的她，在声乐交织的梦中，看见的又是什么呢？".to_string()
    ));
    character.insert("illust".to_string(), serde_json::Value::String("@".to_string()));
    character.insert("artist".to_string(), serde_json::Value::String("清水QR".to_string()));
    character.insert("designer".to_string(), serde_json::Value::String("清水QR".to_string()));
    character.insert("name_size".to_string(), serde_json::Value::Null);
    character.insert("baseline".to_string(), serde_json::Value::Bool(false));
    character.insert("illu_adjust".to_string(), serde_json::json!([0.0, 0.0, 0.0, 0.0]));
    map.insert("character".to_string(), serde_json::Value::Object(character));

    // enable_anys
    map.insert("enable_anys".to_string(), serde_json::Value::Bool(false));

    // anys_gateway
    map.insert("anys_gateway".to_string(), serde_json::Value::String("".to_string()));

    // prefer_reduced_motion
    map.insert("prefer_reduced_motion".to_string(), serde_json::Value::Bool(false));

    // custom_bgm_path
    map.insert("custom_bgm_path".to_string(), serde_json::Value::Null);

    // collections
    map.insert("collections".to_string(), serde_json::Value::Array(vec![]));

    // collection_uuids
    map.insert("collection_uuids".to_string(), serde_json::json!(["7c7063c8-8e9d-4bd8-8a40-7b0754989264"]));

    // import_scan_retry
    map.insert("import_scan_retry".to_string(), serde_json::Value::Object(serde_json::Map::new()));

    serde_json::Value::Object(map)
}