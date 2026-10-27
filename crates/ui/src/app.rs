use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use paint_app::{AppSession, Command};
use paint_core::{Bgra, Point, Rect, Size};
use paint_document::{Document, LayerId, Revision};

use crate::icons::{Icon, icon_button};

const DEFAULT_WIDTH: u32 = 800;
const DEFAULT_HEIGHT: u32 = 600;
const MAX_PREVIEW_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PREVIEW_DECODED_BYTES: usize = 64 * 1024 * 1024;
const PREVIEW_WIDTH: f32 = 180.0;
const PREVIEW_HEIGHT: f32 = 140.0;
const MAX_BROWSER_ENTRIES: usize = 200;
const PRESET_COLORS: &[(&str, Bgra)] = &[
    ("Preset black", Bgra::BLACK),
    ("Preset white", Bgra::WHITE),
    ("Preset red", Bgra::new(0, 0, 255, 255)),
    ("Preset orange", Bgra::new(0, 128, 255, 255)),
    ("Preset yellow", Bgra::new(0, 255, 255, 255)),
    ("Preset green", Bgra::new(0, 255, 0, 255)),
    ("Preset cyan", Bgra::new(255, 255, 0, 255)),
    ("Preset blue", Bgra::new(255, 0, 0, 255)),
    ("Preset magenta", Bgra::new(255, 0, 255, 255)),
    ("Preset gray", Bgra::new(128, 128, 128, 255)),
    ("Preset transparent", Bgra::TRANSPARENT),
];

/// Original development-preview shell. `AppSession` remains the only document writer.
pub struct PaintApp {
    session: AppSession,
    fill_color: Bgra,
    picking_color: bool,
    hex_input: String,
    hex_error: Option<String>,
    selection_input: SelectionInput,
    texture: Option<egui::TextureHandle>,
    texture_dirty: bool,
    zoom: f32,
    layer_thumbnails: HashMap<LayerId, LayerThumbnail>,
    next_layer_number: usize,
    status: String,
    path_dialog: Option<PathDialog>,
    pending_discard: Option<DiscardAction>,
    focus_discard_buttons: bool,
    close_approved: bool,
}

impl PaintApp {
    pub fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self {
            session: default_session(),
            fill_color: Bgra::BLACK,
            picking_color: false,
            hex_input: String::new(),
            hex_error: None,
            selection_input: SelectionInput::default(),
            texture: None,
            texture_dirty: true,
            zoom: 1.0,
            layer_thumbnails: HashMap::new(),
            next_layer_number: 1,
            status: "New 800×600 opaque-white document".to_owned(),
            path_dialog: None,
            pending_discard: None,
            focus_discard_buttons: false,
            close_approved: false,
        };
        configure_style(&creation_context.egui_ctx);
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

    #[doc(hidden)]
    pub const fn close_approved(&self) -> bool {
        self.close_approved
    }

    fn new_document(&mut self) {
        self.session = default_session();
        self.selection_input = SelectionInput::default();
        self.next_layer_number = 1;
        self.path_dialog = None;
        self.pending_discard = None;
        self.focus_discard_buttons = false;
        self.texture_dirty = true;
        self.zoom = 1.0;
        self.layer_thumbnails.clear();
        self.status = "New 800×600 opaque-white document".to_owned();
    }

    fn show_path_dialog(&mut self, kind: PathDialogKind) {
        self.path_dialog = Some(PathDialog::new(kind));
    }

    fn request_discard(&mut self, action: DiscardAction) {
        self.path_dialog = None;
        self.pending_discard = Some(action);
        self.focus_discard_buttons = true;
    }

    fn request_new(&mut self) {
        if self.session.dirty() {
            self.request_discard(DiscardAction::NewDocument);
        } else {
            self.new_document();
        }
    }

