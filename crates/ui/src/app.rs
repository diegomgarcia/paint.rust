use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use paint_app::{AppSession, Command};
use paint_core::{Bgra, Point, Rect, Size};
use paint_document::{Document, LayerId};

use crate::colors::{
    ColorSlot, PRESET_COLORS, color_slots_widget, color_swatch, color_wheel, compact_color_slider,
    format_hex_color, hsv_from_color, hsv_gradient_slider, mix_bgra,
};
use crate::icons::{Icon, IconTextures, icon_button, icon_button_at, tool_button};
use crate::layers::{LayerThumbnail, layer_thumbnail_texture, surface_thumbnail_texture};

const DEFAULT_WIDTH: u32 = 800;
const DEFAULT_HEIGHT: u32 = 600;
const MAX_PREVIEW_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PREVIEW_DECODED_BYTES: usize = 64 * 1024 * 1024;
const PREVIEW_WIDTH: f32 = 180.0;
const PREVIEW_HEIGHT: f32 = 140.0;
const MAX_BROWSER_ENTRIES: usize = 200;

/// Original development-preview shell. `AppSession` remains the only document writer.
pub struct PaintApp {
    session: AppSession,
    fill_color: Bgra,
    secondary_color: Bgra,
    active_color_slot: ColorSlot,
    hsv_hue: f32,
    hsv_saturation: f32,
    hsv_value: f32,
    hsv_base_color: Bgra,
    colors_expanded: bool,
    tools_visible: bool,
    tools_floating: bool,
    colors_visible: bool,
    layers_visible: bool,
    panels_floating: bool,
    picking_color: bool,
    active_tool: Tool,
    brush_size: u32,
    recolor_tolerance: u8,
    fill_tolerance: u8,
    fill_global: bool,
    paint_points: Vec<Point>,
    icons: IconTextures,
    tools_window_rect: Option<egui::Rect>,
    history_window_rect: Option<egui::Rect>,
    colors_window_rect: Option<egui::Rect>,
    colors_close_requested: bool,
    layers_window_rect: Option<egui::Rect>,
    drag_start: Option<Point>,
    drag_current: Option<Point>,
    cursor_point: Option<Point>,
    hex_input: String,
    hex_error: Option<String>,
    history_entries: Vec<String>,
    redo_entries: Vec<String>,
    history_visible: bool,
    selection_input: SelectionInput,
    texture: Option<egui::TextureHandle>,
    texture_dirty: bool,
    zoom: f32,
    fit_canvas_requested: bool,
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
        Self::new_with_panels(creation_context, true)
    }

    pub fn new_docked(creation_context: &eframe::CreationContext<'_>) -> Self {
        Self::new_with_panels(creation_context, false)
    }

    fn new_with_panels(
        creation_context: &eframe::CreationContext<'_>,
        panels_floating: bool,
    ) -> Self {
        let icons = IconTextures::load(&creation_context.egui_ctx);
        let mut app = Self {
            session: default_session(),
            fill_color: Bgra::BLACK,
            secondary_color: Bgra::WHITE,
            active_color_slot: ColorSlot::Primary,
            hsv_hue: 0.0,
            hsv_saturation: 0.0,
            hsv_value: 0.0,
            hsv_base_color: Bgra::BLACK,
            colors_expanded: false,
            tools_visible: true,
            tools_floating: true,
            colors_visible: true,
            layers_visible: true,
            panels_floating,
            picking_color: false,
            active_tool: Tool::Select,
            brush_size: 45,
            recolor_tolerance: 50,
            fill_tolerance: 50,
            fill_global: false,
            paint_points: Vec::new(),
            icons,
            tools_window_rect: None,
            history_window_rect: None,
            colors_window_rect: None,
            colors_close_requested: false,
            layers_window_rect: None,
            drag_start: None,
            drag_current: None,
            cursor_point: None,
            hex_input: format_hex_color(Bgra::BLACK),
            hex_error: None,
            history_entries: vec!["New Image".to_owned()],
            redo_entries: Vec::new(),
            history_visible: true,
            selection_input: SelectionInput::default(),
            texture: None,
            texture_dirty: true,
            zoom: 1.0,
            fit_canvas_requested: false,
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

    pub const fn secondary_color(&self) -> Bgra {
        self.secondary_color
    }

    #[doc(hidden)]
    pub const fn panels_floating(&self) -> bool {
        self.panels_floating
    }

    #[doc(hidden)]
    pub const fn colors_window_bounds(&self) -> Option<egui::Rect> {
        self.colors_window_rect
    }

    #[doc(hidden)]
    pub fn history_entries(&self) -> &[String] {
        &self.history_entries
    }

    const fn active_color(&self) -> Bgra {
        match self.active_color_slot {
            ColorSlot::Primary => self.fill_color,
            ColorSlot::Secondary => self.secondary_color,
        }
    }

    fn set_active_color(&mut self, color: Bgra) {
        match self.active_color_slot {
            ColorSlot::Primary => self.fill_color = color,
            ColorSlot::Secondary => self.secondary_color = color,
        }
    }

    fn swap_colors(&mut self) {
        std::mem::swap(&mut self.fill_color, &mut self.secondary_color);
    }

    fn reset_colors(&mut self) {
        self.fill_color = Bgra::BLACK;
        self.secondary_color = Bgra::WHITE;
        self.active_color_slot = ColorSlot::Primary;
        self.hex_input = format_hex_color(self.fill_color);
        self.hex_error = None;
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
        self.fit_canvas_requested = false;
        self.drag_start = None;
        self.drag_current = None;
        self.layer_thumbnails.clear();
        self.history_entries = vec!["New Image".to_owned()];
        self.redo_entries.clear();
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
        let history_label = history_label(&command);
        let is_undo = matches!(command, Command::Undo);
        let is_redo = matches!(command, Command::Redo);
        match self.session.execute(command) {
            Ok(changed) => {
                if changed {
                    if is_undo {
                        if self.history_entries.len() > 1
                            && let Some(entry) = self.history_entries.pop()
                        {
                            self.redo_entries.push(entry);
                        }
                    } else if is_redo {
                        if let Some(entry) = self.redo_entries.pop() {
                            self.history_entries.push(entry);
                        }
                    } else if let Some(label) = history_label {
                        self.history_entries.push(label);
                        self.redo_entries.clear();
                    }
                }
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
                ui.menu_button("Edit", |ui| {
                    let undo = ui.add_enabled(self.session.can_undo(), egui::Button::new("Undo"));
                    focus_first_menu_item_on_keyboard_open(ui, &undo);
                    if undo.clicked() {
                        self.execute(Command::Undo, "Undid the last edit");
                        ui.close();
                    }
                    if ui
                        .add_enabled(self.session.can_redo(), egui::Button::new("Redo"))
                        .clicked()
                    {
                        self.execute(Command::Redo, "Redid the next edit");
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            self.session.document().selection().is_some(),
                            egui::Button::new("Deselect"),
                        )
                        .clicked()
                    {
                        self.execute(Command::ClearSelection, "Selection cleared");
                        ui.close();
                    }
                });
                ui.menu_button("View", |ui| {
                    let zoom_in = ui.button("Zoom In");
                    focus_first_menu_item_on_keyboard_open(ui, &zoom_in);
                    if zoom_in.clicked() {
                        self.zoom = (self.zoom + 0.25).min(8.0);
                        ui.close();
                    }
                    if ui.button("Zoom Out").clicked() {
                        self.zoom = (self.zoom - 0.25).max(0.1);
                        ui.close();
                    }
                    if ui.button("Actual Size").clicked() {
                        self.zoom = 1.0;
                        ui.close();
                    }
                    if ui.button("Fit Canvas").clicked() {
                        self.fit_canvas_requested = true;
                        ui.close();
                    }
                });
                ui.menu_button("Layers", |ui| {
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
                ui.menu_button("Tools", |ui| {
                    let select = ui.button("Rectangle Select");
                    focus_first_menu_item_on_keyboard_open(ui, &select);
                    if select.clicked() {
                        self.active_tool = Tool::Select;
                        self.picking_color = false;
                        ui.close();
                    }
                    let tools = [
                        ("Zoom", Tool::Zoom),
                        ("Paint Bucket", Tool::Fill),
                        ("Color Picker", Tool::Picker),
                        ("Pencil", Tool::Pencil),
                        ("Paintbrush", Tool::Brush),
                        ("Eraser", Tool::Eraser),
                        ("Line / Curve", Tool::Line),
                        ("Rectangle", Tool::Rectangle),
                        ("Ellipse", Tool::Ellipse),
                        ("Gradient", Tool::Gradient),
                        ("Recolor", Tool::Recolor),
                        ("Move Selected Pixels", Tool::Move),
                        ("Pan", Tool::Pan),
                    ];
                    for (label, tool) in tools {
                        if ui.button(label).clicked() {
                            self.active_tool = tool;
                            self.picking_color = tool == Tool::Picker;
                            ui.close();
                        }
                    }
                });
                ui.menu_button("Window", |ui| {
                    ui.checkbox(&mut self.tools_visible, "Tools window");
                    ui.checkbox(&mut self.tools_floating, "Float Tools");
                    ui.checkbox(&mut self.colors_visible, "Colors window");
                    ui.checkbox(&mut self.layers_visible, "Layers window");
                    ui.checkbox(&mut self.history_visible, "History window");
                    ui.separator();
                    let label = if self.panels_floating {
                        "Dock Panels"
                    } else {
                        "Float Panels"
                    };
                    let toggle = ui.button(label);
                    focus_first_menu_item_on_keyboard_open(ui, &toggle);
                    if toggle.clicked() {
                        self.panels_floating = !self.panels_floating;
                        if self.panels_floating {
                            self.tools_visible = true;
                            self.tools_floating = true;
                            self.colors_visible = true;
                            self.layers_visible = true;
                        } else {
                            self.tools_floating = false;
                        }
                        ui.close();
                    }
                });
                ui.menu_button("Help", |ui| {
                    ui.label("Paint.R development preview · MIT first-party license");
                    ui.separator();
                    ui.label("Keyboard shortcuts (Cmd/Ctrl):");
                    ui.label("N New | O Open | Shift+E Export");
                    ui.label("I Invert | F Fill | R Rectangle selection");
                    ui.label("Layers menu flips the complete active layer");
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
                if icon_button(ui, &self.icons, Icon::NewDocument, "New document", true).clicked() {
                    self.request_new();
                }
                if icon_button(ui, &self.icons, Icon::OpenFile, "Open PNG file", true).clicked() {
                    self.show_path_dialog(PathDialogKind::Open);
                }
                if icon_button(ui, &self.icons, Icon::ExportFile, "Export PNG file", true).clicked()
                {
                    self.show_path_dialog(PathDialogKind::Export);
                }
                ui.add_space(5.0);
                ui.separator();
                ui.add_space(5.0);
                if icon_button(ui, &self.icons, Icon::Undo, "Undo", self.session.can_undo())
                    .clicked()
                {
                    self.execute(Command::Undo, "Undid the last edit");
                }
                if icon_button(ui, &self.icons, Icon::Redo, "Redo", self.session.can_redo())
                    .clicked()
                {
                    self.execute(Command::Redo, "Redid the next edit");
                }
                ui.add_space(5.0);
                ui.separator();
                ui.add_space(5.0);
                if icon_button(ui, &self.icons, Icon::AddLayer, "Add Layer", true).clicked() {
                    self.add_layer();
                }
            });
            ui.add_space(2.0);
        });
    }

    fn tools_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("paint-r-tools")
            .default_size(68.0)
            .min_size(68.0)
            .max_size(68.0)
            .resizable(false)
            .show(ui, |ui| self.tools_content(ui));
    }

    fn tools_content(&mut self, ui: &mut egui::Ui) {
        if !self.tools_floating {
            ui.add_space(4.0);
            ui.strong("Tools");
            ui.separator();
        }
        macro_rules! tool {
            ($ui:expr, $icon:expr, $label:expr, $tool:expr) => {
                if tool_button(
                    $ui,
                    &self.icons,
                    $icon,
                    $label,
                    true,
                    self.active_tool == $tool,
                )
                .clicked()
                {
                    self.active_tool = $tool;
                    self.picking_color = $tool == Tool::Picker;
                }
            };
        }
        egui::Grid::new("paint-r-tool-grid")
            .num_columns(2)
            .spacing([1.0, 1.0])
            .show(ui, |ui| {
                tool!(ui, Icon::SelectRectangle, "Rectangle Select", Tool::Select);
                tool!(ui, Icon::Move, "Move Selected Pixels", Tool::Move);
                ui.end_row();
                tool!(ui, Icon::Zoom, "Zoom", Tool::Zoom);
                tool!(ui, Icon::Pan, "Pan", Tool::Pan);
                ui.end_row();
                tool!(ui, Icon::Fill, "Paint Bucket", Tool::Fill);
                tool!(ui, Icon::Gradient, "Gradient", Tool::Gradient);
                ui.end_row();
                tool!(ui, Icon::Brush, "Paintbrush", Tool::Brush);
                tool!(ui, Icon::Eraser, "Eraser", Tool::Eraser);
                ui.end_row();
                tool!(ui, Icon::Pencil, "Pencil", Tool::Pencil);
                tool!(ui, Icon::EyeDropper, "Color Picker", Tool::Picker);
                ui.end_row();
                tool!(ui, Icon::Recolor, "Recolor", Tool::Recolor);
                tool!(ui, Icon::Line, "Line / Curve", Tool::Line);
                ui.end_row();
                tool!(ui, Icon::RectangleShape, "Rectangle", Tool::Rectangle);
                tool!(ui, Icon::Ellipse, "Ellipse", Tool::Ellipse);
                ui.end_row();
            });
    }

    fn tool_options_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("paint-r-tool-options").show(ui, |ui| {
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| {
                ui.strong("Tool Options");
                ui.separator();
                match self.active_tool {
                    Tool::Select => {
                        ui.label("Rectangle Select");
                        selection_input(ui, "Selection left", &mut self.selection_input.left);
                        selection_input(ui, "Selection top", &mut self.selection_input.top);
                        selection_input(ui, "Selection right", &mut self.selection_input.right);
                        selection_input(ui, "Selection bottom", &mut self.selection_input.bottom);
                        if ui.button("Apply selection").clicked() {
                            self.select_rectangle();
                        }
                        if let Some(selection) = self.session.document().selection() {
                            ui.label(format!(
                                "({}, {})..({}, {})",
                                selection.left(),
                                selection.top(),
                                selection.right(),
                                selection.bottom()
                            ));
                        } else {
                            ui.label("No selection");
                        }
                    }
                    Tool::Zoom => {
                        ui.label("Zoom");
                        ui.label(format!("{}%", (self.zoom * 100.0).round() as u32));
                        for (label, zoom) in
                            [("50%", 0.5), ("100%", 1.0), ("200%", 2.0), ("400%", 4.0)]
                        {
                            if labeled_button(ui, label, label).clicked() {
                                self.zoom = zoom;
                            }
                        }
                        if labeled_button(ui, "Fit", "Fit canvas").clicked() {
                            self.fit_canvas_requested = true;
                        }
                    }
                    Tool::Fill => {
                        ui.label("Paint Bucket");
                        let color = self.fill_color;
                        ui.colored_label(
                            egui::Color32::from_rgba_unmultiplied(
                                color.r, color.g, color.b, color.a,
                            ),
                            "Primary color",
                        );
                        ui.label("Flood Mode");
                        ui.selectable_value(&mut self.fill_global, false, "Contiguous");
                        ui.selectable_value(&mut self.fill_global, true, "Global");
                        ui.label("Fill");
                        ui.label("Solid Color");
                        let tolerance_label = ui.label("Tolerance");
                        ui.add(
                            egui::DragValue::new(&mut self.fill_tolerance)
                                .range(0..=100)
                                .suffix("%"),
                        )
                        .labelled_by(tolerance_label.id);
                        ui.label("Sampling");
                        ui.label("Layer");
                        ui.label("Blend Mode");
                        ui.label("Normal");
                        ui.label("Click the canvas to fill");
                    }
                    Tool::Pencil | Tool::Brush | Tool::Eraser | Tool::Line => {
                        ui.label(match self.active_tool {
                            Tool::Pencil => "Pencil",
                            Tool::Brush => "Paintbrush",
                            Tool::Eraser => "Eraser",
                            _ => "Line / Curve",
                        });
                        if self.active_tool == Tool::Pencil {
                            ui.label("Width is fixed at 1 px");
                        } else {
                            ui.label("Width");
                            ui.add(
                                egui::DragValue::new(&mut self.brush_size)
                                    .range(1..=64)
                                    .suffix(" px"),
                            );
                        }
                        if self.active_tool == Tool::Eraser {
                            ui.label("Erases the active layer to transparency");
                        } else {
                            let color = self.fill_color;
                            ui.colored_label(
                                egui::Color32::from_rgba_unmultiplied(
                                    color.r, color.g, color.b, color.a,
                                ),
                                "Primary color",
                            );
                        }
                        ui.label("Drag on the canvas to paint");
                    }
                    Tool::Rectangle => {
                        ui.label("Rectangle Shape");
                        ui.label("Drag on the canvas to fill with the primary color");
                    }
                    Tool::Ellipse => {
                        ui.label("Ellipse Shape");
                        ui.label("Drag on the canvas to fill with the primary color");
                    }
                    Tool::Gradient => {
                        ui.label("Gradient");
                        let (from, to) = (self.fill_color, self.secondary_color);
                        ui.colored_label(
                            egui::Color32::from_rgba_unmultiplied(from.r, from.g, from.b, from.a),
                            "Primary",
                        );
                        ui.label("to");
                        ui.colored_label(
                            egui::Color32::from_rgba_unmultiplied(to.r, to.g, to.b, to.a),
                            "Secondary",
                        );
                        ui.label("Drag to set the gradient direction");
                    }
                    Tool::Recolor => {
                        ui.label("Recolor");
                        let (target, replacement) = (self.secondary_color, self.fill_color);
                        ui.colored_label(
                            egui::Color32::from_rgba_unmultiplied(
                                target.r, target.g, target.b, target.a,
                            ),
                            "Target: secondary",
                        );
                        ui.colored_label(
                            egui::Color32::from_rgba_unmultiplied(
                                replacement.r,
                                replacement.g,
                                replacement.b,
                                replacement.a,
                            ),
                            "Replacement: primary",
                        );
                        ui.label("Width");
                        ui.add(
                            egui::DragValue::new(&mut self.brush_size)
                                .range(1..=64)
                                .suffix(" px"),
                        );
                        let tolerance_label = ui.label("Tolerance");
                        ui.add(
                            egui::DragValue::new(&mut self.recolor_tolerance)
                                .range(0..=100)
                                .suffix("%"),
                        )
                        .labelled_by(tolerance_label.id);
                        ui.label("Drag over matching pixels to recolor");
                    }
                    Tool::Pan => {
                        ui.label("Pan");
                        ui.label("Drag to move the canvas view");
                    }
                    Tool::Move => {
                        ui.label("Move Selected Pixels");
                        ui.label("Drag to move the selection, or the active layer without one");
                    }
                    Tool::Picker => {
                        ui.label("Color Picker");
                        ui.label("Click the canvas to sample the composed color");
                    }
                }
            });
            ui.add_space(2.0);
        });
    }

    fn layers_content(&mut self, ui: &mut egui::Ui) {
        let mut pending = None;
        ui.horizontal(|ui| {
            if !self.panels_floating {
                ui.heading("Layers");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(ui, &self.icons, Icon::AddLayer, "New Layer", true).clicked() {
                    self.add_layer();
                }
            });
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("paint-r-layer-list")
            .show(ui, |ui| {
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
                                                .fit_to_exact_size(egui::vec2(32.0, 26.0))
                                                .alt_text(format!(
                                                    "Layer {index} thumbnail: {}",
                                                    layer.name()
                                                )),
                                        );
                                    });
                            }
                            ui.vertical(|ui| {
                                let active_label =
                                    ui.selectable_label(active, layer.name()).accessible_name(
                                        format!("Active layer {index}: {}", layer.name()),
                                    );
                                if active_label.clicked() {
                                    pending = Some(Command::SetActiveLayer { index });
                                }
                                let mut visible = layer.is_visible();
                                let visible_response = ui
                                    .checkbox(&mut visible, "Visible")
                                    .accessible_name(format!("Layer {index} visible"));
                                if visible_response.changed() {
                                    pending = Some(Command::SetLayerVisibility { index, visible });
                                }
                            });
                        });
                        let mut opacity = layer.opacity();
                        let opacity_response = ui
                            .add(egui::Slider::new(&mut opacity, 0..=u8::MAX).text("Opacity"))
                            .accessible_name(format!("Layer {index} opacity"));
                        if opacity_response.changed() {
                            pending = Some(Command::SetLayerOpacity { index, opacity });
                        }
                    });
                }
            });
        if let Some(command) = pending {
            self.execute(command, "Layer properties updated");
        }
    }

    fn history_content(&mut self, ui: &mut egui::Ui) {
        ui.heading("History");
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("paint-r-history-list")
            .show(ui, |ui| {
                for entry in &self.history_entries {
                    ui.label(entry);
                }
                for entry in self.redo_entries.iter().rev() {
                    ui.add_enabled(false, egui::Label::new(entry).selectable(false));
                }
            });
        ui.separator();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.session.can_undo(), egui::Button::new("Undo History"))
                .clicked()
            {
                self.execute(Command::Undo, "Undid the last edit");
            }
            if ui
                .add_enabled(self.session.can_redo(), egui::Button::new("Redo History"))
                .clicked()
            {
                self.execute(Command::Redo, "Redid the next edit");
            }
        });
    }

    fn colors_content(&mut self, ui: &mut egui::Ui) {
        if self.panels_floating {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Colors").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button("X")
                        .accessible_name("Close Colors window")
                        .clicked()
                    {
                        self.colors_close_requested = true;
                    }
                });
            });
            ui.separator();
        } else {
            ui.heading("Colors");
        }
        let initial_slot = self.active_color_slot;
        let mut active = self.active_color();
        let initial_color = active;
        ui.horizontal_top(|ui| {
            let left_width = 192.0;
            ui.allocate_ui_with_layout(
                egui::vec2(left_width, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(left_width);
                    let palette_height = if self.colors_expanded { 72.0 } else { 24.0 };
                    let area_height = 178.0 + palette_height;
                    let (area, _) = ui.allocate_exact_size(
                        egui::vec2(left_width, area_height),
                        egui::Sense::hover(),
                    );

                    let slots_rect = egui::Rect::from_min_size(area.min, egui::vec2(64.0, 54.0));
                    let mut slots_ui = ui.new_child(egui::UiBuilder::new().max_rect(slots_rect));
                    let (slot, bounds) = color_slots_widget(
                        &mut slots_ui,
                        self.fill_color,
                        self.secondary_color,
                        self.active_color_slot,
                    );
                    if let Some(slot) = slot {
                        self.active_color_slot = slot;
                    }
                    let swap_rect = egui::Rect::from_min_size(
                        bounds.min + egui::vec2(40.0, 0.0),
                        egui::vec2(16.0, 16.0),
                    );
                    if icon_button_at(ui, &self.icons, swap_rect, Icon::SwapColors, "Swap colors")
                        .clicked()
                    {
                        self.swap_colors();
                        active = self.active_color();
                        self.hex_input = format_hex_color(active);
                        self.hex_error = None;
                    }
                    let reset_rect = egui::Rect::from_min_size(
                        bounds.min + egui::vec2(0.0, 38.0),
                        egui::vec2(16.0, 16.0),
                    );
                    if icon_button_at(
                        ui,
                        &self.icons,
                        reset_rect,
                        Icon::ResetColors,
                        "Reset to black and white",
                    )
                    .clicked()
                    {
                        self.reset_colors();
                        active = self.active_color();
                    }

                    let more_rect = egui::Rect::from_min_size(
                        area.min + egui::vec2(left_width - 60.0, 0.0),
                        egui::vec2(60.0, 22.0),
                    );
                    let mut more_ui = ui.new_child(egui::UiBuilder::new().max_rect(more_rect));
                    if more_ui
                        .add_sized(
                            [60.0, 22.0],
                            egui::Button::new(if self.colors_expanded { "Less" } else { "More" }),
                        )
                        .clicked()
                    {
                        self.colors_expanded = !self.colors_expanded;
                        more_ui.ctx().request_repaint();
                    }

                    if self.active_color_slot != initial_slot {
                        active = self.active_color();
                        self.hex_input = format_hex_color(active);
                        self.hex_error = None;
                    }

                    let wheel_rect = egui::Rect::from_min_size(
                        area.min + egui::vec2(42.0, 24.0),
                        egui::vec2(150.0, 150.0),
                    );
                    let mut wheel_ui = ui.new_child(egui::UiBuilder::new().max_rect(wheel_rect));
                    if let Some(color) = color_wheel(&mut wheel_ui, active) {
                        active = color;
                    }

                    let palette_rect = egui::Rect::from_min_size(
                        area.min + egui::vec2(0.0, 178.0),
                        egui::vec2(192.0, palette_height),
                    );
                    let mut palette_ui =
                        ui.new_child(egui::UiBuilder::new().max_rect(palette_rect));
                    palette_ui.set_width(192.0);
                    palette_ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        for &(label, preset) in PRESET_COLORS {
                            let mut select = |color: Bgra, label: &str| {
                                let response = color_swatch(ui, label, color, color == active);
                                if response.clicked() {
                                    active = color;
                                }
                                if response.secondary_clicked() {
                                    match self.active_color_slot {
                                        ColorSlot::Primary => self.secondary_color = color,
                                        ColorSlot::Secondary => self.fill_color = color,
                                    }
                                }
                            };
                            select(preset, label);
                            if self.colors_expanded {
                                let light = mix_bgra(preset, Bgra::WHITE, 0.5);
                                let light_label = format!("{label} light");
                                select(light, &light_label);
                                let dark = mix_bgra(preset, Bgra::BLACK, 0.5);
                                let dark_label = format!("{label} dark");
                                select(dark, &dark_label);
                            }
                        }
                    });
                },
            );
            if self.colors_expanded {
                let content = ui.max_rect();
                let right_rect = egui::Rect::from_min_size(
                    egui::pos2(content.right() - 180.0, content.top()),
                    egui::vec2(198.0, content.height()),
                );
                let mut right_ui = ui.new_child(egui::UiBuilder::new().max_rect(right_rect));
                let ui = &mut right_ui;
                ui.vertical(|ui| {
                    ui.set_width(198.0);
                    ui.strong("RGB");
                    compact_color_slider(ui, "Red", "R", &mut active.r);
                    compact_color_slider(ui, "Green", "G", &mut active.g);
                    compact_color_slider(ui, "Blue", "B", &mut active.b);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.allocate_ui_with_layout(
                            egui::vec2(26.0, 20.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.label("HEX");
                            },
                        );
                        let response = ui
                            .add_sized(
                                [152.0, 20.0],
                                egui::TextEdit::singleline(&mut self.hex_input)
                                    .desired_width(152.0),
                            )
                            .accessible_name("Hex color");
                        if response.changed() {
                            match crate::color_from_hex(&self.hex_input) {
                                Ok(parsed) => {
                                    active = parsed;
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
                    ui.strong("HSV");
                    if self.hsv_base_color != active {
                        let (hue, saturation, value) = hsv_from_color(active);
                        self.hsv_hue = hue;
                        self.hsv_saturation = saturation;
                        self.hsv_value = value;
                        self.hsv_base_color = active;
                    }
                    let hue = self.hsv_hue;
                    let saturation = self.hsv_saturation;
                    let value = self.hsv_value;
                    let mut hue_value = hue;
                    let mut saturation_value = saturation * 100.0;
                    let mut brightness_value = value * 100.0;
                    let hue_base = hue_value;
                    let saturation_base = saturation_value;
                    let brightness_base = brightness_value;
                    hsv_gradient_slider(ui, "Hue", "H", &mut hue_value, 0.0, 360.0, |t| {
                        let bgra = crate::color_from_hsv(t * 360.0, 1.0, 1.0, 255);
                        egui::Color32::from_rgb(bgra.r, bgra.g, bgra.b)
                    });
                    hsv_gradient_slider(
                        ui,
                        "Saturation",
                        "S",
                        &mut saturation_value,
                        0.0,
                        100.0,
                        |t| {
                            let bgra = crate::color_from_hsv(
                                hue_base,
                                t / 100.0,
                                brightness_base / 100.0,
                                255,
                            );
                            egui::Color32::from_rgb(bgra.r, bgra.g, bgra.b)
                        },
                    );
                    hsv_gradient_slider(ui, "Value", "V", &mut brightness_value, 0.0, 100.0, |t| {
                        let bgra = crate::color_from_hsv(
                            hue_base,
                            saturation_base / 100.0,
                            t / 100.0,
                            255,
                        );
                        egui::Color32::from_rgb(bgra.r, bgra.g, bgra.b)
                    });
                    if (hue_value, saturation_value, brightness_value)
                        != (hue, saturation * 100.0, value * 100.0)
                    {
                        self.hsv_hue = hue_value;
                        self.hsv_saturation = saturation_value / 100.0;
                        self.hsv_value = brightness_value / 100.0;
                        active = crate::color_from_hsv(
                            self.hsv_hue,
                            self.hsv_saturation,
                            self.hsv_value,
                            active.a,
                        );
                        self.hsv_base_color = active;
                    }
                    ui.strong("Opacity");
                    compact_color_slider(ui, "Opacity", "A", &mut active.a);
                });
            }
        });
        self.set_active_color(active);
        if active != initial_color || self.active_color_slot != initial_slot {
            self.hex_input = format_hex_color(active);
            self.hex_error = None;
        }
    }

    fn right_dock(&mut self, ui: &mut egui::Ui) {
        let dock_width = if self.colors_expanded { 402.0 } else { 232.0 };
        egui::Panel::right("paint-r-right-dock")
            .exact_size(dock_width)
            .resizable(false)
            .show(ui, |ui| {
                let colors_height = if self.colors_expanded { 620.0 } else { 380.0 };
                if self.colors_visible {
                    egui::ScrollArea::vertical()
                        .id_salt("paint-r-colors-scroll")
                        .max_height(colors_height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.colors_content(ui));
                }
                if self.colors_visible && self.layers_visible {
                    ui.separator();
                }
                if self.layers_visible {
                    egui::ScrollArea::vertical()
                        .id_salt("paint-r-layers-scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.layers_content(ui));
                }
            });
    }

    fn floating_panels(&mut self, ui: &mut egui::Ui) {
        let context = ui.ctx().clone();
        if self.tools_visible && self.tools_floating {
            let mut open = self.tools_visible;
            let frame = floating_window_frame(&context, self.tools_window_rect);
            if let Some(inner) = egui::Window::new("Tools")
                .open(&mut open)
                .frame(frame)
                .collapsible(false)
                .default_pos(egui::pos2(20.0, 180.0))
                .default_width(68.0)
                .resizable(false)
                .show(&context, |ui| self.tools_content(ui))
            {
                self.tools_window_rect = Some(inner.response.rect);
            }
            self.tools_visible = open;
        }
        if self.history_visible {
            let mut open = self.history_visible;
            let frame = floating_window_frame(&context, self.history_window_rect);
            if let Some(inner) = egui::Window::new("History")
                .open(&mut open)
                .frame(frame)
                .collapsible(false)
                .default_pos(egui::pos2(500.0, 260.0))
                .default_width(200.0)
                .show(&context, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("paint-r-floating-history-scroll")
                        .max_height(260.0)
                        .show(ui, |ui| self.history_content(ui));
                })
            {
                self.history_window_rect = Some(inner.response.rect);
            }
            self.history_visible = open;
        }
        if self.panels_floating && self.colors_visible {
            let mut open = self.colors_visible;
            let frame = floating_window_frame(&context, self.colors_window_rect);
            let colors_width = if self.colors_expanded { 402.0 } else { 206.0 };
            if let Some(inner) = egui::Window::new("Colors")
                .open(&mut open)
                .title_bar(false)
                .frame(frame)
                .collapsible(false)
                .default_pos(egui::pos2(20.0, 470.0))
                .fixed_size(egui::vec2(
                    colors_width,
                    if self.colors_expanded { 354.0 } else { 250.0 },
                ))
                .scroll(false)
                .show(&context, |ui| self.colors_content(ui))
            {
                self.colors_window_rect = Some(inner.response.rect);
                if inner.response.rect.width() + 1.0 < colors_width {
                    context.request_repaint();
                }
            }
            if self.colors_close_requested {
                open = false;
                self.colors_close_requested = false;
            }
            self.colors_visible = open;
        }
        if self.panels_floating && self.layers_visible {
            let mut open = self.layers_visible;
            let frame = floating_window_frame(&context, self.layers_window_rect);
            if let Some(inner) = egui::Window::new("Layers")
                .open(&mut open)
                .frame(frame)
                .collapsible(false)
                .default_pos(egui::pos2(980.0, 260.0))
                .default_width(220.0)
                .show(&context, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("paint-r-floating-layers-scroll")
                        .max_height(520.0)
                        .show(ui, |ui| self.layers_content(ui));
                })
            {
                self.layers_window_rect = Some(inner.response.rect);
            }
            self.layers_visible = open;
        }
    }

    fn status_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("paint-r-status").show(ui, |ui| {
            ui.horizontal(|ui| {
                let dirty = if self.session.dirty() {
                    "Modified"
                } else {
                    "Clean"
                };
                ui.strong(dirty);
                ui.separator();
                ui.label(&self.status);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let size = self.session.document().size();
                    ui.label(format!("{} × {}", size.width(), size.height()));
                    ui.separator();
                    if let Some(point) = self.cursor_point {
                        ui.label(format!("{}, {} px", point.x, point.y));
                    } else {
                        ui.label("—, — px");
                    }
                });
            });
        });
    }

    fn canvas_panel(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            let size = self.session.document().size();
            ui.horizontal(|ui| {
                ui.heading(format!("Canvas — {}×{}", size.width(), size.height()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if labeled_button(ui, "Fit", "Fit canvas").clicked() {
                        self.fit_canvas_requested = true;
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
            if self.fit_canvas_requested {
                let available = ui.available_size();
                self.zoom = ((available.x - 24.0).max(32.0) / size.width() as f32)
                    .min((available.y - 24.0).max(32.0) / size.height() as f32)
                    .clamp(0.05, 8.0);
                self.fit_canvas_requested = false;
            }
            ui.label("Actual composed session surface");
            let texture = self.texture.clone();
            let zoom = self.zoom;
            let picking = self.picking_color;
            let mut picked = None;
            let mut pending_selection = None;
            let mut pending_paint: Option<(Command, String)> = None;
            egui::ScrollArea::both()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if let Some(texture) = texture {
                        let display =
                            egui::vec2(size.width() as f32 * zoom, size.height() as f32 * zoom);
                        let sense = if picking {
                            egui::Sense::click()
                        } else {
                            egui::Sense::click_and_drag()
                        };
                        let (rect, response) = ui.allocate_exact_size(display, sense);
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::Role::Image,
                                true,
                                "Composed document canvas",
                            )
                        });
                        let response = response.on_hover_cursor(egui::CursorIcon::Crosshair);
                        if ui.is_rect_visible(rect) {
                            egui::Image::from_texture(&texture).paint_at(ui, rect);
                        }
                        let pointer = response.interact_pointer_pos();
                        self.cursor_point = pointer
                            .and_then(|position| canvas_point_at(position, rect, zoom, size));
                        if picking {
                            eprintln!(
                                "PICKER_FRAME pointer={pointer:?} clicked={}",
                                response.clicked()
                            );
                            if response.clicked()
                                && let Some(point) = pointer.and_then(|position| {
                                    canvas_point_at(position, rect, zoom, size)
                                })
                            {
                                picked = Some(point);
                            }
                        } else if self.active_tool == Tool::Zoom {
                            if response.clicked() {
                                self.zoom = (self.zoom + 0.25).min(8.0);
                            }
                        } else if self.active_tool == Tool::Pan {
                            if response.dragged() {
                                ui.scroll_with_delta(-response.drag_delta());
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                            } else if response.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                            }
                        } else if self.active_tool == Tool::Fill {
                            if response.clicked()
                                && let Some(point) = pointer.and_then(|position| {
                                    canvas_point_at(position, rect, zoom, size)
                                })
                            {
                                let threshold = (u16::from(self.fill_tolerance) * 255 / 100) as u8;
                                pending_paint = Some((
                                    Command::FloodFill {
                                        point,
                                        color: self.fill_color,
                                        tolerance: threshold,
                                        global: self.fill_global,
                                    },
                                    if self.fill_global {
                                        "Filled matching pixels globally".to_owned()
                                    } else {
                                        "Filled a contiguous area".to_owned()
                                    },
                                ));
                            }
                        } else if self.active_tool != Tool::Picker {
                            let point = || {
                                pointer.and_then(|position| {
                                    canvas_point_at(position, rect, zoom, size)
                                })
                            };
                            let is_paint = matches!(
                                self.active_tool,
                                Tool::Pencil | Tool::Brush | Tool::Eraser | Tool::Recolor
                            );
                            if self.drag_start.is_none()
                                && response.is_pointer_button_down_on()
                                && let Some(point) = point()
                            {
                                self.drag_start = Some(point);
                                self.drag_current = Some(point);
                                self.paint_points.clear();
                                if is_paint {
                                    self.paint_points.push(point);
                                }
                            }
                            if response.dragged()
                                && let Some(point) = point()
                            {
                                self.drag_current = Some(point);
                                if is_paint && self.paint_points.last() != Some(&point) {
                                    self.paint_points.push(point);
                                }
                            }
                            if response.drag_stopped()
                                && let (Some(start), Some(end)) =
                                    (self.drag_start.take(), self.drag_current.take())
                            {
                                let bounds = selection_rect_from_drag(start, end);
                                let color = self.fill_color;
                                let size = match self.active_tool {
                                    Tool::Pencil => 1,
                                    _ => self.brush_size.max(1),
                                };
                                pending_paint = match self.active_tool {
                                    Tool::Select => {
                                        pending_selection = Some(bounds);
                                        None
                                    }
                                    Tool::Rectangle => Some((
                                        Command::FillRect {
                                            rect: bounds,
                                            color,
                                        },
                                        "Filled rectangle shape".to_owned(),
                                    )),
                                    Tool::Ellipse => Some((
                                        Command::FillEllipse {
                                            rect: bounds,
                                            color,
                                        },
                                        "Filled ellipse shape".to_owned(),
                                    )),
                                    Tool::Line => Some((
                                        Command::Stroke {
                                            label: "Line / Curve",
                                            points: vec![start, end],
                                            size,
                                            color: Some(color),
                                        },
                                        "Drew a line".to_owned(),
                                    )),
                                    Tool::Gradient => Some((
                                        Command::Gradient {
                                            start,
                                            end,
                                            from: color,
                                            to: self.secondary_color,
                                        },
                                        "Painted a gradient".to_owned(),
                                    )),
                                    Tool::Recolor => {
                                        let points = if self.paint_points.is_empty() {
                                            vec![start, end]
                                        } else {
                                            std::mem::take(&mut self.paint_points)
                                        };
                                        Some((
                                            Command::Recolor {
                                                points,
                                                size,
                                                target: self.secondary_color,
                                                replacement: color,
                                                tolerance: (u16::from(self.recolor_tolerance) * 255
                                                    / 100)
                                                    as u8,
                                            },
                                            "Recolored matching pixels".to_owned(),
                                        ))
                                    }
                                    Tool::Move => {
                                        let dx = i64::from(end.x) - i64::from(start.x);
                                        let dy = i64::from(end.y) - i64::from(start.y);
                                        (dx != 0 || dy != 0).then(|| {
                                            (
                                                Command::Move {
                                                    dx: dx as i32,
                                                    dy: dy as i32,
                                                },
                                                "Moved selected pixels".to_owned(),
                                            )
                                        })
                                    }
                                    Tool::Pencil | Tool::Brush | Tool::Eraser => {
                                        let points = if self.paint_points.is_empty() {
                                            vec![start, end]
                                        } else {
                                            std::mem::take(&mut self.paint_points)
                                        };
                                        let (label, color, message) = match self.active_tool {
                                            Tool::Pencil => {
                                                ("Pencil", Some(color), "Painted with the pencil")
                                            }
                                            Tool::Brush => (
                                                "Paintbrush",
                                                Some(color),
                                                "Painted with the paintbrush",
                                            ),
                                            _ => ("Eraser", None, "Erased pixels"),
                                        };
                                        Some((
                                            Command::Stroke {
                                                label,
                                                points,
                                                size,
                                                color,
                                            },
                                            message.to_owned(),
                                        ))
                                    }
                                    _ => None,
                                };
                            }
                            if let (Some(start), Some(end)) = (self.drag_start, self.drag_current) {
                                let start_pos = rect.min
                                    + egui::vec2(start.x as f32 * zoom, start.y as f32 * zoom);
                                let end_pos = rect.min
                                    + egui::vec2(
                                        (end.x + 1) as f32 * zoom,
                                        (end.y + 1) as f32 * zoom,
                                    );
                                let marquee = egui::Rect::from_min_max(start_pos, end_pos);
                                if matches!(
                                    self.active_tool,
                                    Tool::Select | Tool::Rectangle | Tool::Ellipse
                                ) {
                                    ui.painter().rect_filled(
                                        marquee,
                                        0,
                                        egui::Color32::from_white_alpha(24),
                                    );
                                    ui.painter().rect_stroke(
                                        marquee,
                                        0,
                                        egui::Stroke::new(1.5, egui::Color32::LIGHT_BLUE),
                                        egui::StrokeKind::Inside,
                                    );
                                } else if matches!(self.active_tool, Tool::Line | Tool::Gradient) {
                                    ui.painter().line_segment(
                                        [
                                            start_pos + egui::vec2(zoom / 2.0, zoom / 2.0),
                                            end_pos - egui::vec2(zoom / 2.0, zoom / 2.0),
                                        ],
                                        egui::Stroke::new(1.5, egui::Color32::LIGHT_BLUE),
                                    );
                                }
                            }
                        }
                    } else {
                        ui.label("Canvas texture is unavailable; see the status message.");
                    }
                });
            if let Some(point) = picked {
                self.pick_color_at(point);
            }
            if let Some(selection) = pending_selection {
                self.execute(
                    Command::SelectRect(selection),
                    "Rectangle selection updated",
                );
            }
            if let Some((command, message)) = pending_paint {
                self.execute(command, &message);
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
                self.active_color_slot = ColorSlot::Primary;
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
        self.tool_options_panel(ui);
        self.status_panel(ui);
        if self.tools_floating || self.panels_floating || self.history_visible {
            self.floating_panels(ui);
        }
        if !self.tools_floating && self.tools_visible {
            self.tools_panel(ui);
        }
        if !self.panels_floating && (self.colors_visible || self.layers_visible) {
            self.right_dock(ui);
        }
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

#[derive(Clone, Copy, Eq, PartialEq)]
enum Tool {
    Select,
    Zoom,
    Fill,
    Rectangle,
    Pencil,
    Brush,
    Eraser,
    Line,
    Ellipse,
    Gradient,
    Recolor,
    Move,
    Pan,
    Picker,
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

fn floating_window_frame(
    context: &egui::Context,
    previous_rect: Option<egui::Rect>,
) -> egui::Frame {
    let style = context.style_of(egui::Theme::Dark);
    let mut frame = egui::Frame::window(&style);
    let pointer_over = previous_rect.is_some_and(|rect| {
        context
            .pointer_hover_pos()
            .is_some_and(|position| rect.contains(position))
    });
    // Paint.NET uses 0.75 opacity while floating and 1.0 once the pointer enters.
    let alpha = if pointer_over { 255 } else { 191 };
    frame.fill = egui::Color32::from_rgba_unmultiplied(38, 38, 38, alpha);
    frame
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

fn history_label(command: &Command) -> Option<String> {
    match command {
        Command::Fill(_) | Command::FloodFill { .. } => Some("Paint Bucket".to_owned()),
        Command::Stroke { label, .. } => Some((*label).to_owned()),
        Command::FillRect { .. } => Some("Rectangle".to_owned()),
        Command::FillEllipse { .. } => Some("Ellipse".to_owned()),
        Command::Gradient { .. } => Some("Gradient".to_owned()),
        Command::Recolor { .. } => Some("Recolor".to_owned()),
        Command::Move { .. } => Some("Move Selected Pixels".to_owned()),
        Command::Invert => Some("Invert Colors".to_owned()),
        Command::FlipLayerHorizontal => Some("Flip Layer Horizontal".to_owned()),
        Command::FlipLayerVertical => Some("Flip Layer Vertical".to_owned()),
        Command::AddLayer { name, .. } => Some(format!("Add Layer: {name}")),
        Command::SetLayerVisibility { index, visible } => {
            Some(format!("Layer {index} visibility: {visible}"))
        }
        Command::SetLayerOpacity { index, opacity } => {
            Some(format!("Layer {index} opacity: {opacity}"))
        }
        Command::SelectRect(_)
        | Command::ClearSelection
        | Command::SetActiveLayer { .. }
        | Command::Undo
        | Command::Redo => None,
    }
}

fn labeled_button(ui: &mut egui::Ui, text: &str, label: &'static str) -> egui::Response {
    let response = ui.button(text);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    response.on_hover_text(label)
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

fn canvas_point_at(position: egui::Pos2, rect: egui::Rect, zoom: f32, size: Size) -> Option<Point> {
    let x = ((position.x - rect.left()) / zoom).floor();
    let y = ((position.y - rect.top()) / zoom).floor();
    if x < 0.0 || y < 0.0 || x >= size.width() as f32 || y >= size.height() as f32 {
        return None;
    }
    Some(Point::new(x as u32, y as u32))
}

fn selection_rect_from_drag(start: Point, end: Point) -> Rect {
    let left = i64::from(start.x.min(end.x));
    let top = i64::from(start.y.min(end.y));
    let right = i64::from(start.x.max(end.x)) + 1;
    let bottom = i64::from(start.y.max(end.y)) + 1;
    Rect::new(left, top, right, bottom).expect("drag selection has positive width and height")
}

fn selection_input(ui: &mut egui::Ui, label: &'static str, value: &mut i64) {
    ui.horizontal(|ui| {
        let label = ui.label(label);
        let _response = ui
            .add(egui::DragValue::new(value).speed(1.0))
            .labelled_by(label.id);
    });
}
