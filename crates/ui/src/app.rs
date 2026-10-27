use std::path::PathBuf;

use paint_app::{AppSession, Command};
use paint_core::{Bgra, Rect, Size};
use paint_document::Document;

const DEFAULT_WIDTH: u32 = 800;
const DEFAULT_HEIGHT: u32 = 600;

/// Original development-preview shell. `AppSession` remains the only document writer.
pub struct PaintApp {
    session: AppSession,
    fill_color: Bgra,
    selection_input: SelectionInput,
    texture: Option<egui::TextureHandle>,
    texture_dirty: bool,
    next_layer_number: usize,
    status: String,
    path_dialog: Option<PathDialog>,
}

impl PaintApp {
    pub fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self {
            session: default_session(),
            fill_color: Bgra::BLACK,
            selection_input: SelectionInput::default(),
            texture: None,
            texture_dirty: true,
            next_layer_number: 1,
            status: "New 800×600 opaque-white document".to_owned(),
            path_dialog: None,
        };
        app.refresh_texture(&creation_context.egui_ctx);
        app
    }

    pub const fn session(&self) -> &AppSession {
        &self.session
    }

    pub const fn fill_color(&self) -> Bgra {
        self.fill_color
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    fn new_document(&mut self) {
        self.session = default_session();
        self.selection_input = SelectionInput::default();
        self.next_layer_number = 1;
        self.path_dialog = None;
        self.texture_dirty = true;
        self.status = "New 800×600 opaque-white document".to_owned();
    }

    fn execute(&mut self, command: Command, success: impl Into<String>) -> bool {
        match self.session.execute(command) {
            Ok(_changed) => {
                self.texture_dirty = true;
                self.status = success.into();
                true
            }
            Err(error) => {
                self.status = format!("Operation failed: {error}");
                false
            }
        }
    }

    fn add_layer(&mut self) {
        let number = self.next_layer_number;
        let name = format!("Layer {number}");
        let message = format!("Added {name}");
        if self.execute(
            Command::AddLayer {
                name,
                opacity: u8::MAX,
                visible: true,
            },
            message,
        ) {
            self.next_layer_number = self.next_layer_number.saturating_add(1);
        }
    }

    fn select_rectangle(&mut self) {
        let input = self.selection_input;
        match Rect::new(input.left, input.top, input.right, input.bottom) {
            Ok(rect) => {
                self.execute(Command::SelectRect(rect), "Rectangle selection updated");
            }
            Err(error) => {
                self.status = format!("Selection failed: {error}");
            }
        }
    }

    fn refresh_texture(&mut self, context: &egui::Context) {
        if !self.texture_dirty {
            return;
        }
        self.texture_dirty = false;
        let surface = match self.session.compose() {
            Ok(surface) => surface,
            Err(error) => {
                self.status = format!("Canvas composition failed: {error}");
                return;
            }
        };
        let rgba = match surface.to_rgba_bytes() {
            Ok(rgba) => rgba,
            Err(error) => {
                self.status = format!("Canvas conversion failed: {error}");
                return;
            }
        };
        let image = egui::ColorImage::from_rgba_unmultiplied(
            [surface.width() as usize, surface.height() as usize],
            &rgba,
        );
        if let Some(texture) = &mut self.texture
            && texture.size() == image.size
        {
            texture.set(image, egui::TextureOptions::NEAREST);
            return;
        }
        self.texture = Some(context.load_texture(
            "paint-r-composed-document",
            image,
            egui::TextureOptions::NEAREST,
        ));
    }

    fn menu_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("paint-r-menu").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New").clicked() {
                        self.new_document();
                        ui.close();
                    }
                    if ui.button("Open PNG…").clicked() {
                        self.path_dialog = Some(PathDialog::new(PathDialogKind::Open));
                        ui.close();
                    }
                    if ui.button("Export PNG…").clicked() {
                        self.path_dialog = Some(PathDialog::new(PathDialogKind::Export));
                        ui.close();
                    }
                });
                ui.menu_button("Adjustments", |ui| {
                    if ui.button("Invert Colors").clicked() {
                        self.execute(Command::Invert, "Inverted colors on the active layer");
                        ui.close();
                    }
                });
                ui.label("Paint.R development preview");
            });
        });
    }

    fn toolbar_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("paint-r-toolbar").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(self.session.can_undo(), egui::Button::new("Undo"))
                    .clicked()
                {
                    self.execute(Command::Undo, "Undid the last edit");
                }
                if ui
                    .add_enabled(self.session.can_redo(), egui::Button::new("Redo"))
                    .clicked()
                {
                    self.execute(Command::Redo, "Redid the next edit");
                }
                ui.separator();
                if ui.button("Select Rectangle").clicked() {
                    self.select_rectangle();
                }
                if ui
                    .add_enabled(
                        self.session.document().selection().is_some(),
                        egui::Button::new("Clear Selection"),
                    )
                    .clicked()
                {
                    self.execute(Command::ClearSelection, "Selection cleared");
                }
                if ui.button("Fill").clicked() {
                    let color = self.fill_color;
                    self.execute(Command::Fill(color), "Filled with the chosen BGRA color");
                }
                ui.separator();
                if ui.button("Add Layer").clicked() {
                    self.add_layer();
                }
            });
        });
    }

    fn layers_panel(&mut self, ui: &mut egui::Ui) {
        let mut pending = None;
        egui::Panel::right("paint-r-layers")
            .default_size(270.0)
            .show(ui, |ui| {
                ui.heading("Layers");
                ui.label("Ordered bottom to top; displayed topmost first.");
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let document = self.session.document();
                    for index in (0..document.layers().len()).rev() {
                        let layer = &document.layers()[index];
                        let active = layer.id() == document.active_layer_id();
                        ui.group(|ui| {
                            if ui
                                .selectable_label(
                                    active,
                                    format!("Active layer {index}: {}", layer.name()),
                                )
                                .clicked()
                            {
                                pending = Some(Command::SetActiveLayer { index });
                            }
                            let mut visible = layer.is_visible();
                            if ui
                                .checkbox(&mut visible, format!("Layer {index} visible"))
                                .changed()
                            {
                                pending = Some(Command::SetLayerVisibility { index, visible });
                            }
                            let mut opacity = layer.opacity();
                            if ui
                                .add(
                                    egui::Slider::new(&mut opacity, 0..=u8::MAX)
                                        .text(format!("Layer {index} opacity")),
                                )
                                .changed()
                            {
                                pending = Some(Command::SetLayerOpacity { index, opacity });
                            }
                        });
                    }
                });
            });
        if let Some(command) = pending {
            self.execute(command, "Layer properties updated");
        }
    }

    fn colors_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("paint-r-colors")
            .default_size(240.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading("Colors");
                    ui.label("Fill color in canonical BGRA channels:");
                    color_slider(ui, "Fill blue", &mut self.fill_color.b);
                    color_slider(ui, "Fill green", &mut self.fill_color.g);
                    color_slider(ui, "Fill red", &mut self.fill_color.r);
                    color_slider(ui, "Fill alpha", &mut self.fill_color.a);
                    let color = self.fill_color;
                    ui.colored_label(
                        egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a),
                        format!("BGRA {}, {}, {}, {}", color.b, color.g, color.r, color.a),
                    );
                    ui.separator();
                    ui.heading("Rectangle selection");
                    ui.label("Signed half-open edges; raster operations clip to the canvas.");
                    selection_input(ui, "Selection left", &mut self.selection_input.left);
                    selection_input(ui, "Selection top", &mut self.selection_input.top);
                    selection_input(ui, "Selection right", &mut self.selection_input.right);
                    selection_input(ui, "Selection bottom", &mut self.selection_input.bottom);
                    if let Some(selection) = self.session.document().selection() {
                        ui.label(format!(
                            "Current: ({}, {})..({}, {})",
                            selection.left(),
                            selection.top(),
                            selection.right(),
                            selection.bottom()
                        ));
                    } else {
                        ui.label("Current: no selection");
                    }
                });
            });
    }

    fn status_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("paint-r-status").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let dirty = if self.session.dirty() {
                    "Modified"
                } else {
                    "Clean"
                };
                ui.strong(dirty);
                ui.separator();
                ui.label(&self.status);
                ui.separator();
                ui.label("PNG open creates one layer; flattened export does not save layers or clear modified state.");
            });
        });
    }

    fn canvas_panel(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            let size = self.session.document().size();
            ui.heading(format!("Canvas — {}×{}", size.width(), size.height()));
            ui.label("Actual composed session surface");
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if let Some(texture) = &self.texture {
                        ui.add(
                            egui::Image::from_texture(texture)
                                .fit_to_original_size(1.0)
                                .alt_text("Composed document canvas"),
                        );
                    } else {
                        ui.label("Canvas texture is unavailable; see the status message.");
                    }
                });
        });
    }

    fn path_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(dialog) = self.path_dialog.as_mut() else {
            return;
        };
        let kind = dialog.kind;
        let mut confirm = false;
        let mut cancel = false;
        egui::Window::new(kind.title())
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.label(kind.explanation());
                let label = ui.label("PNG file path");
                let _response = ui
                    .text_edit_singleline(&mut dialog.path)
                    .labelled_by(label.id);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            !dialog.path.trim().is_empty(),
                            egui::Button::new(kind.action()),
                        )
                        .clicked()
                    {
                        confirm = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if cancel {
            self.path_dialog = None;
            return;
        }
        if !confirm {
            return;
        }
        let path = PathBuf::from(&dialog.path);
        match kind {
            PathDialogKind::Open => match AppSession::open_png(&path) {
                Ok(session) => {
                    self.session = session;
                    self.next_layer_number = 1;
                    self.texture_dirty = true;
                    self.path_dialog = None;
                    self.status = format!("Opened flattened PNG: {}", path.display());
                }
                Err(error) => {
                    self.status = format!("Open failed: {error}");
                }
            },
            PathDialogKind::Export => match self.session.export_png_atomic(&path) {
                Ok(()) => {
                    self.path_dialog = None;
                    self.status = format!(
                        "Exported flattened PNG without changing the layered session: {}",
                        path.display()
                    );
                }
                Err(error) => {
                    self.status = format!("Export failed: {error}");
                }
            },
        }
    }
}

