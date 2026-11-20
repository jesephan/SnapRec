mod actions;
mod annotate;
mod autostart;
mod config;
mod record;
mod tray;

use actions::Action;
use annotate::{Shape, Tool};
use config::{Config, VideoFormat};
use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use global_hotkey::{hotkey::HotKey, GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState};
use image::RgbaImage;
use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;
use tray_icon::menu::MenuEvent;
use tray_icon::TrayIconEvent;

const PRESET_COLORS: &[[u8; 4]] = &[
    [230, 40, 40, 255],
    [255, 140, 0, 255],
    [250, 210, 0, 255],
    [40, 180, 70, 255],
    [30, 120, 255, 255],
    [150, 70, 220, 255],
    [0, 0, 0, 255],
    [255, 255, 255, 255],
];

enum Event {
    Screenshot(Option<PathBuf>),
    Displays(Vec<record::Display>),
    Region(Option<record::Region>),
    RecStatus(String),
    RecDone(Result<PathBuf, String>),
    Menu(String),
    Hotkey(u32),
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Screenshot,
    Record,
    Settings,
}

struct Editor {
    img: RgbaImage,
    tex: egui::TextureHandle,
    shapes: Vec<Shape>,
    drag_start: Option<[f32; 2]>,
}

struct App {
    cfg: Config,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    tab: Tab,
    status: String,
    // screenshot editor
    editor: Option<Editor>,
    tool: Tool,
    text_input: String,
    font: Option<ab_glyph::FontArc>,
    has_annot_font: bool,
    capturing: bool,
    // recording
    helper: Option<PathBuf>,
    ffmpeg: Option<PathBuf>,
    displays: Vec<record::Display>,
    recording: Option<record::Recording>,
    rec_busy: bool,
    selecting_region: bool,
    restore_window_after: bool,
    last_tray_secs: u64,
    // menu bar + shortcuts
    tray: Option<tray::Tray>,
    hotkeys: Option<GlobalHotKeyManager>,
    hotkey_ids: HashMap<u32, Action>,
    registered: Vec<HotKey>,
    editing_shortcut: Option<Action>,
    // window
    launch_at_login: bool,
    window_visible: bool,
    hide_frames: u32,
    quitting: bool,
}

