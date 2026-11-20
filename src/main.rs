mod annotate;
mod autostart;
mod config;
mod record;

use annotate::{Shape, Tool};
use config::{Config, VideoFormat};
use eframe::egui::{self, Color32, FontFamily, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use image::RgbaImage;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

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
    Screens(Vec<(String, String)>),
    RecStatus(String),
    RecDone(Result<PathBuf, String>),
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
    editor: Option<Editor>,
    tool: Tool,
    text_input: String,
    font: Option<ab_glyph::FontArc>,
    has_annot_font: bool,
    ffmpeg: Option<PathBuf>,
    screens: Vec<(String, String)>,
    screen_sel: usize,
    recording: Option<record::Recording>,
    rec_busy: bool,
    launch_at_login: bool,
    capturing: bool,
}

fn c32(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

impl App {
    fn new(cc: &eframe::CreationContext) -> Self {
        let (tx, rx) = channel();
        let font_bytes = annotate::load_font_bytes();
        let font = font_bytes
            .clone()
            .and_then(|b| ab_glyph::FontArc::try_from_vec(b).ok());
        let mut has_annot_font = false;
        if let Some(bytes) = font_bytes {
            let mut fonts = egui::FontDefinitions::default();
            fonts
                .font_data
                .insert("annot".into(), egui::FontData::from_owned(bytes));
            fonts
                .families
                .insert(FontFamily::Name("annot".into()), vec!["annot".into()]);
            cc.egui_ctx.set_fonts(fonts);
            has_annot_font = true;
        }
        cc.egui_ctx.style_mut(|s| s.spacing.item_spacing = Vec2::new(8.0, 6.0));

        let ffmpeg = record::ffmpeg_path();
        let mut app = Self {
            cfg: Config::load(),
            tx,
            rx,
            tab: Tab::Screenshot,
            status: String::new(),
            editor: None,
            tool: Tool::Arrow,
            text_input: "Text".into(),
            font,
            has_annot_font,
            ffmpeg,
            screens: vec![],
            screen_sel: 0,
            recording: None,
            rec_busy: false,
            launch_at_login: autostart::is_enabled(),
            capturing: false,
        };
        app.refresh_screens(&cc.egui_ctx);
        app
    }

    fn refresh_screens(&mut self, ctx: &egui::Context) {
        if let Some(ff) = self.ffmpeg.clone() {
            let tx = self.tx.clone();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Event::Screens(record::list_screens(&ff)));
                ctx.request_repaint();
            });
        }
    }

    // ---------- Screenshot ----------

    fn start_capture(&mut self, ctx: &egui::Context) {
        if self.capturing {
            return;
        }
        self.capturing = true;
        self.tab = Tab::Screenshot;
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        let tx = self.tx.clone();
        let ctx2 = ctx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(400));
            let path = std::env::temp_dir().join(format!(
                "snaprec_{}.png",
                chrono::Local::now().format("%H%M%S%3f")
            ));
            // -i interactive region selection (Space toggles window mode), -x no sound
            let _ = std::process::Command::new("/usr/sbin/screencapture")
                .args(["-i", "-x"])
                .arg(&path)
                .status();
            let res = if path.exists() { Some(path) } else { None };
            let _ = tx.send(Event::Screenshot(res));
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
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
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

        // Toolbar row 1: tools & style
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
        // Toolbar row 2: actions
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
        let scale = (avail.x / iw).min(avail.y / ih).min(1.0).max(0.01);
        let size = Vec2::new(iw * scale, ih * scale);
        let (resp, painter) = ui.allocate_painter(size, Sense::click_and_drag());
        let origin = resp.rect.min;
        let to_screen = |p: [f32; 2]| Pos2::new(origin.x + p[0] * scale, origin.y + p[1] * scale);
        let to_img = |p: Pos2| [((p.x - origin.x) / scale).clamp(0.0, iw), ((p.y - origin.y) / scale).clamp(0.0, ih)];

        painter.image(
            ed.tex.id(),
            resp.rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        // Input handling
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
                        ed.shapes.push(Shape::Text {
                            pos: to_img(p),
                            text: self.text_input.clone(),
                            color,
                            size: self.cfg.text_size,
                        });
                    }
                }
            }
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(if self.tool == Tool::Text {
                egui::CursorIcon::Text
            } else {
                egui::CursorIcon::Crosshair
            });
        }

        // Draw shapes
        let family = if self.has_annot_font { FontFamily::Name("annot".into()) } else { FontFamily::Proportional };
        for s in ed.shapes.iter().chain(preview.iter()) {
            match s {
                Shape::Arrow { a, b, color, width } => {
                    if let Some((shaft, l, r)) = annotate::arrow_head(*a, *b, *width) {
                        let c = c32(*color);
                        painter.line_segment([to_screen(*a), to_screen(shaft)], Stroke::new(width * scale, c));
                        painter.add(egui::Shape::convex_polygon(
                            vec![to_screen(*b), to_screen(l), to_screen(r)],
                            c,
                            Stroke::NONE,
                        ));
                    }
                }
                Shape::Rect { a, b, color, width } => {
                    painter.rect_stroke(
                        Rect::from_two_pos(to_screen(*a), to_screen(*b)),
                        0.0,
                        Stroke::new(width * scale, c32(*color)),
                    );
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

    fn start_recording(&mut self, ctx: &egui::Context) {
        let Some(ff) = self.ffmpeg.clone() else {
            self.status = "ffmpeg not found. Install it with: brew install ffmpeg".into();
            return;
        };
        let idx = self
            .screens
            .get(self.screen_sel)
            .map(|s| s.0.clone())
            .unwrap_or_else(|| "1".into());
        match record::start(&ff, &self.cfg, &idx) {
            Ok(r) => {
                self.status = format!("Recording → {}", r.temp_path.display());
                self.recording = Some(r);
                self.cfg.save();
                if self.cfg.minimize_on_record {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                }
            }
            Err(e) => self.status = e,
        }
    }

    fn stop_recording(&mut self, ctx: &egui::Context) {
        let Some(rec) = self.recording.take() else { return };
        self.rec_busy = true;
        self.status = "Stopping…".into();
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let tx2 = tx.clone();
            let ctx2 = ctx.clone();
            let res = rec.finish(move |s| {
                let _ = tx2.send(Event::RecStatus(s));
                ctx2.request_repaint();
            });
            let _ = tx.send(Event::RecDone(res));
            ctx.request_repaint();
        });
    }

    fn record_ui(&mut self, ui: &mut egui::Ui) {
        if self.ffmpeg.is_none() {
            ui.colored_label(
                Color32::from_rgb(220, 120, 0),
                "ffmpeg is required for recording. Install it with:  brew install ffmpeg  — then restart SnapRec.",
            );
            ui.add_space(8.0);
        }
        let recording = self.recording.is_some();
        ui.add_enabled_ui(!recording && !self.rec_busy, |ui| {
            egui::Grid::new("rec").num_columns(2).spacing([16.0, 10.0]).show(ui, |ui| {
                ui.label("Screen");
                ui.horizontal(|ui| {
                    let sel = self.screens.get(self.screen_sel).map(|s| s.1.clone()).unwrap_or("Main screen".into());
                    egui::ComboBox::from_id_salt("screen").selected_text(sel).show_ui(ui, |ui| {
                        for (i, s) in self.screens.iter().enumerate() {
                            ui.selectable_value(&mut self.screen_sel, i, &s.1);
                        }
                    });
                    if ui.button("⟳").on_hover_text("Refresh screens").clicked() {
                        self.refresh_screens(ui.ctx());
                    }
                });
                ui.end_row();

                ui.label("Format");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.cfg.video_format, VideoFormat::Mp4, "MP4");
                    ui.radio_value(&mut self.cfg.video_format, VideoFormat::Gif, "GIF");
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
                            ui.label("(lower = fewer frames, smaller file)");
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
                    }
                }

                ui.label("Options");
                ui.vertical(|ui| {
                    ui.checkbox(&mut self.cfg.half_resolution, "Half resolution (smaller files on Retina)");
                    ui.checkbox(&mut self.cfg.capture_cursor, "Show mouse cursor & clicks");
                    ui.checkbox(&mut self.cfg.minimize_on_record, "Minimize SnapRec while recording");
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
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("● REC  {:02}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60))
                        .color(Color32::from_rgb(230, 40, 40))
                        .size(22.0)
                        .strong(),
                );
                if ui.add_sized([140.0, 36.0], egui::Button::new(egui::RichText::new("⏹  Stop").size(18.0))).clicked() {
                    self.stop_recording(ui.ctx());
                }
            });
            ui.label("Video is being written directly to disk; there is no time limit.");
        } else if self.rec_busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Processing…");
            });
        } else if ui
            .add_enabled(
                self.ffmpeg.is_some(),
                egui::Button::new(egui::RichText::new("⏺  Start Recording").size(18.0)).min_size(Vec2::new(200.0, 40.0)),
            )
            .clicked()
        {
            self.start_recording(ui.ctx());
        }
    }

    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("General");
        if ui.checkbox(&mut self.launch_at_login, "Start SnapRec when I log in").changed() {
            if let Err(e) = autostart::set_enabled(self.launch_at_login) {
                self.status = format!("Could not change login item: {e}");
                self.launch_at_login = autostart::is_enabled();
            } else {
                self.status = if self.launch_at_login { "Will start at login" } else { "Won't start at login" }.into();
            }
        }
        ui.add_space(10.0);
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
        ui.add_space(10.0);
        ui.label(match &self.ffmpeg {
            Some(p) => format!("ffmpeg: {}", p.display()),
            None => "ffmpeg: not found (brew install ffmpeg)".into(),
        });
        ui.add_space(10.0);
        ui.label("Tip: macOS asks for Screen Recording permission the first time. Grant it in System Settings → Privacy & Security → Screen & System Audio Recording, then restart SnapRec.");
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::Screenshot(p) => {
                    self.capturing = false;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    match p {
                        Some(p) => self.load_screenshot(ctx, p),
                        None => self.status = "Capture cancelled".into(),
                    }
                }
                Event::Screens(s) => {
                    self.screens = s;
                    if self.screen_sel >= self.screens.len() {
                        self.screen_sel = 0;
                    }
                }
                Event::RecStatus(s) => self.status = s,
                Event::RecDone(r) => {
                    self.rec_busy = false;
                    self.status = match r {
                        Ok(p) => format!("Saved recording to {}", p.display()),
                        Err(e) => e,
                    };
                    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
            }
        }
        if let Some(rec) = self.recording.as_mut() {
            if let Some(err) = rec.exited() {
                let rec = self.recording.take().unwrap();
                self.status = err;
                // Keep whatever was captured, if anything
                if rec.temp_path.exists() {
                    self.status.push_str(&format!("\nPartial file: {}", rec.temp_path.display()));
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            }
            ctx.request_repaint_after(Duration::from_millis(250));
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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("SnapRec")
            .with_inner_size([1000.0, 720.0])
            .with_min_inner_size([640.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native("SnapRec", options, Box::new(|cc| Ok(Box::new(App::new(cc)))))
}