impl eframe::App for PaintApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.refresh_texture(ui.ctx());
        self.menu_panel(ui);
        self.toolbar_panel(ui);
        self.layers_panel(ui);
        self.colors_panel(ui);
        self.status_panel(ui);
        self.canvas_panel(ui);
        self.path_dialog(ui);
        if self.texture_dirty {
            // Mutations happen while widgets run, after this frame's initial texture refresh.
            // Schedule the real composed-surface upload instead of relying on incidental repaints.
            ui.ctx().request_repaint();
        }
    }
}

#[derive(Clone, Copy)]
struct SelectionInput {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

impl Default for SelectionInput {
    fn default() -> Self {
        Self {
            left: 0,
            top: 0,
            right: 1,
            bottom: 1,
        }
    }
}

#[derive(Clone, Copy)]
enum PathDialogKind {
    Open,
    Export,
}

impl PathDialogKind {
    const fn title(self) -> &'static str {
        match self {
            Self::Open => "Open PNG (flattened)",
            Self::Export => "Export PNG (flattened)",
        }
    }

    const fn action(self) -> &'static str {
        match self {
            Self::Open => "Open PNG",
            Self::Export => "Export PNG",
        }
    }

    const fn explanation(self) -> &'static str {
        match self {
            Self::Open => "Opening creates one flattened layer; source layers cannot be recovered.",
            Self::Export => {
                "Export writes composed pixels only; the layered session and modified state are preserved."
            }
        }
    }
}

struct PathDialog {
    kind: PathDialogKind,
    path: String,
}

impl PathDialog {
    fn new(kind: PathDialogKind) -> Self {
        Self {
            kind,
            path: String::new(),
        }
    }
}

fn default_session() -> AppSession {
    let size = Size::new(DEFAULT_WIDTH, DEFAULT_HEIGHT).expect("valid default canvas dimensions");
    let document = Document::new(size).expect("valid default opaque-white document");
    AppSession::new(document)
}

fn color_slider(ui: &mut egui::Ui, label: &'static str, value: &mut u8) {
    let _response = ui.add(egui::Slider::new(value, 0..=u8::MAX).text(label));
}

fn selection_input(ui: &mut egui::Ui, label: &'static str, value: &mut i64) {
    ui.horizontal(|ui| {
        let label = ui.label(label);
        let _response = ui.add(egui::DragValue::new(value)).labelled_by(label.id);
    });
}
