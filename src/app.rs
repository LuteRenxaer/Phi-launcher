//! Phi Launcher application (egui UI, state, event handling).

use std::sync::{Arc, Mutex};
use std::time::Instant;

use egui::{
    Align, Color32, FontFamily, FontId, Layout, RichText, Rounding, Stroke, TextureHandle, Vec2,
};

use crate::audio::Audio;
use crate::auth::{AuthManager, LoginState};
use crate::download::{self, DownloadManager, LocalVersion, Status};
use crate::github::{self, Category, FetchState};
use crate::{assets, theme};

/// Which main view is currently displayed in the central panel.
#[derive(PartialEq, Clone, Copy)]
enum View {
    /// Version browser (the default view).
    Versions,
    /// Settings panel (login, mp server, etc.).
    Settings,
    /// About panel.
    About,
}

pub struct PhiLauncher {
    categories: Vec<Category>,
    selected: usize,
    fetch_states: Vec<Arc<Mutex<FetchState>>>,
    downloads: DownloadManager,
    audio: Audio,
    bg_texture: Option<TextureHandle>,
    logo_texture: Option<TextureHandle>,
    show_prerelease: bool,
    toast: Option<(String, Instant)>,
    /// Pending local-version file picker result, if any.
    pending_pick: Option<std::sync::mpsc::Receiver<Option<std::path::PathBuf>>>,
    /// Current active view.
    view: View,
    /// Auth manager (login state, user data, sync).
    auth: AuthManager,
    /// Cached avatar texture, if loaded.
    avatar_texture: Option<TextureHandle>,
    /// Login form inputs.
    login_email: String,
    login_password: String,
    /// Whether an avatar load is in flight (to avoid re-fetching every frame).
    avatar_loading: bool,
    /// 启动闪屏控制
    show_splash: bool,
}

impl PhiLauncher {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let ctx = &cc.egui_ctx;
        let assets_dir = assets::assets_dir();
        let base_dir = assets::base_dir();

        assets::install_fonts(ctx, &assets_dir);
        theme::apply(ctx);

        let bg_texture = assets::load_color_image(&assets_dir.join("background.jpg"))
            .map(|img| ctx.load_texture("bg", img, egui::TextureOptions::LINEAR));
        let logo_texture = assets::load_color_image(&assets_dir.join("icon.png"))
            .map(|img| ctx.load_texture("logo", img, egui::TextureOptions::LINEAR));

        let categories = github::categories();
        let fetch_states = categories
            .iter()
            .map(|_| Arc::new(Mutex::new(FetchState::Idle)))
            .collect();

        let auth = AuthManager::new(base_dir.clone());