fn c32(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

fn notify(text: &str) {
    let esc = text.replace('\\', "\\\\").replace('"', "\\\"");
    let _ = std::process::Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(format!("display notification \"{esc}\" with title \"SnapRec\""))
        .spawn();
}

fn open_path(p: &std::path::Path) {
    let _ = std::fs::create_dir_all(p);
    let _ = std::process::Command::new("/usr/bin/open").arg(p).spawn();
}

impl App {
    fn new(cc: &eframe::CreationContext, start_hidden: bool) -> Self {
        let (tx, rx) = channel();
        let ctx = cc.egui_ctx.clone();

        // Fonts: use the same font for on-screen preview and exported text
        let font_bytes = annotate::load_font_bytes();
        let font = font_bytes.clone().and_then(|b| ab_glyph::FontArc::try_from_vec(b).ok());
        let mut has_annot_font = false;
        if let Some(bytes) = font_bytes {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert("annot".into(), egui::FontData::from_owned(bytes));
            fonts.families.insert(FontFamily::Name("annot".into()), vec!["annot".into()]);
            ctx.set_fonts(fonts);
            has_annot_font = true;
        }
        ctx.style_mut(|s| s.spacing.item_spacing = Vec2::new(8.0, 6.0));

        let mut cfg = Config::load();
        for a in Action::ALL {
            cfg.hotkeys
                .entry(a.id().to_string())
                .or_insert_with(|| a.default_shortcut().unwrap_or("").to_string());
        }
        let launch_at_login = autostart::is_enabled();

        // Menu bar icon + event forwarding
        let tray = match tray::Tray::new(cfg.record_audio, cfg.full_screen, launch_at_login) {
            Ok(t) => Some(t),
            Err(e) => {
                eprintln!("tray error: {e}");
                None
            }
        };
        {
            let (tx, ctx) = (tx.clone(), ctx.clone());
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                let _ = tx.send(Event::Menu(e.id.0.clone()));
                ctx.request_repaint();
            }));
        }
        TrayIconEvent::set_event_handler(Some(|_e: TrayIconEvent| {}));
        {
            let (tx, ctx) = (tx.clone(), ctx.clone());
            GlobalHotKeyEvent::set_event_handler(Some(move |e: GlobalHotKeyEvent| {
                if e.state == HotKeyState::Pressed {
                    let _ = tx.send(Event::Hotkey(e.id));
                    ctx.request_repaint();
                }
            }));
        }

        let mut app = Self {
            cfg,
            tx,
            rx,
            tab: Tab::Screenshot,
            status: String::new(),
            editor: None,
            tool: Tool::Arrow,
            text_input: "Text".into(),
            font,
            has_annot_font,
            capturing: false,
            helper: record::helper_path(),
            ffmpeg: record::ffmpeg_path(),
            displays: vec![],
            recording: None,
            rec_busy: false,
            selecting_region: false,
            restore_window_after: false,
            last_tray_secs: u64::MAX,
            tray,
            hotkeys: GlobalHotKeyManager::new().ok(),
            hotkey_ids: HashMap::new(),
            registered: vec![],
            editing_shortcut: None,
            launch_at_login,
            window_visible: !start_hidden,
            hide_frames: if start_hidden { 3 } else { 0 },
            quitting: false,
        };
        app.apply_hotkeys();
        app.refresh_displays(&ctx);
        app
    }

    // ---------- window ----------

    fn show_window(&mut self, ctx: &egui::Context) {
        self.window_visible = true;
        self.hide_frames = 0;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    }

    fn hide_window(&mut self, ctx: &egui::Context) {
        self.window_visible = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
    }

    // ---------- shortcuts ----------

    fn apply_hotkeys(&mut self) {
        let Some(mgr) = &self.hotkeys else { return };
        let _ = mgr.unregister_all(&self.registered);
        self.registered.clear();
        self.hotkey_ids.clear();
        let mut errors = vec![];
        for &a in Action::ALL {
            let sc = self.cfg.shortcut(a).map(str::to_string);
            if let Some(t) = &self.tray {
                t.set_shortcut(a, sc.as_deref());
            }
            let Some(sc) = sc else { continue };
            if self.editing_shortcut.is_some() {
                continue; // don't fire while the user is typing a new shortcut
            }
            match HotKey::from_str(&sc) {
                Ok(hk) => match mgr.register(hk) {
                    Ok(_) => {
                        self.hotkey_ids.insert(hk.id(), a);
                        self.registered.push(hk);
                    }
                    Err(_) => errors.push(format!("{} ({})", a.label(), actions::pretty(&sc))),
                },
                Err(_) => errors.push(format!("{} (invalid)", a.label())),
            }
        }
        if !errors.is_empty() {
            self.status = format!("Shortcut already in use by another app: {}", errors.join(", "));
        }
    }

    fn set_shortcut(&mut self, a: Action, sc: Option<String>) {
        if let Some(s) = &sc {
            // a shortcut can only belong to one action
            for v in self.cfg.hotkeys.values_mut() {
                if v == s {
                    v.clear();
                }
            }
        }
        self.cfg.hotkeys.insert(a.id().to_string(), sc.unwrap_or_default());
        self.cfg.save();
    }

    fn capture_shortcut_keys(&mut self, ctx: &egui::Context) {
        let Some(a) = self.editing_shortcut else { return };
        let events = ctx.input(|i| i.events.clone());
        for ev in events {
            if let egui::Event::Key { key, physical_key, pressed: true, modifiers, .. } = ev {
                let key = physical_key.unwrap_or(key);
                if key == egui::Key::Escape {
                    self.editing_shortcut = None;
                } else if key == egui::Key::Backspace || key == egui::Key::Delete {
                    self.set_shortcut(a, None);
                    self.editing_shortcut = None;
                } else if let Some(sc) = actions::from_egui(key, modifiers) {
                    self.set_shortcut(a, Some(sc));
                    self.editing_shortcut = None;
                } else {
                    continue;
                }
                self.apply_hotkeys();
                return;
            }
        }
    }

    // ---------- actions (menu bar, shortcuts, buttons) ----------

    fn run_action(&mut self, a: Action, ctx: &egui::Context) {
        match a {
            Action::Screenshot => self.start_capture(ctx),
            Action::ToggleRecording => {
                if self.recording.is_some() {
                    self.stop_recording(ctx);
                } else {
                    self.begin_recording(ctx, self.cfg.full_screen);
                }
            }
            Action::RecordFullScreen => self.begin_recording(ctx, true),
            Action::RecordRegion => self.begin_recording(ctx, false),
            Action::RestartRecording => self.restart_recording(ctx),
            Action::ToggleAudio => {
                self.cfg.record_audio = !self.cfg.record_audio;
                self.cfg.save();
                if let Some(t) = &self.tray {
                    t.set_audio(self.cfg.record_audio);
                }
                self.status = format!(
                    "Computer audio {} (applies to the next recording)",
                    if self.cfg.record_audio { "on" } else { "off" }
                );
            }
            Action::ShowWindow => self.show_window(ctx),
            Action::OpenScreenshots => open_path(&self.cfg.image_dir),
            Action::OpenRecordings => open_path(&self.cfg.video_dir),
            Action::Quit => {
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn handle_menu(&mut self, id: &str, ctx: &egui::Context) {
        if let Some(a) = Action::from_id(id) {
            self.run_action(a, ctx);
            return;
        }
        match id {
            tray::ID_AUDIO => self.run_action(Action::ToggleAudio, ctx),
            tray::ID_FULL => {
                self.cfg.full_screen = !self.cfg.full_screen;
                self.cfg.save();
                if let Some(t) = &self.tray {
                    t.set_full(self.cfg.full_screen);
                }
            }
            tray::ID_LOGIN => self.set_launch_at_login(!self.launch_at_login),
            _ => {}
        }
    }

    fn set_launch_at_login(&mut self, on: bool) {
        match autostart::set_enabled(on) {
            Ok(_) => {
                self.launch_at_login = on;
                self.status = if on { "SnapRec will start at login" } else { "SnapRec won't start at login" }.into();
            }
            Err(e) => self.status = format!("Could not change login item: {e}"),
        }
        self.launch_at_login = autostart::is_enabled();
        if let Some(t) = &self.tray {
            t.set_login(self.launch_at_login);
        }
    }

    // ---------- Screenshot ----------

    fn start_capture(&mut self, ctx: &egui::Context) {
        if self.capturing || self.selecting_region {
            return;
        }
        self.capturing = true;
        self.tab = Tab::Screenshot;
        let was_visible = self.window_visible;
        self.hide_window(ctx);
        let tx = self.tx.clone();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            if was_visible {
                std::thread::sleep(Duration::from_millis(350));
            }
            let path = std::env::temp_dir()
                .join(format!("snaprec_{}.png", chrono::Local::now().format("%H%M%S%3f")));
            // -i interactive selection (Space toggles window mode), -x no sound
            let _ = std::process::Command::new("/usr/sbin/screencapture")
                .args(["-i", "-x"])
                .arg(&path)
                .status();
            let _ = tx.send(Event::Screenshot(path.exists().then_some(path)));
            ctx2.request_repaint();
        });
    }

    fn load_screenshot(&mut self, ctx: &egui::Context, path: PathBuf) {
        match image::open(&path) {
            Ok(img) => {
                let img = img.to_rgba8();
                let ci = egui::ColorImage::from_rgba_unmultiplied(
                    [img.width() as usize, img.height() as usize],
                    img.as_raw(),
                );
                let tex = ctx.load_texture("shot", ci, egui::TextureOptions::LINEAR);
                self.editor = Some(Editor { img, tex, shapes: vec![], drag_start: None });
                self.status = "Captured. Drag to draw arrows/boxes, click to place text.".into();
            }
            Err(e) => self.status = format!("Could not load screenshot: {e}"),
        }
        let _ = std::fs::remove_file(path);
    }

    fn rendered(&self) -> Option<RgbaImage> {
        let ed = self.editor.as_ref()?;
        Some(annotate::render(&ed.img, &ed.shapes, self.font.as_ref()))
    }

    fn copy_to_clipboard(&mut self) {
        let Some(img) = self.rendered() else { return };
        let data = arboard::ImageData {
            width: img.width() as usize,
            height: img.height() as usize,
            bytes: std::borrow::Cow::Owned(img.into_raw()),
        };
        self.status = match arboard::Clipboard::new().and_then(|mut c| c.set_image(data)) {
            Ok(_) => "Copied to clipboard ✔".into(),
            Err(e) => format!("Clipboard error: {e}"),
        };
    }

    fn save_image(&mut self, path: PathBuf) {
        let Some(img) = self.rendered() else { return };
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        let res = if ext == "jpg" || ext == "jpeg" {
            image::DynamicImage::ImageRgba8(img).to_rgb8().save(&path)
        } else {
            img.save(&path)
        };
        self.status = match res {
            Ok(_) => format!("Saved to {}", path.display()),
            Err(e) => format!("Save failed: {e}"),
        };
    }

    fn default_image_name() -> String {
        format!("Screenshot_{}.png", chrono::Local::now().format("%Y-%m-%d_%H-%M-%S"))
    }

    fn save_as(&mut self) {
        let _ = std::fs::create_dir_all(&self.cfg.image_dir);
        if let Some(p) = rfd::FileDialog::new()
            .set_directory(&self.cfg.image_dir)
            .set_file_name(Self::default_image_name())
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .save_file()
        {
            if let Some(dir) = p.parent() {
                self.cfg.image_dir = dir.to_path_buf();
                self.cfg.save();
            }
            self.save_image(p);
        }
    }

    fn quick_save(&mut self) {
        let _ = std::fs::create_dir_all(&self.cfg.image_dir);
        let p = self.cfg.image_dir.join(Self::default_image_name());
        self.save_image(p);
    }

    fn screenshot_ui(&mut self, ui: &mut egui::Ui) {
        if self.editor.is_none() {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.heading("Capture part of your screen");
                ui.add_space(10.0);
                ui.label("Drag to select a region. Press Space to pick a window, Esc to cancel.");
                if let Some(sc) = self.cfg.shortcut(Action::Screenshot) {
                    ui.label(format!("Shortcut: {}", actions::pretty(sc)));
                }
                ui.add_space(20.0);
                if ui
                    .add_sized([220.0, 44.0], egui::Button::new(egui::RichText::new("📷  Capture Region").size(18.0)))
                    .clicked()
                {
                    self.start_capture(ui.ctx());
                }
            });
            return;
        }

        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Arrow, "➡ Arrow");
            ui.selectable_value(&mut self.tool, Tool::Rect, "▭ Box");
            ui.selectable_value(&mut self.tool, Tool::Text, "T Text");
            ui.separator();
            for c in PRESET_COLORS {
                let selected = self.cfg.color == *c;
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::click());
                ui.painter().rect_filled(rect, 4.0, c32(*c));
                let stroke = if selected {
                    Stroke::new(2.5_f32, ui.visuals().strong_text_color())
                } else {
                    Stroke::new(1.0_f32, Color32::GRAY)
                };
                ui.painter().rect_stroke(rect, 4.0, stroke);
                if resp.clicked() {
                    self.cfg.color = *c;
                }
            }
            let mut col = c32(self.cfg.color);
            if egui::color_picker::color_edit_button_srgba(ui, &mut col, egui::color_picker::Alpha::Opaque).changed() {
                self.cfg.color = col.to_array();
            }
            ui.separator();
            if self.tool == Tool::Text {
                ui.label("Size");
                ui.add(egui::DragValue::new(&mut self.cfg.text_size).range(8.0..=300.0));
                ui.add(egui::TextEdit::singleline(&mut self.text_input).desired_width(160.0).hint_text("text to place"));
            } else {
                ui.label("Width");
                ui.add(egui::Slider::new(&mut self.cfg.stroke, 1.0..=30.0));
            }
        });
        ui.horizontal_wrapped(|ui| {
            let ed = self.editor.as_mut().unwrap();
            if ui.button("↶ Undo").clicked() {
                ed.shapes.pop();
            }
            if ui.button("Clear").clicked() {
                ed.shapes.clear();
            }
            ui.separator();
            if ui.button("📋 Copy").clicked() {
                self.copy_to_clipboard();
            }
            if ui.button("💾 Save As…").clicked() {
                self.save_as();
            }
            if ui
                .button("⚡ Quick Save")
                .on_hover_text(format!("Save into {}", self.cfg.image_dir.display()))
                .clicked()
            {
                self.quick_save();
            }
            ui.separator();
            if ui.button("📷 New Capture").clicked() {
                self.start_capture(ui.ctx());
            }
            if ui.button("✖ Close").clicked() {
                self.editor = None;
            }
        });
        ui.separator();

        let Some(ed) = self.editor.as_mut() else { return };
        let (iw, ih) = (ed.img.width() as f32, ed.img.height() as f32);
        let avail = ui.available_size();
        let scale = (avail.x / iw).min(avail.y / ih).clamp(0.01, 1.0);
        let (resp, painter) = ui.allocate_painter(Vec2::new(iw * scale, ih * scale), Sense::click_and_drag());
        let origin = resp.rect.min;
        let to_screen = |p: [f32; 2]| Pos2::new(origin.x + p[0] * scale, origin.y + p[1] * scale);
        let to_img = |p: Pos2| [((p.x - origin.x) / scale).clamp(0.0, iw), ((p.y - origin.y) / scale).clamp(0.0, ih)];

        painter.image(ed.tex.id(), resp.rect, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);

        let color = self.cfg.color;
        let width = self.cfg.stroke;
        let mut preview: Option<Shape> = None;
        match self.tool {
            Tool::Arrow | Tool::Rect => {
                if resp.drag_started() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        ed.drag_start = Some(to_img(p));
                    }
                }
                if let (Some(a), Some(p)) = (ed.drag_start, resp.interact_pointer_pos().or(ui.ctx().pointer_latest_pos())) {
                    let b = to_img(p);
                    let shape = if self.tool == Tool::Arrow {
                        Shape::Arrow { a, b, color, width }
                    } else {
                        Shape::Rect { a: [a[0].min(b[0]), a[1].min(b[1])], b: [a[0].max(b[0]), a[1].max(b[1])], color, width }
                    };
                    if resp.drag_stopped() {
                        ed.drag_start = None;
                        ed.shapes.push(shape);
                    } else {
                        preview = Some(shape);
                    }
                }
            }
            Tool::Text => {
                if resp.clicked() && !self.text_input.trim().is_empty() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        ed.shapes.push(Shape::Text { pos: to_img(p), text: self.text_input.clone(), color, size: self.cfg.text_size });
                    }
                }
            }
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(if self.tool == Tool::Text { egui::CursorIcon::Text } else { egui::CursorIcon::Crosshair });
        }

        let family = if self.has_annot_font { FontFamily::Name("annot".into()) } else { FontFamily::Proportional };
        for s in ed.shapes.iter().chain(preview.iter()) {
            match s {
                Shape::Arrow { a, b, color, width } => {
                    if let Some((shaft, l, r)) = annotate::arrow_head(*a, *b, *width) {
                        let c = c32(*color);
                        painter.line_segment([to_screen(*a), to_screen(shaft)], Stroke::new(width * scale, c));
                        painter.add(egui::Shape::convex_polygon(vec![to_screen(*b), to_screen(l), to_screen(r)], c, Stroke::NONE));
                    }
                }
                Shape::Rect { a, b, color, width } => {
                    painter.rect_stroke(Rect::from_two_pos(to_screen(*a), to_screen(*b)), 0.0, Stroke::new(width * scale, c32(*color)));
                }
                Shape::Text { pos, text, color, size } => {
                    for (i, line) in text.lines().enumerate() {
                        painter.text(
                            to_screen([pos[0], pos[1] + i as f32 * size * 1.2]),
                            egui::Align2::LEFT_TOP,
                            line,
                            FontId::new((size * scale).max(1.0), family.clone()),
                            c32(*color),
                        );
                    }
                }
            }
        }
    }

    // ---------- Recording ----------

    fn refresh_displays(&mut self, ctx: &egui::Context) {
        if let Some(h) = self.helper.clone() {
            let (tx, ctx) = (self.tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                let _ = tx.send(Event::Displays(record::list_displays(&h)));
                ctx.request_repaint();
            });
        }
    }

    fn begin_recording(&mut self, ctx: &egui::Context, full_screen: bool) {
        if self.recording.is_some() || self.rec_busy || self.selecting_region || self.capturing {
            return;
        }
        let Some(helper) = self.helper.clone() else {
            self.status = "Recorder helper missing — please reinstall SnapRec.".into();
            self.show_window(ctx);
            return;
        };
        if self.cfg.video_format == VideoFormat::Gif && self.ffmpeg.is_none() {
            self.status = "GIF needs ffmpeg: run  brew install ffmpeg  then restart SnapRec.".into();
            self.show_window(ctx);
            return;
        }
        self.restore_window_after = self.window_visible;
        if full_screen {
            self.start_recording(ctx, None);
        } else {
            self.selecting_region = true;
            self.hide_window(ctx);
            let (tx, ctx) = (self.tx.clone(), ctx.clone());
            std::thread::spawn(move || {
                let r = record::select_region(&helper);
                let _ = tx.send(Event::Region(r));
                ctx.request_repaint();
            });
        }
    }

    fn start_recording(&mut self, ctx: &egui::Context, region: Option<record::Region>) {
        let Some(helper) = self.helper.clone() else { return };
        let display = self.cfg.display_id.filter(|id| self.displays.iter().any(|d| d.id == *id));
        match record::start(&helper, &self.cfg, display, region) {
            Ok(r) => {
                self.status = format!("Recording → {}", r.writing_path.display());
                self.recording = Some(r);
                self.cfg.save();
                if self.cfg.minimize_on_record {
                    self.hide_window(ctx);
                }
            }
            Err(e) => {
                self.status = e;
                self.show_window(ctx);
            }
        }
    }

    fn restart_recording(&mut self, ctx: &egui::Context) {
        let Some(rec) = self.recording.take() else { return };
        let region = rec.region;
        rec.discard();
        self.status = "Restarted recording".into();
        self.start_recording(ctx, region);
    }

    fn stop_recording(&mut self, ctx: &egui::Context) {
        let Some(rec) = self.recording.take() else { return };
        self.rec_busy = true;
        self.status = "Stopping…".into();
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let (tx2, ctx2) = (tx.clone(), ctx.clone());
            let res = rec.finish(move |s| {
                let _ = tx2.send(Event::RecStatus(s));
                ctx2.request_repaint();
            });
            let _ = tx.send(Event::RecDone(res));
            ctx.request_repaint();
        });
    }

    fn record_ui(&mut self, ui: &mut egui::Ui) {
        if self.helper.is_none() {
            ui.colored_label(Color32::from_rgb(220, 120, 0), "Recorder helper not found — run SnapRec from the installed .app bundle.");
        }
        let recording = self.recording.is_some();
        ui.add_enabled_ui(!recording && !self.rec_busy, |ui| {
            egui::Grid::new("rec").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                ui.label("Area");
                ui.vertical(|ui| {
                    if ui.checkbox(&mut self.cfg.full_screen, "Record entire screen").changed() {
                        if let Some(t) = &self.tray {
                            t.set_full(self.cfg.full_screen);
                        }
                    }
                    if !self.cfg.full_screen {
                        ui.weak("You'll drag to select an area when recording starts.");
                    }
                });
                ui.end_row();

                if self.cfg.full_screen {
                    ui.label("Screen");
                    ui.horizontal(|ui| {
                        let current = self
                            .displays
                            .iter()
                            .find(|d| Some(d.id) == self.cfg.display_id)
                            .map(|d| format!("{} ({}×{})", d.name, d.width, d.height))
                            .unwrap_or_else(|| "Main screen".into());
                        egui::ComboBox::from_id_salt("screen").selected_text(current).show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.cfg.display_id, None, "Main screen");
                            for d in &self.displays {
                                let main = if d.main { " — main" } else { "" };
                                ui.selectable_value(&mut self.cfg.display_id, Some(d.id), format!("{} ({}×{}){main}", d.name, d.width, d.height));
                            }
                        });
                        if ui.button("⟳").on_hover_text("Refresh screens").clicked() {
                            self.refresh_displays(ui.ctx());
                        }
                    });
                    ui.end_row();
                }

                ui.label("Format");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.cfg.video_format, VideoFormat::Mp4, "MP4");
                    ui.radio_value(&mut self.cfg.video_format, VideoFormat::Gif, "GIF");
                });
                ui.end_row();

                ui.label("Audio");
                ui.add_enabled_ui(self.cfg.video_format == VideoFormat::Mp4, |ui| {
                    let mut on = self.cfg.record_audio && self.cfg.video_format == VideoFormat::Mp4;
                    if ui.checkbox(&mut on, "Record computer & app audio").changed() {
                        self.cfg.record_audio = on;
                        if let Some(t) = &self.tray {
                            t.set_audio(on);
                        }
                    }
                    if self.cfg.video_format == VideoFormat::Gif {
                        ui.weak("(GIFs have no sound)");
                    }
                });
                ui.end_row();

                match self.cfg.video_format {
                    VideoFormat::Mp4 => {
                        ui.label("Frame rate");
                        ui.add(egui::Slider::new(&mut self.cfg.mp4_fps, 5..=60).suffix(" fps"));
                        ui.end_row();
                    }
                    VideoFormat::Gif => {
                        ui.label("GIF frame rate");
                        ui.horizontal(|ui| {
                            ui.add(egui::Slider::new(&mut self.cfg.gif_fps, 1..=30).suffix(" fps"));
                            ui.weak("lower = fewer frames, smaller file");
                        });
                        ui.end_row();
                        ui.label("GIF width");
                        egui::ComboBox::from_id_salt("gifw")
                            .selected_text(if self.cfg.gif_width == 0 { "Original".to_string() } else { format!("{} px", self.cfg.gif_width) })
                            .show_ui(ui, |ui| {
                                for w in [0u32, 1920, 1280, 960, 640, 480] {
                                    let label = if w == 0 { "Original".to_string() } else { format!("{w} px") };
                                    ui.selectable_value(&mut self.cfg.gif_width, w, label);
                                }
                            });
                        ui.end_row();
                        if self.ffmpeg.is_none() {
                            ui.label("");
                            ui.colored_label(Color32::from_rgb(220, 120, 0), "GIF needs ffmpeg:  brew install ffmpeg");
                            ui.end_row();
                        }
                    }
                }

                ui.label("Options");
                ui.vertical(|ui| {
                    ui.checkbox(&mut self.cfg.capture_cursor, "Show mouse cursor");
                    ui.checkbox(&mut self.cfg.half_resolution, "Half resolution (smaller files on Retina)");
                    ui.checkbox(&mut self.cfg.minimize_on_record, "Hide SnapRec window while recording");
                });
                ui.end_row();

                ui.label("Save to");
                ui.horizontal(|ui| {
                    ui.monospace(self.cfg.video_dir.display().to_string());
                    if ui.button("Choose…").clicked() {
                        if let Some(d) = rfd::FileDialog::new().set_directory(&self.cfg.video_dir).pick_folder() {
                            self.cfg.video_dir = d;
                            self.cfg.save();
                        }
                    }
                });
                ui.end_row();
            });
        });

        ui.add_space(16.0);
        if let Some(rec) = &self.recording {
            let secs = rec.started.elapsed().as_secs();
            let mut stop = false;
            let mut restart = false;
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("● REC  {:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60))
                        .color(Color32::from_rgb(230, 40, 40))
                        .size(22.0)
                        .strong(),
                );
                stop = ui.add_sized([120.0, 36.0], egui::Button::new(egui::RichText::new("⏹  Stop").size(18.0))).clicked();
                restart = ui.add_sized([120.0, 36.0], egui::Button::new("↻ Restart")).on_hover_text("Discard and start over").clicked();
            });
            ui.label("Writing directly to disk — no time limit. You can also stop from the bird icon in the menu bar.");
            if stop {
                self.stop_recording(ui.ctx());
            } else if restart {
                self.restart_recording(ui.ctx());
            }
        } else if self.rec_busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Processing…");
            });
        } else {
            let label = if self.cfg.full_screen { "⏺  Start Recording" } else { "⏺  Select Area & Record" };
            if ui
                .add_enabled(self.helper.is_some(), egui::Button::new(egui::RichText::new(label).size(18.0)).min_size(Vec2::new(220.0, 40.0)))
                .clicked()
            {
                self.begin_recording(ui.ctx(), self.cfg.full_screen);
            }
        }
    }

    // ---------- Settings ----------

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("General");
            let mut login = self.launch_at_login;
            if ui.checkbox(&mut login, "Start SnapRec when I log in").changed() {
                self.set_launch_at_login(login);
            }
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Screenshot folder (Quick Save):");
                ui.monospace(self.cfg.image_dir.display().to_string());
                if ui.button("Choose…").clicked() {
                    if let Some(d) = rfd::FileDialog::new().set_directory(&self.cfg.image_dir).pick_folder() {
                        self.cfg.image_dir = d;
                        self.cfg.save();
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Recording folder:");
                ui.monospace(self.cfg.video_dir.display().to_string());
                if ui.button("Choose…").clicked() {
                    if let Some(d) = rfd::FileDialog::new().set_directory(&self.cfg.video_dir).pick_folder() {
                        self.cfg.video_dir = d;
                        self.cfg.save();
                    }
                }
            });

            ui.add_space(14.0);
            ui.heading("Keyboard shortcuts");
            ui.weak("Global shortcuts work from any app. Click a shortcut, then press the new key combination (Esc cancels, Delete clears).");
            ui.add_space(6.0);
            egui::Grid::new("shortcuts").num_columns(3).spacing([16.0, 6.0]).striped(true).show(ui, |ui| {
                for &a in Action::ALL {
                    ui.label(a.label());
                    let editing = self.editing_shortcut == Some(a);
                    let text = if editing {
                        "Press keys…".to_string()
                    } else {
                        self.cfg.shortcut(a).map(actions::pretty).unwrap_or_else(|| "—".into())
                    };
                    let btn = egui::Button::new(egui::RichText::new(text).monospace()).min_size(Vec2::new(120.0, 0.0)).selected(editing);
                    if ui.add(btn).clicked() {
                        self.editing_shortcut = if editing { None } else { Some(a) };
                        self.apply_hotkeys();
                    }
                    ui.horizontal(|ui| {
                        if ui.small_button("Clear").clicked() {
                            self.set_shortcut(a, None);
                            self.apply_hotkeys();
                        }
                        if let Some(d) = a.default_shortcut() {
                            if ui.small_button("Default").clicked() {
                                self.set_shortcut(a, Some(d.to_string()));
                                self.apply_hotkeys();
                            }
                        }
                    });
                    ui.end_row();
                }
            });

            ui.add_space(14.0);
            ui.heading("About");
            ui.label(match &self.ffmpeg {
                Some(p) => format!("ffmpeg (for GIF): {}", p.display()),
                None => "ffmpeg (for GIF): not found — brew install ffmpeg".into(),
            });
            ui.label("SnapRec lives in the menu bar (bird icon). Closing this window keeps it running; use Quit from the menu to exit.");
            ui.label("Screen Recording permission: System Settings → Privacy & Security → Screen & System Audio Recording.");
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.hide_frames > 0 {
            self.hide_frames -= 1;
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        // Closing the window hides it; the app keeps running in the menu bar.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.hide_window(ctx);
        }

        self.capture_shortcut_keys(ctx);

        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::Menu(id) => self.handle_menu(&id, ctx),
                Event::Hotkey(id) => {
                    if let Some(a) = self.hotkey_ids.get(&id).copied() {
                        self.run_action(a, ctx);
                    }
                }
                Event::Screenshot(p) => {
                    self.capturing = false;
                    match p {
                        Some(p) => {
                            self.load_screenshot(ctx, p);
                            self.show_window(ctx);
                        }
                        None => self.status = "Capture cancelled".into(),
                    }
                }
                Event::Displays(d) => self.displays = d,
                Event::Region(r) => {
                    self.selecting_region = false;
                    match r {
                        Some(r) => self.start_recording(ctx, Some(r)),
                        None => {
                            self.status = "Area selection cancelled".into();
                            if self.restore_window_after {
                                self.show_window(ctx);
                            }
                        }
                    }
                }
                Event::RecStatus(s) => self.status = s,
                Event::RecDone(r) => {
                    self.rec_busy = false;
                    match r {
                        Ok(p) => {
                            self.status = format!("Saved recording to {}", p.display());
                            notify(&format!("Recording saved: {}", p.file_name().unwrap_or_default().to_string_lossy()));
                        }
                        Err(e) => {
                            self.status = e;
                            self.show_window(ctx);
                        }
                    }
                    if self.restore_window_after {
                        self.show_window(ctx);
                    }
                }
            }
        }

        // Recording state -> menu bar
        let mut crashed = None;
        if let Some(rec) = self.recording.as_mut() {
            crashed = rec.exited();
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        if let Some(err) = crashed {
            let rec = self.recording.take().unwrap();
            self.status = err;
            if rec.writing_path.exists() {
                self.status.push_str(&format!("\nPartial file: {}", rec.writing_path.display()));
            }
            self.show_window(ctx);
        }
        if let Some(t) = self.tray.as_mut() {
            let secs = self.recording.as_ref().map(|r| r.started.elapsed().as_secs());
            t.set_recording_icon(secs.is_some());
            let key = secs.unwrap_or(u64::MAX - 1);
            if key != self.last_tray_secs {
                self.last_tray_secs = key;
                t.set_recording(secs.is_some(), secs);
            }
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Screenshot, "📷 Screenshot");
                ui.selectable_value(&mut self.tab, Tab::Record, "🎬 Record");
                ui.selectable_value(&mut self.tab, Tab::Settings, "⚙ Settings");
                if self.recording.is_some() {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("⏹ Stop recording").clicked() {
                            self.stop_recording(ctx);
                        }
                        ui.colored_label(Color32::from_rgb(230, 40, 40), "● REC");
                    });
                }
            });
            ui.add_space(4.0);
        });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.add_space(2.0);
            ui.label(if self.status.is_empty() { "Ready" } else { &self.status });
            ui.add_space(2.0);
        });
        egui::CentralPanel::default().show(ctx, |ui| match self.tab {
            Tab::Screenshot => self.screenshot_ui(ui),
            Tab::Record => self.record_ui(ui),
            Tab::Settings => self.settings_ui(ui),
        });
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.cfg.save();
        if let Some(rec) = self.recording.take() {
            let _ = rec.finish(|_| {});
        }
    }
}

fn main() -> eframe::Result<()> {
    let start_hidden = std::env::args().any(|a| a == "--hidden");
    let icon = image::load_from_memory(include_bytes!("../assets/icon_1024.png"))
        .ok()
        .map(|i| {
            let i = i.resize(256, 256, image::imageops::FilterType::Lanczos3).to_rgba8();
            let (w, h) = i.dimensions();
            egui::IconData { rgba: i.into_raw(), width: w, height: h }
        });
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("SnapRec")
        .with_inner_size([1000.0, 720.0])
        .with_min_inner_size([640.0, 420.0]);
    if let Some(icon) = icon {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        // Menu bar app: no Dock icon
        event_loop_builder: Some(Box::new(|builder| {
            use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
            builder.with_activation_policy(ActivationPolicy::Accessory);
        })),
        ..Default::default()
    };
    eframe::run_native("SnapRec", options, Box::new(move |cc| Ok(Box::new(App::new(cc, start_hidden)))))
}
