use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use paint_app::{
    AppSession, CanvasAnchor, Command, Effect, MagicWandSampling, ResourceLimits,
    SelectionCombineMode,
};
use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_document::{BlendMode, Document, LayerId};
use paint_raster::{
    BrushOptions, GradientOptions, GradientRepeat, GradientType, LineCap, LineDash, LineStyle,
    MAX_ADJUSTMENT_VALUE, MAX_GAUSSIAN_BLUR_RADIUS, MAX_HUE_DELTA, MIN_ADJUSTMENT_VALUE,
    MIN_HUE_DELTA, ShapeStyle, TextFontFamily, TextStyle,
};

use crate::checkerboard::paint_checkerboard;
use crate::colors::{
    ColorSlot, PRESET_COLORS, color_slots_widget, color_swatch, color_wheel, compact_color_slider,
    format_hex_color, hsv_from_color, hsv_gradient_slider, mix_bgra,
};
use crate::icons::{
    Icon, IconTextures, icon_button, icon_button_at, tool_button, visibility_button_at,
};
use crate::layers::{LayerThumbnail, layer_thumbnail_texture, surface_thumbnail_texture};
#[cfg(target_os = "macos")]
use crate::menu::{NativeCommand, NativeMenu, NativeMenuState};
use crate::selection::{paint_selection, paint_selection_outline};

const DEFAULT_WIDTH: u32 = 800;
const DEFAULT_HEIGHT: u32 = 600;
const MAX_PREVIEW_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PREVIEW_DECODED_BYTES: usize = 64 * 1024 * 1024;
const PREVIEW_WIDTH: f32 = 180.0;
const PREVIEW_HEIGHT: f32 = 140.0;
const MAX_BROWSER_ENTRIES: usize = 200;

/// Paint.NET LayersStrip metrics: 50 px thumbnail + 3 × 4 px padding = 62 px rows.
const LAYER_ITEM_HEIGHT: f32 = 62.0;
const LAYER_THUMBNAIL_SIZE: f32 = 50.0;
const LAYER_ITEM_PADDING: f32 = 4.0;
const LAYER_VISIBILITY_HIT_WIDTH: f32 = 22.0;

/// Screen geometry of one layer row, captured for usability and geometry tests.
#[derive(Clone, Debug)]
pub struct LayerRowGeometry {
    pub index: usize,
    pub rect: egui::Rect,
    pub thumbnail: egui::Rect,
    pub visibility: egui::Rect,
}

/// Geometry of the layer list viewport, its optional scrollbar, and the drag indicator.
#[derive(Clone, Debug)]
pub struct LayerListGeometry {
    pub item_height: f32,
    pub thumbnail_size: f32,
    pub padding: f32,
    pub outer: egui::Rect,
    pub viewport: egui::Rect,
    pub scrollbar: Option<egui::Rect>,
    pub rows: Vec<LayerRowGeometry>,
    pub insertion_indicator: Option<egui::Rect>,
}

#[derive(Clone, Copy, Debug)]
struct LayerDragState {
    layer_id: LayerId,
    from_index: usize,
    pointer_y: f32,
}

struct DocumentTabState {
    session: AppSession,
    title: String,
    texture: Option<egui::TextureHandle>,
    thumbnail: Option<egui::TextureHandle>,
    zoom: f32,
    fit_canvas_requested: bool,
    zoom_to_selection_requested: bool,
    history_entries: Vec<String>,
    redo_entries: Vec<String>,
    layer_thumbnails: HashMap<LayerId, LayerThumbnail>,
    selection_input: SelectionInput,
    drag_start: Option<Point>,
    drag_current: Option<Point>,
    paint_points: Vec<Point>,
    floating_selection: Option<FloatingSelectionState>,
    next_layer_number: usize,
}