    fn request_quit(&mut self, context: &egui::Context) {
        if self.session.dirty() {
            self.request_discard(DiscardAction::CloseViewport);
        } else {
            self.close_approved = true;
            context.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn request_open(&mut self, path: String) {
        if self.session.dirty() {
            self.request_discard(DiscardAction::OpenPng(path));
        } else {
            self.path_dialog = None;
            self.open_png(path);
        }
    }

    fn open_png(&mut self, path: String) {
        let path_buffer = PathBuf::from(&path);
        match AppSession::open_png(&path_buffer) {
            Ok(session) => {
                self.session = session;
                self.next_layer_number = 1;
                self.texture_dirty = true;
                self.zoom = 1.0;
                self.layer_thumbnails.clear();
                self.path_dialog = None;
                self.status = format!("Opened flattened PNG: {}", path_buffer.display());
            }
            Err(error) => {
                self.path_dialog = Some(PathDialog::with_path(PathDialogKind::Open, path));
                self.status = format!("Open failed: {error}");
            }
        }
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

    fn refresh_layer_thumbnails(&mut self, context: &egui::Context) {
        let mut refreshed = HashMap::with_capacity(self.session.document().layers().len());
        for layer in self.session.document().layers() {
            let texture = match self.layer_thumbnails.get(&layer.id()) {
                Some(existing) if existing.revision == layer.revision() => existing.texture.clone(),
                _ => layer_thumbnail_texture(context, layer),
            };
            refreshed.insert(
                layer.id(),
                LayerThumbnail {
                    revision: layer.revision(),
                    texture,
                },
            );
        }
        self.layer_thumbnails = refreshed;
    }

    fn menu_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("paint-r-menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    let new_response = ui.button("New");
                    focus_first_menu_item_on_keyboard_open(ui, &new_response);
                    if new_response.clicked() {
                        self.request_new();
                        ui.close();
                    }
                    if ui.button("Open PNG…").clicked() {
                        self.show_path_dialog(PathDialogKind::Open);
                        ui.close();
                    }
                    if ui.button("Export PNG…").clicked() {
                        self.show_path_dialog(PathDialogKind::Export);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Close Window").clicked() {
                        self.request_quit(ui.ctx());
                        ui.close();
                    }
                    if ui.button("Quit").clicked() {
                        self.request_quit(ui.ctx());
                        ui.close();
                    }
                });
                ui.menu_button("Layer", |ui| {
                    let horizontal = ui.button("Flip Active Layer Horizontally");
                    focus_first_menu_item_on_keyboard_open(ui, &horizontal);
                    if horizontal.clicked() {
                        self.execute(
                            Command::FlipLayerHorizontal,
                            "Flipped the active layer horizontally",
                        );
                        ui.close();
                    }
                    if ui.button("Flip Active Layer Vertically").clicked() {
                        self.execute(
                            Command::FlipLayerVertical,
                            "Flipped the active layer vertically",
                        );
                        ui.close();
                    }
                });
                ui.menu_button("Adjustments", |ui| {
                    let invert_response = ui.button("Invert Colors");
                    focus_first_menu_item_on_keyboard_open(ui, &invert_response);
                    if invert_response.clicked() {
                        self.execute(Command::Invert, "Inverted colors on the active layer");
                        ui.close();
                    }
                });
                ui.menu_button("Keyboard", |ui| {
                    ui.label("Development-preview shortcuts (Cmd/Ctrl):");
                    ui.label("N New | O Open | Shift+E Export");
                    ui.label("I Invert | F Fill | R Rectangle selection");
                    ui.label("Layer menu flips the complete active layer");
                    ui.label("Shift+L Add layer | Z Undo | Shift+Z Redo");
                    ui.separator();
                    ui.label("Tab / Shift+Tab move focus; Enter / Space activate");
                    ui.label("Arrows adjust focused values; Enter confirms a path");
                    ui.label("Escape cancels a dialog");
                });
                ui.label("Paint.R development preview");
            });
        });
    }

    fn toolbar_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("paint-r-toolbar").show(ui, |ui| {
            ui.add_space(3.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                if icon_button(ui, Icon::NewDocument, "New document", true).clicked() {
                    self.request_new();
                }
                if icon_button(ui, Icon::OpenFile, "Open PNG file", true).clicked() {
                    self.show_path_dialog(PathDialogKind::Open);
                }
                if icon_button(ui, Icon::ExportFile, "Export PNG file", true).clicked() {
                    self.show_path_dialog(PathDialogKind::Export);
                }
                ui.add_space(5.0);
                ui.separator();
                ui.add_space(5.0);
                if icon_button(ui, Icon::Undo, "Undo", self.session.can_undo()).clicked() {
                    self.execute(Command::Undo, "Undid the last edit");
                }
                if icon_button(ui, Icon::Redo, "Redo", self.session.can_redo()).clicked() {
                    self.execute(Command::Redo, "Redid the next edit");
                }
                ui.add_space(5.0);
                ui.separator();
                ui.add_space(5.0);
                if icon_button(ui, Icon::SelectRectangle, "Select Rectangle", true).clicked() {
                    self.select_rectangle();
                }
                if icon_button(
                    ui,
                    Icon::ClearSelection,
                    "Clear Selection",
                    self.session.document().selection().is_some(),
                )
                .clicked()
                {
                    self.execute(Command::ClearSelection, "Selection cleared");
                }
                if icon_button(ui, Icon::Fill, "Fill", true).clicked() {
                    let color = self.fill_color;
                    self.execute(Command::Fill(color), "Filled with the chosen BGRA color");
                }
                if icon_button(ui, Icon::EyeDropper, "Pick color from canvas", true).clicked() {
                    self.picking_color = !self.picking_color;
                    self.status = if self.picking_color {
                        "Pick a composed canvas color".to_owned()
                    } else {
                        "Color picker canceled".to_owned()
                    };
                }
                ui.add_space(5.0);
                ui.separator();
                ui.add_space(5.0);
                if icon_button(ui, Icon::AddLayer, "Add Layer", true).clicked() {
                    self.add_layer();
                }
            });
            ui.add_space(2.0);
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
                            ui.horizontal_top(|ui| {
                                if let Some(thumbnail) = self.layer_thumbnails.get(&layer.id()) {
                                    egui::Frame::new()
                                        .fill(egui::Color32::from_rgb(24, 24, 24))
                                        .corner_radius(3)
                                        .inner_margin(2)
                                        .show(ui, |ui| {
                                            ui.add(
                                                egui::Image::from_texture(&thumbnail.texture)
                                                    .maintain_aspect_ratio(true)
                                                    .fit_to_exact_size(egui::vec2(48.0, 38.0))
                                                    .alt_text(format!(
                                                        "Layer {index} thumbnail: {}",
                                                        layer.name()
                                                    )),
                                            );
                                        });
                                }
                                ui.vertical(|ui| {
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
                                        pending =
                                            Some(Command::SetLayerVisibility { index, visible });
                                    }
                                });
                            });
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
                    let initial_color = self.fill_color;
                    let color = self.fill_color;
                    color_preview(ui, color);
                    ui.label(format!(
                        "Primary color preview · #{:02X}{:02X}{:02X}",
                        color.r, color.g, color.b
                    ));
                    ui.label(format!(
                        "Alpha {}% · canonical BGRA channels",
                        (u16::from(color.a) * 100 / 255)
                    ));
                    ui.horizontal(|ui| {
                        let label = ui.label("Hex color");
                        let _response = ui
                            .text_edit_singleline(&mut self.hex_input)
                            .labelled_by(label.id);
                        if ui.button("Apply color").clicked() {
                            match crate::color_from_hex(&self.hex_input) {
                                Ok(parsed) => {
                                    self.fill_color = parsed;
                                    self.hex_input = format_hex_color(parsed);
                                    self.hex_error = None;
                                }
                                Err(error) => self.hex_error = Some(error),
                            }
                        }
                    });
                    if let Some(error) = &self.hex_error {
                        ui.colored_label(
                            egui::Color32::LIGHT_RED,
                            format!("Invalid hex color: {error}"),
                        );
                    }
                    ui.label("Color wheel");
                    if let Some(color) = color_wheel(ui, self.fill_color) {
                        self.fill_color = color;
                    }
                    ui.label("Palette");
                    ui.horizontal_wrapped(|ui| {
                        for &(label, preset) in PRESET_COLORS {
                            if color_swatch(ui, label, preset, preset == self.fill_color).clicked()
                            {
                                self.fill_color = preset;
                            }
                        }
                    });
                    ui.separator();
                    color_slider(ui, "Fill blue", &mut self.fill_color.b);
                    color_slider(ui, "Fill green", &mut self.fill_color.g);
                    color_slider(ui, "Fill red", &mut self.fill_color.r);
                    color_slider(ui, "Fill alpha", &mut self.fill_color.a);
                    if self.fill_color != initial_color {
                        self.hex_input = format_hex_color(self.fill_color);
                        self.hex_error = None;
                    }
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
            let mut fit_requested = false;
            ui.horizontal(|ui| {
                ui.heading(format!("Canvas — {}×{}", size.width(), size.height()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if labeled_button(ui, "Fit", "Fit canvas").clicked() {
                        fit_requested = true;
                    }
                    if labeled_button(ui, "+", "Zoom in").clicked() {
                        self.zoom = (self.zoom + 0.25).min(8.0);
                    }
                    ui.label(format!("{}%", (self.zoom * 100.0).round() as u32));
                    if labeled_button(ui, "−", "Zoom out").clicked() {
                        self.zoom = (self.zoom - 0.25).max(0.1);
                    }
                });
            });
            if fit_requested {
                let available = ui.available_size();
                self.zoom = ((available.x - 24.0).max(32.0) / size.width() as f32)
                    .min((available.y - 24.0).max(32.0) / size.height() as f32)
                    .clamp(0.05, 8.0);
            }
            ui.label("Actual composed session surface");
            let texture = self.texture.clone();
            let zoom = self.zoom;
            let picking = self.picking_color;
            let mut picked = None;
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if let Some(texture) = texture {
                        let display =
                            egui::vec2(size.width() as f32 * zoom, size.height() as f32 * zoom);
                        let sense = if picking {
                            egui::Sense::click()
                        } else {
                            egui::Sense::hover()
                        };
                        let (rect, response) = ui.allocate_exact_size(display, sense);
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::Role::Image,
                                true,
                                "Composed document canvas",
                            )
                        });
                        let response = if picking {
                            response.on_hover_cursor(egui::CursorIcon::Crosshair)
                        } else {
                            response
                        };
                        if ui.is_rect_visible(rect) {
                            egui::Image::from_texture(&texture).paint_at(ui, rect);
                        }
                        if picking
                            && response.clicked()
                            && let Some(position) = response.interact_pointer_pos()
                        {
                            let x = ((position.x - rect.left()) / zoom).floor();
                            let y = ((position.y - rect.top()) / zoom).floor();
                            if x >= 0.0
                                && y >= 0.0
                                && x < size.width() as f32
                                && y < size.height() as f32
                            {
                                picked = Some(Point::new(x as u32, y as u32));
                            }
                        }
                    } else {
                        ui.label("Canvas texture is unavailable; see the status message.");
                    }
                });
            if let Some(point) = picked {
                self.pick_color_at(point);
            }
        });
    }

    fn pick_color_at(&mut self, point: Point) {
        match self.session.compose() {
            Ok(surface) => {
                let Ok(color) = surface.pixel(point) else {
                    self.status = "The picked canvas position is outside the document".to_owned();
                    return;
                };
                self.fill_color = *color;
                self.hex_input = format_hex_color(*color);
                self.hex_error = None;
                self.picking_color = false;
                self.status = format!(
                    "Picked #{:02X}{:02X}{:02X} at ({}, {})",
                    color.r, color.g, color.b, point.x, point.y
                );
            }
            Err(error) => self.status = format!("Could not sample the canvas: {error}"),
        }
    }

    fn path_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(dialog) = self.path_dialog.as_mut() else {
            return;
        };
        let kind = dialog.kind;
        let enter_in_path = ui.ctx().text_edit_focused()
            && ui
                .ctx()
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
        let mut confirm = enter_in_path && !dialog.path.trim().is_empty();
        let mut preview_requested = false;
        let mut browse_from_path = false;
        let mut browser_navigation =
            if matches!(kind, PathDialogKind::Open) && !dialog.browser_loaded {
                dialog.browser_loaded = true;
                Some(dialog.browser_dir.clone())
            } else {
                None
            };
        let mut selected_image = None;
        let mut cancel = ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        egui::Window::new(kind.title())
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.label(kind.explanation());
                let label = ui.label("PNG file path");
                let response = ui
                    .text_edit_singleline(&mut dialog.path)
                    .labelled_by(label.id);
                if dialog.focus_path {
                    response.request_focus();
                    dialog.focus_path = false;
                }
                if response.changed() {
                    dialog.preview = None;
                    dialog.preview_error = None;
                }
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
                    if ui
                        .add_enabled(!dialog.path.trim().is_empty(), egui::Button::new("Preview"))
                        .clicked()
                    {
                        preview_requested = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
                if let Some(preview) = &dialog.preview {
                    ui.separator();
                    ui.label(format!("PNG preview: {}", preview.label));
                    ui.add(
                        egui::Image::from_texture(&preview.texture)
                            .maintain_aspect_ratio(true)
                            .fit_to_exact_size(egui::vec2(PREVIEW_WIDTH, PREVIEW_HEIGHT))
                            .alt_text(format!("PNG preview image: {}", preview.label)),
                    );
                }
                if let Some(error) = &dialog.preview_error {
                    ui.colored_label(egui::Color32::LIGHT_RED, format!("Preview failed: {error}"));
                }
                if matches!(kind, PathDialogKind::Open) {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.strong("PNG browser");
                        if ui.button("Browse folder").clicked() {
                            browse_from_path = true;
                        }
                    });
                    ui.label(format!("Folder: {}", dialog.browser_dir.display()));
                    if dialog.browser_truncated {
                        ui.label(format!(
                            "Showing the first {MAX_BROWSER_ENTRIES} folders and PNG files."
                        ));
                    }
                    if let Some(error) = &dialog.browser_error {
                        ui.colored_label(
                            egui::Color32::LIGHT_RED,
                            format!("Folder unavailable: {error}"),
                        );
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("paint-r-png-browser")
                        .max_height(150.0)
                        .show(ui, |ui| {
                            for entry in &dialog.browser_entries {
                                let label = if entry.is_dir {
                                    format!("Folder: {}", entry.name)
                                } else {
                                    format!("PNG: {}", entry.name)
                                };
                                if ui.selectable_label(false, label).clicked() {
                                    if entry.is_dir {
                                        browser_navigation = Some(entry.path.clone());
                                    } else {
                                        selected_image = Some(entry.path.clone());
                                    }
                                }
                            }
                        });
                }
            });

        if browse_from_path {
            let candidate = PathBuf::from(dialog.path.trim());
            let directory = if candidate.is_dir() {
                candidate
            } else {
                candidate
                    .parent()
                    .filter(|parent| parent.is_dir())
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| dialog.browser_dir.clone())
            };
            browser_navigation = Some(directory);
        }
        if let Some(directory) = browser_navigation {
            match list_png_directory(&directory) {
                Ok(listing) => {
                    dialog.browser_dir = directory;
                    dialog.browser_entries = listing.entries;
                    dialog.browser_truncated = listing.truncated;
                    dialog.browser_error = None;
                }
                Err(error) => {
                    dialog.browser_dir = directory;
                    dialog.browser_entries.clear();
                    dialog.browser_truncated = false;
                    dialog.browser_error = Some(error);
                }
            }
            ui.ctx().request_repaint();
        }
        if let Some(image) = selected_image {
            dialog.path = image.display().to_string();
            dialog.preview = None;
            dialog.preview_error = None;
            preview_requested = true;
            ui.ctx().request_repaint();
        }
        if preview_requested {
            let path = PathBuf::from(dialog.path.trim());
            match kind {
                PathDialogKind::Open => match open_png_preview(ui.ctx(), &path) {
                    Ok(preview) => {
                        dialog.preview = Some(preview);
                        dialog.preview_error = None;
                    }
                    Err(error) => {
                        dialog.preview = None;
                        dialog.preview_error = Some(error);
                    }
                },
                PathDialogKind::Export => match self.session.compose() {
                    Ok(surface) => {
                        let label = format!(
                            "current composition {}×{}",
                            surface.width(),
                            surface.height()
                        );
                        dialog.preview = Some(DialogPreview {
                            label,
                            texture: surface_thumbnail_texture(
                                ui.ctx(),
                                "paint-r-export-preview",
                                &surface,
                                PREVIEW_WIDTH,
                                PREVIEW_HEIGHT,
                            ),
                        });
                        dialog.preview_error = None;
                    }
                    Err(error) => {
                        dialog.preview = None;
                        dialog.preview_error = Some(error.to_string());
                    }
                },
            }
            ui.ctx().request_repaint();
        }
        if cancel {
            self.path_dialog = None;
            return;
        }
        if !confirm {
            return;
        }
        let path = dialog.path.clone();
        match kind {
            PathDialogKind::Open => self.request_open(path),
            PathDialogKind::Export => {
                let path = PathBuf::from(path);
                match self.session.export_png_atomic(&path) {
                    Ok(()) => {
                        self.path_dialog = None;
                        self.status = format!(
                            "Exported flattened PNG without changing the layered session: {}",
                            path.display()
                        );
                    }
                    Err(error) => {
                        dialog.focus_path = true;
                        self.status = format!("Export failed: {error}");
                    }
                }
            }
        }
    }

    fn cancel_discard(&mut self) {
        self.focus_discard_buttons = false;
        if let Some(DiscardAction::OpenPng(path)) = self.pending_discard.take() {
            self.path_dialog = Some(PathDialog::with_path(PathDialogKind::Open, path));
        }
    }

    fn discard_confirmation(&mut self, ui: &mut egui::Ui) {
        let explanation = match self.pending_discard.as_ref() {
            Some(DiscardAction::NewDocument) => {
                "Creating a new document will discard the modified document and its undo history."
            }
            Some(DiscardAction::OpenPng(_)) => {
                "Opening a PNG will replace the modified document and its undo history with one flattened layer."
            }
            Some(DiscardAction::CloseViewport) => {
                "Closing Paint.R will discard the modified document and its undo history."
            }
            None => return,
        };
        let focus_cancel = self.focus_discard_buttons;
        let mut cancel = ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let mut discard = false;
        egui::Window::new("Discard unsaved changes?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.label(explanation);
                ui.horizontal(|ui| {
                    let cancel_response = ui.button("Cancel");
                    if focus_cancel {
                        cancel_response.request_focus();
                    }
                    if cancel_response.clicked() {
                        cancel = true;
                    }
                    if ui.button("Discard").clicked() {
                        discard = true;
                    }
                });
            });
        self.focus_discard_buttons = false;

        if cancel {
            self.cancel_discard();
            return;
        }
        if !discard {
            return;
        }
        match self.pending_discard.take() {
            Some(DiscardAction::NewDocument) => self.new_document(),
            Some(DiscardAction::OpenPng(path)) => self.open_png(path),
            Some(DiscardAction::CloseViewport) => {
                // egui-winit delivers this command as a fresh ViewportEvent::Close.
                self.close_approved = true;
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            None => {}
        }
    }

    fn handle_keyboard_shortcuts(&mut self, context: &egui::Context) {
        if self.path_dialog.is_some()
            || self.pending_discard.is_some()
            || context.text_edit_focused()
        {
            return;
        }
        let command = egui::Modifiers::COMMAND;
        let command_shift = command | egui::Modifiers::SHIFT;

        if shortcut_pressed(context, command, egui::Key::Q)
            || shortcut_pressed(context, command, egui::Key::W)
        {
            self.request_quit(context);
            return;
        }
        if shortcut_pressed(context, command_shift, egui::Key::Z) {
            self.execute(Command::Redo, "Redid the next edit");
            return;
        }
        if shortcut_pressed(context, command_shift, egui::Key::E) {
            self.show_path_dialog(PathDialogKind::Export);
            return;
        }
        if shortcut_pressed(context, command_shift, egui::Key::L) {
            self.add_layer();
            return;
        }
        if shortcut_pressed(context, command, egui::Key::N) {
            self.request_new();
            return;
        }
        if shortcut_pressed(context, command, egui::Key::O) {
            self.show_path_dialog(PathDialogKind::Open);
            return;
        }
        if shortcut_pressed(context, command, egui::Key::I) {
            self.execute(Command::Invert, "Inverted colors on the active layer");
            return;
        }
        if shortcut_pressed(context, command, egui::Key::R) {
            self.select_rectangle();
            return;
        }
        if shortcut_pressed(context, command, egui::Key::F) {
            let color = self.fill_color;
            self.execute(Command::Fill(color), "Filled with the chosen BGRA color");
            return;
        }
        if shortcut_pressed(context, command, egui::Key::Z) {
            self.execute(Command::Undo, "Undid the last edit");
        }
    }

    fn handle_close_request(&mut self, context: &egui::Context) {
        let close_requested = context.input(|input| input.viewport().close_requested());
        if !close_requested {
            return;
        }
        if self.close_approved {
            // Allow only the close event forwarded after explicit Discard.
            self.close_approved = false;
            return;
        }
        if self.session.dirty() {
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.request_discard(DiscardAction::CloseViewport);
        }
    }
}