        Self {
            categories,
            selected: 0,
            fetch_states,
            downloads: DownloadManager::new(base_dir),
            audio: Audio::new(&assets_dir),
            bg_texture,
            logo_texture,
            show_prerelease: true,
            toast: None,
            pending_pick: None,
            view: View::Versions,
            auth,
            avatar_texture: None,
            login_email: String::new(),
            login_password: String::new(),
            avatar_loading: false,
            show_splash: true,
        }
    }

    fn ensure_fetch(&self, index: usize, ctx: &egui::Context) {
        let state = &self.fetch_states[index];
        let idle = matches!(*state.lock().unwrap(), FetchState::Idle);
        if idle {
            github::fetch_releases(&self.categories[index], Arc::clone(state), ctx.clone());
        }
    }

    /// Poll the pending file picker, if any, and add the chosen exe as a local version.
    fn poll_pending_pick(&mut self, repo: &str) {
        let Some(rx) = self.pending_pick.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Some(path)) => {
                let tag = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("本地版本")
                    .to_string();
                let name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("本地版本")
                    .to_string();
                let v = LocalVersion {
                    repo: repo.to_string(),
                    tag,
                    name: Some(name),
                    exe_path: path.clone(),
                };
                match self.downloads.add_local(v) {
                    Ok(_) => self.set_toast(format!("已添加本地版本 {}", path.display())),
                    Err(e) => self.set_toast(format!("添加失败: {e}")),
                }
                self.pending_pick = None;
            }
            Ok(None) => {
                self.pending_pick = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.pending_pick = None;
            }
        }
    }

    /// Open a native file picker to add a local exe for the given repo.
    fn start_add_local(&mut self, repo: &str) {
        if self.pending_pick.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let repo = repo.to_string();
        let _ = repo;
        std::thread::spawn(move || {
            let path = rfd::FileDialog::new()
                .add_filter("可执行文件", &["exe"])
                .add_filter("所有文件", &["*"])
                .set_title("选择本地 Phigros 模拟器 exe")
                .pick_file();
            let _ = tx.send(path);
        });
        self.pending_pick = Some(rx);
    }

    /// Load the user's avatar from URL into a texture (synchronous).
    fn try_load_avatar(&mut self, ctx: &egui::Context) {
        if self.avatar_loading || self.avatar_texture.is_some() {
            return;
        }
        let Some(user) = self.auth.data.me.as_ref() else {
            return;
        };
        let Some(avatar_url) = user.avatar.as_ref() else {
            return;
        };
        if avatar_url.is_empty() {
            return;
        }
        self.avatar_loading = true;
        if let Ok(bytes) = download_avatar(avatar_url) {
            if let Ok(img) = image::load_from_memory(&bytes) {
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let color_image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                let tex = ctx.load_texture("avatar", color_image, egui::TextureOptions::LINEAR);
                self.avatar_texture = Some(tex);
            }
        }
        self.avatar_loading = false;
    }

    /// 显示启动闪屏（居中 Logo）
    fn show_splash_screen(&mut self, ctx: &egui::Context) {
        let painter = ctx.layer_painter(egui::LayerId::background());
        let rect = ctx.screen_rect();
        painter.rect_filled(rect, Rounding::ZERO, Color32::from_rgb(8, 14, 24));

        if let Some(logo) = &self.logo_texture {
            let size = 256.0;
            let center = rect.center();
            let image_rect = egui::Rect::from_center_size(center, Vec2::splat(size));

            painter.image(
                logo.id(),
                image_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }

        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("splash_text")));
        let text = "加载中...";
        let font_id = FontId::new(18.0, FontFamily::Proportional);
        let text_color = Color32::from_rgb(150, 180, 210);
        let galley = painter.layout(text.to_string(), font_id, text_color, f32::INFINITY);
        let text_pos = egui::pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y + 160.0,
        );
        painter.galley(text_pos, galley, text_color);
    }
}

fn download_avatar(url: &str) -> Result<Vec<u8>, reqwest::Error> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let resp = client.get(url).send()?;
    if !resp.status().is_success() {
        return Err(resp.error_for_status().unwrap_err());
    }
    Ok(resp.bytes()?.to_vec())
}

fn brand_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(assets::BRAND_FAMILY.into()))
}

impl eframe::App for PhiLauncher {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.03, 0.06, 0.10, 1.0]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // 闪屏：第一帧显示 Logo，第二帧自动消失
        if self.show_splash {
            self.show_splash_screen(ctx);
            ctx.request_repaint();
            self.show_splash = false;
            return;
        }

        // --- 正常主界面 ---
        if self.view == View::Versions {
            self.ensure_fetch(self.selected, ctx);
        }

        if self.auth.is_logged_in() && self.avatar_texture.is_none() && !self.avatar_loading {
            self.try_load_avatar(ctx);
        }

        if let Some(tex) = &self.bg_texture {
            let painter = ctx.layer_painter(egui::LayerId::background());
            let rect = ctx.screen_rect();
            painter.image(
                tex.id(),
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            painter.rect_filled(rect, Rounding::ZERO, Color32::from_black_alpha(150));
        }

        self.top_bar(ctx);
        self.side_panel(ctx);
        self.central(ctx);
        self.toast_overlay(ctx);
    }
}

