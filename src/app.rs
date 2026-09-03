use crate::{
    composite::{CompositeEvent, CompositeParams, CompositeRequest},
    video::{default_output_path, ensure_extension, extract_first_frame_mat, get_video_info, is_supported_video_extension, mat_to_color_image, VideoInfo},
};
use anyhow::Result;
use egui::{ColorImage, TextureHandle, TextureOptions, Vec2};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{channel, Receiver, Sender},
        Arc,
    },
    thread,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preset {
    Fine,     // きめ細かい
    Standard, // 標準
    Light,    // あっさり
}

impl Preset {
    pub fn label(&self) -> &'static str {
        match self {
            Preset::Fine => "きめ細かい",
            Preset::Standard => "標準",
            Preset::Light => "あっさり",
        }
    }
    pub fn apply(&self, params: &mut CompositeParams) {
        match self {
            Preset::Fine => {
                params.interval_sec = 0.10;
                params.min_area = 50.0;
                params.threshold = 12.0;
                params.history = 300;
            }
            Preset::Standard => {
                params.interval_sec = 0.25;
                params.min_area = 200.0;
                params.threshold = 16.0;
                params.history = 200;
            }
            Preset::Light => {
                params.interval_sec = 0.50;
                params.min_area = 600.0;
                params.threshold = 28.0;
                params.history = 100;
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppStatus {
    Ready,
    Processing,
    Done,
    Cancelled,
    Error,
}

impl AppStatus {
    pub fn label(&self) -> &'static str {
        match self {
            AppStatus::Ready => "READY",
            AppStatus::Processing => "PROCESSING",
            AppStatus::Done => "DONE",
            AppStatus::Cancelled => "CANCELLED",
            AppStatus::Error => "ERROR",
        }
    }
    pub fn color(&self) -> egui::Color32 {
        match self {
            AppStatus::Ready => egui::Color32::from_rgb(120, 180, 255),
            AppStatus::Processing => egui::Color32::from_rgb(255, 200, 80),
            AppStatus::Done => egui::Color32::from_rgb(80, 220, 120),
            AppStatus::Cancelled => egui::Color32::from_rgb(200, 200, 200),
            AppStatus::Error => egui::Color32::from_rgb(255, 90, 90),
        }
    }
}

pub struct FrameMotionApp {
    // paths
    pub video_path: Option<PathBuf>,
    pub video_info: Option<VideoInfo>,
    pub video_info_error: Option<String>,
    pub output_path: Option<PathBuf>,
    pub output_ext: String, // "png" or "jpg"

    // params
    pub params: CompositeParams,
    pub preset: Preset,
    pub show_advanced: bool,

    // preview
    pub before_image: Option<ColorImage>,
    pub after_image: Option<ColorImage>,
    pub before_texture: Option<TextureHandle>,
    pub after_texture: Option<TextureHandle>,
    pub preview_is_after: bool,

    // status
    pub status: AppStatus,
    pub status_detail: String,
    pub progress: f32,
    pub current_sec: f32,
    pub total_sec: f32,

    // processing
    cancel_flag: Arc<AtomicBool>,
    receiver: Option<Receiver<CompositeEvent>>,
    _sender: Option<Sender<CompositeEvent>>,

    // UI state
    pub error_dialog: Option<String>,
    pub overwrite_confirm: Option<PathBuf>,
    pub show_close_confirm: bool,
    // For file dialog remember last dir
    last_dir: Option<PathBuf>,

    // To avoid texture re-creation every frame
    needs_texture_reload: bool,
}

impl Default for FrameMotionApp {
    fn default() -> Self {
        Self {
            video_path: None,
            video_info: None,
            video_info_error: None,
            output_path: None,
            output_ext: "png".to_string(),
            params: CompositeParams::default(),
            preset: Preset::Standard,
            show_advanced: false,
            before_image: None,
            after_image: None,
            before_texture: None,
            after_texture: None,
            preview_is_after: false,
            status: AppStatus::Ready,
            status_detail: "待機中".to_string(),
            progress: 0.0,
            current_sec: 0.0,
            total_sec: 0.0,
            cancel_flag: Arc::new(AtomicBool::new(false)),
            receiver: None,
            _sender: None,
            error_dialog: None,
            overwrite_confirm: None,
            show_close_confirm: false,
            last_dir: None,
            needs_texture_reload: false,
        }
    }
}

impl FrameMotionApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // setup dark visuals
        let mut visuals = egui::Visuals::dark();
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(30, 32, 40);
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(45, 48, 60);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(60, 65, 85);
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(70, 75, 100);
        visuals.panel_fill = egui::Color32::from_rgb(18, 20, 28);
        visuals.extreme_bg_color = egui::Color32::from_rgb(24, 26, 34);
        cc.egui_ctx.set_visuals(visuals);

        // fonts: Japanese support via embedded BIZ UDPGothic
        {
            let mut fonts = egui::FontDefinitions::default();
            // Embedded font ensures Japanese works on all OS (Windows/macOS/Linux)
            let jp_bytes = include_bytes!("../assets/BIZUDPGothic-Regular.ttf").to_vec();
            fonts.font_data.insert(
                "jp".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(jp_bytes)),
            );
            // Make jp the first priority for proportional
            if let Some(proportional) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
                proportional.insert(0, "jp".to_owned());
            }
            // Also add as fallback for monospace
            if let Some(monospace) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
                monospace.push("jp".to_owned());
            }
            cc.egui_ctx.set_fonts(fonts);
        }

        // Style: larger, denser (fix small/sparse)
        {
            let mut style = (*cc.egui_ctx.style()).clone();
            // Increase font sizes for readability and to fill space
            style.text_styles.insert(
                egui::TextStyle::Heading,
                egui::FontId::new(22.0, egui::FontFamily::Proportional),
            );
            style.text_styles.insert(
                egui::TextStyle::Body,
                egui::FontId::new(15.0, egui::FontFamily::Proportional),
            );
            style.text_styles.insert(
                egui::TextStyle::Button,
                egui::FontId::new(14.0, egui::FontFamily::Proportional),
            );
            style.text_styles.insert(
                egui::TextStyle::Small,
                egui::FontId::new(12.0, egui::FontFamily::Proportional),
            );
            style.text_styles.insert(
                egui::TextStyle::Monospace,
                egui::FontId::new(13.0, egui::FontFamily::Monospace),
            );
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            style.spacing.window_margin = egui::Margin::same(8);
            style.spacing.button_padding = egui::vec2(10.0, 6.0);
            style.spacing.indent = 16.0;
            style.spacing.scroll = egui::style::ScrollStyle {
                bar_width: 8.0,
                ..Default::default()
            };
            cc.egui_ctx.set_style(style);
            // Slightly larger UI scale for better visibility
            cc.egui_ctx.set_pixels_per_point(1.05);
        }

        let mut app = Self::default();
        // Apply preset standard
        app.preset.apply(&mut app.params);
        app
    }

    fn set_status(&mut self, status: AppStatus, detail: impl Into<String>) {
        self.status = status;
        self.status_detail = detail.into();
    }

    fn load_video(&mut self, path: PathBuf, ctx: &egui::Context) {
        // validate extension
        if !is_supported_video_extension(&path) {
            self.error_dialog = Some(format!(
                "非対応の形式です: {}\n対応形式: MP4, AVI, MOV, MKV, M4V, WEBM 等",
                path.display()
            ));
            return;
        }
        self.video_path = Some(path.clone());
        self.last_dir = path.parent().map(|p| p.to_path_buf());
        // Update output path if not set or if previously auto-generated
        let ext = self.output_ext.clone();
        if self.output_path.is_none() {
            self.output_path = Some(default_output_path(&path, &ext));
        } else {
            // if output path was auto-generated from previous video, update? Simple: keep user set path
            // But if output path's stem matches previous video stem, update? For simplicity, if user hasn't manually changed, auto-update.
            // We'll auto-update if output path is default for previous video? Check previous video path.
            // Simpler: if output_path exists and its parent equals input parent and stem contains previous input stem, update.
            // For now, only auto-set if output_path is None; else keep.
        }

        // Get video info
        match get_video_info(&path) {
            Ok(info) => {
                // validate fps
                if info.fps <= 1e-6 || !info.fps.is_finite() {
                    self.video_info_error = Some("FPS取得失敗".to_string());
                } else {
                    self.video_info_error = None;
                }
                self.total_sec = info.duration_sec as f32;
                self.video_info = Some(info);
                self.set_status(AppStatus::Ready, "動画を読み込みました");
            }
            Err(e) => {
                self.video_info = None;
                self.video_info_error = Some(e.to_string());
                self.set_status(AppStatus::Error, format!("動画情報取得失敗: {}", e));
                self.error_dialog = Some(format!("動画情報の取得に失敗しました:\n{}", e));
            }
        }

        // Extract first frame for preview
        match extract_first_frame_mat(&path) {
            Ok(mat) => match mat_to_color_image(&mat) {
                Ok(img) => {
                    self.before_image = Some(img.clone());
                    // create texture
                    self.before_texture = Some(ctx.load_texture(
                        "before",
                        img,
                        TextureOptions::LINEAR,
                    ));
                    self.after_texture = None;
                    self.after_image = None;
                    self.preview_is_after = false;
                    self.needs_texture_reload = false;
                }
                Err(e) => {
                    self.error_dialog = Some(format!("プレビュー生成失敗:\n{}", e));
                }
            },
            Err(e) => {
                self.error_dialog = Some(format!("先頭フレーム読込失敗:\n{}", e));
                self.set_status(AppStatus::Error, e.to_string());
            }
        }
    }

    fn pick_video_dialog(&mut self, ctx: &egui::Context) {
        let mut dlg = rfd::FileDialog::new()
            .set_title("入力動画を選択")
            .add_filter("動画", &["mp4", "avi", "mov", "mkv", "m4v", "webm", "wmv", "flv", "mpg", "mpeg"]);
        if let Some(dir) = &self.last_dir {
            dlg = dlg.set_directory(dir);
        }
        if let Some(path) = dlg.pick_file() {
            self.load_video(path, ctx);
        }
    }

    fn pick_output_dialog(&mut self) {
        let mut dlg = rfd::FileDialog::new().set_title("保存先を選択");
        if let Some(dir) = &self.last_dir {
            dlg = dlg.set_directory(dir);
        }
        // Provide default filename if we have video path
        if let Some(vp) = &self.video_path {
            let def = default_output_path(vp, &self.output_ext);
            if let Some(fname) = def.file_name() {
                dlg = dlg.set_file_name(fname.to_string_lossy().to_string());
            }
        }
        dlg = dlg.add_filter("PNG", &["png"]).add_filter("JPEG", &["jpg", "jpeg"]);
        if let Some(path) = dlg.save_file() {
            let path = ensure_extension(&path, &self.output_ext);
            // update ext based on chosen extension
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                self.output_ext = ext.to_ascii_lowercase();
                if self.output_ext == "jpeg" {
                    self.output_ext = "jpg".to_string();
                }
            }
            self.output_path = Some(path.clone());
            self.last_dir = path.parent().map(|p| p.to_path_buf());
        }
    }

    pub fn start_composite(&mut self, ctx: &egui::Context) {
        // validation
        let video_path = match &self.video_path {
            Some(p) => p.clone(),
            None => {
                self.error_dialog = Some("動画を選択してください".to_string());
                return;
            }
        };
        let output_path = match &self.output_path {
            Some(p) => ensure_extension(p, &self.output_ext),
            None => default_output_path(&video_path, &self.output_ext),
        };

        // Check overwrite
        if output_path.exists() && self.overwrite_confirm.is_none() {
            self.overwrite_confirm = Some(output_path);
            return;
        }
        self.overwrite_confirm = None;

        // Validate params
        if self.params.interval_sec < 0.05 || self.params.interval_sec > 1.0 {
            self.error_dialog = Some("間隔は0.05〜1.0秒の範囲で指定してください".to_string());
            return;
        }

        // Ensure parent dir exists will be done in backend

        // Reset progress
        self.progress = 0.0;
        self.current_sec = 0.0;
        if let Some(info) = &self.video_info {
            self.total_sec = info.duration_sec as f32;
        }
        self.set_status(AppStatus::Processing, "合成中...");
        self.preview_is_after = false;

        // Setup channel and cancel flag
        let (tx, rx) = channel::<CompositeEvent>();
        self.receiver = Some(rx);
        self.cancel_flag = Arc::new(AtomicBool::new(false));
        let cancel_clone = self.cancel_flag.clone();
        let req = CompositeRequest {
            input_path: video_path,
            output_path: output_path.clone(),
            params: self.params.clone(),
        };
        let ctx_clone = ctx.clone();
        // Spawn thread
        thread::spawn(move || {
            let res = crate::composite::run_composite(req, tx.clone(), cancel_clone);
            if let Err(e) = res {
                let _ = tx.send(CompositeEvent::Error(e.to_string()));
            }
            // wake UI
            ctx_clone.request_repaint();
        });
        // store output for after preview
        self.output_path = Some(output_path);
    }

    pub fn cancel_composite(&mut self) {
        self.cancel_flag.store(true, Ordering::Relaxed);
        self.set_status(AppStatus::Cancelled, "キャンセル中...");
    }

    pub fn poll_receiver(&mut self, ctx: &egui::Context) {
        // Collect events without holding immutable borrow across mutable operations
        let events: Vec<CompositeEvent> = if let Some(rx) = &self.receiver {
            let mut v = Vec::new();
            while let Ok(ev) = rx.try_recv() {
                v.push(ev);
            }
            v
        } else {
            Vec::new()
        };

        let mut should_repaint = false;
        let mut clear_receiver = false;
        for ev in events {
            match ev {
                CompositeEvent::Progress(p) => {
                    self.progress = p.percent;
                    self.current_sec = p.current_sec;
                    self.total_sec = p.total_sec;
                    should_repaint = true;
                }
                CompositeEvent::Done(path) => {
                    self.progress = 100.0;
                    self.set_status(AppStatus::Done, "合成完了");
                    clear_receiver = true;
                    // Load after image via image crate
                    if let Ok(img) = image::open(&path) {
                        let rgba = img.to_rgba8();
                        let size = [rgba.width() as usize, rgba.height() as usize];
                        let color = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                        self.after_image = Some(color.clone());
                        self.after_texture = Some(ctx.load_texture("after", color, TextureOptions::LINEAR));
                        self.preview_is_after = true;
                    } else {
                        // fallback via opencv imread
                        use opencv::imgcodecs;
                        if let Ok(mat) = imgcodecs::imread(&path.to_string_lossy().to_string(), imgcodecs::IMREAD_COLOR) {
                            if let Ok(img) = mat_to_color_image(&mat) {
                                self.after_image = Some(img.clone());
                                self.after_texture = Some(ctx.load_texture("after", img, TextureOptions::LINEAR));
                                self.preview_is_after = true;
                            }
                        }
                    }
                    should_repaint = true;
                }
                CompositeEvent::Error(msg) => {
                    self.set_status(AppStatus::Error, msg.clone());
                    self.error_dialog = Some(msg);
                    clear_receiver = true;
                }
                CompositeEvent::Cancelled => {
                    self.set_status(AppStatus::Cancelled, "キャンセルしました");
                    self.progress = 0.0;
                    clear_receiver = true;
                }
            }
        }
        if clear_receiver {
            self.receiver = None;
        }
        if should_repaint {
            ctx.request_repaint();
        }
        if self.status == AppStatus::Processing {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    pub fn open_output(&self) {
        if let Some(path) = &self.output_path {
            if path.exists() {
                let _ = open::that(path);
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if !dropped.is_empty() {
            for file in dropped {
                if let Some(path) = file.path {
                    // Only first file
                    self.load_video(path, ctx);
                    break;
                }
            }
        }
    }
}

impl eframe::App for FrameMotionApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll background thread
        self.poll_receiver(ctx);
        self.handle_dropped_files(ctx);

        // Check close request if processing – confirm dialog (非機能要件 7.3)
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.status == AppStatus::Processing && !self.show_close_confirm {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.show_close_confirm = true;
            }
        }

        // Top header
        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.add_space(8.0);
                    ui.heading(egui::RichText::new("FRAME MOTION STUDIO").size(20.0).strong().color(egui::Color32::from_rgb(220, 230, 255)));
                    ui.label(egui::RichText::new("動画から軌跡を1枚に — 残像合成スタジオ").size(13.0).color(egui::Color32::from_rgb(150, 160, 190)));
                    ui.add_space(6.0);
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(12.0);
                    let status_text = format!("● {}", self.status.label());
                    ui.label(
                        egui::RichText::new(status_text)
                            .size(12.0)
                            .strong()
                            .color(self.status.color()),
                    );
                    ui.label(
                        egui::RichText::new(&self.status_detail)
                            .size(13.0)
                            .color(egui::Color32::from_rgb(180, 190, 220)),
                    );
                });
            });
            ui.add_space(4.0);
            // separator
            ui.separator();
        });

        egui::TopBottomPanel::bottom("footer").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(egui::RichText::new("© 2026 FRAME MOTION STUDIO  •  Offline • No GPU required").size(12.0).color(egui::Color32::from_rgb(120, 130, 160)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new("v0.1.0").size(12.0).color(egui::Color32::from_rgb(120,130,160)));
                    ui.add_space(12.0);
                });
            });
            ui.add_space(4.0);
        });

        // Main content with two panes
        egui::CentralPanel::default().show(ctx, |ui| {
            // Use columns
            ui.columns(2, |cols| {
                // Left pane
                cols[0].vertical(|ui| {
                    egui::ScrollArea::vertical().id_salt("left_pane").show(ui, |ui| {

                        // Video selection card
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(30, 32, 44))
                            .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(55, 60, 85)))
                            .corner_radius(10)
                            .inner_margin(egui::Margin::symmetric(14, 14))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("📁  入力動画").size(16.0).strong().color(egui::Color32::from_rgb(210, 220, 255)));
                                });
                                ui.add_space(8.0);

                                // Drop area
                                let hovered_files = !ctx.input(|i| i.raw.hovered_files.is_empty());
                                let drop_bg = if hovered_files {
                                    egui::Color32::from_rgb(45, 55, 90)
                                } else {
                                    egui::Color32::from_rgb(36, 38, 52)
                                };
                                let drop_stroke = if hovered_files {
                                    egui::Stroke::new(1.5_f32, egui::Color32::from_rgb(90, 120, 255))
                                } else {
                                    egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(65, 70, 100))
                                };
                                egui::Frame::new()
                                    .fill(drop_bg)
                                    .stroke(drop_stroke)
                                    .corner_radius(8)
                                    .inner_margin(12)
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.vertical_centered(|ui| {
                                            ui.label(egui::RichText::new("ここに動画をドラッグ＆ドロップ").size(13.0).color(egui::Color32::from_rgb(160, 170, 210)));
                                            ui.label(egui::RichText::new("または「参照」から選択").size(12.0).color(egui::Color32::from_rgb(120, 130, 170)));
                                            ui.add_space(8.0);
                                            let btn = egui::Button::new(egui::RichText::new("  参照  ").size(13.0).strong())
                                                .fill(egui::Color32::from_rgb(70, 85, 160))
                                                .stroke(egui::Stroke::NONE)
                                                .corner_radius(6);
                                            if ui.add(btn).clicked() {
                                                self.pick_video_dialog(ctx);
                                            }
                                        });
                                    });

                                ui.add_space(10.0);

                                // File name
                                if let Some(path) = &self.video_path {
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("ファイル:").size(13.0).color(egui::Color32::from_rgb(150, 160, 190)));
                                        ui.label(egui::RichText::new(path.file_name().unwrap_or_default().to_string_lossy()).size(13.0).color(egui::Color32::WHITE).strong());
                                    });
                                } else {
                                    ui.label(egui::RichText::new("未選択").size(13.0).color(egui::Color32::from_rgb(120, 130, 170)).italics());
                                }

                                ui.add_space(8.0);
                                ui.separator();
                                ui.add_space(6.0);

                                // Video info
                                ui.label(egui::RichText::new("動画情報").size(13.0).strong().color(egui::Color32::from_rgb(180, 190, 220)));
                                ui.add_space(4.0);
                                egui::Grid::new("video_info_grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                                    let info = self.video_info.clone();
                                    let err = self.video_info_error.clone();
                                    let (res, fps, frames, dur) = if let Some(inf) = info {
                                        (inf.resolution_str(), inf.fps_str(), inf.frame_count_str(), inf.duration_str())
                                    } else if err.is_some() {
                                        ("--".to_string(), "--".to_string(), "--".to_string(), "--".to_string())
                                    } else {
                                        ("--".to_string(), "--".to_string(), "--".to_string(), "--".to_string())
                                    };
                                    ui.label(egui::RichText::new("解像度").size(12.0).color(egui::Color32::from_rgb(130, 140, 180)));
                                    ui.label(egui::RichText::new(res).size(12.0).color(egui::Color32::WHITE));
                                    ui.end_row();
                                    ui.label(egui::RichText::new("FPS").size(12.0).color(egui::Color32::from_rgb(130, 140, 180)));
                                    ui.label(egui::RichText::new(fps).size(12.0).color(egui::Color32::WHITE));
                                    ui.end_row();
                                    ui.label(egui::RichText::new("総フレーム").size(12.0).color(egui::Color32::from_rgb(130, 140, 180)));
                                    ui.label(egui::RichText::new(frames).size(12.0).color(egui::Color32::WHITE));
                                    ui.end_row();
                                    ui.label(egui::RichText::new("再生時間").size(12.0).color(egui::Color32::from_rgb(130, 140, 180)));
                                    ui.label(egui::RichText::new(dur).size(12.0).color(egui::Color32::WHITE));
                                    ui.end_row();
                                });
                                if let Some(err) = &self.video_info_error {
                                    ui.add_space(6.0);
                                    ui.label(egui::RichText::new(format!("⚠ {}", err)).size(12.0).color(egui::Color32::from_rgb(255, 180, 80)));
                                }
                            });

                        ui.add_space(12.0);

                        // Output card
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(30, 32, 44))
                            .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(55, 60, 85)))
                            .corner_radius(10)
                            .inner_margin(14)
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("💾  保存先").size(16.0).strong().color(egui::Color32::from_rgb(210, 220, 255)));
                                ui.add_space(8.0);
                                let path_str = if let Some(p) = &self.output_path {
                                    p.display().to_string()
                                } else if let Some(vp) = &self.video_path {
                                    default_output_path(vp, &self.output_ext).display().to_string() + " (自動)"
                                } else {
                                    "未設定（動画選択後に自動設定）".to_string()
                                };
                                egui::Frame::new()
                                    .fill(egui::Color32::from_rgb(36, 38, 52))
                                    .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(55, 60, 85)))
                                    .corner_radius(6)
                                    .inner_margin(8)
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.label(egui::RichText::new(path_str).size(12.0).color(egui::Color32::from_rgb(200, 210, 235)));
                                    });
                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    if ui.button(egui::RichText::new("  変更  ").size(13.0)).clicked() {
                                        self.pick_output_dialog();
                                    }
                                    ui.add_space(8.0);
                                    // ext selector
                                    ui.label(egui::RichText::new("形式:").size(12.0).color(egui::Color32::from_rgb(150,160,190)));
                                    egui::ComboBox::from_id_salt("ext_combo")
                                        .selected_text(self.output_ext.to_uppercase())
                                        .width(70.0)
                                        .show_ui(ui, |ui| {
                                            if ui.selectable_value(&mut self.output_ext, "png".to_string(), "PNG").clicked() {
                                                // update path ext if needed
                                                if let Some(p) = &self.output_path {
                                                    let mut newp = p.clone();
                                                    newp.set_extension("png");
                                                    self.output_path = Some(newp);
                                                }
                                            }
                                            if ui.selectable_value(&mut self.output_ext, "jpg".to_string(), "JPEG").clicked() {
                                                if let Some(p) = &self.output_path {
                                                    let mut newp = p.clone();
                                                    newp.set_extension("jpg");
                                                    self.output_path = Some(newp);
                                                }
                                            }
                                        });
                                });
                            });

                        ui.add_space(12.0);

                        // Preset card
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(30, 32, 44))
                            .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(55, 60, 85)))
                            .corner_radius(10)
                            .inner_margin(14)
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("✨  仕上がり").size(16.0).strong().color(egui::Color32::from_rgb(210, 220, 255)));
                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    for preset in [Preset::Fine, Preset::Standard, Preset::Light] {
                                        let is_selected = self.preset == preset;
                                        let btn_fill = if is_selected {
                                            egui::Color32::from_rgb(70, 85, 160)
                                        } else {
                                            egui::Color32::from_rgb(45, 48, 64)
                                        };
                                        let btn = egui::Button::new(egui::RichText::new(preset.label()).size(13.0).strong().color(if is_selected { egui::Color32::WHITE } else { egui::Color32::from_rgb(200,210,235) }))
                                            .fill(btn_fill)
                                            .stroke(egui::Stroke::new(1.0_f32, if is_selected { egui::Color32::from_rgb(90,110,200) } else { egui::Color32::from_rgb(65,70,100) }))
                                            .corner_radius(6);
                                        if ui.add(btn).clicked() {
                                            self.preset = preset;
                                            self.preset.apply(&mut self.params);
                                        }
                                    }
                                });
                                ui.add_space(10.0);
                                // advanced toggle
                                let adv_label = if self.show_advanced { "▲ 詳細設定を隠す" } else { "▼ 詳細設定を表示" };
                                if ui.button(egui::RichText::new(adv_label).size(12.0).color(egui::Color32::from_rgb(150,160,200))).clicked() {
                                    self.show_advanced = !self.show_advanced;
                                }
                                if self.show_advanced {
                                    ui.add_space(8.0);
                                    ui.separator();
                                    ui.add_space(8.0);

                                    // Interval
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("間隔").size(13.0).color(egui::Color32::from_rgb(180,190,220)));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(egui::RichText::new(format!("{:.2} 秒", self.params.interval_sec)).size(13.0).color(egui::Color32::WHITE).strong());
                                        });
                                    });
                                    let mut interval = self.params.interval_sec as f32;
                                    if ui.add(egui::Slider::new(&mut interval, 0.05..=1.0).step_by(0.05).show_value(false)).changed() {
                                        self.params.interval_sec = interval as f64;
                                    }
                                    ui.add_space(6.0);

                                    // Min area
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("最小面積").size(13.0).color(egui::Color32::from_rgb(180,190,220)));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(egui::RichText::new(format!("{} px", self.params.min_area as i32)).size(13.0).color(egui::Color32::WHITE).strong());
                                        });
                                    });
                                    let mut min_area = self.params.min_area as f32;
                                    if ui.add(egui::Slider::new(&mut min_area, 10.0..=2000.0).step_by(10.0).show_value(false)).changed() {
                                        self.params.min_area = min_area as f64;
                                    }
                                    ui.add_space(6.0);

                                    // Threshold (sensitivity)
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("検出感度 (閾値)").size(13.0).color(egui::Color32::from_rgb(180,190,220)));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(egui::RichText::new(format!("{:.0}", self.params.threshold)).size(13.0).color(egui::Color32::WHITE).strong());
                                        });
                                    });
                                    let mut thr = self.params.threshold as f32;
                                    if ui.add(egui::Slider::new(&mut thr, 5.0..=50.0).step_by(1.0).show_value(false)).changed() {
                                        self.params.threshold = thr as f64;
                                    }
                                    ui.label(egui::RichText::new("※ 小さいほど敏感").size(13.0).color(egui::Color32::from_rgb(130,140,180)).italics());
                                    ui.add_space(6.0);

                                    // History
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new("履歴フレーム数").size(13.0).color(egui::Color32::from_rgb(180,190,220)));
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.label(egui::RichText::new(format!("{}", self.params.history)).size(13.0).color(egui::Color32::WHITE).strong());
                                        });
                                    });
                                    let mut hist = self.params.history as f32;
                                    if ui.add(egui::Slider::new(&mut hist, 20.0..=500.0).step_by(10.0).show_value(false)).changed() {
                                        self.params.history = hist as i32;
                                    }
                                }
                            });
                    });
                });

                // Right pane
                cols[1].vertical(|ui| {
                    egui::ScrollArea::vertical().id_salt("right_pane").show(ui, |ui| {

                        // Preview card
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(30, 32, 44))
                            .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(55, 60, 85)))
                            .corner_radius(10)
                            .inner_margin(14)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("👁  プレビュー").size(16.0).strong().color(egui::Color32::from_rgb(210,220,255)));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        // BEFORE / AFTER toggle
                                        let before_btn = egui::Button::new(egui::RichText::new("BEFORE").size(12.0).strong().color(if !self.preview_is_after { egui::Color32::WHITE } else { egui::Color32::from_rgb(160,170,210)}))
                                            .fill(if !self.preview_is_after { egui::Color32::from_rgb(70,85,160)} else { egui::Color32::from_rgb(45,48,64)})
                                            .corner_radius(6);
                                        if ui.add(before_btn).clicked() {
                                            self.preview_is_after = false;
                                        }
                                        let after_enabled = self.after_texture.is_some();
                                        let after_btn = egui::Button::new(egui::RichText::new("AFTER").size(12.0).strong().color(if self.preview_is_after { egui::Color32::WHITE } else { egui::Color32::from_rgb(160,170,210)}))
                                            .fill(if self.preview_is_after { egui::Color32::from_rgb(70,85,160)} else { egui::Color32::from_rgb(45,48,64)})
                                            .corner_radius(6);
                                        ui.add_enabled(after_enabled, after_btn).clicked().then(|| {
                                            if after_enabled { self.preview_is_after = true; }
                                        });
                                    });
                                });
                                ui.add_space(8.0);
                                // Preview area
                                let preview_h = 320.0;
                                let avail_w = ui.available_width();
                                egui::Frame::new()
                                    .fill(egui::Color32::from_rgb(18, 20, 28))
                                    .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(55,60,85)))
                                    .corner_radius(8)
                                    .inner_margin(6)
                                    .show(ui, |ui| {
                                        ui.set_width(avail_w);
                                        ui.set_height(preview_h);
                                        // Centered image
                                        let texture_opt = if self.preview_is_after {
                                            self.after_texture.as_ref().or(self.before_texture.as_ref())
                                        } else {
                                            self.before_texture.as_ref()
                                        };
                                        if let Some(tex) = texture_opt {
                                            let img_size = tex.size_vec2();
                                            // fit inside available: keep aspect
                                            let max_size = Vec2::new(avail_w - 12.0, preview_h - 12.0);
                                            let scale = (max_size.x / img_size.x).min(max_size.y / img_size.y).min(1.0);
                                            let draw_size = img_size * scale;
                                            // Center
                                            let available = ui.available_size();
                                            let offset = (available - draw_size) * 0.5;
                                            // Use allocated rect
                                            let (rect, _) = ui.allocate_exact_size(available, egui::Sense::hover());
                                            let img_rect = egui::Rect::from_min_size(rect.min + offset, draw_size);
                                            ui.put(img_rect, egui::Image::from_texture(tex).fit_to_exact_size(draw_size));
                                        } else {
                                            ui.centered_and_justified(|ui| {
                                                ui.label(egui::RichText::new("プレビューなし\n動画を選択してください").size(13.0).color(egui::Color32::from_rgb(120,130,170)).italics());
                                            });
                                        }
                                    });
                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    let can_open = self.output_path.as_ref().map(|p| p.exists()).unwrap_or(false) || self.after_texture.is_some();
                                    let open_btn = egui::Button::new(egui::RichText::new("  開く  ").size(13.0))
                                        .fill(egui::Color32::from_rgb(45,48,64))
                                        .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(65,70,100)))
                                        .corner_radius(6);
                                    if ui.add_enabled(can_open, open_btn).clicked() {
                                        self.open_output();
                                    }
                                    ui.label(egui::RichText::new("OS標準ビューアで開きます").size(13.0).color(egui::Color32::from_rgb(120,130,170)).italics());
                                });
                            });

                        ui.add_space(12.0);

                        // Execution card
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(30, 32, 44))
                            .stroke(egui::Stroke::new(1.0_f32, egui::Color32::from_rgb(55, 60, 85)))
                            .corner_radius(10)
                            .inner_margin(14)
                            .show(ui, |ui| {
                                ui.label(egui::RichText::new("▶  実行").size(16.0).strong().color(egui::Color32::from_rgb(210,220,255)));
                                ui.add_space(8.0);
                                // Status row
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("ステータス:").size(13.0).color(egui::Color32::from_rgb(150,160,190)));
                                    ui.label(egui::RichText::new(self.status.label()).size(13.0).strong().color(self.status.color()));
                                    ui.label(egui::RichText::new(format!(" — {}", self.status_detail)).size(12.0).color(egui::Color32::from_rgb(180,190,220)));
                                });
                                ui.add_space(8.0);
                                // Progress bar
                                let pct = self.progress / 100.0;
                                let progress_bar = egui::ProgressBar::new(pct)
                                    .show_percentage()
                                    .fill(egui::Color32::from_rgb(90, 120, 255))
                                    .animate(self.status == AppStatus::Processing);
                                ui.add(progress_bar);
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(format!("進捗: {:.1}%", self.progress)).size(12.0).color(egui::Color32::from_rgb(180,190,220)));
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(egui::RichText::new(format!("{:.2}s / {:.2}s", self.current_sec, self.total_sec)).size(12.0).color(egui::Color32::from_rgb(180,190,220)));
                                    });
                                });
                                ui.add_space(10.0);
                                ui.horizontal(|ui| {
                                    let is_processing = self.status == AppStatus::Processing;
                                    // Cancel button
                                    let cancel_btn = egui::Button::new(egui::RichText::new("  キャンセル  ").size(13.0).color(egui::Color32::WHITE))
                                        .fill(egui::Color32::from_rgb(180, 60, 60))
                                        .stroke(egui::Stroke::NONE)
                                        .corner_radius(6);
                                    if ui.add_enabled(is_processing, cancel_btn).clicked() {
                                        self.cancel_composite();
                                    }
                                    ui.add_space(8.0);
                                    // Composite button
                                    let comp_btn = egui::Button::new(egui::RichText::new("  合成する  ").size(14.0).strong().color(egui::Color32::WHITE))
                                        .fill(if is_processing { egui::Color32::from_rgb(60,65,85) } else { egui::Color32::from_rgb(90, 120, 255) })
                                        .stroke(egui::Stroke::NONE)
                                        .corner_radius(8);
                                    // Fill width
                                    let btn = ui.add_enabled(!is_processing, comp_btn);
                                    if btn.clicked() {
                                        self.start_composite(ctx);
                                    }
                                    // Hover feedback is automatic in egui
                                });
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new("※ 合成中はUIはフリーズしません。キャンセル可能です。").size(13.0).color(egui::Color32::from_rgb(120,130,170)).italics());
                            });
                    });
                });
            });
        });

        // Error dialog popup
        if let Some(msg) = self.error_dialog.clone() {
            let mut open = true;
            egui::Window::new("通知")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(egui::RichText::new(&msg).size(13.0).color(egui::Color32::from_rgb(230,230,230)));
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            open = false;
                        }
                    });
                });
            if !open {
                self.error_dialog = None;
            }
        }

        // Overwrite confirm
        if let Some(path) = self.overwrite_confirm.clone() {
            let mut open = true;
            let mut confirm = false;
            let mut cancel = false;
            egui::Window::new("上書き確認")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(egui::RichText::new(format!("既に存在します:\n{}\n上書きしますか？", path.display())).size(13.0));
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("上書き").strong().color(egui::Color32::from_rgb(255,100,100))).clicked() {
                            confirm = true;
                            open = false;
                        }
                        if ui.button("キャンセル").clicked() {
                            cancel = true;
                            open = false;
                        }
                    });
                });
            if confirm {
                // proceed: set flag to allow overwrite and retry
                self.overwrite_confirm = None;
                // Temporarily remove file existence check by storing a token? Simpler: remove file? But we will force overwrite by directly starting composite without check.
                // We need to bypass the overwrite check next time: we can just delete the confirm and call start_composite again but with overwrite allowed.
                // To do that, we need to set a one-time flag. Easiest: just call start_composite again but we need to avoid infinite recursion.
                // We'll directly launch composite without checking overwrite by inlining.
                // Copy logic of start_composite but skip overwrite check.
                // For simplicity, just set overwrite_confirm to None and call start_composite with a flag.
                // We'll implement by manually starting composite now (duplicate code without overwrite guard)
                let video_path = self.video_path.clone().unwrap();
                let output_path = path.clone();
                self.progress = 0.0;
                self.current_sec = 0.0;
                self.set_status(AppStatus::Processing, "合成中...");
                let (tx, rx) = channel::<CompositeEvent>();
                self.receiver = Some(rx);
                self.cancel_flag = Arc::new(AtomicBool::new(false));
                let cancel_clone = self.cancel_flag.clone();
                let req = CompositeRequest {
                    input_path: video_path,
                    output_path: output_path.clone(),
                    params: self.params.clone(),
                };
                let ctx_clone = ctx.clone();
                thread::spawn(move || {
                    let res = crate::composite::run_composite(req, tx.clone(), cancel_clone);
                    if let Err(e) = res {
                        let _ = tx.send(CompositeEvent::Error(e.to_string()));
                    }
                    ctx_clone.request_repaint();
                });
                self.output_path = Some(output_path);
            } else if cancel || !open {
                self.overwrite_confirm = None;
            }
            if !open && !confirm {
                // already handled
            }
        }

        // Close confirmation dialog (7.3)
        if self.show_close_confirm {
            let mut close_now = false;
            let mut stay = false;
            egui::Window::new("終了確認")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(egui::RichText::new("合成処理中です。\n終了すると処理は中断され、出力は保存されません。\n本当に終了しますか？").size(13.0));
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("終了する").strong().color(egui::Color32::from_rgb(255,100,100))).clicked() {
                            close_now = true;
                        }
                        if ui.button("処理を続ける").clicked() {
                            stay = true;
                        }
                    });
                });
            if close_now {
                self.cancel_composite();
                self.show_close_confirm = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if stay {
                self.show_close_confirm = false;
            }
        }

        // Request repaint if processing
        if self.status == AppStatus::Processing {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }
}