impl eframe::App for PaintApp {
    fn logic(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        // `logic` also runs for hidden windows, where `ui` would not see the close request.
        self.handle_keyboard_shortcuts(context);
        self.handle_close_request(context);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.refresh_texture(ui.ctx());
        self.refresh_layer_thumbnails(ui.ctx());
        self.menu_panel(ui);
        self.toolbar_panel(ui);
        self.layers_panel(ui);
        self.colors_panel(ui);
        self.status_panel(ui);
        self.canvas_panel(ui);
        self.path_dialog(ui);
        self.discard_confirmation(ui);
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

enum DiscardAction {
    NewDocument,
    OpenPng(String),
    CloseViewport,
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
    focus_path: bool,
    preview: Option<DialogPreview>,
    preview_error: Option<String>,
    browser_dir: PathBuf,
    browser_entries: Vec<BrowserEntry>,
    browser_truncated: bool,
    browser_error: Option<String>,
    browser_loaded: bool,
}

struct DialogPreview {
    label: String,
    texture: egui::TextureHandle,
}

struct BrowserEntry {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

struct DirectoryListing {
    entries: Vec<BrowserEntry>,
    truncated: bool,
}

impl PathDialog {
    fn new(kind: PathDialogKind) -> Self {
        Self::with_path(kind, String::new())
    }

    fn with_path(kind: PathDialogKind, path: String) -> Self {
        let browser_dir = browser_start_dir(&path);
        Self {
            kind,
            path,
            focus_path: true,
            preview: None,
            preview_error: None,
            browser_dir,
            browser_entries: Vec::new(),
            browser_truncated: false,
            browser_error: None,
            browser_loaded: false,
        }
    }
}

struct LayerThumbnail {
    revision: Revision,
    texture: egui::TextureHandle,
}

fn configure_style(context: &egui::Context) {
    context.set_theme(egui::ThemePreference::Dark);
    context.all_styles_mut(|style| {
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = egui::Color32::from_rgb(38, 38, 38);
        style.visuals.window_fill = egui::Color32::from_rgb(45, 45, 45);
        style.visuals.faint_bg_color = egui::Color32::from_rgb(32, 32, 32);
        style.visuals.extreme_bg_color = egui::Color32::from_rgb(22, 22, 22);
        style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(58, 58, 58);
        style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(70, 70, 70);
        style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(80, 80, 80);
        style.visuals.selection.bg_fill = egui::Color32::from_rgb(0, 102, 184);
        style.visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(9.0, 5.0);
    });
}

fn layer_thumbnail_texture(
    context: &egui::Context,
    layer: &paint_document::Layer,
) -> egui::TextureHandle {
    surface_thumbnail_texture(
        context,
        format!(
            "paint-r-layer-{}-{}",
            layer.id().get(),
            layer.revision().get()
        ),
        layer.surface(),
        48.0,
        38.0,
    )
}

fn surface_thumbnail_texture(
    context: &egui::Context,
    name: impl Into<String>,
    surface: &paint_core::Surface,
    max_width: f32,
    max_height: f32,
) -> egui::TextureHandle {
    let width = surface.width().max(1);
    let height = surface.height().max(1);
    let scale = (max_width / width as f32).min(max_height / height as f32);
    let thumbnail_width = ((width as f32 * scale).round() as u32).max(1);
    let thumbnail_height = ((height as f32 * scale).round() as u32).max(1);
    let mut rgba = Vec::with_capacity((thumbnail_width * thumbnail_height * 4) as usize);
    for y in 0..thumbnail_height {
        let source_y = u64::from(y) * u64::from(height) / u64::from(thumbnail_height);
        for x in 0..thumbnail_width {
            let source_x = u64::from(x) * u64::from(width) / u64::from(thumbnail_width);
            let pixel = surface.pixels()[(source_y * u64::from(width) + source_x) as usize];
            rgba.extend_from_slice(&pixel.to_rgba_bytes());
        }
    }
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [thumbnail_width as usize, thumbnail_height as usize],
        &rgba,
    );
    context.load_texture(name, image, egui::TextureOptions::NEAREST)
}

fn browser_start_dir(path_text: &str) -> PathBuf {
    let candidate = PathBuf::from(path_text.trim());
    if candidate.is_dir() {
        return candidate;
    }
    if let Some(parent) = candidate.parent()
        && parent.is_dir()
    {
        return parent.to_path_buf();
    }
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn list_png_directory(directory: &Path) -> Result<DirectoryListing, String> {
    let mut entries = Vec::new();
    let read_dir = std::fs::read_dir(directory)
        .map_err(|error| format!("cannot read {}: {error}", directory.display()))?;
    for result in read_dir.take(MAX_BROWSER_ENTRIES + 1) {
        let entry = result.map_err(|error| format!("cannot read folder entry: {error}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let file_type = entry
            .file_type()
            .map_err(|error| format!("cannot inspect {name}: {error}"))?;
        let is_dir = file_type.is_dir();
        let is_png = file_type.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("png"));
        if !is_dir && !is_png {
            continue;
        }
        entries.push(BrowserEntry {
            name,
            path: entry.path(),
            is_dir,
        });
    }
    entries.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    let truncated = entries.len() > MAX_BROWSER_ENTRIES;
    entries.truncate(MAX_BROWSER_ENTRIES);
    Ok(DirectoryListing { entries, truncated })
}

fn open_png_preview(
    context: &egui::Context,
    path: &std::path::Path,
) -> Result<DialogPreview, String> {
    let metadata = path
        .metadata()
        .map_err(|error| format!("cannot inspect file: {error}"))?;
    if !metadata.is_file() {
        return Err("path is not a regular file".to_owned());
    }
    if metadata.len() > MAX_PREVIEW_FILE_BYTES {
        return Err(format!(
            "file is larger than the {MAX_PREVIEW_FILE_BYTES}-byte preview limit"
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| {
            file.take(MAX_PREVIEW_FILE_BYTES + 1)
                .read_to_end(&mut bytes)
        })
        .map_err(|error| format!("cannot read file: {error}"))?;
    if bytes.len() as u64 > MAX_PREVIEW_FILE_BYTES {
        return Err(format!(
            "file grew beyond the {MAX_PREVIEW_FILE_BYTES}-byte preview limit"
        ));
    }
    let limits = paint_image_io::PngLimits {
        max_decoded_bytes: MAX_PREVIEW_DECODED_BYTES,
        max_decoder_bytes: MAX_PREVIEW_FILE_BYTES as usize,
        ..paint_image_io::PngLimits::default()
    };
    let surface = paint_image_io::decode_png_with_limits(&bytes, &limits)
        .map_err(|error| format!("invalid PNG: {error}"))?;
    let label = format!("{}×{} PNG", surface.width(), surface.height());
    let texture = surface_thumbnail_texture(
        context,
        "paint-r-open-preview",
        &surface,
        PREVIEW_WIDTH,
        PREVIEW_HEIGHT,
    );
    Ok(DialogPreview { label, texture })
}

fn format_hex_color(color: Bgra) -> String {
    if color.a == u8::MAX {
        format!("#{:02X}{:02X}{:02X}", color.r, color.g, color.b)
    } else {
        format!(
            "#{:02X}{:02X}{:02X}{:02X}",
            color.r, color.g, color.b, color.a
        )
    }
}

fn color_wheel(ui: &mut egui::Ui, current: Bgra) -> Option<Bgra> {
    let diameter = ui.available_width().clamp(96.0, 170.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(diameter, diameter), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Color wheel"));
    if ui.is_rect_visible(rect) {
        let center = rect.center();
        let radius = diameter * 0.5 - 3.0;
        let segments = 64;
        let mut mesh = egui::Mesh::default();
        mesh.vertices.push(egui::epaint::Vertex {
            pos: center,
            uv: egui::epaint::WHITE_UV,
            color: egui::Color32::WHITE,
        });
        for index in 0..=segments {
            let angle = std::f32::consts::TAU * index as f32 / segments as f32;
            let color = crate::color_from_hsv(angle.to_degrees(), 1.0, 1.0, 255);
            mesh.vertices.push(egui::epaint::Vertex {
                pos: center + egui::vec2(angle.cos(), angle.sin()) * radius,
                uv: egui::epaint::WHITE_UV,
                color: egui::Color32::from_rgb(color.r, color.g, color.b),
            });
        }
        for index in 1..=segments {
            mesh.indices.extend([0, index as u32, (index + 1) as u32]);
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        ui.painter().circle_stroke(
            center,
            radius,
            egui::Stroke::new(1.0, egui::Color32::from_gray(80)),
        );
        let maximum = current.r.max(current.g).max(current.b);
        let minimum = current.r.min(current.g).min(current.b);
        let chroma = maximum - minimum;
        if chroma > 0 {
            let hue = if maximum == current.r {
                (f32::from(current.g) - f32::from(current.b)) / f32::from(chroma)
            } else if maximum == current.g {
                (f32::from(current.b) - f32::from(current.r)) / f32::from(chroma) + 2.0
            } else {
                (f32::from(current.r) - f32::from(current.g)) / f32::from(chroma) + 4.0
            }
            .rem_euclid(6.0)
                * 60.0;
            let saturation = f32::from(chroma) / f32::from(maximum);
            let angle = hue.to_radians();
            ui.painter().circle_stroke(
                center + egui::vec2(angle.cos(), angle.sin()) * radius * saturation,
                5.0,
                egui::Stroke::new(2.0, egui::Color32::WHITE),
            );
        }
    }
    if response.clicked()
        && let Some(position) = response.interact_pointer_pos()
    {
        let offset = position - rect.center();
        let radius = diameter * 0.5 - 3.0;
        if offset.length() <= radius {
            let hue = offset.y.atan2(offset.x).to_degrees().rem_euclid(360.0);
            let saturation = (offset.length() / radius).clamp(0.0, 1.0);
            return Some(crate::color_from_hsv(hue, saturation, 1.0, current.a));
        }
    }
    None
}

fn labeled_button(ui: &mut egui::Ui, text: &str, label: &'static str) -> egui::Response {
    let response = ui.button(text);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    response.on_hover_text(label)
}

fn color_swatch(
    ui: &mut egui::Ui,
    label: &'static str,
    color: Bgra,
    selected: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let checker = 6.0;
        let mut y = rect.top();
        let mut row = 0;
        while y < rect.bottom() {
            let mut x = rect.left();
            let mut column = 0;
            while x < rect.right() {
                let cell = egui::Rect::from_min_max(
                    egui::pos2(x, y),
                    egui::pos2(
                        (x + checker).min(rect.right()),
                        (y + checker).min(rect.bottom()),
                    ),
                );
                painter.rect_filled(
                    cell,
                    0,
                    if (row + column) % 2 == 0 {
                        egui::Color32::from_gray(190)
                    } else {
                        egui::Color32::from_gray(110)
                    },
                );
                x += checker;
                column += 1;
            }
            y += checker;
            row += 1;
        }
        painter.rect_filled(
            rect,
            3,
            egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a),
        );
        let stroke = if selected {
            egui::Stroke::new(2.0, egui::Color32::WHITE)
        } else if response.hovered() {
            ui.visuals().widgets.hovered.bg_stroke
        } else {
            egui::Stroke::new(1.0, egui::Color32::from_gray(80))
        };
        painter.rect_stroke(rect, 3, stroke, egui::StrokeKind::Inside);
    }
    response.on_hover_text(label)
}

fn color_preview(ui: &mut egui::Ui, color: Bgra) {
    let size = egui::vec2(ui.available_width().clamp(80.0, 184.0), 48.0);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter();
    let checker = 8.0;
    let light = egui::Color32::from_rgb(190, 190, 190);
    let dark = egui::Color32::from_rgb(120, 120, 120);
    let mut y = rect.top();
    let mut row = 0;
    while y < rect.bottom() {
        let mut x = rect.left();
        let mut column = 0;
        while x < rect.right() {
            let cell = egui::Rect::from_min_max(
                egui::pos2(x, y),
                egui::pos2(
                    (x + checker).min(rect.right()),
                    (y + checker).min(rect.bottom()),
                ),
            );
            painter.rect_filled(cell, 0, if (row + column) % 2 == 0 { light } else { dark });
            x += checker;
            column += 1;
        }
        y += checker;
        row += 1;
    }
    painter.rect_filled(
        rect,
        4,
        egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a),
    );
    painter.rect_stroke(
        rect,
        4,
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        egui::StrokeKind::Inside,
    );
}

fn default_session() -> AppSession {
    let size = Size::new(DEFAULT_WIDTH, DEFAULT_HEIGHT).expect("valid default canvas dimensions");
    let document = Document::new(size).expect("valid default opaque-white document");
    AppSession::new(document)
}

fn focus_first_menu_item_on_keyboard_open(ui: &egui::Ui, response: &egui::Response) {
    // The popup's first pass is a sizing pass: supplement only that keyboard-open focus handoff.
    // Later passes preserve menu navigation, and pointer opens retain egui's normal behavior.
    let keyboard_open = ui.is_sizing_pass()
        && ui.input(|input| {
            input.key_pressed(egui::Key::Enter) || input.key_pressed(egui::Key::Space)
        });
    if keyboard_open {
        response.request_focus();
    }
}

fn shortcut_pressed(context: &egui::Context, modifiers: egui::Modifiers, key: egui::Key) -> bool {
    context.input_mut(|input| input.consume_key(modifiers, key))
}

fn color_slider(ui: &mut egui::Ui, label: &'static str, value: &mut u8) {
    let _response = ui.add(egui::Slider::new(value, 0..=u8::MAX).text(label));
}

fn selection_input(ui: &mut egui::Ui, label: &'static str, value: &mut i64) {
    ui.horizontal(|ui| {
        let label = ui.label(label);
        let _response = ui
            .add(egui::DragValue::new(value).speed(1.0))
            .labelled_by(label.id);
    });
}