impl PhiLauncher {
    fn top_bar(&mut self, ctx: &egui::Context) {
        let Self {
            logo_texture, audio, ..
        } = self;
        let logo_texture = logo_texture.as_ref();
        egui::TopBottomPanel::top("top")
            .exact_height(78.0)
            .frame(
                egui::Frame::none()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(20.0, 12.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    if let Some(logo) = logo_texture {
                        ui.add(egui::Image::new(logo).fit_to_exact_size(Vec2::splat(48.0)));
                        ui.add_space(6.0);
                    }
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Phi Launcher")
                                .font(brand_font(28.0))
                                .color(theme::ACCENT),
                        );
                        ui.label(
                            RichText::new("Phigros 模拟器启动器 · 下载与启动多个版本")
                                .size(13.0)
                                .color(theme::TEXT_DIM),
                        );
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let mut enabled = audio.enabled();
                        if ui.checkbox(&mut enabled, "音效").changed() {
                            audio.set_enabled(enabled);
                            if enabled {
                                audio.click();
                            }
                        }
                    });
                });
            });
    }

    fn side_panel(&mut self, ctx: &egui::Context) {
        let Self {
            categories,
            selected,
            audio,
            view,
            ..
        } = self;
        egui::SidePanel::left("categories")
            .exact_width(240.0)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::same(14.0)),
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("版本分类")
                        .size(15.0)
                        .strong()
                        .color(theme::TEXT_DIM),
                );
                ui.add_space(10.0);
                for (i, cat) in categories.iter().enumerate() {
                    let is_sel = *view == View::Versions && *selected == i;
                    let resp = category_button(ui, cat, is_sel);
                    if resp.clicked() {
                        *view = View::Versions;
                        *selected = i;
                        audio.click();
                    }
                    ui.add_space(8.0);
                }

                ui.add_space(20.0);
                ui.separator();
                ui.add_space(8.0);

                let settings_sel = *view == View::Settings;
                let resp = nav_button(ui, "⚙ 设置", settings_sel);
                if resp.clicked() {
                    *view = View::Settings;
                    audio.click();
                }
                ui.add_space(6.0);

                let about_sel = *view == View::About;
                let resp = nav_button(ui, "ℹ 关于", about_sel);
                if resp.clicked() {
                    *view = View::About;
                    audio.click();
                }

                ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("资源均来自各仓库的 GitHub Releases，下载后可直接启动；也可手动添加本地版本。")
                            .size(12.0)
                            .color(theme::TEXT_DIM),
                    );
                });
            });
    }

    fn central(&mut self, ctx: &egui::Context) {
        match self.view {
            View::Versions => self.central_versions(ctx),
            View::Settings => self.central_settings(ctx),
            View::About => self.central_about(ctx),
        }
    }

    // ---- Versions view ----

    fn central_versions(&mut self, ctx: &egui::Context) {
        let idx = self.selected;
        let cat = self.categories[idx].clone();
        self.poll_pending_pick(&cat.repo);

        let snapshot = {
            let guard = self.fetch_states[idx].lock().unwrap();
            match &*guard {
                FetchState::Idle | FetchState::Loading => Snap::Loading,
                FetchState::Loaded(r) => Snap::Loaded(r.clone()),
                FetchState::Error(e) => Snap::Error(e.clone()),
            }
        };

        egui::CentralPanel::default()
            .frame(egui::Frame::none().inner_margin(egui::Margin::same(18.0)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(cat.name).size(24.0).strong().color(theme::TEXT));
                    ui.label(RichText::new(cat.tagline).size(14.0).color(theme::TEXT_DIM));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if pill_button(ui, "🌐 发布页").clicked() {
                            self.audio.click();
                            let _ = open::that(cat.page_url());
                        }
                        if pill_button(ui, "🔄 刷新").clicked() {
                            self.audio.click();
                            *self.fetch_states[idx].lock().unwrap() = FetchState::Idle;
                        }
                        ui.checkbox(&mut self.show_prerelease, "显示预览版");
                    });
                });
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(6.0);

                self.local_versions_section(ui, &cat);

                ui.add_space(6.0);
                ui.label(RichText::new("在线版本").size(15.0).strong().color(theme::TEXT_DIM));
                ui.add_space(4.0);

                match snapshot {
                    Snap::Loading => {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| {
                            ui.spinner();
                            ui.add_space(8.0);
                            ui.label(RichText::new("正在获取版本列表…").color(theme::TEXT_DIM));
                        });
                    }
                    Snap::Error(e) => {
                        ui.add_space(20.0);
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new("⚠ 获取失败")
                                    .size(18.0)
                                    .color(Color32::from_rgb(0xFF, 0x8A, 0x8A)),
                            );
                            ui.add_space(4.0);
                            ui.label(RichText::new(e).color(theme::TEXT_DIM));
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new("（GitHub 可能需要代理，或触发了 API 频率限制）")
                                    .size(12.0)
                                    .color(theme::TEXT_DIM),
                            );
                            ui.add_space(10.0);
                            if pill_button(ui, "重试").clicked() {
                                self.audio.click();
                                *self.fetch_states[idx].lock().unwrap() = FetchState::Idle;
                            }
                        });
                    }
                    Snap::Loaded(releases) => {
                        let filtered: Vec<_> = releases
                            .iter()
                            .filter(|r| self.show_prerelease || !r.prerelease)
                            .collect();
                        if filtered.is_empty() {
                            ui.add_space(20.0);
                            ui.vertical_centered(|ui| {
                                ui.label(RichText::new("暂无可用版本").color(theme::TEXT_DIM));
                            });
                        } else {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false; 2])
                                .show(ui, |ui| {
                                    for rel in filtered {
                                        self.release_card(ui, &cat, rel, ctx);
                                        ui.add_space(12.0);
                                    }
                                });
                        }
                    }
                }
            });
    }

    fn local_versions_section(&mut self, ui: &mut egui::Ui, cat: &Category) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("本地版本").size(15.0).strong().color(theme::TEXT_DIM));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if primary_button(ui, "＋ 添加本地版本").clicked() {
                    self.audio.click_large();
                    self.start_add_local(&cat.repo);
                }
            });
        });
        ui.add_space(4.0);

        let locals: Vec<LocalVersion> = self
            .downloads
            .local_versions(&cat.repo)
            .into_iter()
            .cloned()
            .collect();
        if locals.is_empty() {
            ui.label(
                RichText::new("还没有添加本地版本，点右上角按钮选择一个 .exe 即可加入。")
                    .size(13.0)
                    .color(theme::TEXT_DIM),
            );
        } else {
            for v in &locals {
                self.local_version_row(ui, cat, v);
                ui.add_space(6.0);
            }
        }
        ui.add_space(6.0);
        ui.separator();
    }

    fn local_version_row(&mut self, ui: &mut egui::Ui, cat: &Category, v: &LocalVersion) {
        let display_name = v
            .name
            .as_deref()
            .unwrap_or(&v.tag)
            .to_string();
        egui::Frame::none()
            .fill(Color32::from_rgba_premultiplied(10, 18, 30, 180))
            .rounding(Rounding::same(8.0))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgba_premultiplied(120, 90, 180, 160)))
            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            badge(ui, "本地", Color32::from_rgb(0xA0, 0x80, 0xE0));
                            ui.label(RichText::new(&display_name).size(14.0).color(theme::TEXT));
                        });
                        ui.label(
                            RichText::new(v.exe_path.display().to_string())
                                .size(12.0)
                                .color(theme::TEXT_DIM),
                        );
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if danger_button(ui, "移除").clicked() {
                            self.audio.click();
                            let _ = self.downloads.remove_local(&v.repo, &v.tag);
                            self.set_toast(format!("已移除本地版本 {}", display_name));
                        }
                        if primary_button(ui, "▶ 启动").clicked() {
                            self.audio.click_large();
                            match download::launch(&v.exe_path) {
                                Ok(_) => self.set_toast(format!("已启动 {}", display_name)),
                                Err(e) => self.set_toast(format!("启动失败: {e}")),
                            }
                        }
                    });
                });
            });
        let _ = cat;
    }

    // ✅ 修复：release_card 方法
    fn release_card(
        &mut self,
        ui: &mut egui::Ui,
        cat: &Category,
        rel: &github::Release,
        ctx: &egui::Context,
    ) {
        let frame = egui::Frame::none()
            .fill(theme::CARD)
            .rounding(Rounding::same(12.0))
            .stroke(Stroke::new(
                1.0_f32,
                Color32::from_rgba_premultiplied(60, 90, 120, 120),
            ))
            .inner_margin(egui::Margin::same(14.0));
        frame.show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(rel.title()).size(18.0).strong().color(theme::TEXT));
                if rel.prerelease {
                    badge(ui, "预览版", Color32::from_rgb(0xC9, 0x8A, 0x2B));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if !rel.date().is_empty() {
                        ui.label(RichText::new(rel.date()).size(13.0).color(theme::TEXT_DIM));
                    }
                    ui.label(RichText::new(&rel.tag_name).size(13.0).color(theme::ACCENT));
                });
            });

            // Release notes
            if let Some(body) = rel.body.as_deref() {
                let body = body.trim();
                if !body.is_empty() {
                    ui.add_space(6.0);
                    let text = truncate_notes(body, 500);
                    ui.label(RichText::new(text).size(13.0).color(theme::TEXT_DIM));
                }
            }

            // 只显示 Windows 资产
            let relevant: Vec<_> = rel
                .assets
                .iter()
                .filter(|a| download::is_windows_asset(&a.name))
                .collect();

            if relevant.is_empty() {
                ui.add_space(6.0);
                ui.label(
                    RichText::new("该版本没有可下载的 Windows / phi 相关文件（已过滤移动端包）。")
                        .size(13.0)
                        .color(theme::TEXT_DIM),
                );
                return;
            }

            ui.add_space(8.0);
            for asset in relevant {
                self.asset_row(ui, cat, rel, asset, ctx);
                ui.add_space(6.0);
            }
        });
    }

    // ✅ 修改：asset_row 使用 is_asset_installed 和 uninstall_asset
    fn asset_row(
        &mut self,
        ui: &mut egui::Ui,
        cat: &Category,
        rel: &github::Release,
        asset: &github::Asset,
        ctx: &egui::Context,
    ) {
        let key = DownloadManager::key(cat.repo, &rel.tag_name, &asset.name);
        let status = self.downloads.status(&key);
        let installed = self.downloads.is_asset_installed(cat.repo, &rel.tag_name, &asset.name);

        egui::Frame::none()
            .fill(Color32::from_rgba_premultiplied(10, 18, 30, 180))
            .rounding(Rounding::same(8.0))
            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&asset.name).size(14.0).color(theme::TEXT));
                        if asset.size > 0 {
                            ui.label(
                                RichText::new(download::human_size(asset.size))
                                    .size(12.0)
                                    .color(theme::TEXT_DIM),
                            );
                        }
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        match &status {
                            Some(Status::Downloading { received, total }) => {
                                let frac = if *total > 0 {
                                    *received as f32 / *total as f32
                                } else {
                                    0.0
                                };
                                ui.add_sized(
                                    Vec2::new(180.0, 22.0),
                                    egui::ProgressBar::new(frac)
                                        .fill(theme::ACCENT_DIM)
                                        .text(download::human_size(*received)),
                                );
                            }
                            Some(Status::Queued) => {
                                ui.label(RichText::new("排队中…").color(theme::TEXT_DIM));
                            }
                            Some(Status::Extracting) => {
                                ui.spinner();
                                ui.label(RichText::new("解压中…").color(theme::TEXT_DIM));
                            }
                            Some(Status::Failed(e)) => {
                                if primary_button(ui, "重试").clicked() {
                                    self.audio.click_large();
                                    self.downloads.start(
                                        cat.repo,
                                        &rel.tag_name,
                                        &asset.name,
                                        &asset.download_url,
                                        ctx.clone(),
                                    );
                                }
                                ui.label(
                                    RichText::new(format!("失败: {e}"))
                                        .size(12.0)
                                        .color(Color32::from_rgb(0xFF, 0x8A, 0x8A)),
                                );
                            }
                            Some(Status::Installed(_)) | None if installed => {
                                // ✅ 删除单个资产文件
                                if danger_button(ui, "删除").clicked() {
                                    self.audio.click();
                                    let _ = self.downloads.uninstall_asset(cat.repo, &rel.tag_name, &asset.name);
                                    self.set_toast(format!("已删除 {}", asset.name));
                                }
                                if primary_button(ui, "▶ 启动").clicked() {
                                    self.audio.click_large();
                                    self.launch_version(cat, rel);
                                }
                            }
                            _ => {
                                if primary_button(ui, "⬇ 下载").clicked() {
                                    self.audio.click_large();
                                    self.downloads.start(
                                        cat.repo,
                                        &rel.tag_name,
                                        &asset.name,
                                        &asset.download_url,
                                        ctx.clone(),
                                    );
                                    self.set_toast(format!("开始下载 {}", asset.name));
                                }
                            }
                        }
                    });
                });
            });
    }

    fn launch_version(&mut self, cat: &Category, rel: &github::Release) {
        match self.downloads.find_executable(cat.repo, &rel.tag_name) {
            Some(exe) => match download::launch(&exe) {
                Ok(_) => {
                    self.audio.ending();
                    self.set_toast(format!("已启动 {} {}", cat.name, rel.tag_name));
                }
                Err(e) => self.set_toast(format!("启动失败: {e}")),
            },
            None => {
                let dir = self.downloads.install_dir(cat.repo, &rel.tag_name);
                let _ = open::that(&dir);
                self.set_toast("未找到可执行文件，已为你打开安装目录".to_string());
            }
        }
    }

    // ---- Settings view ----

    fn central_settings(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
                ui.label(RichText::new("设置").size(28.0).strong().color(theme::TEXT));
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(16.0);

                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        self.settings_account_section(ui, ctx);
                        ui.add_space(20.0);
                        self.settings_mp_section(ui);
                        ui.add_space(20.0);
                        self.settings_sync_section(ui);
                    });
            });
    }

    fn settings_account_section(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context) {
        ui.label(RichText::new("账号").size(18.0).strong().color(theme::TEXT));
        ui.add_space(8.0);

        if self.auth.is_logged_in() {
            let user = self.auth.data.me.as_ref().unwrap().clone();
            egui::Frame::none()
                .fill(theme::CARD)
                .rounding(Rounding::same(12.0))
                .stroke(Stroke::new(
                    1.0_f32,
                    Color32::from_rgba_premultiplied(60, 90, 120, 120),
                ))
                .inner_margin(egui::Margin::same(16.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    ui.horizontal(|ui| {
                        if let Some(tex) = &self.avatar_texture {
                            ui.add(
                                egui::Image::new(tex)
                                    .fit_to_exact_size(Vec2::splat(64.0))
                                    .rounding(32.0),
                            );
                        } else {
                            let first_char = user.name.chars().next().unwrap_or('?');
                            ui.allocate_ui(Vec2::splat(64.0), |ui| {
                                let rect = ui.max_rect();
                                ui.painter().circle_filled(
                                    rect.center(),
                                    32.0,
                                    theme::ACCENT_DIM,
                                );
                                ui.painter().text(
                                    rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    first_char.to_string(),
                                    FontId::proportional(24.0),
                                    theme::TEXT,
                                );
                            });
                        }
                        ui.add_space(12.0);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(&user.name)
                                    .size(20.0)
                                    .strong()
                                    .color(theme::TEXT),
                            );
                            ui.label(
                                RichText::new(&user.email)
                                    .size(13.0)
                                    .color(theme::TEXT_DIM),
                            );
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                badge(
                                    ui,
                                    &format!("RKS {:.2}", user.rks),
                                    theme::ACCENT,
                                );
                                badge(
                                    ui,
                                    &format!("ID {}", user.id),
                                    Color32::from_rgb(0xA0, 0x80, 0xE0),
                                );
                            });
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if danger_button(ui, "退出登录").clicked() {
                                self.audio.click();
                                self.auth.logout();
                                self.avatar_texture = None;
                                self.set_toast("已退出登录".to_string());
                            }
                        });
                    });
                });
        } else {
            let login_state = self.auth.login_state.lock().unwrap().clone();
            let is_loading = matches!(login_state, LoginState::Loading);

            egui::Frame::none()
                .fill(theme::CARD)
                .rounding(Rounding::same(12.0))
                .stroke(Stroke::new(
                    1.0_f32,
                    Color32::from_rgba_premultiplied(60, 90, 120, 120),
                ))
                .inner_margin(egui::Margin::same(16.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    ui.label(
                        RichText::new("登录 Phira 账号")
                            .size(16.0)
                            .strong()
                            .color(theme::TEXT),
                    );
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("登录后自动同步账号信息到各个版本的 data 目录。")
                            .size(12.0)
                            .color(theme::TEXT_DIM),
                    );
                    ui.add_space(12.0);

                    ui.label(RichText::new("邮箱").size(13.0).color(theme::TEXT_DIM));
                    ui.add_space(4.0);
                    ui.add_sized(
                        Vec2::new(ui.available_width(), 32.0),
                        egui::TextEdit::singleline(&mut self.login_email)
                            .hint_text("your@email.com"),
                    );
                    ui.add_space(8.0);

                    ui.label(RichText::new("密码").size(13.0).color(theme::TEXT_DIM));
                    ui.add_space(4.0);
                    ui.add_sized(
                        Vec2::new(ui.available_width(), 32.0),
                        egui::TextEdit::singleline(&mut self.login_password)
                            .password(true)
                            .hint_text("••••••••"),
                    );
                    ui.add_space(12.0);

                    ui.horizontal(|ui| {
                        if is_loading {
                            ui.spinner();
                            ui.label(RichText::new("登录中…").color(theme::TEXT_DIM));
                        } else {
                            if primary_button(ui, "登录").clicked() {
                                self.audio.click_large();
                                if !self.login_email.is_empty()
                                    && !self.login_password.is_empty()
                                {
                                    self.auth.start_login(
                                        self.login_email.clone(),
                                        self.login_password.clone(),
                                        ui.ctx().clone(),
                                    );
                                } else {
                                    self.set_toast("请输入邮箱和密码".to_string());
                                }
                            }
                        }
                    });

                    if let LoginState::Failed(msg) = &login_state {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(msg)
                                .size(13.0)
                                .color(Color32::from_rgb(0xFF, 0x8A, 0x8A)),
                        );
                    }

                    if matches!(login_state, LoginState::Success) {
                        let _ = self.auth.reload();
                        self.avatar_texture = None;
                        self.login_password.clear();

                        match self.auth.sync_to_versions() {
                            Ok(n) => {
                                self.set_toast(format!("✅ 登录成功！已同步到 {} 个版本", n));
                            }
                            Err(e) => {
                                self.set_toast(format!("⚠️ 登录成功，但同步失败: {}", e));
                            }
                        }

                        *self.auth.login_state.lock().unwrap() = LoginState::Idle;
                    }
                });
        }
    }

    fn settings_mp_section(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("多人游戏").size(18.0).strong().color(theme::TEXT));
        ui.add_space(8.0);

        egui::Frame::none()
            .fill(theme::CARD)
            .rounding(Rounding::same(12.0))
            .stroke(Stroke::new(
                1.0_f32,
                Color32::from_rgba_premultiplied(60, 90, 120, 120),
            ))
            .inner_margin(egui::Margin::same(16.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("服务器地址")
                        .size(13.0)
                        .color(theme::TEXT_DIM),
                );
                ui.add_space(4.0);
                let resp = ui.add_sized(
                    Vec2::new(ui.available_width(), 32.0),
                    egui::TextEdit::singleline(&mut self.auth.data.mp_address)
                        .hint_text("mp2.phira.cn:12345"),
                );
                if resp.lost_focus() {
                    let _ = self.auth.save();
                }
                ui.add_space(8.0);

                let mut enabled = self.auth.data.mp_enabled;
                if ui.checkbox(&mut enabled, "启用多人游戏").changed() {
                    self.auth.data.mp_enabled = enabled;
                    let _ = self.auth.save();
                    self.audio.click();
                }
                ui.add_space(4.0);
                ui.label(
                    RichText::new("同步 data.json 后，各版本将使用此服务器地址。")
                        .size(12.0)
                        .color(theme::TEXT_DIM),
                );
            });
    }

    fn settings_sync_section(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("数据同步").size(18.0).strong().color(theme::TEXT));
        ui.add_space(8.0);

        egui::Frame::none()
            .fill(theme::CARD)
            .rounding(Rounding::same(12.0))
            .stroke(Stroke::new(
                1.0_f32,
                Color32::from_rgba_premultiplied(60, 90, 120, 120),
            ))
            .inner_margin(egui::Margin::same(16.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("将启动器的账号信息同步到所有已安装版本的 data 目录。")
                        .size(13.0)
                        .color(theme::TEXT_DIM),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "每个版本文件夹下会创建 data/data.json，包含登录信息、token 和多人服务器配置。",
                    )
                    .size(12.0)
                    .color(theme::TEXT_DIM),
                );
                ui.add_space(12.0);

                if primary_button(ui, "同步到所有版本").clicked() {
                    self.audio.click_large();
                    match self.auth.sync_to_versions() {
                        Ok(n) => {
                            self.set_toast(format!("已同步到 {} 个版本目录", n));
                        }
                        Err(e) => {
                            self.set_toast(format!("同步失败: {e}"));
                        }
                    }
                }
            });
    }

    // ---- About view ----

    fn central_about(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);

                    if let Some(logo) = &self.logo_texture {
                        ui.add(
                            egui::Image::new(logo).fit_to_exact_size(Vec2::splat(128.0)),
                        );
                        ui.add_space(16.0);
                    }

                    ui.label(
                        RichText::new(format!("Phi Launcher v{}", env!("CARGO_PKG_VERSION")))
                            .font(brand_font(36.0))
                            .color(theme::ACCENT),
                    );
                    ui.add_space(8.0);

                    ui.label(
                        RichText::new("作者：Lute_Rencai")
                            .size(18.0)
                            .color(theme::TEXT),
                    );
                    ui.add_space(4.0);

                    ui.label(
                        RichText::new("支持：无")
                            .size(16.0)
                            .color(theme::TEXT_DIM),
                    );
                    ui.add_space(24.0);

                    egui::Frame::none()
                        .fill(theme::CARD)
                        .rounding(Rounding::same(12.0))
                        .stroke(Stroke::new(
                            1.0_f32,
                            Color32::from_rgba_premultiplied(60, 90, 120, 120),
                        ))
                        .inner_margin(egui::Margin::symmetric(24.0, 16.0))
                        .show(ui, |ui| {
                            ui.set_max_width(520.0);
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new(
                                        "Phi Launcher 是为电脑玩家游玩 phira、等 phira 改版的启动器",
                                    )
                                    .size(15.0)
                                    .color(theme::TEXT),
                                );
                                ui.add_space(12.0);
                                ui.label(
                                    RichText::new(
                                        "目前只收集到了四种改版，如果你也是改版作者，想让 PL 收录，请联系 2332208506@qq.com。",
                                    )
                                    .size(14.0)
                                    .color(theme::TEXT_DIM),
                                );
                            });
                        });

                    ui.add_space(20.0);
                    ui.label(
                        RichText::new("资源来自各项目 GitHub Releases · 本启动器与各改版项目无关")
                            .size(12.0)
                            .color(theme::TEXT_DIM),
                    );
                });
            });
    }

    fn set_toast(&mut self, msg: String) {
        self.toast = Some((msg, Instant::now()));
    }

    fn toast_overlay(&mut self, ctx: &egui::Context) {
        let Some((msg, at)) = self.toast.clone() else {
            return;
        };
        if at.elapsed().as_secs_f32() > 3.5 {
            self.toast = None;
            return;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(200));

        let screen_width = ctx.screen_rect().width();
        let toast_width = (screen_width * 0.5).min(600.0).max(350.0);

        egui::Area::new(egui::Id::new("toast"))
            .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -24.0))
            .show(ctx, |ui| {
                ui.set_min_width(toast_width);
                egui::Frame::none()
                    .fill(Color32::from_rgba_premultiplied(20, 32, 52, 245))
                    .rounding(Rounding::same(10.0))
                    .stroke(Stroke::new(1.0_f32, theme::ACCENT))
                    .inner_margin(egui::Margin::symmetric(24.0, 12.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new(msg).size(15.0).color(theme::TEXT));
                    });
            });
    }
}