struct FloatingSelectionState {
    floating: paint_app::FloatingSelection,
    texture: egui::TextureHandle,
    offset_x: i64,
    offset_y: i64,
    base_x: i64,
    base_y: i64,
}

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
    previous_tool_before_picker: Option<Tool>,
    shape_mode: ShapeMode,
    shape_style: ShapeStyle,
    shape_border_width: u32,
    brush_size: u32,
    brush_hardness: u8,
    brush_spacing: u8,
    gradient_type: GradientType,
    gradient_repeat: GradientRepeat,
    line_start_cap: LineCap,
    line_end_cap: LineCap,
    line_dash: LineDash,
    picker_click_behavior: PickerClickBehavior,
    picker_sample_source: PickerSampleSource,
    picker_sample_size: PickerSampleSize,
    move_selection_dx: i32,
    move_selection_dy: i32,
    clone_hardness: u8,
    clone_source: Option<Point>,
    clone_offset: Option<(i64, i64)>,
    clone_alt_down: bool,
    text_content: String,
    text_font_family: TextFontFamily,
    text_size: u32,
    text_bold: bool,
    text_italic: bool,
    text_antialias: bool,
    text_origin: Option<Point>,
    text_applied_key: Option<TextPreviewKey>,
    recolor_tolerance: u8,
    fill_tolerance: u8,
    fill_global: bool,
    selection_combine_mode: SelectionCombineMode,
    magic_wand_sampling: MagicWandSampling,
    paint_points: Vec<Point>,
    icons: IconTextures,
    #[cfg(target_os = "macos")]
    native_menu: Option<NativeMenu>,
    tools_window_rect: Option<egui::Rect>,
    tools_close_requested: bool,
    layers_close_requested: bool,
    history_close_requested: bool,
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
    zoom_to_selection_requested: bool,
    show_grid: bool,
    unit: CanvasUnit,
    layer_thumbnails: HashMap<LayerId, LayerThumbnail>,
    floating_selection: Option<FloatingSelectionState>,
    floating_generation: u64,
    layer_list_geometry: Option<LayerListGeometry>,
    layer_drag: Option<LayerDragState>,
    layer_drop_pointer_y: Option<f32>,
    layer_properties_open: bool,
    layer_properties_layer: Option<LayerId>,
    layer_properties_index: usize,
    layer_properties_name: String,
    layer_properties_name_error: Option<String>,
    layer_properties_opacity: u8,
    layer_properties_visible: bool,
    layer_properties_blend_mode: BlendMode,
    gaussian_blur_open: bool,
    gaussian_blur_radius: u32,
    adjustment_dialog: Option<AdjustmentDialog>,
    adjustment_previewed: Option<Effect>,
    document_title: String,
    document_thumbnail: Option<egui::TextureHandle>,
    tabs: Vec<Option<DocumentTabState>>,
    active_tab: usize,
    next_layer_number: usize,
    status: String,
    resize_dialog: Option<ResizeImageDialogState>,
    canvas_size_dialog: Option<CanvasSizeDialogState>,
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
            previous_tool_before_picker: None,
            shape_mode: ShapeMode::Rectangle,
            shape_style: ShapeStyle::Outline,
            shape_border_width: 2,
            brush_size: 45,
            brush_hardness: 75,
            brush_spacing: 15,
            gradient_type: GradientType::Linear,
            gradient_repeat: GradientRepeat::None,
            line_start_cap: LineCap::Flat,
            line_end_cap: LineCap::Flat,
            line_dash: LineDash::Solid,
            picker_click_behavior: PickerClickBehavior::NoToolSwitch,
            picker_sample_source: PickerSampleSource::Image,
            picker_sample_size: PickerSampleSize::Point,
            move_selection_dx: 0,
            move_selection_dy: 0,
            clone_hardness: 80,
            clone_source: None,
            clone_offset: None,
            clone_alt_down: false,
            text_content: String::new(),
            text_font_family: TextFontFamily::Sans,
            text_size: 48,
            text_bold: false,
            text_italic: false,
            text_antialias: true,
            text_origin: None,
            text_applied_key: None,
            recolor_tolerance: 50,
            fill_tolerance: 50,
            fill_global: false,
            selection_combine_mode: SelectionCombineMode::Replace,
            magic_wand_sampling: MagicWandSampling::ActiveLayer,
            paint_points: Vec::new(),
            icons,
            #[cfg(target_os = "macos")]
            native_menu: NativeMenu::install(creation_context.egui_ctx.clone()),
            tools_window_rect: None,
            tools_close_requested: false,
            layers_close_requested: false,
            history_close_requested: false,
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
            zoom_to_selection_requested: false,
            show_grid: false,
            unit: CanvasUnit::Pixels,
            layer_thumbnails: HashMap::new(),
            floating_selection: None,
            floating_generation: 0,
            layer_list_geometry: None,
            layer_drag: None,
            layer_drop_pointer_y: None,
            layer_properties_open: false,
            layer_properties_layer: None,
            layer_properties_index: 0,
            layer_properties_name: String::new(),
            layer_properties_name_error: None,
            layer_properties_opacity: u8::MAX,
            layer_properties_visible: true,
            layer_properties_blend_mode: BlendMode::Normal,
            gaussian_blur_open: false,
            gaussian_blur_radius: 2,
            adjustment_dialog: None,
            adjustment_previewed: None,
            document_title: "New Image".to_owned(),
            document_thumbnail: None,
            tabs: vec![None],
            active_tab: 0,
            next_layer_number: 1,
            status: "New 800×600 opaque-white document".to_owned(),
            resize_dialog: None,
            canvas_size_dialog: None,
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
    pub const fn brush_size(&self) -> u32 {
        self.brush_size
    }

    #[doc(hidden)]
    pub const fn brush_hardness(&self) -> u8 {
        self.brush_hardness
    }

    #[doc(hidden)]
    pub const fn brush_spacing(&self) -> u8 {
        self.brush_spacing
    }

    #[doc(hidden)]
    pub const fn shape_style(&self) -> ShapeStyle {
        self.shape_style
    }

    #[doc(hidden)]
    pub const fn shape_border_width(&self) -> u32 {
        self.shape_border_width
    }

    #[doc(hidden)]
    pub const fn gradient_type(&self) -> GradientType {
        self.gradient_type
    }

    #[doc(hidden)]
    pub const fn gradient_repeat(&self) -> GradientRepeat {
        self.gradient_repeat
    }

    #[doc(hidden)]
    pub fn set_brush_options(&mut self, size: u32, hardness: u8, spacing: u8) {
        self.brush_size = size.max(1);
        self.brush_hardness = hardness.min(100);
        self.brush_spacing = spacing.clamp(1, 100);
    }

    #[doc(hidden)]
    pub fn set_shape_options(&mut self, style: ShapeStyle, border_width: u32) {
        self.shape_style = style;
        self.shape_border_width = border_width.max(1);
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
    pub const fn tools_window_bounds(&self) -> Option<egui::Rect> {
        self.tools_window_rect
    }

    #[doc(hidden)]
    pub const fn layers_window_bounds(&self) -> Option<egui::Rect> {
        self.layers_window_rect
    }

    #[doc(hidden)]
    pub const fn layer_list_geometry(&self) -> Option<&LayerListGeometry> {
        self.layer_list_geometry.as_ref()
    }

    #[doc(hidden)]
    pub const fn layer_properties_open(&self) -> bool {
        self.layer_properties_open
    }

    #[doc(hidden)]
    pub fn layer_properties_target(&self) -> Option<usize> {
        if !self.layer_properties_open {
            return None;
        }
        self.layer_properties_layer
            .and_then(|layer_id| self.session.document().layer_index(layer_id))
    }

    #[doc(hidden)]
    pub const fn history_window_bounds(&self) -> Option<egui::Rect> {
        self.history_window_rect
    }

    #[doc(hidden)]
    pub const fn document_tab_count(&self) -> usize {
        self.tabs.len()
    }

    #[doc(hidden)]
    pub const fn active_document_index(&self) -> usize {
        self.active_tab
    }

    #[doc(hidden)]
    pub const fn zoom(&self) -> f32 {
        self.zoom
    }

    #[doc(hidden)]
    pub const fn grid_enabled(&self) -> bool {
        self.show_grid
    }

    #[doc(hidden)]
    pub fn document_titles(&self) -> Vec<String> {
        (0..self.tabs.len())
            .map(|index| {
                if index == self.active_tab {
                    self.document_title.clone()
                } else {
                    self.tabs[index]
                        .as_ref()
                        .map_or_else(|| "Untitled".to_owned(), |tab| tab.title.clone())
                }
            })
            .collect()
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

    fn take_active_tab(&mut self) -> DocumentTabState {
        DocumentTabState {
            session: std::mem::replace(&mut self.session, default_session()),
            title: std::mem::take(&mut self.document_title),
            texture: self.texture.take(),
            thumbnail: self.document_thumbnail.take(),
            zoom: self.zoom,
            fit_canvas_requested: self.fit_canvas_requested,
            zoom_to_selection_requested: self.zoom_to_selection_requested,
            history_entries: std::mem::take(&mut self.history_entries),
            redo_entries: std::mem::take(&mut self.redo_entries),
            layer_thumbnails: std::mem::take(&mut self.layer_thumbnails),
            selection_input: self.selection_input,
            drag_start: self.drag_start.take(),
            drag_current: self.drag_current.take(),
            paint_points: std::mem::take(&mut self.paint_points),
            floating_selection: self.floating_selection.take(),
            next_layer_number: self.next_layer_number,
        }
    }

    fn load_active_tab(&mut self, state: DocumentTabState) {
        self.session = state.session;
        self.document_title = state.title;
        self.texture = state.texture;
        self.texture_dirty = true;
        self.document_thumbnail = state.thumbnail;
        self.zoom = state.zoom;
        self.fit_canvas_requested = state.fit_canvas_requested;
        self.zoom_to_selection_requested = state.zoom_to_selection_requested;
        self.history_entries = state.history_entries;
        self.redo_entries = state.redo_entries;
        self.layer_thumbnails = state.layer_thumbnails;
        self.selection_input = state.selection_input;
        self.drag_start = state.drag_start;
        self.drag_current = state.drag_current;
        self.paint_points = state.paint_points;
        self.floating_selection = state.floating_selection;
        self.next_layer_number = state.next_layer_number;
        self.adjustment_dialog = None;
        self.adjustment_previewed = None;
    }

    fn store_active_tab(&mut self) {
        self.close_adjustment_preview();
        self.commit_text_preview();
        let state = self.take_active_tab();
        self.tabs[self.active_tab] = Some(state);
    }

    fn switch_to_tab(&mut self, index: usize) {
        if index == self.active_tab || index >= self.tabs.len() {
            return;
        }
        self.store_active_tab();
        let Some(state) = self.tabs[index].take() else {
            return;
        };
        self.load_active_tab(state);
        self.active_tab = index;
        self.reset_transient_tool_state();
        self.reset_layer_interaction_state();
        self.layer_properties_open = false;
        self.gaussian_blur_open = false;
        self.resize_dialog = None;
        self.canvas_size_dialog = None;
        self.status = format!("Switched to {}", self.document_title);
    }

    fn reset_layer_interaction_state(&mut self) {
        self.layer_list_geometry = None;
        self.layer_drag = None;
        self.layer_drop_pointer_y = None;
        self.layer_properties_open = false;
        self.layer_properties_layer = None;
        self.layer_properties_name.clear();
        self.layer_properties_name_error = None;
    }

    fn request_close_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }
        let dirty = if index == self.active_tab {
            self.session.dirty()
        } else {
            self.tabs[index]
                .as_ref()
                .is_some_and(|tab| tab.session.dirty())
        };
        if dirty {
            self.pending_discard = Some(DiscardAction::CloseTab(index));
            self.focus_discard_buttons = true;
        } else {
            self.close_document_tab(index);
        }
    }

    fn close_document_tab(&mut self, index: usize) {
        self.close_adjustment_preview();
        if index >= self.tabs.len() {
            return;
        }
        if index == self.active_tab {
            self.finalize_floating_selection();
            self.tabs.remove(index);
            if self.tabs.is_empty() {
                self.tabs.push(None);
                self.active_tab = 0;
                self.new_document();
                self.status = "Closed the last document; created a new one".to_owned();
                return;
            }
            let next = index.min(self.tabs.len() - 1);
            let Some(state) = self.tabs[next].take() else {
                return;
            };
            self.load_active_tab(state);
            self.active_tab = next;
            self.layer_properties_open = false;
            self.gaussian_blur_open = false;
            self.resize_dialog = None;
            self.canvas_size_dialog = None;
            self.status = format!("Closed a document; switched to {}", self.document_title);
        } else {
            self.tabs.remove(index);
            if index < self.active_tab {
                self.active_tab -= 1;
            }
            self.status = "Closed a document".to_owned();
        }
    }

    fn new_tab(&mut self) {
        self.store_active_tab();
        self.tabs.push(None);
        self.active_tab = self.tabs.len() - 1;
        self.new_document();
        self.document_title = format!("New Image {}", self.active_tab + 1);
    }

    fn has_dirty_documents(&self) -> bool {
        self.session.dirty() || self.tabs.iter().flatten().any(|tab| tab.session.dirty())
    }

    fn new_document(&mut self) {
        self.close_adjustment_preview();
        self.session = default_session();
        self.selection_input = SelectionInput::default();
        self.next_layer_number = 1;
        self.path_dialog = None;
        self.resize_dialog = None;
        self.canvas_size_dialog = None;
        self.pending_discard = None;
        self.focus_discard_buttons = false;
        self.gaussian_blur_open = false;
        self.texture_dirty = true;
        self.zoom = 1.0;
        self.fit_canvas_requested = false;
        self.drag_start = None;
        self.drag_current = None;
        self.layer_thumbnails.clear();
        self.history_entries = vec!["New Image".to_owned()];
        self.redo_entries.clear();
        self.reset_transient_tool_state();
        self.document_title = "New Image".to_owned();
        self.document_thumbnail = None;
        self.status = "New 800×600 opaque-white document".to_owned();
        self.reset_layer_interaction_state();
    }

    fn show_path_dialog(&mut self, kind: PathDialogKind) {
        self.finalize_floating_selection();
        self.path_dialog = Some(PathDialog::new(kind));
    }

    fn show_resize_dialog(&mut self) {
        self.finalize_floating_selection();
        self.resize_dialog = Some(ResizeImageDialogState::new(self.session.document().size()));
    }

    fn show_canvas_size_dialog(&mut self) {
        self.finalize_floating_selection();
        self.canvas_size_dialog = Some(CanvasSizeDialogState::new(self.session.document().size()));
    }

    fn request_discard(&mut self, action: DiscardAction) {
        self.path_dialog = None;
        self.pending_discard = Some(action);
        self.focus_discard_buttons = true;
    }

    fn request_new(&mut self) {
        self.finalize_floating_selection();
        self.new_tab();
    }

    fn request_quit(&mut self, context: &egui::Context) {
        if self.has_dirty_documents() {
            self.request_discard(DiscardAction::CloseViewport);
        } else {
            self.close_approved = true;
            context.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn request_open(&mut self, path: String) {
        self.path_dialog = None;
        self.open_png(path);
    }

    fn open_png(&mut self, path: String) {
        let path_buffer = PathBuf::from(&path);
        match AppSession::open_png(&path_buffer) {
            Ok(session) => {
                self.finalize_floating_selection();
                self.store_active_tab();
                self.tabs.push(None);
                self.active_tab = self.tabs.len() - 1;
                self.session = session;
                self.document_title = path_buffer
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("Untitled")
                    .to_owned();
                self.document_thumbnail = None;
                self.next_layer_number = 1;
                self.history_entries = vec!["Open PNG".to_owned()];
                self.redo_entries.clear();
                self.texture_dirty = true;
                self.zoom = 1.0;
                self.layer_thumbnails.clear();
                self.path_dialog = None;
                self.resize_dialog = None;
                self.canvas_size_dialog = None;
                self.reset_transient_tool_state();
                self.reset_layer_interaction_state();
                self.status = format!("Opened flattened PNG: {}", path_buffer.display());
            }
            Err(error) => {
                self.path_dialog = Some(PathDialog::with_path(PathDialogKind::Open, path));
                self.status = format!("Open failed: {error}");
            }
        }
    }

    fn execute(&mut self, command: Command, success: impl Into<String>) -> bool {
        if self.adjustment_dialog.take().is_some() {
            self.cancel_pending_adjustment();
        }
        if !matches!(
            command,
            Command::SelectRect { .. }
                | Command::ClearSelection
                | Command::TranslateSelection { .. }
        ) {
            self.finish_active_preview();
        }
        if !matches!(
            command,
            Command::SelectRect { .. } | Command::TranslateSelection { .. }
        ) {
            self.finalize_floating_selection();
        }
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

    fn preview_live_stroke(&mut self) {
        let color = (self.active_tool != Tool::Eraser).then_some(self.fill_color);
        let brush = self.paint_brush_options();
        match self
            .session
            .preview_stroke(&self.paint_points, brush, color)
        {
            Ok(_) => self.texture_dirty = true,
            Err(error) => self.status = format!("Stroke preview failed: {error}"),
        }
    }

    /// Builds the brush geometry for the active paint, erase, or recolor stroke.
    fn paint_brush_options(&self) -> BrushOptions {
        match self.active_tool {
            Tool::Pencil => BrushOptions::new(1, 100, 15),
            Tool::Line => BrushOptions::new(self.brush_size.max(1), 100, 15),
            _ => BrushOptions::new(
                self.brush_size.max(1),
                self.brush_hardness,
                self.brush_spacing,
            ),
        }
    }

    fn finish_live_stroke(&mut self) {
        match self.session.finish_stroke_preview() {
            Ok(true) => {
                let (label, message) = match self.active_tool {
                    Tool::Pencil => ("Pencil", "Painted with the pencil"),
                    Tool::Brush => ("Paintbrush", "Painted with the paintbrush"),
                    _ => ("Eraser", "Erased pixels"),
                };
                self.history_entries.push(label.to_owned());
                self.redo_entries.clear();
                self.texture_dirty = true;
                self.status = message.to_owned();
            }
            Ok(false) => {}
            Err(error) => self.status = format!("Could not finish the stroke: {error}"),
        }
        self.paint_points.clear();
    }

    fn text_style(&self) -> TextStyle {
        TextStyle {
            family: self.text_font_family,
            size: self.text_size,
            bold: self.text_bold,
            italic: self.text_italic,
            antialias: self.text_antialias,
        }
    }

    /// Re-rasterizes the live text preview only when placement or style inputs change.
    fn sync_text_preview(&mut self) {
        if self.active_tool != Tool::Text {
            return;
        }
        let Some(origin) = self.text_origin else {
            self.text_applied_key = None;
            if self.session.has_preview() {
                self.session.cancel_preview();
                self.texture_dirty = true;
            }
            return;
        };
        if self.text_content.trim().is_empty() {
            self.text_applied_key = None;
            if self.session.has_preview() {
                self.session.cancel_preview();
                self.texture_dirty = true;
            }
            return;
        }
        let key = TextPreviewKey {
            origin,
            content: self.text_content.clone(),
            style: self.text_style(),
            color: self.fill_color,
            layer_id: self.session.document().active_layer_id(),
            selection: self.session.document().selection(),
        };
        if self.text_applied_key.as_ref() == Some(&key) {
            return;
        }
        if !self.session.has_preview() {
            self.session.begin_preview();
        }
        match self.session.preview_text(
            origin,
            &self.text_content,
            self.text_style(),
            self.fill_color,
        ) {
            Ok(_) => {
                self.text_applied_key = Some(key);
                self.texture_dirty = true;
            }
            Err(error) => {
                self.text_applied_key = Some(key);
                self.status = format!("Text preview failed: {error}");
            }
        }
    }

    /// Commits the live text preview as one undoable history entry.
    fn commit_text_preview(&mut self) -> bool {
        if self.active_tool != Tool::Text {
            return false;
        }
        let had_preview = self.session.has_preview();
        self.text_applied_key = None;
        if !had_preview {
            self.text_origin = None;
            return false;
        }
        match self.session.finish_preview() {
            Ok(true) => {
                self.history_entries.push("Text".to_owned());
                self.redo_entries.clear();
                self.text_origin = None;
                self.texture_dirty = true;
                self.status = "Placed text on the active layer".to_owned();
                true
            }
            Ok(false) => {
                self.text_origin = None;
                self.texture_dirty = true;
                false
            }
            Err(error) => {
                self.status = format!("Could not place the text: {error}");
                false
            }
        }
    }

    fn cancel_text_preview(&mut self) {
        self.session.cancel_preview();
        self.text_origin = None;
        self.text_applied_key = None;
        self.paint_points.clear();
        self.texture_dirty = true;
        self.status = "Canceled the text preview".to_owned();
    }

    fn text_tool_options(&mut self, ui: &mut egui::Ui) {
        ui.label("Text");
        egui::ComboBox::from_label("Font family")
            .selected_text(self.text_font_family.label())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.text_font_family, TextFontFamily::Sans, "Sans");
                ui.selectable_value(&mut self.text_font_family, TextFontFamily::Mono, "Mono");
            });
        let size_label = ui.label("Font size");
        ui.add(
            egui::DragValue::new(&mut self.text_size)
                .range(1..=paint_raster::MAX_TEXT_SIZE)
                .suffix(" px"),
        )
        .labelled_by(size_label.id);
        ui.toggle_value(&mut self.text_bold, "Bold");
        ui.toggle_value(&mut self.text_italic, "Italic");
        ui.checkbox(&mut self.text_antialias, "Antialiasing");
        let color = self.fill_color;
        ui.colored_label(
            egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a),
            "Primary color",
        );
        let content_label = ui.label("Text content");
        ui.add(
            egui::TextEdit::multiline(&mut self.text_content)
                .desired_rows(2)
                .desired_width(220.0)
                .hint_text("Type text"),
        )
        .labelled_by(content_label.id);
        if let Some(origin) = self.text_origin {
            ui.label(format!("Origin: ({}, {})", origin.x, origin.y));
        } else {
            ui.label("Click the canvas to set the text origin");
        }
        self.sync_text_preview();
        if ui
            .add_enabled(self.session.has_preview(), egui::Button::new("Place text"))
            .clicked()
        {
            self.commit_text_preview();
        }
        if ui.button("Cancel text").clicked() {
            self.cancel_text_preview();
        }
    }

    fn clone_tool_options(&mut self, ui: &mut egui::Ui) {
        ui.label("Clone Stamp");
        ui.label("Width");
        ui.add(
            egui::DragValue::new(&mut self.brush_size)
                .range(1..=64)
                .suffix(" px"),
        );
        let hardness_label = ui.label("Hardness");
        ui.add(
            egui::DragValue::new(&mut self.clone_hardness)
                .range(0..=100)
                .suffix("%"),
        )
        .labelled_by(hardness_label.id);
        if let Some(source) = self.clone_source {
            ui.label(format!("Source: ({}, {})", source.x, source.y));
            if let Some((offset_x, offset_y)) = self.clone_offset {
                ui.label(format!("Offset: ({offset_x}, {offset_y})"));
            }
        } else {
            ui.label("Alt-click the canvas to set the clone source");
        }
        ui.add_enabled_ui(self.clone_source.is_some(), |ui| {
            if ui.button("Clear clone source").clicked() {
                self.clone_source = None;
                self.clone_offset = None;
            }
        });
        ui.label("Samples the active layer before the stroke");
        ui.label("Drag from any point to clone the source offset");
    }

    fn preview_live_clone(&mut self) {
        let Some(source) = self.clone_source else {
            return;
        };
        let size = self.brush_size.max(1);
        match self.session.preview_clone_stamp(
            &self.paint_points,
            size,
            source,
            self.clone_hardness,
        ) {
            Ok(_) => self.texture_dirty = true,
            Err(error) => self.status = format!("Clone preview failed: {error}"),
        }
    }

    fn finish_live_clone(&mut self) {
        match self.session.finish_preview() {
            Ok(true) => {
                self.history_entries.push("Clone Stamp".to_owned());
                self.redo_entries.clear();
                self.texture_dirty = true;
                self.status = "Cloned pixels from the source".to_owned();
            }
            Ok(false) => {}
            Err(error) => self.status = format!("Could not finish the clone stroke: {error}"),
        }
        self.paint_points.clear();
    }

    /// Finishes whichever live preview belongs to the active tool before a new command.
    fn finish_active_preview(&mut self) {
        if !self.session.has_preview() {
            return;
        }
        match self.active_tool {
            Tool::Text => {
                self.commit_text_preview();
            }
            Tool::CloneStamp => self.finish_live_clone(),
            Tool::Pencil | Tool::Brush | Tool::Eraser => self.finish_live_stroke(),
            _ => {}
        }
    }

    fn reset_transient_tool_state(&mut self) {
        self.text_origin = None;
        self.text_applied_key = None;
        self.clone_source = None;
        self.clone_offset = None;
        self.clone_alt_down = false;
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
                self.execute(
                    Command::SelectRect {
                        rect,
                        mode: self.selection_combine_mode,
                    },
                    "Rectangle selection updated",
                );
            }
            Err(error) => {
                self.status = format!("Selection failed: {error}");
            }
        }
    }

    fn lift_floating_selection(&mut self, context: &egui::Context) -> bool {
        if self.floating_selection.is_some() {
            return true;
        }
        match self.session.lift_selection() {
            Ok(Some(floating)) => {
                self.floating_generation += 1;
                let texture = surface_thumbnail_texture(
                    context,
                    format!("paint-r-floating-{}", self.floating_generation),
                    &floating.surface,
                    floating.surface.width() as f32,
                    floating.surface.height() as f32,
                );
                self.floating_selection = Some(FloatingSelectionState {
                    floating,
                    texture,
                    offset_x: 0,
                    offset_y: 0,
                    base_x: 0,
                    base_y: 0,
                });
                self.history_entries
                    .push("Lifted selected pixels".to_owned());
                self.redo_entries.clear();
                self.texture_dirty = true;
                self.status = "Lifted selected pixels; drag to place them".to_owned();
                true
            }
            Ok(None) => false,
            Err(error) => {
                self.status = format!("Could not lift the selection: {error}");
                false
            }
        }
    }

    fn finalize_floating_selection(&mut self) {
        let Some(floating) = self.floating_selection.take() else {
            return;
        };
        let dest_x = floating.floating.bounds.left() + floating.offset_x;
        let dest_y = floating.floating.bounds.top() + floating.offset_y;
        match self
            .session
            .stamp_floating(&floating.floating.surface, dest_x, dest_y)
        {
            Ok(changed) => {
                if changed {
                    self.history_entries
                        .push("Moved selected pixels".to_owned());
                    self.redo_entries.clear();
                }
                self.texture_dirty = true;
                self.status = "Placed the moved pixels".to_owned();
            }
            Err(error) => self.status = format!("Could not place the moved pixels: {error}"),
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
        self.document_thumbnail = Some(surface_thumbnail_texture(
            context,
            "paint-r-document-tab",
            &surface,
            44.0,
            32.0,
        ));
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

    fn shows_in_window_menu(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.native_menu.is_none() || cfg!(debug_assertions)
        }
        #[cfg(not(target_os = "macos"))]
        {
            true
        }
    }

    #[cfg(target_os = "macos")]
    fn poll_native_menu(&mut self, context: &egui::Context) {
        for command in NativeMenu::poll() {
            match command {
                NativeCommand::New => self.request_new(),
                NativeCommand::Open => self.show_path_dialog(PathDialogKind::Open),
                NativeCommand::Export => self.show_path_dialog(PathDialogKind::Export),
                NativeCommand::Quit => self.request_quit(context),
                NativeCommand::Undo => {
                    self.execute(Command::Undo, "Undid the last edit");
                }
                NativeCommand::Redo => {
                    self.execute(Command::Redo, "Redid the next edit");
                }
                NativeCommand::Deselect => {
                    self.execute(Command::ClearSelection, "Selection cleared");
                }
                NativeCommand::ZoomIn => self.zoom = (self.zoom + 0.25).min(8.0),
                NativeCommand::ZoomOut => self.zoom = (self.zoom - 0.25).max(0.1),
                NativeCommand::ActualSize => self.zoom = 1.0,
                NativeCommand::FitCanvas => self.fit_canvas_requested = true,
                NativeCommand::FlipHorizontal => {
                    self.execute(
                        Command::FlipLayerHorizontal,
                        "Flipped the active layer horizontally",
                    );
                }
                NativeCommand::FlipVertical => {
                    self.execute(
                        Command::FlipLayerVertical,
                        "Flipped the active layer vertically",
                    );
                }
                NativeCommand::Invert => {
                    self.execute(Command::Invert, "Inverted colors on the active layer");
                }
                NativeCommand::Grayscale => {
                    self.execute(
                        Command::ApplyEffect(Effect::Grayscale),
                        "Applied Grayscale to the selection",
                    );
                }
                NativeCommand::Sepia => {
                    self.execute(
                        Command::ApplyEffect(Effect::Sepia),
                        "Applied Sepia to the selection",
                    );
                }
                NativeCommand::GaussianBlur => {
                    self.open_gaussian_blur_dialog();
                }
                NativeCommand::BrightnessContrast => {
                    self.open_adjustment_dialog(AdjustmentDialog::BrightnessContrast {
                        brightness: 0,
                        contrast: 0,
                    });
                }
                NativeCommand::HueSaturation => {
                    self.open_adjustment_dialog(AdjustmentDialog::HueSaturation {
                        hue: 0,
                        saturation: 0,
                        lightness: 0,
                    });
                }
                NativeCommand::AutoLevel => {
                    self.open_adjustment_dialog(AdjustmentDialog::AutoLevel);
                }
                NativeCommand::ToggleTools => self.tools_visible = !self.tools_visible,
                NativeCommand::ToggleColors => self.colors_visible = !self.colors_visible,
                NativeCommand::ToggleLayers => self.layers_visible = !self.layers_visible,
                NativeCommand::ToggleHistory => self.history_visible = !self.history_visible,
                NativeCommand::ToggleFloatPanels => {
                    self.panels_floating = !self.panels_floating;
                    if self.panels_floating {
                        self.tools_visible = true;
                        self.tools_floating = true;
                        self.colors_visible = true;
                        self.layers_visible = true;
                    } else {
                        self.tools_floating = false;
                    }
                }
                NativeCommand::About => {
                    self.status =
                        "Paint.R development preview · MIT first-party license".to_owned();
                }
            }
        }
        let state = NativeMenuState {
            can_undo: self.session.can_undo(),
            can_redo: self.session.can_redo(),
            has_selection: self.session.document().selection().is_some(),
            tools_visible: self.tools_visible,
            colors_visible: self.colors_visible,
            layers_visible: self.layers_visible,
            history_visible: self.history_visible,
            float_panels: self.panels_floating,
        };
        if let Some(menu) = self.native_menu.as_ref() {
            menu.sync(state);
        }
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
                    if ui.button("Zoom to Window").clicked() {
                        self.fit_canvas_requested = true;
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            self.session.document().selection().is_some(),
                            egui::Button::new("Zoom to Selection"),
                        )
                        .clicked()
                    {
                        self.zoom_to_selection_requested = true;
                        ui.close();
                    }
                    if ui.button("Actual Size").clicked() {
                        self.zoom = 1.0;
                        ui.close();
                    }
                    ui.separator();
                    ui.checkbox(&mut self.show_grid, "Grid");
                    ui.add_enabled(false, egui::Button::new("Rulers"));
                    ui.separator();
                    ui.radio_value(&mut self.unit, CanvasUnit::Pixels, "Pixels");
                    ui.radio_value(&mut self.unit, CanvasUnit::Inches, "Inches");
                    ui.radio_value(&mut self.unit, CanvasUnit::Centimeters, "Centimeters");
                });
                ui.menu_button("Image", |ui| {
                    let has_selection = self.session.document().selection().is_some();
                    let image_layers = self.session.document().layers().len();
                    let crop =
                        ui.add_enabled(has_selection, egui::Button::new("Crop to Selection"));
                    focus_first_menu_item_on_keyboard_open(ui, &crop);
                    if crop.clicked() {
                        self.execute(
                            Command::CropToSelection,
                            "Cropped the image to the selection",
                        );
                        ui.close();
                    }
                    if ui.button("Resize Image…").clicked() {
                        self.show_resize_dialog();
                        ui.close();
                    }
                    if ui.button("Canvas Size…").clicked() {
                        self.show_canvas_size_dialog();
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Flip Horizontal").clicked() {
                        self.execute(
                            Command::FlipImageHorizontal,
                            "Flipped the image horizontally",
                        );
                        ui.close();
                    }
                    if ui.button("Flip Vertical").clicked() {
                        self.execute(Command::FlipImageVertical, "Flipped the image vertically");
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Rotate 90° Clockwise").clicked() {
                        self.execute(
                            Command::Rotate90Clockwise,
                            "Rotated the image 90° clockwise",
                        );
                        ui.close();
                    }
                    if ui.button("Rotate 90° Counter-Clockwise").clicked() {
                        self.execute(
                            Command::Rotate90CounterClockwise,
                            "Rotated the image 90° counter-clockwise",
                        );
                        ui.close();
                    }
                    if ui.button("Rotate 180°").clicked() {
                        self.execute(Command::Rotate180, "Rotated the image 180°");
                        ui.close();
                    }
                    ui.separator();
                    ui.add_enabled(false, egui::Button::new("Color Profile…"));
                    ui.separator();
                    if ui
                        .add_enabled(image_layers > 1, egui::Button::new("Flatten Image"))
                        .clicked()
                    {
                        self.execute(Command::FlattenImage, "Flattened the image");
                        ui.close();
                    }
                });
                let (layer_count, active_layer_index, active_layer_visible) = {
                    let document = self.session.document();
                    let index = document
                        .layers()
                        .iter()
                        .position(|layer| layer.id() == document.active_layer_id())
                        .unwrap_or(0);
                    let layer = document.layers().get(index);
                    (
                        document.layers().len(),
                        index,
                        layer.is_none_or(|layer| layer.is_visible()),
                    )
                };
                ui.menu_button("Layers", |ui| {
                    let add = ui.button("Add New Layer");
                    focus_first_menu_item_on_keyboard_open(ui, &add);
                    if add.clicked() {
                        self.add_layer();
                        ui.close();
                    }
                    if ui
                        .add_enabled(layer_count > 1, egui::Button::new("Delete Layer"))
                        .clicked()
                    {
                        self.execute(
                            Command::RemoveLayer {
                                index: active_layer_index,
                            },
                            "Deleted the active layer",
                        );
                        ui.close();
                    }
                    if ui.button("Duplicate Layer").clicked() {
                        self.execute(
                            Command::DuplicateLayer {
                                index: active_layer_index,
                            },
                            "Duplicated the active layer",
                        );
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            active_layer_index > 0,
                            egui::Button::new("Merge Layer Down"),
                        )
                        .clicked()
                    {
                        self.execute(
                            Command::MergeLayerDown {
                                index: active_layer_index,
                            },
                            "Merged the active layer down",
                        );
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            layer_count > 1,
                            egui::Button::new("Toggle Layer Visibility"),
                        )
                        .clicked()
                    {
                        self.execute(
                            Command::SetLayerVisibility {
                                index: active_layer_index,
                                visible: !active_layer_visible,
                            },
                            "Toggled layer visibility",
                        );
                        ui.close();
                    }
                    ui.separator();
                    ui.add_enabled(false, egui::Button::new("Import From File…"));
                    ui.separator();
                    if ui.button("Flip Active Layer Horizontally").clicked() {
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
                    ui.add_enabled(false, egui::Button::new("Rotate 180°"));
                    ui.add_enabled(false, egui::Button::new("Rotate / Zoom…"));
                    ui.separator();
                    for (label, target) in [
                        ("Go to Top Layer", layer_count.saturating_sub(1)),
                        (
                            "Go to Layer Above",
                            (active_layer_index + 1).min(layer_count - 1),
                        ),
                        ("Go to Layer Below", active_layer_index.saturating_sub(1)),
                        ("Go to Bottom Layer", 0),
                    ] {
                        if ui.button(label).clicked() {
                            self.execute(
                                Command::SetActiveLayer { index: target },
                                "Selected a layer",
                            );
                            ui.close();
                        }
                    }
                    ui.separator();
                    for (label, offset) in [
                        (
                            "Move Layer to Top",
                            layer_count as i32 - 1 - active_layer_index as i32,
                        ),
                        ("Move Layer Up", 1),
                        ("Move Layer Down", -1),
                        ("Move Layer to Bottom", -(active_layer_index as i32)),
                    ] {
                        if ui.button(label).clicked() {
                            self.execute(
                                Command::MoveLayer {
                                    index: active_layer_index,
                                    offset,
                                },
                                "Moved the active layer",
                            );
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Layer Properties…").clicked() {
                        self.open_layer_properties(active_layer_index);
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
                    ui.separator();
                    if ui.button("Brightness / Contrast…").clicked() {
                        self.open_adjustment_dialog(AdjustmentDialog::BrightnessContrast {
                            brightness: 0,
                            contrast: 0,
                        });
                        ui.close();
                    }
                    if ui.button("Hue / Saturation…").clicked() {
                        self.open_adjustment_dialog(AdjustmentDialog::HueSaturation {
                            hue: 0,
                            saturation: 0,
                            lightness: 0,
                        });
                        ui.close();
                    }
                    if ui.button("Auto-Level…").clicked() {
                        self.open_adjustment_dialog(AdjustmentDialog::AutoLevel);
                        ui.close();
                    }
                });
                ui.menu_button("Effects", |ui| {
                    let invert = ui.button("Invert Colors");
                    focus_first_menu_item_on_keyboard_open(ui, &invert);
                    if invert.clicked() {
                        self.execute(Command::Invert, "Inverted colors on the active layer");
                        ui.close();
                    }
                    ui.separator();
                    ui.menu_button("Artistic", |ui| {
                        for label in ["Ink Sketch", "Oil Painting", "Pencil Sketch"] {
                            disabled_effect_item(ui, label);
                        }
                    });
                    ui.menu_button("Blurs", |ui| {
                        if ui.button("Gaussian Blur…").clicked() {
                            self.open_gaussian_blur_dialog();
                            ui.close();
                        }
                        for label in ["Motion Blur", "Radial Blur", "Unfocus", "Zoom Blur"] {
                            disabled_effect_item(ui, label);
                        }
                    });
                    ui.menu_button("Distort", |ui| {
                        for label in ["Bulge", "Dents", "Pixelate", "Twist"] {
                            disabled_effect_item(ui, label);
                        }
                    });
                    ui.menu_button("Noise", |ui| {
                        for label in ["Add Noise", "Median", "Reduce Noise"] {
                            disabled_effect_item(ui, label);
                        }
                    });
                    ui.menu_button("Photo", |ui| {
                        if ui.button("Grayscale").clicked() {
                            self.execute(
                                Command::ApplyEffect(Effect::Grayscale),
                                "Applied Grayscale to the selection",
                            );
                            ui.close();
                        }
                        if ui.button("Sepia").clicked() {
                            self.execute(
                                Command::ApplyEffect(Effect::Sepia),
                                "Applied Sepia to the selection",
                            );
                            ui.close();
                        }
                        ui.separator();
                        for label in ["Glow", "Sharpen", "Soften Portrait", "Vignette"] {
                            disabled_effect_item(ui, label);
                        }
                    });
                    ui.menu_button("Render", |ui| {
                        for label in ["Clouds", "Julia Fractal", "Mandelbrot Fractal"] {
                            disabled_effect_item(ui, label);
                        }
                    });
                    ui.menu_button("Stylize", |ui| {
                        for label in ["Edge Detect", "Emboss", "Outline", "Relief"] {
                            disabled_effect_item(ui, label);
                        }
                    });
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

    fn compact_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            if icon_button(ui, &self.icons, Icon::NewDocument, "New document", true).clicked() {
                self.request_new();
            }
            if icon_button(ui, &self.icons, Icon::OpenFile, "Open PNG file", true).clicked() {
                self.show_path_dialog(PathDialogKind::Open);
            }
            if icon_button(ui, &self.icons, Icon::ExportFile, "Export PNG file", true).clicked() {
                self.show_path_dialog(PathDialogKind::Export);
            }
            ui.add_space(5.0);
            ui.separator();
            ui.add_space(5.0);
            if icon_button(ui, &self.icons, Icon::Undo, "Undo", self.session.can_undo()).clicked() {
                self.execute(Command::Undo, "Undid the last edit");
            }
            if icon_button(ui, &self.icons, Icon::Redo, "Redo", self.session.can_redo()).clicked() {
                self.execute(Command::Redo, "Redid the next edit");
            }
            if self.panels_floating {
                let mut pending_close = None;
                for index in 0..self.tabs.len() {
                    let active = index == self.active_tab;
                    let (title, thumbnail) = if active {
                        (self.document_title.clone(), self.document_thumbnail.clone())
                    } else {
                        self.tabs[index].as_ref().map_or_else(
                            || ("Untitled".to_owned(), None),
                            |tab| (tab.title.clone(), tab.thumbnail.clone()),
                        )
                    };
                    ui.add_space(4.0);
                    let mut tab_clicked = false;
                    egui::Frame::new()
                        .fill(if active {
                            egui::Color32::from_rgb(31, 78, 116)
                        } else {
                            egui::Color32::from_rgb(48, 48, 48)
                        })
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(84, 84, 84)))
                        .corner_radius(4)
                        .inner_margin(egui::Margin::symmetric(5, 0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(40.0, 28.0),
                                    egui::Sense::hover(),
                                );
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::Role::Image,
                                        true,
                                        if active {
                                            "Document tab preview".to_owned()
                                        } else {
                                            format!("Document tab preview {index}")
                                        },
                                    )
                                });
                                paint_checkerboard(ui.painter(), rect);
                                if let Some(thumbnail) = &thumbnail {
                                    egui::Image::from_texture(thumbnail).paint_at(ui, rect);
                                }
                                if ui
                                    .selectable_label(active, egui::RichText::new(&title).strong())
                                    .accessible_name(format!("Document tab: {title}"))
                                    .clicked()
                                {
                                    tab_clicked = true;
                                }
                                if ui
                                    .add(
                                        egui::Button::new(egui::RichText::new("×").size(14.0))
                                            .frame(false)
                                            .min_size(egui::vec2(16.0, 16.0)),
                                    )
                                    .on_hover_text("Close document")
                                    .accessible_name(format!("Close document {title}"))
                                    .clicked()
                                {
                                    pending_close = Some(index);
                                }
                            });
                        });
                    if tab_clicked && !active && pending_close.is_none() {
                        self.switch_to_tab(index);
                    }
                }
                if let Some(index) = pending_close {
                    self.request_close_tab(index);
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if tool_button(
                    ui,
                    &self.icons,
                    Icon::PanelHistory,
                    "History panel",
                    true,
                    self.history_visible,
                )
                .clicked()
                {
                    self.history_visible = !self.history_visible;
                }
                if tool_button(
                    ui,
                    &self.icons,
                    Icon::PanelLayers,
                    "Layers panel",
                    true,
                    self.layers_visible,
                )
                .clicked()
                {
                    self.layers_visible = !self.layers_visible;
                }
                if tool_button(
                    ui,
                    &self.icons,
                    Icon::PanelColors,
                    "Colors panel",
                    true,
                    self.colors_visible,
                )
                .clicked()
                {
                    self.colors_visible = !self.colors_visible;
                }
                if tool_button(
                    ui,
                    &self.icons,
                    Icon::PanelTools,
                    "Tools panel",
                    true,
                    self.tools_visible,
                )
                .clicked()
                {
                    self.tools_visible = !self.tools_visible;
                }
            });
        });
    }

    fn toolbar_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("paint-r-toolbar").show(ui, |ui| {
            ui.add_space(2.0);
            self.compact_toolbar(ui);
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
        if self.tools_floating {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.strong("Tools");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if window_close_button(ui, "Close Tools window") {
                        self.tools_close_requested = true;
                    }
                });
            });
            ui.separator();
        } else {
            ui.add_space(4.0);
            ui.strong("Tools");
            ui.separator();
        }
        macro_rules! tool_slot {
            ($ui:expr, $icon:expr, $label:expr, $enabled:expr) => {
                let _ = tool_button($ui, &self.icons, $icon, $label, $enabled, false);
            };
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
                    if self.active_tool != $tool {
                        if $tool == Tool::Picker {
                            self.previous_tool_before_picker = Some(self.active_tool);
                        }
                        if self.active_tool == Tool::Text {
                            self.commit_text_preview();
                        }
                        self.finalize_floating_selection();
                    }
                    self.active_tool = $tool;
                    self.picking_color = $tool == Tool::Picker;
                }
            };
        }
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let row = |ui: &mut egui::Ui, draw: &mut dyn FnMut(&mut egui::Ui)| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    draw(ui);
                });
            };
            row(ui, &mut |ui| {
                tool!(ui, Icon::SelectRectangle, "Rectangle Select", Tool::Select);
                tool!(ui, Icon::Move, "Move Selected Pixels", Tool::Move);
            });
            row(ui, &mut |ui| {
                tool!(ui, Icon::LassoSelect, "Lasso Select", Tool::Lasso);
                tool!(
                    ui,
                    Icon::MoveSelection,
                    "Move Selection",
                    Tool::MoveSelection
                );
            });
            row(ui, &mut |ui| {
                tool!(ui, Icon::Ellipse, "Ellipse Select", Tool::EllipseSelect);
                tool!(ui, Icon::Zoom, "Zoom", Tool::Zoom);
            });
            row(ui, &mut |ui| {
                tool!(ui, Icon::MagicWand, "Magic Wand", Tool::MagicWand);
                tool!(ui, Icon::Pan, "Pan", Tool::Pan);
            });
            row(ui, &mut |ui| {
                tool!(ui, Icon::Fill, "Paint Bucket", Tool::Fill);
                tool!(ui, Icon::Gradient, "Gradient", Tool::Gradient);
            });
            row(ui, &mut |ui| {
                tool!(ui, Icon::Brush, "Paintbrush", Tool::Brush);
                tool!(ui, Icon::Eraser, "Eraser", Tool::Eraser);
            });
            row(ui, &mut |ui| {
                tool!(ui, Icon::Pencil, "Pencil", Tool::Pencil);
                tool!(ui, Icon::EyeDropper, "Color Picker", Tool::Picker);
            });
            row(ui, &mut |ui| {
                tool!(ui, Icon::CloneStamp, "Clone Stamp", Tool::CloneStamp);
                tool!(ui, Icon::Recolor, "Recolor", Tool::Recolor);
            });
            row(ui, &mut |ui| {
                tool!(ui, Icon::Text, "Text", Tool::Text);
                tool!(ui, Icon::Line, "Line / Curve", Tool::Line);
            });
            row(ui, &mut |ui| {
                tool!(ui, Icon::Shapes, "Shapes", Tool::Shapes);
                tool_slot!(ui, Icon::Freeform, "Shapes (Freeform)", false);
            });
        });
    }

    fn selection_status(&self, ui: &mut egui::Ui) {
        let Some(selection) = self.session.document().selection_model() else {
            ui.label("No selection");
            return;
        };
        let bounds = selection.bounds();
        match selection.mask() {
            Some(mask) => {
                ui.label(format!("Mask {} px", mask.selected_count()));
                ui.label(format!(
                    "Bounds ({}, {})..({}, {})",
                    bounds.left(),
                    bounds.top(),
                    bounds.right(),
                    bounds.bottom()
                ));
                ui.label("Marching ants show the selection bounds; painting uses the real mask");
            }
            None => {
                ui.label(format!(
                    "({}, {})..({}, {})",
                    bounds.left(),
                    bounds.top(),
                    bounds.right(),
                    bounds.bottom()
                ));
            }
        }
    }

    fn selection_combine_options(&mut self, ui: &mut egui::Ui) {
        ui.label("Selection Mode");
        for mode in SelectionCombineMode::ALL {
            if ui
                .selectable_value(&mut self.selection_combine_mode, mode, mode.label())
                .clicked()
            {
                self.status = format!("Selection mode: {}", mode.label());
            }
        }
        ui.label("Modifiers: Shift adds, Alt subtracts");
    }

    /// Reads selection modifiers for one canvas gesture, falling back to the tool option mode.
    fn gesture_selection_mode(&self, ui: &egui::Ui) -> SelectionCombineMode {
        let modifiers = ui.input(|input| input.modifiers);
        if modifiers.alt {
            SelectionCombineMode::Subtract
        } else if modifiers.shift {
            SelectionCombineMode::Union
        } else {
            self.selection_combine_mode
        }
    }

    fn tool_options_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("paint-r-tool-options").show(ui, |ui| {
            ui.add_space(2.0);
            if matches!(
                self.active_tool,
                Tool::Move | Tool::MoveSelection | Tool::Picker | Tool::Line
            ) {
                self.grouped_tool_options(ui);
                ui.add_space(2.0);
                return;
            }
            ui.horizontal_wrapped(|ui| {
                ui.strong("Tool Options");
                ui.separator();
                match self.active_tool {
                    Tool::Select => {
                        ui.label("Rectangle Select");
                        self.selection_combine_options(ui);
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
                    Tool::Lasso => {
                        ui.label("Lasso Select");
                        self.selection_combine_options(ui);
                        ui.label("Drag to trace a freehand mask; releasing closes the path");
                        self.selection_status(ui);
                    }
                    Tool::EllipseSelect => {
                        ui.label("Ellipse Select");
                        self.selection_combine_options(ui);
                        ui.label("Drag to select an elliptical pixel mask");
                        self.selection_status(ui);
                    }
                    Tool::MagicWand => {
                        ui.label("Magic Wand");
                        self.selection_combine_options(ui);
                        let tolerance_label = ui.label("Tolerance");
                        ui.add(
                            egui::DragValue::new(&mut self.fill_tolerance)
                                .range(0..=100)
                                .suffix("%"),
                        )
                        .labelled_by(tolerance_label.id);
                        ui.label("Flood Mode");
                        ui.selectable_value(&mut self.fill_global, false, "Contiguous");
                        ui.selectable_value(&mut self.fill_global, true, "Global");
                        ui.label("Sampling");
                        for sampling in MagicWandSampling::ALL {
                            ui.selectable_value(
                                &mut self.magic_wand_sampling,
                                sampling,
                                sampling.label(),
                            );
                        }
                        self.selection_status(ui);
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
                    Tool::Pencil | Tool::Brush | Tool::Eraser => {
                        ui.label(match self.active_tool {
                            Tool::Pencil => "Pencil",
                            Tool::Brush => "Paintbrush",
                            _ => "Eraser",
                        });
                        ui.separator();
                        if self.active_tool == Tool::Pencil {
                            ui.label("Width is fixed at 1 px");
                        } else {
                            let width_label = ui.label("Width");
                            ui.add(
                                egui::DragValue::new(&mut self.brush_size)
                                    .range(1..=64)
                                    .suffix(" px"),
                            )
                            .labelled_by(width_label.id);
                        }
                        if matches!(self.active_tool, Tool::Brush | Tool::Eraser) {
                            ui.separator();
                            let hardness_label = ui.label("Hardness");
                            ui.add(
                                egui::DragValue::new(&mut self.brush_hardness)
                                    .range(0..=100)
                                    .suffix("%"),
                            )
                            .labelled_by(hardness_label.id);
                            let spacing_label = ui.label("Spacing");
                            ui.add(
                                egui::DragValue::new(&mut self.brush_spacing)
                                    .range(1..=100)
                                    .suffix("%"),
                            )
                            .labelled_by(spacing_label.id);
                        }
                        ui.separator();
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
                        ui.separator();
                        ui.label("Drag on the canvas to paint");
                    }
                    Tool::Shapes => {
                        ui.label("Shapes");
                        ui.separator();
                        ui.label("Shape type");
                        ui.selectable_value(
                            &mut self.shape_mode,
                            ShapeMode::Rectangle,
                            "Rectangle",
                        );
                        ui.selectable_value(&mut self.shape_mode, ShapeMode::Ellipse, "Ellipse");
                        ui.separator();
                        ui.label("Fill style");
                        ui.selectable_value(&mut self.shape_style, ShapeStyle::Outline, "Outline");
                        ui.selectable_value(&mut self.shape_style, ShapeStyle::Fill, "Fill");
                        ui.selectable_value(&mut self.shape_style, ShapeStyle::Both, "Both");
                        ui.separator();
                        let border_label = ui.label("Border width");
                        ui.add(
                            egui::DragValue::new(&mut self.shape_border_width)
                                .range(1..=64)
                                .suffix(" px"),
                        )
                        .labelled_by(border_label.id);
                        ui.separator();
                        ui.label(match self.shape_mode {
                            ShapeMode::Rectangle => "Drag to draw a rectangle",
                            ShapeMode::Ellipse => "Drag to draw an ellipse",
                        });
                    }
                    Tool::Gradient => {
                        ui.label("Gradient");
                        ui.separator();
                        ui.label("Gradient type");
                        for (kind, label) in [
                            (GradientType::Linear, "Linear"),
                            (GradientType::Reflected, "Reflected"),
                            (GradientType::Diamond, "Diamond"),
                            (GradientType::Radial, "Radial"),
                            (GradientType::Conical, "Conical"),
                        ] {
                            ui.selectable_value(&mut self.gradient_type, kind, label);
                        }
                        ui.separator();
                        ui.label("Repeat mode");
                        ui.selectable_value(
                            &mut self.gradient_repeat,
                            GradientRepeat::None,
                            "No Repeat",
                        );
                        ui.selectable_value(
                            &mut self.gradient_repeat,
                            GradientRepeat::Repeat,
                            "Repeat",
                        );
                        ui.separator();
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
                        ui.separator();
                        ui.label("Drag to set the gradient direction");
                    }
                    Tool::Recolor => {
                        ui.label("Recolor");
                        ui.separator();
                        let width_label = ui.label("Width");
                        ui.add(
                            egui::DragValue::new(&mut self.brush_size)
                                .range(1..=64)
                                .suffix(" px"),
                        )
                        .labelled_by(width_label.id);
                        ui.separator();
                        let hardness_label = ui.label("Hardness");
                        ui.add(
                            egui::DragValue::new(&mut self.brush_hardness)
                                .range(0..=100)
                                .suffix("%"),
                        )
                        .labelled_by(hardness_label.id);
                        let spacing_label = ui.label("Spacing");
                        ui.add(
                            egui::DragValue::new(&mut self.brush_spacing)
                                .range(1..=100)
                                .suffix("%"),
                        )
                        .labelled_by(spacing_label.id);
                        ui.separator();
                        let tolerance_label = ui.label("Tolerance");
                        ui.add(
                            egui::DragValue::new(&mut self.recolor_tolerance)
                                .range(0..=100)
                                .suffix("%"),
                        )
                        .labelled_by(tolerance_label.id);
                        ui.separator();
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
                        ui.separator();
                        ui.label("Drag over matching pixels to recolor");
                    }
                    Tool::Text => self.text_tool_options(ui),
                    Tool::CloneStamp => self.clone_tool_options(ui),
                    Tool::Pan => {
                        ui.label("Pan");
                        ui.label("Drag to move the canvas view");
                    }
                    Tool::Move | Tool::MoveSelection | Tool::Picker | Tool::Line => {}
                }
            });
            ui.add_space(2.0);
        });
    }

    /// Tool Options for tools whose controls need separate rows instead of one wrapped row.
    fn grouped_tool_options(&mut self, ui: &mut egui::Ui) {
        let title = match self.active_tool {
            Tool::Move => "Move Selected Pixels",
            Tool::MoveSelection => "Move Selection",
            Tool::Picker => "Color Picker",
            Tool::Line => "Line / Curve",
            _ => return,
        };
        ui.horizontal(|ui| {
            ui.strong("Tool Options");
            ui.separator();
            ui.label(title);
        });
        match self.active_tool {
            Tool::Move => self.move_tool_options(ui),
            Tool::MoveSelection => self.move_selection_tool_options(ui),
            Tool::Picker => self.picker_tool_options(ui),
            Tool::Line => self.line_tool_options(ui),
            _ => {}
        }
    }

    fn move_tool_options(&mut self, ui: &mut egui::Ui) {
        tool_option_row(ui, |ui| {
            ui.label("Drag to move the selection, or the active layer without one");
        });
        tool_option_row(ui, |ui| {
            let floating = self.floating_selection.is_some();
            if ui
                .add_enabled(floating, egui::Button::new("Finish"))
                .on_hover_text("Place the floating selection")
                .clicked()
            {
                self.finalize_floating_selection();
            }
            if floating {
                ui.label("Floating pixels are placed by Finish or when the tool changes");
            } else {
                // Integer translation never resamples, so a Nearest/Bilinear/Bicubic
                // selector would have no effect until scaled moves exist.
                ui.label("Sampling: Nearest (integer moves do not resample)");
            }
        });
    }

    fn move_selection_tool_options(&mut self, ui: &mut egui::Ui) {
        tool_option_row(ui, |ui| {
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
        });
        tool_option_row(ui, |ui| {
            let x_label = ui.label("Offset X");
            ui.add(
                egui::DragValue::new(&mut self.move_selection_dx)
                    .range(-16_384..=16_384)
                    .speed(1.0),
            )
            .labelled_by(x_label.id);
            let y_label = ui.label("Offset Y");
            ui.add(
                egui::DragValue::new(&mut self.move_selection_dy)
                    .range(-16_384..=16_384)
                    .speed(1.0),
            )
            .labelled_by(y_label.id);
            let has_selection = self.session.document().selection().is_some();
            if ui
                .add_enabled(has_selection, egui::Button::new("Apply offset"))
                .clicked()
            {
                let (dx, dy) = (self.move_selection_dx, self.move_selection_dy);
                if dx == 0 && dy == 0 {
                    self.status = "Selection offset is zero; nothing to apply".to_owned();
                } else {
                    self.execute(
                        Command::TranslateSelection { dx, dy },
                        "Moved the selection",
                    );
                }
                self.move_selection_dx = 0;
                self.move_selection_dy = 0;
            }
            let pending = self.move_selection_dx != 0 || self.move_selection_dy != 0;
            if ui
                .add_enabled(pending, egui::Button::new("Cancel offset"))
                .clicked()
            {
                self.move_selection_dx = 0;
                self.move_selection_dy = 0;
                self.status = "Canceled the pending selection offset".to_owned();
            }
        });
        tool_option_row(ui, |ui| {
            ui.label("Drag the selection on the canvas to move it");
        });
    }

    fn picker_tool_options(&mut self, ui: &mut egui::Ui) {
        tool_option_row(ui, |ui| {
            ui.label("Click behavior");
            ui.selectable_value(
                &mut self.picker_click_behavior,
                PickerClickBehavior::NoToolSwitch,
                "Do not switch",
            );
            ui.selectable_value(
                &mut self.picker_click_behavior,
                PickerClickBehavior::SwitchToPreviousTool,
                "Switch to previous tool",
            );
        });
        tool_option_row(ui, |ui| {
            ui.label("Sampling");
            ui.selectable_value(
                &mut self.picker_sample_source,
                PickerSampleSource::Layer,
                "Layer",
            );
            ui.selectable_value(
                &mut self.picker_sample_source,
                PickerSampleSource::Image,
                "Composed image",
            );
        });
        tool_option_row(ui, |ui| {
            ui.label("Sample size");
            for size in [
                PickerSampleSize::Point,
                PickerSampleSize::Average3x3,
                PickerSampleSize::Average5x5,
            ] {
                ui.selectable_value(&mut self.picker_sample_size, size, size.label());
            }
        });
        tool_option_row(ui, |ui| {
            ui.label("Click the canvas to sample the primary color");
        });
    }

    fn line_tool_options(&mut self, ui: &mut egui::Ui) {
        tool_option_row(ui, |ui| {
            let width_label = ui.label("Width");
            ui.add(
                egui::DragValue::new(&mut self.brush_size)
                    .range(1..=64)
                    .suffix(" px"),
            )
            .labelled_by(width_label.id);
            let color = self.fill_color;
            ui.colored_label(
                egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a),
                "Primary color",
            );
        });
        tool_option_row(ui, |ui| {
            ui.label("Start cap");
            for (text, cap, accessible) in [
                ("None", LineCap::Flat, "Start cap: None"),
                ("Rounded", LineCap::Rounded, "Start cap: Rounded"),
                ("Arrow", LineCap::Arrow, "Start cap: Arrow"),
            ] {
                if option_selectable(ui, self.line_start_cap == cap, text, accessible) {
                    self.line_start_cap = cap;
                }
            }
        });
        tool_option_row(ui, |ui| {
            ui.label("Dash style");
            for (text, dash, accessible) in [
                ("Solid", LineDash::Solid, "Dash style: Solid"),
                ("Dash", LineDash::Dash, "Dash style: Dash"),
                ("Dot", LineDash::Dot, "Dash style: Dot"),
            ] {
                if option_selectable(ui, self.line_dash == dash, text, accessible) {
                    self.line_dash = dash;
                }
            }
        });
        tool_option_row(ui, |ui| {
            ui.label("End cap");
            for (text, cap, accessible) in [
                ("None", LineCap::Flat, "End cap: None"),
                ("Rounded", LineCap::Rounded, "End cap: Rounded"),
                ("Arrow", LineCap::Arrow, "End cap: Arrow"),
            ] {
                if option_selectable(ui, self.line_end_cap == cap, text, accessible) {
                    self.line_end_cap = cap;
                }
            }
        });
        tool_option_row(ui, |ui| {
            ui.label("Drag on the canvas to draw a line; the active selection clips it");
        });
    }

    fn open_layer_properties(&mut self, index: usize) {
        let Some(layer) = self.session.document().layers().get(index) else {
            return;
        };
        self.layer_properties_open = true;
        self.layer_properties_layer = Some(layer.id());
        self.layer_properties_index = index;
        self.layer_properties_name = layer.name().to_owned();
        self.layer_properties_name_error = None;
        self.layer_properties_opacity = layer.opacity();
        self.layer_properties_visible = layer.is_visible();
        self.layer_properties_blend_mode = layer.blend_mode();
    }

    fn layers_content(&mut self, ui: &mut egui::Ui) {
        let mut pending = None;
        self.layer_list_geometry = None;
        if self.panels_floating {
            ui.horizontal(|ui| {
                ui.strong("Layers");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if window_close_button(ui, "Close Layers window") {
                        self.layers_close_requested = true;
                    }
                });
            });
            ui.separator();
        } else {
            ui.heading("Layers");
            ui.separator();
        }
        let (count, active_index) = {
            let document = self.session.document();
            let active_index = document
                .layers()
                .iter()
                .position(|layer| layer.id() == document.active_layer_id())
                .unwrap_or(0);
            (document.layers().len(), active_index)
        };
        let can_delete = count > 1;
        let can_merge = active_index > 0;
        let can_up = active_index + 1 < count;
        let can_down = active_index > 0;
        egui::Panel::bottom("paint-r-layer-buttons")
            .frame(egui::Frame::NONE)
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.separator();
                ui.horizontal(|ui| {
                    let pad = ((ui.available_width() - 175.0) / 2.0).max(0.0);
                    ui.add_space(pad);
                    ui.spacing_mut().item_spacing.x = 0.0;
                    if tool_button(ui, &self.icons, Icon::AddLayer, "Add Layer", true, false)
                        .clicked()
                    {
                        self.add_layer();
                    }
                    if tool_button(
                        ui,
                        &self.icons,
                        Icon::LayerDelete,
                        "Delete Layer",
                        can_delete,
                        false,
                    )
                    .clicked()
                    {
                        pending = Some(Command::RemoveLayer {
                            index: active_index,
                        });
                    }
                    if tool_button(
                        ui,
                        &self.icons,
                        Icon::LayerDuplicate,
                        "Duplicate Layer",
                        true,
                        false,
                    )
                    .clicked()
                    {
                        pending = Some(Command::DuplicateLayer {
                            index: active_index,
                        });
                    }
                    if tool_button(
                        ui,
                        &self.icons,
                        Icon::LayerMerge,
                        "Merge Layer Down",
                        can_merge,
                        false,
                    )
                    .clicked()
                    {
                        pending = Some(Command::MergeLayerDown {
                            index: active_index,
                        });
                    }
                    if tool_button(
                        ui,
                        &self.icons,
                        Icon::LayerUp,
                        "Move Layer Up",
                        can_up,
                        false,
                    )
                    .clicked()
                    {
                        pending = Some(Command::MoveLayer {
                            index: active_index,
                            offset: 1,
                        });
                    }
                    if tool_button(
                        ui,
                        &self.icons,
                        Icon::LayerDown,
                        "Move Layer Down",
                        can_down,
                        false,
                    )
                    .clicked()
                    {
                        pending = Some(Command::MoveLayer {
                            index: active_index,
                            offset: -1,
                        });
                    }
                    if tool_button(
                        ui,
                        &self.icons,
                        Icon::LayerSettings,
                        "Layer Properties",
                        true,
                        false,
                    )
                    .clicked()
                    {
                        self.open_layer_properties(active_index);
                    }
                });
            });

        // Add Layer runs immediately, so refresh the state the list is about to render.
        let count = self.session.document().layers().len();

        // Paint.NET docks a solid scrollbar beside the strip, so content is never covered.
        let mut scroll_style = ui.spacing().scroll;
        scroll_style.floating = false;
        scroll_style.bar_width = scroll_style.bar_width.max(8.0);
        scroll_style.bar_inner_margin = 4.0;
        scroll_style.bar_outer_margin = 0.0;
        scroll_style.content_margin = egui::Margin::ZERO;
        ui.spacing_mut().scroll = scroll_style;

        let outer = ui.available_rect_before_wrap();
        let mut rows = Vec::with_capacity(count);
        let mut drag_started = None;
        let mut drag_pointer_y = None;
        let mut drop_pointer_y = None;
        let dragged_layer = self.layer_drag.map(|drag| drag.layer_id);
        let output = egui::ScrollArea::vertical()
            .id_salt("paint-r-layer-list")
            .auto_shrink([false, false])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let row_width = ui.available_width();
                for index in (0..count).rev() {
                    let (layer_id, name, visible, active, opacity, blend_mode) = {
                        let document = self.session.document();
                        let layer = &document.layers()[index];
                        (
                            layer.id(),
                            layer.name().to_owned(),
                            layer.is_visible(),
                            layer.id() == document.active_layer_id(),
                            layer.opacity(),
                            layer.blend_mode(),
                        )
                    };
                    let (row_rect, row_response) = ui.allocate_exact_size(
                        egui::vec2(row_width, LAYER_ITEM_HEIGHT),
                        egui::Sense::click_and_drag(),
                    );
                    row_response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::Role::Button,
                            true,
                            active,
                            format!("Layer {index}: {name}"),
                        )
                    });
                    let row_hovered = row_response.hovered();
                    let selected_fill = ui.visuals().selection.bg_fill;
                    let painter = ui.painter();
                    if active {
                        painter.rect_filled(
                            row_rect.shrink2(egui::vec2(0.0, 1.0)),
                            4,
                            selected_fill,
                        );
                    } else if row_hovered {
                        painter.rect_filled(
                            row_rect.shrink2(egui::vec2(0.0, 1.0)),
                            4,
                            egui::Color32::from_rgb(52, 52, 52),
                        );
                    }
                    if dragged_layer == Some(layer_id) {
                        painter.rect_filled(
                            row_rect.shrink2(egui::vec2(2.0, 1.0)),
                            4,
                            egui::Color32::from_rgba_unmultiplied(31, 78, 116, 110),
                        );
                    }
                    let thumbnail_rect = egui::Rect::from_min_size(
                        egui::pos2(
                            row_rect.left() + LAYER_ITEM_PADDING,
                            row_rect.center().y - LAYER_THUMBNAIL_SIZE / 2.0,
                        ),
                        egui::vec2(LAYER_THUMBNAIL_SIZE, LAYER_THUMBNAIL_SIZE),
                    );
                    let visibility_rect = egui::Rect::from_min_size(
                        egui::pos2(
                            row_rect.right() - LAYER_ITEM_PADDING - LAYER_VISIBILITY_HIT_WIDTH,
                            row_rect.top() + LAYER_ITEM_PADDING,
                        ),
                        egui::vec2(
                            LAYER_VISIBILITY_HIT_WIDTH,
                            LAYER_ITEM_HEIGHT - LAYER_ITEM_PADDING * 2.0,
                        ),
                    );
                    let text_rect = egui::Rect::from_min_max(
                        egui::pos2(
                            thumbnail_rect.right() + LAYER_ITEM_PADDING,
                            row_rect.top() + LAYER_ITEM_PADDING,
                        ),
                        egui::pos2(
                            visibility_rect.left() - LAYER_ITEM_PADDING,
                            row_rect.bottom() - LAYER_ITEM_PADDING,
                        ),
                    );

                    let thumbnail_response = ui.interact(
                        thumbnail_rect,
                        ui.scope_id()
                            .with(("paint-r-layer-thumbnail", layer_id.get())),
                        egui::Sense::hover(),
                    );
                    thumbnail_response.widget_info(|| {
                        egui::WidgetInfo::labeled(
                            egui::Role::Image,
                            true,
                            format!("Layer {index} thumbnail: {name}"),
                        )
                    });
                    paint_checkerboard(ui.painter(), thumbnail_rect);
                    if let Some(thumbnail) = self.layer_thumbnails.get(&layer_id) {
                        egui::Image::from_texture(&thumbnail.texture).paint_at(ui, thumbnail_rect);
                    }
                    ui.painter().rect_stroke(
                        thumbnail_rect,
                        2,
                        egui::Stroke::new(1.0, egui::Color32::from_gray(70)),
                        egui::StrokeKind::Inside,
                    );

                    let mut text_ui = ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(text_rect)
                            .layout(egui::Layout::top_down(egui::Align::LEFT)),
                    );
                    text_ui.spacing_mut().item_spacing.y = 0.0;
                    text_ui.set_clip_rect(text_rect.intersect(ui.clip_rect()));
                    let active_label = text_ui
                        .selectable_label(active, name.clone())
                        .accessible_name(format!("Active layer {index}: {name}"));
                    text_ui.label(
                        egui::RichText::new(format!(
                            "{}% · {}",
                            u16::from(opacity) * 100 / 255,
                            blend_mode_label(blend_mode)
                        ))
                        .size(11.0)
                        .color(egui::Color32::from_gray(185)),
                    );

                    let visibility_label = format!("Layer {index} visible");
                    let (_, toggled) = visibility_button_at(
                        ui,
                        &self.icons,
                        visibility_rect,
                        visible,
                        &visibility_label,
                    );
                    if toggled {
                        pending = Some(Command::SetLayerVisibility {
                            index,
                            visible: !visible,
                        });
                    }

                    let pointer_y = ui.input(|input| input.pointer.interact_pos().map(|pos| pos.y));
                    if row_response.drag_started() {
                        if let Some(pointer_y) = pointer_y {
                            drag_started = Some((layer_id, index, pointer_y));
                        }
                    } else if row_response.dragged() {
                        drag_pointer_y = pointer_y;
                    }
                    if row_response.drag_stopped() {
                        drop_pointer_y = pointer_y;
                    }
                    if row_response.double_clicked() || active_label.double_clicked() {
                        self.open_layer_properties(index);
                    } else if row_response.clicked() || active_label.clicked() {
                        pending = Some(Command::SetActiveLayer { index });
                    }

                    rows.push(LayerRowGeometry {
                        index,
                        rect: row_rect,
                        thumbnail: thumbnail_rect,
                        visibility: visibility_rect,
                    });
                }
            });

        if let Some((layer_id, from_index, pointer_y)) = drag_started {
            self.layer_drag = Some(LayerDragState {
                layer_id,
                from_index,
                pointer_y,
            });
        }
        if let Some(pointer_y) = drag_pointer_y
            && let Some(drag) = &mut self.layer_drag
        {
            drag.pointer_y = pointer_y;
        }
        if let Some(pointer_y) = drop_pointer_y {
            self.layer_drop_pointer_y = Some(pointer_y);
        }

        let viewport = output.inner_rect;
        let offset_y = output.state.clamped_offset().y;
        let content_height = count as f32 * LAYER_ITEM_HEIGHT;
        let scrollbar = (content_height > viewport.height() + 0.5).then(|| {
            egui::Rect::from_min_max(
                egui::pos2(viewport.right(), viewport.top()),
                egui::pos2(outer.right(), viewport.bottom()),
            )
        });
        let insertion_slot = |pointer_y: f32| {
            (((pointer_y - viewport.top() + offset_y) / LAYER_ITEM_HEIGHT)
                .round()
                .clamp(0.0, count as f32)) as usize
        };
        let insertion_indicator = self.layer_drag.map(|drag| {
            let slot = insertion_slot(drag.pointer_y);
            let y = viewport.top() - offset_y + slot as f32 * LAYER_ITEM_HEIGHT;
            egui::Rect::from_min_max(
                egui::pos2(viewport.left() + LAYER_ITEM_PADDING, y - 1.0),
                egui::pos2(viewport.right() - LAYER_ITEM_PADDING, y + 1.0),
            )
        });

        if let Some(pointer_y) = self.layer_drop_pointer_y.take()
            && let Some(drag) = self.layer_drag.take()
        {
            let slot = insertion_slot(pointer_y);
            let insertion_layer = count - slot;
            let mut target = if drag.from_index < insertion_layer {
                insertion_layer - 1
            } else {
                insertion_layer
            };
            target = target.min(count.saturating_sub(1));
            if target != drag.from_index {
                pending = Some(Command::MoveLayer {
                    index: drag.from_index,
                    offset: target as i32 - drag.from_index as i32,
                });
            }
        }

        if let Some(indicator) = insertion_indicator {
            ui.painter().with_clip_rect(viewport).rect_filled(
                indicator,
                1,
                egui::Color32::from_rgb(100, 178, 235),
            );
        }

        self.layer_list_geometry = Some(LayerListGeometry {
            item_height: LAYER_ITEM_HEIGHT,
            thumbnail_size: LAYER_THUMBNAIL_SIZE,
            padding: LAYER_ITEM_PADDING,
            outer,
            viewport,
            scrollbar,
            rows,
            insertion_indicator,
        });

        if let Some(command) = pending {
            let message = if matches!(command, Command::MoveLayer { .. }) {
                "Moved layer"
            } else if matches!(command, Command::SetActiveLayer { .. }) {
                "Selected layer"
            } else {
                "Layer properties updated"
            };
            self.execute(command, message);
        }
    }

    fn history_content(&mut self, ui: &mut egui::Ui) {
        if self.panels_floating {
            ui.horizontal(|ui| {
                ui.strong("History");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if window_close_button(ui, "Close History window") {
                        self.history_close_requested = true;
                    }
                });
            });
            ui.separator();
        } else {
            ui.heading("History");
            ui.separator();
        }
        egui::Panel::bottom("paint-r-history-buttons")
            .frame(egui::Frame::NONE)
            .show_separator_line(false)
            .show(ui, |ui| {
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
            });
        egui::ScrollArea::vertical()
            .id_salt("paint-r-history-list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                for entry in &self.history_entries {
                    ui.label(entry);
                }
                for entry in self.redo_entries.iter().rev() {
                    ui.add_enabled(false, egui::Label::new(entry).selectable(false));
                }
            });
    }

    fn colors_content(&mut self, ui: &mut egui::Ui) {
        if self.panels_floating {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Colors").strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if window_close_button(ui, "Close Colors window") {
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
                let right_bottom = {
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
                        hsv_gradient_slider(
                            ui,
                            "Value",
                            "V",
                            &mut brightness_value,
                            0.0,
                            100.0,
                            |t| {
                                let bgra = crate::color_from_hsv(
                                    hue_base,
                                    saturation_base / 100.0,
                                    t / 100.0,
                                    255,
                                );
                                egui::Color32::from_rgb(bgra.r, bgra.g, bgra.b)
                            },
                        );
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
                    right_ui.min_rect().bottom()
                };
                ui.expand_to_include_rect(egui::Rect::from_min_max(
                    egui::pos2(content.right(), content.top()),
                    egui::pos2(content.right(), right_bottom),
                ));
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
                    self.layers_content(ui);
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
                .title_bar(false)
                .frame(frame)
                .collapsible(false)
                .default_pos(egui::pos2(20.0, 180.0))
                .min_width(68.0)
                .max_width(68.0)
                .resizable(false)
                .show(&context, |ui| self.tools_content(ui))
            {
                self.tools_window_rect = Some(inner.response.rect);
            }
            if self.tools_close_requested {
                open = false;
                self.tools_close_requested = false;
            }
            self.tools_visible = open;
        }
        if self.history_visible {
            let mut open = self.history_visible;
            let frame = floating_window_frame(&context, self.history_window_rect);
            if let Some(inner) = egui::Window::new("History")
                .open(&mut open)
                .title_bar(false)
                .frame(frame)
                .collapsible(false)
                .default_pos(egui::pos2(500.0, 260.0))
                .fixed_size(egui::vec2(196.0, 219.0))
                .show(&context, |ui| self.history_content(ui))
            {
                self.history_window_rect = Some(inner.response.rect);
            }
            if self.history_close_requested {
                open = false;
                self.history_close_requested = false;
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
                .title_bar(false)
                .frame(frame)
                .collapsible(false)
                .default_pos(egui::pos2(980.0, 260.0))
                .fixed_size(egui::vec2(196.0, 219.0))
                .show(&context, |ui| self.layers_content(ui))
            {
                self.layers_window_rect = Some(inner.response.rect);
            }
            if self.layers_close_requested {
                open = false;
                self.layers_close_requested = false;
            }
            self.layers_visible = open;
        }
    }

    fn layer_properties_dialog(&mut self, context: &egui::Context) {
        if !self.layer_properties_open {
            return;
        }
        let Some(layer_id) = self.layer_properties_layer else {
            self.layer_properties_open = false;
            return;
        };
        let Some(index) = self.session.document().layer_index(layer_id) else {
            self.layer_properties_open = false;
            return;
        };
        self.layer_properties_index = index;
        let name = self.session.document().layers()[index].name().to_owned();
        let mut open = self.layer_properties_open;
        let mut apply = false;
        let mut close = false;
        egui::Window::new("Layer Properties")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(240.0)
            .show(context, |ui| {
                ui.label(format!("Layer: {name}"));
                ui.separator();
                let name_label = ui.label("Layer name");
                ui.add(
                    egui::TextEdit::singleline(&mut self.layer_properties_name)
                        .desired_width(f32::INFINITY),
                )
                .labelled_by(name_label.id);
                if let Some(error) = &self.layer_properties_name_error {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
                layer_opacity_slider(ui, &mut self.layer_properties_opacity);
                ui.checkbox(&mut self.layer_properties_visible, "Visible");
                egui::ComboBox::from_label("Blend Mode")
                    .selected_text(blend_mode_label(self.layer_properties_blend_mode))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.layer_properties_blend_mode,
                            BlendMode::Normal,
                            "Normal",
                        );
                        ui.selectable_value(
                            &mut self.layer_properties_blend_mode,
                            BlendMode::Multiply,
                            "Multiply",
                        );
                        ui.selectable_value(
                            &mut self.layer_properties_blend_mode,
                            BlendMode::Screen,
                            "Screen",
                        );
                        ui.selectable_value(
                            &mut self.layer_properties_blend_mode,
                            BlendMode::Overlay,
                            "Overlay",
                        );
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        apply = true;
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
        if apply {
            let trimmed = self.layer_properties_name.trim().to_owned();
            let maximum = self.session.limits().max_layer_name_bytes;
            if trimmed.is_empty() {
                self.layer_properties_name_error = Some("Layer name cannot be empty".to_owned());
            } else if trimmed.len() > maximum {
                self.layer_properties_name_error = Some(format!(
                    "Layer name is {} bytes; the maximum is {maximum}",
                    trimmed.len()
                ));
            } else {
                self.layer_properties_name_error = None;
                let index = self.layer_properties_index;
                if trimmed != name {
                    self.execute(
                        Command::RenameLayer {
                            index,
                            name: trimmed,
                        },
                        "Layer renamed",
                    );
                }
                self.execute(
                    Command::SetLayerOpacity {
                        index,
                        opacity: self.layer_properties_opacity,
                    },
                    "Layer properties updated",
                );
                self.execute(
                    Command::SetLayerVisibility {
                        index,
                        visible: self.layer_properties_visible,
                    },
                    "Layer properties updated",
                );
                self.execute(
                    Command::SetLayerBlendMode {
                        index,
                        mode: self.layer_properties_blend_mode,
                    },
                    "Layer properties updated",
                );
            }
        }
        if close {
            open = false;
        }
        self.layer_properties_open = open;
    }

    fn adjustment_dialog_window(&mut self, context: &egui::Context) {
        let Some(mut dialog) = self.adjustment_dialog.take() else {
            return;
        };
        let mut open = true;
        let mut apply = false;
        let mut cancel =
            context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        egui::Window::new(dialog.title())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(context, |ui| {
                dialog.show(ui);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if apply {
            self.apply_adjustment(dialog);
        } else if cancel || !open {
            let title = dialog.title();
            self.cancel_pending_adjustment();
            self.status = format!("Canceled {title}");
        } else {
            let effect = dialog.effect();
            if self.adjustment_previewed != Some(effect) {
                match self.session.preview_effect(effect) {
                    Ok(_) => {
                        self.adjustment_previewed = Some(effect);
                        self.texture_dirty = true;
                    }
                    Err(error) => {
                        self.status = format!("Adjustment preview failed: {error}");
                    }
                }
            }
            self.adjustment_dialog = Some(dialog);
        }
    }

    fn open_adjustment_dialog(&mut self, dialog: AdjustmentDialog) {
        self.close_adjustment_preview();
        self.finish_active_preview();
        self.finalize_floating_selection();
        self.gaussian_blur_open = false;
        self.session.begin_preview();
        self.adjustment_previewed = None;
        self.adjustment_dialog = Some(dialog);
        self.texture_dirty = true;
    }

    fn apply_adjustment(&mut self, dialog: AdjustmentDialog) {
        let effect = dialog.effect();
        let label = effect.history_label();
        self.cancel_pending_adjustment();
        self.execute(Command::ApplyEffect(effect), format!("Applied {label}"));
    }

    fn cancel_pending_adjustment(&mut self) {
        self.session.cancel_preview();
        self.adjustment_previewed = None;
        self.texture_dirty = true;
    }

    fn close_adjustment_preview(&mut self) {
        if self.adjustment_dialog.take().is_some() {
            self.cancel_pending_adjustment();
        }
    }

    fn open_gaussian_blur_dialog(&mut self) {
        self.close_adjustment_preview();
        self.gaussian_blur_open = true;
    }

    fn gaussian_blur_dialog(&mut self, context: &egui::Context) {
        if !self.gaussian_blur_open {
            return;
        }
        let mut open = self.gaussian_blur_open;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new("Gaussian Blur")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(260.0)
            .show(context, |ui| {
                ui.label("Soften the selection with a separable Gaussian kernel.");
                ui.separator();
                ui.add(
                    egui::Slider::new(&mut self.gaussian_blur_radius, 0..=MAX_GAUSSIAN_BLUR_RADIUS)
                        .text("Radius"),
                );
                ui.label("A radius of zero leaves the selection unchanged.");
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Apply").clicked() {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel {
            open = false;
        }
        if apply {
            let radius = self.gaussian_blur_radius;
            self.execute(
                Command::ApplyEffect(Effect::GaussianBlur { radius }),
                format!("Applied Gaussian Blur (radius {radius})"),
            );
            open = false;
        }
        self.gaussian_blur_open = open;
    }

    fn image_size_dialogs(&mut self, context: &egui::Context) {
        self.resize_image_dialog(context);
        self.canvas_size_dialog(context);
    }

    fn resize_image_dialog(&mut self, context: &egui::Context) {
        let Some(mut dialog) = self.resize_dialog.take() else {
            return;
        };
        let limits = self.session.limits();
        let mut apply = false;
        let mut cancel =
            context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        egui::Window::new("Resize Image")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(context, |ui| {
                ui.label(
                    "Resample every layer to a new pixel size. Bilinear sampling is used and \
                     the selection is cleared.",
                );
                let validated = size_fields_ui(ui, &mut dialog.fields, limits);
                ui.separator();
                match &validated {
                    Ok(size) => {
                        ui.label(format!(
                            "New image: {} × {} pixels across {} layers",
                            size.width(),
                            size.height(),
                            self.session.document().layers().len()
                        ));
                    }
                    Err(error) => {
                        ui.colored_label(egui::Color32::LIGHT_RED, format!("Error: {error}"));
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(validated.is_ok(), egui::Button::new("Apply"))
                        .clicked()
                    {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel {
            self.status = "Resize Image canceled".to_owned();
            return;
        }
        if !apply {
            self.resize_dialog = Some(dialog);
            return;
        }
        let Ok(size) = dialog.fields.validated_size(limits) else {
            self.resize_dialog = Some(dialog);
            return;
        };
        let message = format!("Resized the image to {}×{}", size.width(), size.height());
        let _ = self.execute(
            Command::ResizeImage {
                width: size.width(),
                height: size.height(),
            },
            message,
        );
        if self.session.document().size() != size {
            self.resize_dialog = Some(dialog);
        }
    }

    fn canvas_size_dialog(&mut self, context: &egui::Context) {
        let Some(mut dialog) = self.canvas_size_dialog.take() else {
            return;
        };
        let limits = self.session.limits();
        let mut apply = false;
        let mut cancel =
            context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        egui::Window::new("Canvas Size")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(context, |ui| {
                ui.label(
                    "Pad or crop the canvas around the existing pixels. Anchors choose where \
                     the current canvas sits; the fill color covers exposed pixels.",
                );
                let validated = size_fields_ui(ui, &mut dialog.fields, limits);
                ui.separator();
                ui.strong("Anchor");
                ui.label("Anchor: where the existing canvas is placed");
                egui::Grid::new("paint-r-canvas-anchor-grid")
                    .spacing([10.0, 4.0])
                    .show(ui, |ui| {
                        for anchors in [
                            [
                                CanvasAnchor::TopLeft,
                                CanvasAnchor::Top,
                                CanvasAnchor::TopRight,
                            ],
                            [
                                CanvasAnchor::Left,
                                CanvasAnchor::Middle,
                                CanvasAnchor::Right,
                            ],
                            [
                                CanvasAnchor::BottomLeft,
                                CanvasAnchor::Bottom,
                                CanvasAnchor::BottomRight,
                            ],
                        ] {
                            for anchor in anchors {
                                ui.radio_value(
                                    &mut dialog.anchor,
                                    anchor,
                                    canvas_anchor_label(anchor),
                                );
                            }
                            ui.end_row();
                        }
                    });
                ui.separator();
                ui.strong("Fill color");
                ui.horizontal_wrapped(|ui| {
                    for (fill, label) in CANVAS_FILLS {
                        ui.radio_value(&mut dialog.fill, fill, label);
                    }
                });
                ui.separator();
                match &validated {
                    Ok(size) => {
                        ui.label(format!(
                            "New canvas: {} × {} pixels across {} layers",
                            size.width(),
                            size.height(),
                            self.session.document().layers().len()
                        ));
                    }
                    Err(error) => {
                        ui.colored_label(egui::Color32::LIGHT_RED, format!("Error: {error}"));
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(validated.is_ok(), egui::Button::new("Apply"))
                        .clicked()
                    {
                        apply = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if cancel {
            self.status = "Canvas Size canceled".to_owned();
            return;
        }
        if !apply {
            self.canvas_size_dialog = Some(dialog);
            return;
        }
        let Ok(size) = dialog.fields.validated_size(limits) else {
            self.canvas_size_dialog = Some(dialog);
            return;
        };
        let fill = match dialog.fill {
            CanvasFill::Transparent => Bgra::TRANSPARENT,
            CanvasFill::White => Bgra::WHITE,
            CanvasFill::Black => Bgra::BLACK,
            CanvasFill::Primary => self.fill_color,
            CanvasFill::Secondary => self.secondary_color,
        };
        let message = format!("Resized the canvas to {}×{}", size.width(), size.height());
        let _ = self.execute(
            Command::CanvasSize {
                width: size.width(),
                height: size.height(),
                anchor: dialog.anchor,
                fill,
            },
            message,
        );
        if self.session.document().size() != size {
            self.canvas_size_dialog = Some(dialog);
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
                    ui.label(format!(
                        "{} × {} {}",
                        self.unit.format_value(f64::from(size.width())),
                        self.unit.format_value(f64::from(size.height())),
                        self.unit.suffix()
                    ));
                    ui.separator();
                    if let Some(point) = self.cursor_point {
                        ui.label(format!(
                            "{}, {} {}",
                            self.unit.format_value(f64::from(point.x)),
                            self.unit.format_value(f64::from(point.y)),
                            self.unit.suffix()
                        ));
                    } else {
                        ui.label(format!("—, — {}", self.unit.suffix()));
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
            if self.zoom_to_selection_requested {
                if let Some(selection) = self
                    .session
                    .document()
                    .selection()
                    .and_then(|selection| selection.clip_to(size))
                {
                    let available = ui.available_size();
                    self.zoom = ((available.x - 24.0).max(32.0) / selection.width() as f32)
                        .min((available.y - 24.0).max(32.0) / selection.height() as f32)
                        .clamp(0.05, 8.0);
                }
                self.zoom_to_selection_requested = false;
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
                            paint_checkerboard(ui.painter(), rect);
                            egui::Image::from_texture(&texture).paint_at(ui, rect);
                        }
                        if let Some(floating) = &self.floating_selection {
                            let min = rect.min
                                + egui::vec2(
                                    (floating.floating.bounds.left() + floating.offset_x) as f32
                                        * zoom,
                                    (floating.floating.bounds.top() + floating.offset_y) as f32
                                        * zoom,
                                );
                            let size = egui::vec2(
                                floating.floating.bounds.width() as f32 * zoom,
                                floating.floating.bounds.height() as f32 * zoom,
                            );
                            egui::Image::from_texture(&floating.texture)
                                .paint_at(ui, egui::Rect::from_min_size(min, size));
                        }
                        if self.show_grid {
                            let step = (8.0 * zoom).max(4.0);
                            let grid = egui::Color32::from_rgba_unmultiplied(128, 128, 128, 110);
                            let mut x = rect.left();
                            while x <= rect.right() {
                                ui.painter().line_segment(
                                    [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                                    egui::Stroke::new(1.0, grid),
                                );
                                x += step;
                            }
                            let mut y = rect.top();
                            while y <= rect.bottom() {
                                ui.painter().line_segment(
                                    [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                                    egui::Stroke::new(1.0, grid),
                                );
                                y += step;
                            }
                        }
                        let phase = ui.input(|input| input.time) as f32;
                        let dragging_selection = matches!(
                            self.active_tool,
                            Tool::Select
                                | Tool::Lasso
                                | Tool::EllipseSelect
                                | Tool::MoveSelection
                                | Tool::Move
                        ) && self.drag_start.is_some()
                            && self.drag_current.is_some();
                        if !dragging_selection
                            && let Some(selection) = self.session.document().selection_model()
                        {
                            paint_selection(ui.painter(), rect, zoom, selection, phase);
                            if ui.style().animation_time > 0.0 {
                                ui.ctx()
                                    .request_repaint_after(std::time::Duration::from_millis(80));
                            }
                        }
                        let pointer = response.interact_pointer_pos();
                        self.cursor_point = pointer
                            .and_then(|position| canvas_point_at(position, rect, zoom, size));
                        if self.active_tool == Tool::Text
                            && let Some(origin) = self.text_origin
                        {
                            let marker = rect.min
                                + egui::vec2(origin.x as f32 * zoom, origin.y as f32 * zoom);
                            let stroke = egui::Stroke::new(1.5, egui::Color32::LIGHT_BLUE);
                            ui.painter().line_segment(
                                [marker - egui::vec2(7.0, 0.0), marker + egui::vec2(7.0, 0.0)],
                                stroke,
                            );
                            ui.painter().line_segment(
                                [marker - egui::vec2(0.0, 7.0), marker + egui::vec2(0.0, 7.0)],
                                stroke,
                            );
                            ui.painter().circle_stroke(marker, 4.0, stroke);
                        }
                        if self.active_tool == Tool::CloneStamp
                            && let Some(source) = self.clone_source
                        {
                            let marker = rect.min
                                + egui::vec2(source.x as f32 * zoom, source.y as f32 * zoom);
                            let radius = (self.brush_size.max(1) as f32 * zoom / 2.0).max(2.0);
                            let stroke = egui::Stroke::new(1.5, egui::Color32::LIGHT_BLUE);
                            ui.painter().circle_stroke(marker, radius, stroke);
                            ui.painter().line_segment(
                                [marker - egui::vec2(6.0, 0.0), marker + egui::vec2(6.0, 0.0)],
                                stroke,
                            );
                            ui.painter().line_segment(
                                [marker - egui::vec2(0.0, 6.0), marker + egui::vec2(0.0, 6.0)],
                                stroke,
                            );
                        }
                        if picking {
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
                        } else if self.active_tool == Tool::MagicWand {
                            if response.clicked()
                                && let Some(point) = pointer.and_then(|position| {
                                    canvas_point_at(position, rect, zoom, size)
                                })
                            {
                                let threshold = (u16::from(self.fill_tolerance) * 255 / 100) as u8;
                                let mode = self.gesture_selection_mode(ui);
                                pending_paint = Some((
                                    Command::SelectMagicWand {
                                        point,
                                        tolerance: threshold,
                                        global: self.fill_global,
                                        sampling: self.magic_wand_sampling,
                                        mode,
                                    },
                                    if self.fill_global {
                                        "Selected matching pixels globally".to_owned()
                                    } else {
                                        "Selected a contiguous region".to_owned()
                                    },
                                ));
                            }
                        } else if self.active_tool == Tool::Text {
                            if response.clicked()
                                && let Some(point) = pointer.and_then(|position| {
                                    canvas_point_at(position, rect, zoom, size)
                                })
                            {
                                self.text_origin = Some(point);
                                self.text_applied_key = None;
                                self.status = format!(
                                    "Text origin set at ({}, {}); type text in Tool Options",
                                    point.x, point.y
                                );
                            }
                        } else if self.active_tool == Tool::CloneStamp {
                            let point = pointer
                                .and_then(|position| canvas_point_at(position, rect, zoom, size));
                            let alt = ui.input(|input| input.modifiers.alt);
                            if alt {
                                if response.is_pointer_button_down_on()
                                    && !self.clone_alt_down
                                    && let Some(point) = point
                                {
                                    self.clone_source = Some(point);
                                    self.clone_offset = None;
                                    self.clone_alt_down = true;
                                    self.status =
                                        format!("Clone source set at ({}, {})", point.x, point.y);
                                }
                                self.drag_start = None;
                                self.drag_current = None;
                                self.paint_points.clear();
                                if self.session.has_preview() {
                                    self.session.cancel_preview();
                                    self.texture_dirty = true;
                                }
                            } else {
                                if response.is_pointer_button_down_on()
                                    && self.drag_start.is_none()
                                    && let Some(point) = point
                                {
                                    if let Some(source) = self.clone_source {
                                        self.drag_start = Some(point);
                                        self.drag_current = Some(point);
                                        self.paint_points.clear();
                                        self.paint_points.push(point);
                                        self.clone_offset = Some((
                                            i64::from(point.x) - i64::from(source.x),
                                            i64::from(point.y) - i64::from(source.y),
                                        ));
                                        self.session.begin_preview();
                                        self.preview_live_clone();
                                    } else {
                                        self.status =
                                            "Alt-click the canvas to set a clone source first"
                                                .to_owned();
                                    }
                                }
                                if response.dragged()
                                    && self.drag_start.is_some()
                                    && let Some(point) = point
                                {
                                    self.drag_current = Some(point);
                                    if self.paint_points.last() != Some(&point) {
                                        self.paint_points.push(point);
                                        self.preview_live_clone();
                                    }
                                }
                                if response.drag_stopped() && self.drag_start.take().is_some() {
                                    self.drag_current = None;
                                    self.finish_live_clone();
                                }
                            }
                            if !response.is_pointer_button_down_on() {
                                self.clone_alt_down = false;
                            }
                        } else if self.active_tool != Tool::Picker {
                            let point = || {
                                pointer.and_then(|position| {
                                    canvas_point_at(position, rect, zoom, size)
                                })
                            };
                            let is_paint = matches!(
                                self.active_tool,
                                Tool::Pencil | Tool::Brush | Tool::Eraser
                            );
                            let collects_points = is_paint
                                || self.active_tool == Tool::Recolor
                                || self.active_tool == Tool::Lasso;
                            if self.drag_start.is_none()
                                && response.is_pointer_button_down_on()
                                && let Some(point) = point()
                            {
                                self.drag_start = Some(point);
                                self.drag_current = Some(point);
                                self.paint_points.clear();
                                if collects_points {
                                    self.paint_points.push(point);
                                }
                                if is_paint {
                                    self.session.begin_stroke_preview();
                                    self.preview_live_stroke();
                                }
                                if self.active_tool == Tool::Move
                                    && self.session.document().selection().is_some()
                                {
                                    self.lift_floating_selection(ui.ctx());
                                    if let Some(floating) = self.floating_selection.as_mut() {
                                        floating.base_x = floating.offset_x;
                                        floating.base_y = floating.offset_y;
                                    }
                                }
                            }
                            if response.dragged()
                                && let Some(point) = point()
                            {
                                self.drag_current = Some(point);
                                if self.active_tool == Tool::Move
                                    && let (Some(start), Some(floating)) =
                                        (self.drag_start.as_ref(), self.floating_selection.as_mut())
                                {
                                    floating.offset_x =
                                        floating.base_x + i64::from(point.x) - i64::from(start.x);
                                    floating.offset_y =
                                        floating.base_y + i64::from(point.y) - i64::from(start.y);
                                }
                                if collects_points && self.paint_points.last() != Some(&point) {
                                    self.paint_points.push(point);
                                    if is_paint {
                                        self.preview_live_stroke();
                                    }
                                }
                            }
                            if response.drag_stopped()
                                && let (Some(start), Some(end)) =
                                    (self.drag_start.take(), self.drag_current.take())
                            {
                                let bounds = selection_rect_from_drag(start, end);
                                let selection_mode = self.gesture_selection_mode(ui);
                                let color = self.fill_color;
                                let size = match self.active_tool {
                                    Tool::Pencil => 1,
                                    _ => self.brush_size.max(1),
                                };
                                pending_paint = match self.active_tool {
                                    Tool::Lasso => {
                                        if self.paint_points.last() != Some(&end) {
                                            self.paint_points.push(end);
                                        }
                                        let points = std::mem::take(&mut self.paint_points);
                                        Some((
                                            Command::SelectLasso {
                                                points,
                                                mode: selection_mode,
                                            },
                                            "Lasso selection updated".to_owned(),
                                        ))
                                    }
                                    Tool::EllipseSelect => Some((
                                        Command::SelectEllipse {
                                            rect: bounds,
                                            mode: selection_mode,
                                        },
                                        "Ellipse selection updated".to_owned(),
                                    )),
                                    Tool::MoveSelection => {
                                        let dx = i64::from(end.x) - i64::from(start.x);
                                        let dy = i64::from(end.y) - i64::from(start.y);
                                        if dx != 0 || dy != 0 {
                                            Some((
                                                Command::TranslateSelection {
                                                    dx: dx as i32,
                                                    dy: dy as i32,
                                                },
                                                "Selection moved".to_owned(),
                                            ))
                                        } else {
                                            None
                                        }
                                    }
                                    Tool::Select => {
                                        pending_selection = Some((bounds, selection_mode));
                                        None
                                    }
                                    Tool::Shapes if self.shape_mode == ShapeMode::Rectangle => {
                                        Some((
                                            Command::FillRect {
                                                rect: bounds,
                                                color,
                                                style: self.shape_style,
                                                border_width: self.shape_border_width,
                                            },
                                            "Drew a rectangle shape".to_owned(),
                                        ))
                                    }
                                    Tool::Shapes if self.shape_mode == ShapeMode::Ellipse => {
                                        Some((
                                            Command::FillEllipse {
                                                rect: bounds,
                                                color,
                                                style: self.shape_style,
                                                border_width: self.shape_border_width,
                                            },
                                            "Drew an ellipse shape".to_owned(),
                                        ))
                                    }
                                    Tool::Line => Some((
                                        Command::StrokeLine {
                                            label: "Line / Curve",
                                            points: vec![start, end],
                                            style: LineStyle {
                                                width: size,
                                                color: Some(color),
                                                start_cap: self.line_start_cap,
                                                end_cap: self.line_end_cap,
                                                dash: self.line_dash,
                                            },
                                        },
                                        "Drew a line".to_owned(),
                                    )),
                                    Tool::Gradient => Some((
                                        Command::Gradient {
                                            start,
                                            end,
                                            from: color,
                                            to: self.secondary_color,
                                            options: GradientOptions::new(
                                                self.gradient_type,
                                                self.gradient_repeat,
                                            ),
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
                                                brush: BrushOptions::new(
                                                    size,
                                                    self.brush_hardness,
                                                    self.brush_spacing,
                                                ),
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
                                        if dx == 0 && dy == 0 {
                                            None
                                        } else if self.floating_selection.is_some() {
                                            // The floating surface already previews the move.
                                            // Translate the real selection model by the same delta.
                                            Some((
                                                Command::TranslateSelection {
                                                    dx: dx as i32,
                                                    dy: dy as i32,
                                                },
                                                "Moved selected pixels".to_owned(),
                                            ))
                                        } else {
                                            Some((
                                                Command::Move {
                                                    dx: dx as i32,
                                                    dy: dy as i32,
                                                },
                                                "Moved selected pixels".to_owned(),
                                            ))
                                        }
                                    }
                                    Tool::Pencil | Tool::Brush | Tool::Eraser => {
                                        if self.paint_points.is_empty() {
                                            self.paint_points.push(start);
                                        }
                                        self.preview_live_stroke();
                                        self.finish_live_stroke();
                                        None
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
                                if self.active_tool == Tool::Select {
                                    ui.painter().rect_filled(
                                        marquee,
                                        0,
                                        egui::Color32::from_white_alpha(24),
                                    );
                                    paint_selection_outline(ui.painter(), marquee, phase);
                                } else if self.active_tool == Tool::EllipseSelect {
                                    let center = marquee.center();
                                    let radius = marquee.size() / 2.0;
                                    let ellipse: Vec<egui::Pos2> = (0..64)
                                        .map(|step| {
                                            let angle = std::f32::consts::TAU * step as f32 / 64.0;
                                            center
                                                + egui::vec2(
                                                    radius.x * angle.cos(),
                                                    radius.y * angle.sin(),
                                                )
                                        })
                                        .collect();
                                    ui.painter().add(egui::Shape::closed_line(
                                        ellipse,
                                        egui::Stroke::new(1.5, egui::Color32::LIGHT_BLUE),
                                    ));
                                } else if self.active_tool == Tool::Lasso
                                    && self.paint_points.len() >= 2
                                {
                                    let path: Vec<egui::Pos2> = self
                                        .paint_points
                                        .iter()
                                        .map(|point| {
                                            rect.min
                                                + egui::vec2(
                                                    (point.x as f32 + 0.5) * zoom,
                                                    (point.y as f32 + 0.5) * zoom,
                                                )
                                        })
                                        .collect();
                                    ui.painter().add(egui::Shape::closed_line(
                                        path,
                                        egui::Stroke::new(1.5, egui::Color32::LIGHT_BLUE),
                                    ));
                                } else if matches!(
                                    self.active_tool,
                                    Tool::Move | Tool::MoveSelection
                                ) {
                                    if let Some(floating) = &self.floating_selection {
                                        let min = rect.min
                                            + egui::vec2(
                                                (floating.floating.bounds.left()
                                                    + floating.offset_x)
                                                    as f32
                                                    * zoom,
                                                (floating.floating.bounds.top() + floating.offset_y)
                                                    as f32
                                                    * zoom,
                                            );
                                        let max = rect.min
                                            + egui::vec2(
                                                (floating.floating.bounds.right()
                                                    + floating.offset_x)
                                                    as f32
                                                    * zoom,
                                                (floating.floating.bounds.bottom()
                                                    + floating.offset_y)
                                                    as f32
                                                    * zoom,
                                            );
                                        let preview = egui::Rect::from_min_max(min, max);
                                        ui.painter().rect_filled(
                                            preview,
                                            0,
                                            egui::Color32::from_white_alpha(20),
                                        );
                                        paint_selection_outline(ui.painter(), preview, phase);
                                    } else if let Some(selection) =
                                        self.session.document().selection()
                                    {
                                        let dx = i64::from(end.x) - i64::from(start.x);
                                        let dy = i64::from(end.y) - i64::from(start.y);
                                        let min = rect.min
                                            + egui::vec2(
                                                (selection.left() + dx) as f32 * zoom,
                                                (selection.top() + dy) as f32 * zoom,
                                            );
                                        let max = rect.min
                                            + egui::vec2(
                                                (selection.right() + dx) as f32 * zoom,
                                                (selection.bottom() + dy) as f32 * zoom,
                                            );
                                        let preview = egui::Rect::from_min_max(min, max);
                                        ui.painter().rect_filled(
                                            preview,
                                            0,
                                            egui::Color32::from_white_alpha(20),
                                        );
                                        paint_selection_outline(ui.painter(), preview, phase);
                                    }
                                } else if self.active_tool == Tool::Shapes {
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
            if let Some((rect, mode)) = pending_selection {
                self.execute(
                    Command::SelectRect { rect, mode },
                    "Rectangle selection updated",
                );
            }
            if let Some((command, message)) = pending_paint {
                self.execute(command, &message);
            }
        });
    }

    fn pick_color_at(&mut self, point: Point) {
        let color = match self.sample_color(point) {
            Ok(color) => color,
            Err(message) => {
                self.status = message;
                return;
            }
        };
        self.fill_color = color;
        self.active_color_slot = ColorSlot::Primary;
        self.hex_input = format_hex_color(color);
        self.hex_error = None;
        let source = match self.picker_sample_source {
            PickerSampleSource::Layer => "active layer",
            PickerSampleSource::Image => "composed image",
        };
        let size = self.picker_sample_size.label();
        if self.picker_click_behavior == PickerClickBehavior::SwitchToPreviousTool
            && let Some(previous) = self.previous_tool_before_picker
        {
            self.active_tool = previous;
            self.picking_color = false;
            self.status = format!(
                "Picked #{:02X}{:02X}{:02X} at ({}, {}) from the {source} ({size}); switched back",
                color.r, color.g, color.b, point.x, point.y
            );
            return;
        }
        // Paint.NET keeps the picker active with Do not switch, so repeated samples work.
        self.picking_color = self.active_tool == Tool::Picker;
        self.status = format!(
            "Picked #{:02X}{:02X}{:02X} at ({}, {}) from the {source} ({size})",
            color.r, color.g, color.b, point.x, point.y
        );
    }

    /// Samples one pixel or a clipped square average from the active layer or the composition.
    fn sample_color(&self, point: Point) -> Result<Bgra, String> {
        let document = self.session.document();
        let side = i64::from(self.picker_sample_size.side());
        let radius = (side - 1) / 2;
        let left = (i64::from(point.x) - radius).max(0);
        let top = (i64::from(point.y) - radius).max(0);
        let right = (i64::from(point.x) + radius + 1).min(i64::from(document.size().width()));
        let bottom = (i64::from(point.y) + radius + 1).min(i64::from(document.size().height()));
        let rect = Rect::new(left, top, right, bottom)
            .map_err(|_| "The picked canvas position is outside the document".to_owned())?;
        let composed;
        let surface: &Surface = match self.picker_sample_source {
            PickerSampleSource::Layer => document.active_layer().surface(),
            PickerSampleSource::Image => {
                composed = self.session.compose().map_err(|error| {
                    format!("Could not compose the canvas for sampling: {error}")
                })?;
                &composed
            }
        };
        average_surface_color(surface, rect)
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
        self.pending_discard = None;
    }

    fn discard_confirmation(&mut self, ui: &mut egui::Ui) {
        let explanation = match self.pending_discard.as_ref() {
            Some(DiscardAction::CloseViewport) => {
                "Closing Paint.R will discard the modified document and its undo history."
            }
            Some(DiscardAction::CloseTab(_)) => {
                "Closing this document will discard its modified content and undo history."
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
            Some(DiscardAction::CloseTab(index)) => self.close_document_tab(index),
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
            || self.resize_dialog.is_some()
            || self.canvas_size_dialog.is_some()
            || self.pending_discard.is_some()
            || self.adjustment_dialog.is_some()
            || context.text_edit_focused()
        {
            return;
        }
        if self.active_tool == Tool::Text
            && (self.session.has_preview() || self.text_origin.is_some())
            && context
                .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
        {
            self.cancel_text_preview();
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
        #[cfg(target_os = "macos")]
        self.poll_native_menu(context);
        self.handle_keyboard_shortcuts(context);
        self.handle_close_request(context);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.refresh_texture(ui.ctx());
        self.refresh_layer_thumbnails(ui.ctx());
        if self.shows_in_window_menu() {
            self.menu_panel(ui);
        }
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
        self.layer_properties_dialog(ui.ctx());
        self.gaussian_blur_dialog(ui.ctx());
        self.adjustment_dialog_window(ui.ctx());
        self.canvas_panel(ui);
        self.path_dialog(ui);
        self.image_size_dialogs(ui.ctx());
        self.discard_confirmation(ui);
        if self.texture_dirty {
            // Mutations happen while widgets run, after this frame's initial texture refresh.
            // Schedule the real composed-surface upload instead of relying on incidental repaints.
            ui.ctx().request_repaint();
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShapeMode {
    Rectangle,
    Ellipse,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Select,
    Lasso,
    EllipseSelect,
    MagicWand,
    MoveSelection,
    Zoom,
    Fill,
    Shapes,
    Pencil,
    Brush,
    Eraser,
    Line,
    Gradient,
    Recolor,
    Move,
    Pan,
    Picker,
    Text,
    CloneStamp,
}

/// What the color picker does after a click samples a color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickerClickBehavior {
    NoToolSwitch,
    SwitchToPreviousTool,
}

/// Which surface the color picker reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickerSampleSource {
    Layer,
    Image,
}

/// Point or square-average sample used by the color picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickerSampleSize {
    Point,
    Average3x3,
    Average5x5,
}

impl PickerSampleSize {
    const fn side(self) -> u32 {
        match self {
            Self::Point => 1,
            Self::Average3x3 => 3,
            Self::Average5x5 => 5,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Point => "1 px",
            Self::Average3x3 => "3×3 average",
            Self::Average5x5 => "5×5 average",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TextPreviewKey {
    origin: Point,
    content: String,
    style: TextStyle,
    color: Bgra,
    layer_id: LayerId,
    selection: Option<Rect>,
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
    CloseViewport,
    CloseTab(usize),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CanvasUnit {
    Pixels,
    Inches,
    Centimeters,
}

impl CanvasUnit {
    const fn suffix(self) -> &'static str {
        match self {
            Self::Pixels => "px",
            Self::Inches => "in",
            Self::Centimeters => "cm",
        }
    }

    fn convert(self, pixels: f64) -> f64 {
        match self {
            Self::Pixels => pixels,
            Self::Inches => pixels / 96.0,
            Self::Centimeters => pixels / 96.0 * 2.54,
        }
    }

    fn format_value(self, pixels: f64) -> String {
        let value = self.convert(pixels);
        if self == Self::Pixels {
            value.round().to_string()
        } else {
            format!("{value:.2}")
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SizeMode {
    Absolute,
    Percentage,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EditedDimension {
    Width,
    Height,
}

struct SizeFields {
    original: Size,
    mode: SizeMode,
    width: String,
    height: String,
    percent: String,
    constrain: bool,
    last_edited: EditedDimension,
}

impl SizeFields {
    fn new(original: Size) -> Self {
        Self {
            original,
            mode: SizeMode::Absolute,
            width: original.width().to_string(),
            height: original.height().to_string(),
            percent: "100".to_owned(),
            constrain: false,
            last_edited: EditedDimension::Width,
        }
    }

    fn aspect(&self) -> f64 {
        f64::from(self.original.width()) / f64::from(self.original.height())
    }

    fn sync_from_percent(&mut self) {
        if let Ok(percent) = parse_percent(&self.percent) {
            self.width = scaled_dimension(self.original.width(), percent).to_string();
            self.height = scaled_dimension(self.original.height(), percent).to_string();
        }
    }

    fn sync_from_width(&mut self) {
        if !self.constrain {
            return;
        }
        if let Ok(width) = parse_dimension(&self.width, "New width") {
            let height = (f64::from(width) / self.aspect()).round().max(1.0) as u32;
            self.height = height.to_string();
        }
    }

    fn sync_from_height(&mut self) {
        if !self.constrain {
            return;
        }
        if let Ok(height) = parse_dimension(&self.height, "New height") {
            let width = (f64::from(height) * self.aspect()).round().max(1.0) as u32;
            self.width = width.to_string();
        }
    }

    fn sync_percent_from_size(&mut self) {
        if let (Ok(width), Ok(_height)) = (
            parse_dimension(&self.width, "New width"),
            parse_dimension(&self.height, "New height"),
        ) {
            let percent = f64::from(width) / f64::from(self.original.width()) * 100.0;
            self.percent = format_percent(percent);
        }
    }

    fn validated_size(&self, limits: ResourceLimits) -> Result<Size, String> {
        let (width, height) = match self.mode {
            SizeMode::Absolute => (
                parse_dimension(&self.width, "New width")?,
                parse_dimension(&self.height, "New height")?,
            ),
            SizeMode::Percentage => {
                let percent = parse_percent(&self.percent)?;
                (
                    scaled_dimension(self.original.width(), percent),
                    scaled_dimension(self.original.height(), percent),
                )
            }
        };
        let size =
            Size::new(width, height).map_err(|_| "Width and height must be at least 1 pixel")?;
        if size.width() > limits.max_width || size.height() > limits.max_height {
            return Err(format!(
                "New size {}×{} exceeds the maximum {}×{}",
                size.width(),
                size.height(),
                limits.max_width,
                limits.max_height
            ));
        }
        let pixels = u64::from(size.width()) * u64::from(size.height());
        if pixels > limits.max_pixels {
            return Err(format!(
                "New size needs {pixels} pixels; the maximum is {}",
                limits.max_pixels
            ));
        }
        Ok(size)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum AdjustmentDialog {
    BrightnessContrast {
        brightness: i32,
        contrast: i32,
    },
    HueSaturation {
        hue: i32,
        saturation: i32,
        lightness: i32,
    },
    AutoLevel,
}

impl AdjustmentDialog {
    const fn title(self) -> &'static str {
        match self {
            Self::BrightnessContrast { .. } => "Brightness / Contrast",
            Self::HueSaturation { .. } => "Hue / Saturation",
            Self::AutoLevel => "Auto-Level",
        }
    }

    const fn effect(self) -> Effect {
        match self {
            Self::BrightnessContrast {
                brightness,
                contrast,
            } => Effect::BrightnessContrast {
                brightness,
                contrast,
            },
            Self::HueSaturation {
                hue,
                saturation,
                lightness,
            } => Effect::HueSaturation {
                hue,
                saturation,
                lightness,
            },
            Self::AutoLevel => Effect::AutoLevel,
        }
    }

    fn show(&mut self, ui: &mut egui::Ui) {
        match self {
            Self::BrightnessContrast {
                brightness,
                contrast,
            } => {
                ui.label("Brighten or darken the selection and expand or contract its tones.");
                ui.separator();
                ui.add(
                    egui::Slider::new(brightness, MIN_ADJUSTMENT_VALUE..=MAX_ADJUSTMENT_VALUE)
                        .text("Brightness"),
                );
                ui.add(
                    egui::Slider::new(contrast, MIN_ADJUSTMENT_VALUE..=MAX_ADJUSTMENT_VALUE)
                        .text("Contrast"),
                );
            }
            Self::HueSaturation {
                hue,
                saturation,
                lightness,
            } => {
                ui.label("Rotate hue and adjust saturation and lightness in HSL.");
                ui.separator();
                ui.add(egui::Slider::new(hue, MIN_HUE_DELTA..=MAX_HUE_DELTA).text("Hue shift"));
                ui.add(
                    egui::Slider::new(saturation, MIN_ADJUSTMENT_VALUE..=MAX_ADJUSTMENT_VALUE)
                        .text("Saturation amount"),
                );
                ui.add(
                    egui::Slider::new(lightness, MIN_ADJUSTMENT_VALUE..=MAX_ADJUSTMENT_VALUE)
                        .text("Lightness"),
                );
            }
            Self::AutoLevel => {
                ui.label("Stretch each RGB channel from the selected pixels to 0-255.");
                ui.label("A flat selection leaves the image unchanged.");
            }
        }
    }
}

struct ResizeImageDialogState {
    fields: SizeFields,
}

impl ResizeImageDialogState {
    fn new(original: Size) -> Self {
        Self {
            fields: SizeFields::new(original),
        }
    }
}

struct CanvasSizeDialogState {
    fields: SizeFields,
    anchor: CanvasAnchor,
    fill: CanvasFill,
}

impl CanvasSizeDialogState {
    fn new(original: Size) -> Self {
        Self {
            fields: SizeFields::new(original),
            anchor: CanvasAnchor::TopLeft,
            fill: CanvasFill::Transparent,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CanvasFill {
    Transparent,
    White,
    Black,
    Primary,
    Secondary,
}

const CANVAS_FILLS: [(CanvasFill, &str); 5] = [
    (CanvasFill::Transparent, "Fill: transparent"),
    (CanvasFill::White, "Fill: white"),
    (CanvasFill::Black, "Fill: black"),
    (CanvasFill::Primary, "Fill: primary color"),
    (CanvasFill::Secondary, "Fill: secondary color"),
];

const fn canvas_anchor_label(anchor: CanvasAnchor) -> &'static str {
    match anchor {
        CanvasAnchor::TopLeft => "Anchor: top left",
        CanvasAnchor::Top => "Anchor: top",
        CanvasAnchor::TopRight => "Anchor: top right",
        CanvasAnchor::Left => "Anchor: left",
        CanvasAnchor::Middle => "Anchor: middle",
        CanvasAnchor::Right => "Anchor: right",
        CanvasAnchor::BottomLeft => "Anchor: bottom left",
        CanvasAnchor::Bottom => "Anchor: bottom",
        CanvasAnchor::BottomRight => "Anchor: bottom right",
    }
}

fn parse_dimension(text: &str, name: &str) -> Result<u32, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(format!("{name} is required"));
    }
    trimmed
        .parse::<u32>()
        .map_err(|_| format!("{name} must be a whole number of pixels"))
}

fn parse_percent(text: &str) -> Result<f64, String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("Percent is required".to_owned());
    }
    let value = trimmed
        .parse::<f64>()
        .map_err(|_| "Percent must be a number".to_owned())?;
    if !value.is_finite() || value <= 0.0 || value > 2000.0 {
        return Err("Percent must be greater than 0 and at most 2000".to_owned());
    }
    Ok(value)
}

fn scaled_dimension(original: u32, percent: f64) -> u32 {
    (f64::from(original) * percent / 100.0)
        .round()
        .clamp(1.0, f64::from(u32::MAX)) as u32
}

fn format_percent(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    if rounded.fract().abs() < f64::EPSILON {
        format!("{}", rounded as u64)
    } else {
        format!("{rounded:.2}")
    }
}

fn size_fields_ui(
    ui: &mut egui::Ui,
    fields: &mut SizeFields,
    limits: ResourceLimits,
) -> Result<Size, String> {
    ui.label(format!(
        "Original: {} × {} pixels",
        fields.original.width(),
        fields.original.height()
    ));
    let previous_mode = fields.mode;
    ui.horizontal(|ui| {
        ui.radio_value(&mut fields.mode, SizeMode::Absolute, "Absolute size");
        ui.radio_value(&mut fields.mode, SizeMode::Percentage, "Percentage");
    });
    if fields.mode != previous_mode {
        if fields.mode == SizeMode::Percentage {
            fields.sync_percent_from_size();
        } else {
            fields.sync_from_percent();
        }
    }
    match fields.mode {
        SizeMode::Absolute => {
            ui.horizontal(|ui| {
                let label = ui.label("New width");
                let response = ui
                    .add(egui::TextEdit::singleline(&mut fields.width).desired_width(72.0))
                    .labelled_by(label.id);
                if response.changed() {
                    fields.last_edited = EditedDimension::Width;
                    fields.sync_from_width();
                }
                ui.label("pixels");
            });
            ui.horizontal(|ui| {
                let label = ui.label("New height");
                let response = ui
                    .add(egui::TextEdit::singleline(&mut fields.height).desired_width(72.0))
                    .labelled_by(label.id);
                if response.changed() {
                    fields.last_edited = EditedDimension::Height;
                    fields.sync_from_height();
                }
                ui.label("pixels");
            });
        }
        SizeMode::Percentage => {
            ui.horizontal(|ui| {
                let label = ui.label("Percent");
                let response = ui
                    .add(egui::TextEdit::singleline(&mut fields.percent).desired_width(72.0))
                    .labelled_by(label.id);
                if response.changed() {
                    fields.sync_from_percent();
                }
                ui.label("%");
            });
        }
    }
    let constrain = ui.checkbox(&mut fields.constrain, "Maintain aspect ratio");
    if constrain.changed() && fields.constrain {
        match fields.last_edited {
            EditedDimension::Width => fields.sync_from_width(),
            EditedDimension::Height => fields.sync_from_height(),
        }
    }
    fields.validated_size(limits)
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

fn blend_mode_label(mode: paint_document::BlendMode) -> &'static str {
    match mode {
        paint_document::BlendMode::Normal => "Normal",
        paint_document::BlendMode::Multiply => "Multiply",
        paint_document::BlendMode::Screen => "Screen",
        paint_document::BlendMode::Overlay => "Overlay",
    }
}

fn layer_opacity_slider(ui: &mut egui::Ui, value: &mut u8) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(170.0, 20.0), egui::Sense::click_and_drag());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, true, "Layer opacity"));
    let painter = ui.painter();
    painter.rect_filled(rect, 3, ui.visuals().extreme_bg_color);
    let fraction = f32::from(*value) / 255.0;
    painter.rect_filled(
        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width() * fraction, rect.height())),
        3,
        egui::Color32::from_rgb(31, 78, 116),
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        format!("Opacity {value}"),
        egui::FontId::proportional(12.0),
        egui::Color32::WHITE,
    );
    if (response.dragged() || response.clicked() || response.is_pointer_button_down_on())
        && let Some(position) = response.interact_pointer_pos()
    {
        let fraction = ((position.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        *value = (fraction * 255.0).round() as u8;
    }
    if response.has_focus() {
        let (left, right) = ui.input(|input| {
            (
                input.key_pressed(egui::Key::ArrowLeft),
                input.key_pressed(egui::Key::ArrowRight),
            )
        });
        if left {
            *value = value.saturating_sub(1);
        }
        if right {
            *value = value.saturating_add(1);
        }
    }
    response
}

fn window_close_button(ui: &mut egui::Ui, label: &'static str) -> bool {
    ui.add(
        egui::Button::new(egui::RichText::new("×").size(18.0))
            .frame(false)
            .min_size(egui::vec2(20.0, 20.0)),
    )
    .on_hover_text(label)
    .accessible_name(label)
    .clicked()
}

fn history_label(command: &Command) -> Option<String> {
    match command {
        Command::Fill(_) | Command::FloodFill { .. } => Some("Paint Bucket".to_owned()),
        Command::Stroke { label, .. } | Command::StrokeLine { label, .. } => {
            Some((*label).to_owned())
        }
        Command::FillRect { .. } => Some("Rectangle".to_owned()),
        Command::FillEllipse { .. } => Some("Ellipse".to_owned()),
        Command::Gradient { .. } => Some("Gradient".to_owned()),
        Command::Recolor { .. } => Some("Recolor".to_owned()),
        Command::PlaceText { .. } => Some("Text".to_owned()),
        Command::CloneStamp { .. } => Some("Clone Stamp".to_owned()),
        Command::Move { .. } => Some("Move Selected Pixels".to_owned()),
        Command::Invert => Some("Invert Colors".to_owned()),
        Command::ApplyEffect(effect) => Some(effect.history_label()),
        Command::FlipLayerHorizontal => Some("Flip Layer Horizontal".to_owned()),
        Command::FlipLayerVertical => Some("Flip Layer Vertical".to_owned()),
        Command::FlipImageHorizontal => Some("Flip Image Horizontal".to_owned()),
        Command::FlipImageVertical => Some("Flip Image Vertical".to_owned()),
        Command::CropToSelection => Some("Crop to Selection".to_owned()),
        Command::ResizeImage { .. } => Some("Resize Image".to_owned()),
        Command::CanvasSize { .. } => Some("Canvas Size".to_owned()),
        Command::Rotate90Clockwise => Some("Rotate 90° Clockwise".to_owned()),
        Command::Rotate90CounterClockwise => Some("Rotate 90° Counter-Clockwise".to_owned()),
        Command::Rotate180 => Some("Rotate 180°".to_owned()),
        Command::FlattenImage => Some("Flatten Image".to_owned()),
        Command::AddLayer { name, .. } => Some(format!("Add Layer: {name}")),
        Command::RemoveLayer { .. } => Some("Delete Layer".to_owned()),
        Command::DuplicateLayer { .. } => Some("Duplicate Layer".to_owned()),
        Command::MergeLayerDown { .. } => Some("Merge Layer Down".to_owned()),
        Command::MoveLayer { .. } => Some("Move Layer".to_owned()),
        Command::RenameLayer { name, .. } => Some(format!("Rename Layer: {name}")),
        Command::SetLayerVisibility { index, visible } => {
            Some(format!("Layer {index} visibility: {visible}"))
        }
        Command::SetLayerOpacity { index, opacity } => {
            Some(format!("Layer {index} opacity: {opacity}"))
        }
        Command::SetLayerBlendMode { index, mode } => {
            Some(format!("Layer {index} blend: {mode:?}"))
        }
        Command::SelectRect { .. }
        | Command::SelectEllipse { .. }
        | Command::SelectLasso { .. }
        | Command::SelectMagicWand { .. }
        | Command::TranslateSelection { .. }
        | Command::ClearSelection
        | Command::SetActiveLayer { .. }
        | Command::Undo
        | Command::Redo => None,
    }
}

fn disabled_effect_item(ui: &mut egui::Ui, label: &str) {
    ui.add_enabled(false, egui::Button::new(label))
        .on_disabled_hover_text("Not implemented in the Paint.R preview");
}

fn labeled_button(ui: &mut egui::Ui, text: &str, label: &'static str) -> egui::Response {
    let response = ui.button(text);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    response.on_hover_text(label)
}

/// Averages straight-alpha BGRA pixels over `rect` using premultiplied alpha, matching
/// Paint.NET's Color Picker average sample.
fn average_surface_color(surface: &Surface, rect: Rect) -> Result<Bgra, String> {
    let mut count = 0_u64;
    let mut alpha_sum = 0.0_f64;
    let mut red_sum = 0.0_f64;
    let mut green_sum = 0.0_f64;
    let mut blue_sum = 0.0_f64;
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            let point = Point::new(
                u32::try_from(x).map_err(|_| "sample x is outside the document".to_owned())?,
                u32::try_from(y).map_err(|_| "sample y is outside the document".to_owned())?,
            );
            let pixel = *surface
                .pixel(point)
                .map_err(|_| "The picked canvas position is outside the document".to_owned())?;
            let alpha = f64::from(pixel.a);
            alpha_sum += alpha;
            red_sum += f64::from(pixel.r) * alpha;
            green_sum += f64::from(pixel.g) * alpha;
            blue_sum += f64::from(pixel.b) * alpha;
            count += 1;
        }
    }
    if count == 0 {
        return Err("The picked canvas position is outside the document".to_owned());
    }
    if alpha_sum == 0.0 {
        return Ok(Bgra::TRANSPARENT);
    }
    let channel = |sum: f64| (sum / alpha_sum).round().clamp(0.0, 255.0) as u8;
    Ok(Bgra::new(
        channel(blue_sum),
        channel(green_sum),
        channel(red_sum),
        (alpha_sum / count as f64).round().clamp(0.0, 255.0) as u8,
    ))
}

/// One wrapped Tool Options row with a small indent, so grouped tools stay scannable.
fn tool_option_row(ui: &mut egui::Ui, draw: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_wrapped(|ui| {
        ui.add_space(10.0);
        draw(ui);
    });
}

/// A selectable option whose accessible name includes its group, for unambiguous testing.
fn option_selectable(
    ui: &mut egui::Ui,
    selected: bool,
    text: &str,
    accessible: &'static str,
) -> bool {
    let response = ui.selectable_label(selected, text);
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, accessible));
    response.clicked()
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