enum Snap {
    Loading,
    Loaded(Vec<github::Release>),
    Error(String),
}

/// Trim Markdown-ish release notes to a short, single-block preview.
fn truncate_notes(body: &str, max_chars: usize) -> String {
    let cleaned: String = body
        .lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    if cleaned.chars().count() > max_chars {
        let mut s: String = cleaned.chars().take(max_chars).collect();
        s.push('…');
        s
    } else {
        cleaned
    }
}

// ---- small UI helpers ----

fn category_button(ui: &mut egui::Ui, cat: &Category, selected: bool) -> egui::Response {
    let fill = if selected {
        theme::ACCENT_DIM
    } else {
        theme::CARD
    };
    let stroke = if selected {
        Stroke::new(1.5_f32, theme::ACCENT)
    } else {
        Stroke::new(1.0_f32, Color32::from_rgba_premultiplied(60, 90, 120, 100))
    };
    let resp = egui::Frame::none()
        .fill(fill)
        .rounding(Rounding::same(10.0))
        .stroke(stroke)
        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical(|ui| {
                ui.label(RichText::new(cat.name).size(16.0).strong().color(theme::TEXT));
                ui.label(RichText::new(cat.tagline).size(12.0).color(theme::TEXT_DIM));
            });
        })
        .response;
    resp.interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A simple nav button for settings / about.
fn nav_button(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let fill = if selected {
        theme::ACCENT_DIM
    } else {
        Color32::TRANSPARENT
    };
    let stroke = if selected {
        Stroke::new(1.0_f32, theme::ACCENT)
    } else {
        Stroke::NONE
    };
    let resp = egui::Frame::none()
        .fill(fill)
        .rounding(Rounding::same(8.0))
        .stroke(stroke)
        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(text).size(15.0).color(theme::TEXT));
        })
        .response;
    resp.interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(Color32::BLACK).strong())
            .fill(theme::ACCENT)
            .rounding(Rounding::same(8.0)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn danger_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(theme::TEXT))
            .fill(Color32::from_rgb(0x5A, 0x2A, 0x2A))
            .rounding(Rounding::same(8.0)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn pill_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(theme::TEXT))
            .fill(theme::CARD)
            .rounding(Rounding::same(8.0)),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::none()
        .fill(Color32::from_rgba_premultiplied(
            color.r(),
            color.g(),
            color.b(),
            60,
        ))
        .rounding(Rounding::same(6.0))
        .stroke(Stroke::new(1.0_f32, color))
        .inner_margin(egui::Margin::symmetric(6.0, 2.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(color));
        });
}