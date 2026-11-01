use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use paint_app::{
    AppSession, CanvasAnchor, ClipboardContent, Command, Effect, MagicWandSampling, ResourceLimits,
    SelectionCombineMode,
};
use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_document::{BlendMode, Document, Layer, LayerId};
use paint_raster::{
    BrushOptions, GradientOptions, GradientRepeat, GradientType, LevelsChannel, LevelsSettings,
    LineCap, LineDash, LineStyle, MAX_ADJUSTMENT_VALUE, MAX_BLACK_AND_WHITE_WEIGHT,
    MAX_GAUSSIAN_BLUR_RADIUS, MAX_HUE_DELTA, MAX_LEVELS_GAMMA_HUNDREDTHS, MAX_MOTION_BLUR_DISTANCE,
    MAX_NOISE_INTENSITY, MAX_POSTERIZE_LEVELS, MAX_SHARPEN_AMOUNT, MIN_ADJUSTMENT_VALUE,
    MIN_HUE_DELTA, MIN_LEVELS_GAMMA_HUNDREDTHS, MIN_POSTERIZE_LEVELS, ResampleMode, ShapeStyle,
    TextAlignment, TextFontFamily, TextStyle,
};

use crate::checkerboard::paint_checkerboard;
use crate::colors::{
    ColorSlot, PRESET_COLORS, color_slots_widget, color_swatch, color_wheel, compact_color_slider,
    format_hex_color, hsv_from_color, hsv_gradient_slider, mix_bgra,
};
use crate::icons::{
    Icon, IconTextures, VectorIcon, icon_button, icon_button_at, menu_button, panel_button,
    tool_button, tool_button_with_tooltip, vector_icon_button, vector_icon_button_sized,
    vector_toggle_button, visibility_button_at,
};
use crate::layers::{LayerThumbnail, layer_thumbnail_texture, surface_thumbnail_texture};
#[cfg(target_os = "macos")]
use crate::menu::{NativeCommand, NativeMenu, NativeMenuState};
use crate::rulers::{RULER_THICKNESS, RulerLayout};
use crate::selection::{
    paint_selection, paint_selection_boundary, paint_selection_outline,
    paint_selection_rect_boundary,
};

const DEFAULT_WIDTH: u32 = 800;
const DEFAULT_HEIGHT: u32 = 600;
const DEFAULT_MOTION_BLUR_ANGLE: i32 = 25;
const DEFAULT_MOTION_BLUR_DISTANCE: u32 = 10;
const DEFAULT_SHARPEN_AMOUNT: u32 = 2;
const DEFAULT_NOISE_INTENSITY: u32 = 64;
const DEFAULT_NOISE_SEED: u64 = 1;
const DEFAULT_POSTERIZE_LEVELS: u8 = 16;
const DEFAULT_THRESHOLD_LEVEL: u8 = 128;
const DEFAULT_BLACK_AND_WHITE_RED: u8 = 30;
const DEFAULT_BLACK_AND_WHITE_GREEN: u8 = 59;
const DEFAULT_BLACK_AND_WHITE_BLUE: u8 = 11;
const MAX_PREVIEW_FILE_BYTES: u64 = 16 * 1024 * 1024;
const MAX_PREVIEW_DECODED_BYTES: usize = 64 * 1024 * 1024;
const PREVIEW_WIDTH: f32 = 180.0;
const PREVIEW_HEIGHT: f32 = 140.0;
const MAX_BROWSER_ENTRIES: usize = 200;
/// Accessible name of the canvas inline text editor.
const INLINE_TEXT_EDITOR_LABEL: &str = "Inline text editor";
/// Named egui families that match the embedded Roboto faces used by the rasterizer,
/// so the caret and selection line up with the previewed glyphs.
const TEXT_FONT_SANS: &str = "paint-r-text-sans";
const TEXT_FONT_SANS_ITALIC: &str = "paint-r-text-sans-italic";
const TEXT_FONT_MONO: &str = "paint-r-text-mono";
const TEXT_FONT_MONO_ITALIC: &str = "paint-r-text-mono-italic";

fn inline_text_editor_id() -> egui::Id {
    egui::Id::unique("paint-r-inline-text-editor")
}

fn text_content_field_id() -> egui::Id {
    egui::Id::unique("paint-r-text-content-field")
}

/// Paint.NET LayersStrip metrics: 50 px thumbnail + 3 × 4 px padding = 62 px rows.
pub const LAYER_ITEM_HEIGHT: f32 = 62.0;
const LAYER_THUMBNAIL_SIZE: f32 = 50.0;
const LAYER_ITEM_PADDING: f32 = 4.0;
const LAYER_VISIBILITY_HIT_WIDTH: f32 = 22.0;

/// Floating window titles and default geometry, taken from the Paint.NET forms.
pub const TOOLS_PANEL_TITLE: &str = "Tools";
pub const COLORS_PANEL_TITLE: &str = "Colors";
pub const LAYERS_PANEL_TITLE: &str = "Layers";
pub const HISTORY_PANEL_TITLE: &str = "History";
/// ToolsForm uses a 50 DIP tool grid; the egui frame adds 9 px on each side.
pub const TOOLS_PANEL_WIDTH: f32 = 68.0;
/// LayersForm.GetClientSizeDips() and the HistoryForm client size.
pub const LAYERS_PANEL_SIZE: [f32; 2] = [196.0, 219.0];
pub const HISTORY_PANEL_SIZE: [f32; 2] = [196.0, 219.0];
/// ColorsForm stores `moreSize` from the designer and derives the less-mode
/// width from `193 + 16` DIPs. Both modes keep the same client height so the
/// More/Less control does not move when the form expands.
pub const COLORS_PANEL_LESS_SIZE: [f32; 2] = [209.0, 306.0];
pub const COLORS_PANEL_MORE_SIZE: [f32; 2] = [386.0, 306.0];
const PANEL_SCREEN_MARGIN: f32 = 12.0;
const PANEL_TOP_OFFSET: f32 = 200.0;
const STATUS_BAR_HEIGHT: f32 = 24.0;

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
    path: Option<PathBuf>,
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
    move_drag_origin: Option<(f64, f64)>,
    move_drag_current: Option<(f64, f64)>,
    paint_points: Vec<Point>,
    floating_selection: Option<FloatingSelectionState>,
    next_layer_number: usize,
}

struct FloatingSelectionState {
    floating: paint_app::FloatingSelection,
    texture: egui::TextureHandle,
    offset_x: f64,
    offset_y: f64,
    base_x: f64,
    base_y: f64,
    pasted: bool,
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
    clone_spacing: u8,
    clone_source: Option<Point>,
    clone_offset: Option<(i64, i64)>,
    clone_alt_down: bool,
    text_content: String,
    text_font_family: TextFontFamily,
    text_size: u32,
    text_bold: bool,
    text_italic: bool,
    text_antialias: bool,
    text_alignment: TextAlignment,
    text_blend: BlendMode,
    text_origin: Option<Point>,
    text_applied_key: Option<TextPreviewKey>,
    text_editor_focus_requested: bool,
    text_editor_focus_release: bool,
    recolor_tolerance: u8,
    recolor_sampling: MagicWandSampling,
    fill_tolerance: u8,
    fill_global: bool,
    fill_sampling: MagicWandSampling,
    fill_blend: BlendMode,
    gradient_blend: BlendMode,
    gradient_alpha_only: bool,
    selection_combine_mode: SelectionCombineMode,
    selection_fields_open: bool,
    layer_move_base: (f64, f64),
    layer_move_offset: (f64, f64),
    move_drag_origin: Option<(f64, f64)>,
    move_drag_current: Option<(f64, f64)>,
    resample_mode: ResampleMode,
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
    show_rulers: bool,
    ruler_layout: Option<RulerLayout>,
    unit: CanvasUnit,
    layer_thumbnails: HashMap<LayerId, LayerThumbnail>,
    floating_selection: Option<FloatingSelectionState>,
    floating_generation: u64,
    clipboard: Option<ClipboardContent>,
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
    effect_dialog: Option<EffectDialog>,
    effect_dialog_previewed: Option<Effect>,
    document_title: String,
    document_path: Option<PathBuf>,
    document_thumbnail: Option<egui::TextureHandle>,
    tabs: Vec<Option<DocumentTabState>>,
    active_tab: usize,
    next_layer_number: usize,
    status: String,
    new_image_dialog: Option<NewImageDialogState>,
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
            clone_spacing: 15,
            clone_source: None,
            clone_offset: None,
            clone_alt_down: false,
            text_content: String::new(),
            text_font_family: TextFontFamily::Sans,
            text_size: 48,
            text_bold: false,
            text_italic: false,
            text_antialias: true,
            text_alignment: TextAlignment::Left,
            text_blend: BlendMode::Normal,
            text_origin: None,
            text_applied_key: None,
            text_editor_focus_requested: false,
            text_editor_focus_release: false,
            recolor_tolerance: 50,
            recolor_sampling: MagicWandSampling::ActiveLayer,
            fill_tolerance: 50,
            fill_global: false,
            fill_sampling: MagicWandSampling::ActiveLayer,
            fill_blend: BlendMode::Normal,
            gradient_blend: BlendMode::Normal,
            gradient_alpha_only: false,
            selection_combine_mode: SelectionCombineMode::Replace,
            selection_fields_open: false,
            layer_move_base: (0.0, 0.0),
            layer_move_offset: (0.0, 0.0),
            move_drag_origin: None,
            move_drag_current: None,
            resample_mode: ResampleMode::Bicubic,
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
            show_rulers: false,
            ruler_layout: None,
            unit: CanvasUnit::Pixels,
            layer_thumbnails: HashMap::new(),
            floating_selection: None,
            floating_generation: 0,
            clipboard: None,
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
            effect_dialog: None,
            effect_dialog_previewed: None,
            document_title: "New Image".to_owned(),
            document_path: None,
            document_thumbnail: None,
            tabs: vec![None],
            active_tab: 0,
            next_layer_number: 1,
            status: "New 800×600 opaque-white document".to_owned(),
            new_image_dialog: None,
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
    pub const fn fill_sampling(&self) -> MagicWandSampling {
        self.fill_sampling
    }

    #[doc(hidden)]
    pub const fn fill_blend(&self) -> BlendMode {
        self.fill_blend
    }

    #[doc(hidden)]
    pub const fn gradient_blend(&self) -> BlendMode {
        self.gradient_blend
    }

    #[doc(hidden)]
    pub const fn gradient_alpha_only(&self) -> bool {
        self.gradient_alpha_only
    }

    #[doc(hidden)]
    pub const fn recolor_sampling(&self) -> MagicWandSampling {
        self.recolor_sampling
    }

    #[doc(hidden)]
    pub const fn text_alignment(&self) -> TextAlignment {
        self.text_alignment
    }

    /// Current shared text buffer, kept in sync between the canvas editor and Tool Options.
    #[doc(hidden)]
    pub fn text_content(&self) -> &str {
        &self.text_content
    }

    /// Canvas origin of the in-progress text placement, or `None` when no editor is open.
    #[doc(hidden)]
    pub const fn text_origin(&self) -> Option<Point> {
        self.text_origin
    }

    #[doc(hidden)]
    pub const fn clone_spacing(&self) -> u8 {
        self.clone_spacing
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
    pub const fn rulers_enabled(&self) -> bool {
        self.show_rulers
    }

    #[doc(hidden)]
    pub const fn ruler_layout(&self) -> Option<&RulerLayout> {
        self.ruler_layout.as_ref()
    }

    #[doc(hidden)]
    pub fn document_titles(&self) -> Vec<String> {
        (0..self.tabs.len())
            .map(|index| self.raw_tab_title(index))
            .collect()
    }

    /// Display titles include the dirty marker shown in each tab's visible and accessible label.
    #[doc(hidden)]
    pub fn document_display_titles(&self) -> Vec<String> {
        (0..self.tabs.len())
            .map(|index| self.tab_display_title(index))
            .collect()
    }

    #[doc(hidden)]
    pub fn active_document_path(&self) -> Option<&Path> {
        self.document_path.as_deref()
    }

    #[doc(hidden)]
    pub fn document_paths(&self) -> Vec<Option<PathBuf>> {
        (0..self.tabs.len())
            .map(|index| {
                if index == self.active_tab {
                    self.document_path.clone()
                } else {
                    self.tabs[index].as_ref().and_then(|tab| tab.path.clone())
                }
            })
            .collect()
    }

    fn raw_tab_title(&self, index: usize) -> String {
        if index == self.active_tab {
            self.document_title.clone()
        } else {
            self.tabs[index]
                .as_ref()
                .map_or_else(|| "Untitled".to_owned(), |tab| tab.title.clone())
        }
    }

    fn tab_is_dirty(&self, index: usize) -> bool {
        if index == self.active_tab {
            self.session.dirty()
        } else {
            self.tabs[index]
                .as_ref()
                .is_some_and(|tab| tab.session.dirty())
        }
    }

    fn tab_display_title(&self, index: usize) -> String {
        format!(
            "{}{}",
            self.raw_tab_title(index),
            if self.tab_is_dirty(index) { " •" } else { "" }
        )
    }

    fn can_save(&self) -> bool {
        self.document_path.is_some() && self.session.dirty()
    }

    #[doc(hidden)]
    pub fn history_entries(&self) -> &[String] {
        &self.history_entries
    }

    /// Bounds of the last Edit → Copy / Cut content, or `None` when the clipboard is empty.
    #[doc(hidden)]
    pub fn clipboard_bounds(&self) -> Option<Rect> {
        self.clipboard.as_ref().map(ClipboardContent::bounds)
    }

    /// Canvas bounds of the floating selection that Paste or Move is previewing.
    #[doc(hidden)]
    pub fn floating_selection_bounds(&self) -> Option<Rect> {
        self.floating_selection
            .as_ref()
            .map(|floating| floating.floating.bounds)
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

    /// Reference toolbox order, labels, hotkeys, and tooltip hints.
    ///
    /// Kept public so parity tests assert the exact Paint.NET strip without scraping hover
    /// rendering internals.
    pub fn toolbox_entries() -> Vec<ToolboxEntry> {
        TOOLBOX.iter().map(|spec| spec.entry).collect()
    }

    /// Accessible label of the currently active tool.
    pub fn active_tool_label(&self) -> &'static str {
        TOOLBOX
            .iter()
            .find(|spec| spec.tool == self.active_tool)
            .map_or("", |spec| spec.entry.label)
    }

    /// Reference Move sampling mode currently selected in Tool Options.
    pub fn move_resample_mode(&self) -> ResampleMode {
        self.resample_mode
    }

    /// Display label of the active Move sampling mode.
    pub fn move_resample_mode_label(&self) -> &'static str {
        resample_label(self.resample_mode)
    }

    #[doc(hidden)]
    pub const fn close_approved(&self) -> bool {
        self.close_approved
    }

    fn take_active_tab(&mut self) -> DocumentTabState {
        DocumentTabState {
            session: std::mem::replace(&mut self.session, default_session()),
            title: std::mem::take(&mut self.document_title),
            path: self.document_path.take(),
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
            move_drag_origin: self.move_drag_origin.take(),
            move_drag_current: self.move_drag_current.take(),
            paint_points: std::mem::take(&mut self.paint_points),
            floating_selection: self.floating_selection.take(),
            next_layer_number: self.next_layer_number,
        }
    }

    fn load_active_tab(&mut self, state: DocumentTabState) {
        self.session = state.session;
        self.document_title = state.title;
        self.document_path = state.path;
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
        self.move_drag_origin = state.move_drag_origin;
        self.move_drag_current = state.move_drag_current;
        self.paint_points = state.paint_points;
        self.floating_selection = state.floating_selection;
        self.next_layer_number = state.next_layer_number;
        self.effect_dialog = None;
        self.effect_dialog_previewed = None;
    }

    fn store_active_tab(&mut self) {
        self.close_effect_preview();
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
        self.close_effect_preview();
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

    fn has_dirty_documents(&self) -> bool {
        self.session.dirty() || self.tabs.iter().flatten().any(|tab| tab.session.dirty())
    }

    fn new_document(&mut self) {
        self.new_document_with_session(
            default_session(),
            "New Image".to_owned(),
            "New 800×600 opaque-white document".to_owned(),
        );
    }

    /// Installs a fresh session in the active tab slot and resets all per-document UI state.
    fn new_document_with_session(&mut self, session: AppSession, title: String, status: String) {
        self.close_effect_preview();
        self.session = session;
        self.selection_input = SelectionInput::default();
        self.next_layer_number = 1;
        self.path_dialog = None;
        self.new_image_dialog = None;
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
        self.document_title = title;
        self.document_path = None;
        self.document_thumbnail = None;
        self.status = status;
        self.reset_layer_interaction_state();
    }

    /// Creates a new tab from the New Image dialog using the chosen size and background.
    fn create_new_document(&mut self, size: Size, background: Bgra) {
        self.finalize_floating_selection();
        self.store_active_tab();
        self.tabs.push(None);
        self.active_tab = self.tabs.len() - 1;
        let title = format!("New Image {}", self.active_tab + 1);
        let status = format!(
            "New {}×{} {} document",
            size.width(),
            size.height(),
            background_label(background)
        );
        let session = AppSession::new(
            Document::new_with_background(size, background)
                .expect("validated New Image size and background"),
        );
        self.new_document_with_session(session, title, status);
    }

    fn show_new_image_dialog(&mut self) {
        self.finalize_floating_selection();
        self.new_image_dialog = Some(NewImageDialogState::new());
    }

    fn show_path_dialog(&mut self, kind: PathDialogKind) {
        self.finalize_floating_selection();
        self.path_dialog = Some(PathDialog::new(kind));
    }

    fn show_save_as_dialog(&mut self) {
        self.finish_active_preview();
        self.finalize_floating_selection();
        let path = self
            .document_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        self.path_dialog = Some(PathDialog::with_path(PathDialogKind::SaveAs, path));
    }

    /// Saves the active document to its known path. Returns whether the file was written.
    fn save_active_document(&mut self) -> bool {
        let Some(path) = self.document_path.clone() else {
            return false;
        };
        self.finish_active_preview();
        self.finalize_floating_selection();
        if !self.session.dirty() {
            self.status = "No changes to save".to_owned();
            return false;
        }
        match self.session.save_png_atomic(&path) {
            Ok(()) => {
                self.texture_dirty = true;
                self.status = format!("Saved PNG: {}", path.display());
                true
            }
            Err(error) => {
                self.status = format!("Save failed: {error}");
                false
            }
        }
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
        self.show_new_image_dialog();
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
                self.document_title = file_title(&path_buffer);
                self.document_path = Some(path_buffer.clone());
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
        if self.effect_dialog.take().is_some() {
            self.cancel_pending_effect();
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
            alignment: self.text_alignment,
            blend: self.text_blend,
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
        self.text_editor_focus_requested = false;
        self.text_editor_focus_release = true;
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
        self.text_editor_focus_requested = false;
        self.text_editor_focus_release = true;
        self.paint_points.clear();
        self.texture_dirty = true;
        self.status = "Canceled the text preview".to_owned();
    }

    /// Consumes Enter/Escape while the canvas editor or the compact Text content field has
    /// focus. Enter commits the placement; Shift+Enter reaches the editor as a newline and
    /// Escape restores the preview snapshot. Everything else stays inside the text widget so
    /// Cmd+C/Cmd+V, Select All, and Backspace never reach the image commands.
    fn handle_inline_text_keys(&mut self, context: &egui::Context) {
        if self.active_tool != Tool::Text || self.text_origin.is_none() {
            return;
        }
        let focused = context.memory(|memory| {
            memory.has_focus(inline_text_editor_id()) || memory.has_focus(text_content_field_id())
        });
        if !focused {
            return;
        }
        let shift_enter = context.input(|input| {
            input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key {
                        key: egui::Key::Enter,
                        pressed: true,
                        modifiers,
                        ..
                    } if modifiers.shift
                )
            })
        });
        if !shift_enter
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            self.commit_text_preview();
            return;
        }
        if context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            self.cancel_text_preview();
        }
    }

    /// Family matching the rasterizer's embedded Roboto face for caret and selection metrics.
    fn inline_text_font_family(&self) -> egui::FontFamily {
        let name = match (self.text_font_family, self.text_italic) {
            (TextFontFamily::Sans, false) => TEXT_FONT_SANS,
            (TextFontFamily::Sans, true) => TEXT_FONT_SANS_ITALIC,
            (TextFontFamily::Mono, false) => TEXT_FONT_MONO,
            (TextFontFamily::Mono, true) => TEXT_FONT_MONO_ITALIC,
        };
        egui::FontFamily::Name(name.into())
    }

    /// Draws the transparent, canvas-anchored editor that owns typing, the caret, and text
    /// selection. The visible glyphs come from the existing raster preview underneath it.
    fn inline_text_editor(&mut self, ui: &mut egui::Ui, canvas: egui::Rect, zoom: f32) {
        if self.active_tool != Tool::Text {
            return;
        }
        let Some(origin) = self.text_origin else {
            return;
        };
        let origin = egui::pos2(
            canvas.min.x + origin.x as f32 * zoom,
            canvas.min.y + origin.y as f32 * zoom,
        );
        let font_size = (self.text_size as f32 * zoom).clamp(4.0, 2048.0);
        let font_id = egui::FontId::new(font_size, self.inline_text_font_family());
        let line_height =
            ui.fonts_mut(|fonts| fonts.row_height(&font_id)) + ui.spacing().extra_text_line_spacing;
        let mut content_width = 0.0_f32;
        let mut line_count = 1usize;
        for (index, line) in self.text_content.split('\n').enumerate() {
            if index > 0 {
                line_count += 1;
            }
            let width = ui.fonts_mut(|fonts| {
                fonts
                    .layout_no_wrap(line.to_owned(), font_id.clone(), egui::Color32::WHITE)
                    .size()
                    .x
            });
            content_width = content_width.max(width);
        }
        let padding = 8.0_f32;
        let width = (content_width + padding).max(font_size * 0.6);
        let height = (line_height * line_count as f32 + 2.0).max(line_height);
        let (left, align) = match self.text_alignment {
            TextAlignment::Left => (origin.x, egui::Align2::LEFT_TOP),
            TextAlignment::Center => (origin.x - width / 2.0, egui::Align2::CENTER_TOP),
            TextAlignment::Right => (origin.x - width, egui::Align2::RIGHT_TOP),
        };
        let desired = egui::Rect::from_min_size(
            egui::pos2(left, origin.y),
            egui::vec2(width.max(24.0), height),
        );
        // Keep the editor on-screen while scrolling, but never let it cover panes outside
        // the canvas viewport.
        let editor_rect = desired.intersect(canvas.intersect(ui.clip_rect()));
        if editor_rect.width() < 4.0 || editor_rect.height() < 4.0 {
            return;
        }
        let editor = egui::TextEdit::multiline(&mut self.text_content)
            .id(inline_text_editor_id())
            .font(font_id)
            .text_color(egui::Color32::TRANSPARENT)
            .background_color(egui::Color32::TRANSPARENT)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .desired_width(editor_rect.width())
            .desired_rows(1)
            .align(align)
            .lock_focus(true);
        let response = ui.put(editor_rect, editor);
        if self.text_editor_focus_requested {
            response.request_focus();
            self.text_editor_focus_requested = false;
        }
        ui.ctx()
            .accesskit_node_builder(inline_text_editor_id(), |node| {
                node.set_label(INLINE_TEXT_EDITOR_LABEL);
            });
        if response.changed() {
            self.sync_text_preview();
            self.texture_dirty = true;
        }
    }

    fn text_tool_options(&mut self, ui: &mut egui::Ui) {
        tool_option_row(ui, |ui| {
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
        });
        tool_option_row(ui, |ui| {
            ui.label("Alignment");
            for (text, alignment, accessible) in [
                ("Left", TextAlignment::Left, "Align left"),
                ("Center", TextAlignment::Center, "Align center"),
                ("Right", TextAlignment::Right, "Align right"),
            ] {
                if option_selectable(ui, self.text_alignment == alignment, text, accessible) {
                    self.text_alignment = alignment;
                }
            }
            ui.separator();
            ui.label("Blend Mode");
            for mode in blend_modes() {
                if tool_blend_mode_option(ui, self.text_blend, mode) {
                    self.text_blend = mode;
                }
            }
        });
        tool_option_row(ui, |ui| {
            let color = self.fill_color;
            ui.colored_label(
                egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a),
                "Primary color",
            );
        });
        tool_option_row(ui, |ui| {
            let content_label = ui.label("Text content");
            ui.add(
                egui::TextEdit::multiline(&mut self.text_content)
                    .id(text_content_field_id())
                    .desired_rows(1)
                    .desired_width(200.0)
                    .hint_text("Click the canvas and type"),
            )
            .labelled_by(content_label.id);
            ui.label("Enter places · Shift+Enter adds a line · Esc cancels");
            if ui
                .add_enabled(self.session.has_preview(), egui::Button::new("Place text"))
                .clicked()
            {
                self.commit_text_preview();
            }
            if ui.button("Cancel text").clicked() {
                self.cancel_text_preview();
            }
        });
        self.sync_text_preview();
    }

    /// The source crosshair follows the brush by the fixed stroke offset while stamping.
    #[doc(hidden)]
    pub fn clone_marker_point(&self) -> Option<(i64, i64)> {
        let source = self.clone_source?;
        let (dx, dy) = match (self.drag_start, self.drag_current) {
            (Some(start), Some(current)) => (
                i64::from(current.x) - i64::from(start.x),
                i64::from(current.y) - i64::from(start.y),
            ),
            _ => (0, 0),
        };
        Some((i64::from(source.x) + dx, i64::from(source.y) + dy))
    }

    fn clone_tool_options(&mut self, ui: &mut egui::Ui) {
        tool_option_row(ui, |ui| {
            let width_label = ui.label("Width");
            ui.add(
                egui::DragValue::new(&mut self.brush_size)
                    .range(1..=64)
                    .suffix(" px"),
            )
            .labelled_by(width_label.id);
            let hardness_label = ui.label("Hardness");
            ui.add(
                egui::DragValue::new(&mut self.clone_hardness)
                    .range(0..=100)
                    .suffix("%"),
            )
            .labelled_by(hardness_label.id);
            let spacing_label = ui.label("Spacing");
            ui.add(
                egui::DragValue::new(&mut self.clone_spacing)
                    .range(1..=100)
                    .suffix("%"),
            )
            .labelled_by(spacing_label.id);
        });
        tool_option_row(ui, |ui| {
            if let Some(source) = self.clone_source {
                ui.label(format!("Source: ({}, {})", source.x, source.y));
                if let Some((offset_x, offset_y)) = self.clone_offset {
                    ui.label(format!("Offset: ({offset_x}, {offset_y})"));
                }
            } else {
                ui.label("Alt-click to set source");
            }
            ui.add_enabled_ui(self.clone_source.is_some(), |ui| {
                if ui.button("Clear clone source").clicked() {
                    self.clone_source = None;
                    self.clone_offset = None;
                }
            });
        });
    }

    fn preview_live_translate(&mut self) {
        let (Some(origin), Some(current)) = (self.move_drag_origin, self.move_drag_current) else {
            return;
        };
        let dx = self.layer_move_base.0 + (current.0 - origin.0);
        let dy = self.layer_move_base.1 + (current.1 - origin.1);
        self.layer_move_offset = (dx, dy);
        match self
            .session
            .preview_translate_resampled(dx, dy, self.resample_mode)
        {
            Ok(_) => self.texture_dirty = true,
            Err(error) => self.status = format!("Move preview failed: {error}"),
        }
    }

    fn preview_live_gradient(&mut self) {
        let (Some(start), Some(end)) = (self.drag_start, self.drag_current) else {
            return;
        };
        match self.session.preview_gradient(
            start,
            end,
            self.fill_color,
            self.secondary_color,
            GradientOptions::new(self.gradient_type, self.gradient_repeat)
                .with_alpha_only(self.gradient_alpha_only)
                .with_blend(self.gradient_blend),
        ) {
            Ok(_) => self.texture_dirty = true,
            Err(error) => self.status = format!("Gradient preview failed: {error}"),
        }
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
            self.clone_spacing,
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
            Tool::Gradient => {
                self.session.cancel_preview();
                self.texture_dirty = true;
            }
            Tool::Move => self.finish_layer_move_preview(),
            _ => {}
        }
    }

    fn reset_transient_tool_state(&mut self) {
        self.text_origin = None;
        self.text_applied_key = None;
        self.text_editor_focus_requested = false;
        self.text_editor_focus_release = true;
        self.clone_source = None;
        self.clone_offset = None;
        self.clone_alt_down = false;
        self.layer_move_base = (0.0, 0.0);
        self.layer_move_offset = (0.0, 0.0);
        self.move_drag_origin = None;
        self.move_drag_current = None;
        if self.session.has_preview() {
            self.session.cancel_preview();
            self.texture_dirty = true;
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

    fn has_selection(&self) -> bool {
        self.session
            .document()
            .selection_model()
            .is_some_and(|selection| !selection.is_empty())
    }

    fn publish_clipboard(&mut self, context: &egui::Context, content: ClipboardContent) {
        if let Some(image) = color_image_from_surface(content.surface()) {
            context.copy_image(image);
        }
        self.clipboard = Some(content);
    }

    fn copy_to_clipboard(&mut self, context: &egui::Context) -> bool {
        self.finish_active_preview();
        self.finalize_floating_selection();
        let selected = self.has_selection();
        match self.session.copy_selection() {
            Ok(Some(content)) => {
                self.publish_clipboard(context, content);
                self.status = if selected {
                    "Copied the selected pixels".to_owned()
                } else {
                    "Copied the whole active layer".to_owned()
                };
                true
            }
            Ok(None) => {
                self.status =
                    "Could not copy: the selection does not overlap the canvas".to_owned();
                false
            }
            Err(error) => {
                self.status = format!("Copy failed: {error}");
                false
            }
        }
    }

    fn cut_to_clipboard(&mut self, context: &egui::Context) -> bool {
        self.finish_active_preview();
        self.finalize_floating_selection();
        if !self.has_selection() {
            self.status = "Cut requires a selection; Copy can copy the whole layer".to_owned();
            return false;
        }
        match self.session.cut_selection() {
            Ok(Some(content)) => {
                self.publish_clipboard(context, content);
                self.history_entries.push("Cut".to_owned());
                self.redo_entries.clear();
                self.texture_dirty = true;
                self.status = "Cut the selected pixels and cleared the selection".to_owned();
                true
            }
            Ok(None) => {
                self.status = "Could not cut: the selection does not overlap the canvas".to_owned();
                false
            }
            Err(error) => {
                self.status = format!("Cut failed: {error}");
                false
            }
        }
    }

    fn clipboard_content(
        &self,
        context: &egui::Context,
    ) -> Result<Option<ClipboardContent>, String> {
        if let Some(content) = self.clipboard.clone() {
            return Ok(Some(content));
        }
        let Some(surface) = external_clipboard_surface(context) else {
            return Ok(None);
        };
        let bounds = Rect::from_size(surface.size());
        ClipboardContent::new(surface, bounds)
            .map(Some)
            .map_err(|error| format!("Could not import the clipboard image: {error}"))
    }

    /// Installs clipboard content as a floating paste over the current active layer.
    fn begin_floating_paste(&mut self, context: &egui::Context, content: ClipboardContent) -> bool {
        let (dest_x, dest_y) = content.destination(self.session.document().size());
        let selection = match content.selection_at(dest_x, dest_y) {
            Ok(selection) => selection,
            Err(error) => {
                self.status = format!("Could not prepare the paste: {error}");
                return false;
            }
        };
        if let Err(error) = self.session.set_selection_shape(selection) {
            self.status = format!("Could not prepare the paste selection: {error}");
            return false;
        }
        self.floating_generation += 1;
        let surface = content.surface().clone();
        let bounds = content.bounds();
        let texture = surface_thumbnail_texture(
            context,
            format!("paint-r-floating-{}", self.floating_generation),
            &surface,
            surface.width() as f32,
            surface.height() as f32,
        );
        let offset_x = (dest_x - bounds.left()) as f64;
        let offset_y = (dest_y - bounds.top()) as f64;
        self.clipboard = Some(content);
        self.floating_selection = Some(FloatingSelectionState {
            floating: paint_app::FloatingSelection { surface, bounds },
            texture,
            offset_x,
            offset_y,
            base_x: offset_x,
            base_y: offset_y,
            pasted: true,
        });
        self.active_tool = Tool::Move;
        self.texture_dirty = true;
        true
    }

    fn paste_clipboard(&mut self, context: &egui::Context) -> bool {
        self.finish_active_preview();
        self.finalize_floating_selection();
        let content = match self.clipboard_content(context) {
            Ok(Some(content)) => content,
            Ok(None) => {
                self.status = "The clipboard does not contain an image".to_owned();
                return false;
            }
            Err(error) => {
                self.status = error;
                return false;
            }
        };
        if !self.begin_floating_paste(context, content) {
            return false;
        }
        self.status = "Pasted pixels; drag to place them or press Finish".to_owned();
        true
    }

    /// Pastes clipboard pixels into a new transparent layer of the active document.
    ///
    /// This mirrors Paint.NET's Paste Into New Layer: one Add New Layer history step, then the
    /// pasted pixels stay floating so they can be positioned before Finish commits them.
    fn paste_into_new_layer(&mut self, context: &egui::Context) -> bool {
        self.finish_active_preview();
        self.finalize_floating_selection();
        let content = match self.clipboard_content(context) {
            Ok(Some(content)) => content,
            Ok(None) => {
                self.status = "The clipboard does not contain an image".to_owned();
                return false;
            }
            Err(error) => {
                self.status = error;
                return false;
            }
        };
        let number = self.next_layer_number;
        let name = format!("Layer {number}");
        if !self.execute(
            Command::AddLayer {
                name: name.clone(),
                opacity: u8::MAX,
                visible: true,
            },
            format!("Added {name}"),
        ) {
            return false;
        }
        self.next_layer_number = self.next_layer_number.saturating_add(1);
        if !self.begin_floating_paste(context, content) {
            return false;
        }
        self.status = format!("Pasted pixels into {name}; drag to place them or press Finish");
        true
    }

    /// Creates a new image sized to the clipboard bounds and fills it with the clipboard pixels.
    fn paste_into_new_image(&mut self, context: &egui::Context) -> bool {
        self.finish_active_preview();
        self.finalize_floating_selection();
        let content = match self.clipboard_content(context) {
            Ok(Some(content)) => content,
            Ok(None) => {
                self.status = "The clipboard does not contain an image".to_owned();
                return false;
            }
            Err(error) => {
                self.status = error;
                return false;
            }
        };
        let bounds = content.bounds();
        let size = bounds.size();
        let limits = self.session.limits();
        let pixels = u64::from(size.width()) * u64::from(size.height());
        if size.width() > limits.max_width
            || size.height() > limits.max_height
            || pixels > limits.max_pixels
        {
            self.status = format!(
                "Cannot create a {}×{} image; the maximum is {}×{} and {} pixels",
                size.width(),
                size.height(),
                limits.max_width,
                limits.max_height,
                limits.max_pixels
            );
            return false;
        }
        let layer = Layer::new(LayerId::new(1), "Pasted Image", content.surface().clone())
            .with_background(true);
        let document = match Document::from_layers(size, vec![layer]) {
            Ok(document) => document,
            Err(error) => {
                self.status = format!("Could not create the pasted image: {error}");
                return false;
            }
        };
        self.store_active_tab();
        self.tabs.push(None);
        self.active_tab = self.tabs.len() - 1;
        let title = format!("Pasted Image {}", self.active_tab + 1);
        let status = format!(
            "Pasted clipboard pixels into a new {}×{} image",
            size.width(),
            size.height()
        );
        self.new_document_with_session(AppSession::new(document), title, status);
        self.history_entries = vec!["Paste Into New Image".to_owned()];
        self.fit_canvas_requested = true;
        true
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
                    offset_x: 0.0,
                    offset_y: 0.0,
                    base_x: 0.0,
                    base_y: 0.0,
                    pasted: false,
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
        let dest_x = floating.floating.bounds.left() as f64 + floating.offset_x;
        let dest_y = floating.floating.bounds.top() as f64 + floating.offset_y;
        let label = if floating.pasted {
            "Paste"
        } else {
            "Moved selected pixels"
        };
        match self.session.stamp_floating_resampled(
            &floating.floating.surface,
            dest_x,
            dest_y,
            self.resample_mode,
        ) {
            Ok(changed) => {
                if changed {
                    self.history_entries.push(label.to_owned());
                    self.redo_entries.clear();
                }
                self.texture_dirty = true;
                self.status = if floating.pasted {
                    "Placed the pasted pixels".to_owned()
                } else {
                    "Placed the moved pixels".to_owned()
                };
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
                NativeCommand::Save => {
                    if self.document_path.is_some() {
                        self.save_active_document();
                    } else {
                        self.show_save_as_dialog();
                    }
                }
                NativeCommand::SaveAs => self.show_save_as_dialog(),
                NativeCommand::Export => self.show_path_dialog(PathDialogKind::Export),
                NativeCommand::Close => self.request_quit(context),
                NativeCommand::Quit => self.request_quit(context),
                NativeCommand::Undo => {
                    self.execute(Command::Undo, "Undid the last edit");
                }
                NativeCommand::Redo => {
                    self.execute(Command::Redo, "Redid the next edit");
                }
                NativeCommand::Cut => {
                    if !context.text_edit_focused() {
                        self.cut_to_clipboard(context);
                    }
                }
                NativeCommand::Copy => {
                    if !context.text_edit_focused() {
                        self.copy_to_clipboard(context);
                    }
                }
                NativeCommand::Paste => {
                    if !context.text_edit_focused() {
                        self.paste_clipboard(context);
                    }
                }
                NativeCommand::PasteIntoNewLayer => {
                    if !context.text_edit_focused() {
                        self.paste_into_new_layer(context);
                    }
                }
                NativeCommand::PasteIntoNewImage => {
                    if !context.text_edit_focused() {
                        self.paste_into_new_image(context);
                    }
                }
                NativeCommand::EraseSelection => {
                    if !context.text_edit_focused() && self.has_selection() {
                        self.execute(Command::EraseSelection, "Erased the selected pixels");
                    }
                }
                NativeCommand::FillSelection => {
                    if !context.text_edit_focused() && self.has_selection() {
                        let color = self.fill_color;
                        self.execute(
                            Command::FillSelection(color),
                            "Filled the selection with the primary color",
                        );
                    }
                }
                NativeCommand::InvertSelection => {
                    if self.has_selection() {
                        self.execute(Command::InvertSelection, "Inverted the selection");
                    }
                }
                NativeCommand::SelectAll => {
                    self.execute(Command::SelectAll, "Selected the whole canvas");
                }
                NativeCommand::Deselect => {
                    self.execute(Command::ClearSelection, "Selection cleared");
                }
                NativeCommand::ZoomIn => self.zoom = (self.zoom + 0.25).min(8.0),
                NativeCommand::ZoomOut => self.zoom = (self.zoom - 0.25).max(0.1),
                NativeCommand::ActualSize => self.zoom = 1.0,
                NativeCommand::FitCanvas => self.fit_canvas_requested = true,
                NativeCommand::ToggleRulers => self.show_rulers = !self.show_rulers,
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
                NativeCommand::MotionBlur => {
                    self.open_effect_dialog(EffectDialog::MotionBlur {
                        angle: DEFAULT_MOTION_BLUR_ANGLE,
                        distance: DEFAULT_MOTION_BLUR_DISTANCE,
                    });
                }
                NativeCommand::Sharpen => {
                    self.open_effect_dialog(EffectDialog::Sharpen {
                        amount: DEFAULT_SHARPEN_AMOUNT,
                    });
                }
                NativeCommand::AddNoise => {
                    self.open_effect_dialog(EffectDialog::AddNoise {
                        intensity: DEFAULT_NOISE_INTENSITY,
                        seed: DEFAULT_NOISE_SEED,
                    });
                }
                NativeCommand::BrightnessContrast => {
                    self.open_effect_dialog(EffectDialog::BrightnessContrast {
                        brightness: 0,
                        contrast: 0,
                    });
                }
                NativeCommand::HueSaturation => {
                    self.open_effect_dialog(EffectDialog::HueSaturation {
                        hue: 0,
                        saturation: 0,
                        lightness: 0,
                    });
                }
                NativeCommand::AutoLevel => {
                    self.open_effect_dialog(EffectDialog::AutoLevel);
                }
                NativeCommand::BlackAndWhite => {
                    self.open_effect_dialog(EffectDialog::BlackAndWhite {
                        red_weight: DEFAULT_BLACK_AND_WHITE_RED,
                        green_weight: DEFAULT_BLACK_AND_WHITE_GREEN,
                        blue_weight: DEFAULT_BLACK_AND_WHITE_BLUE,
                    });
                }
                NativeCommand::Levels => {
                    self.open_effect_dialog(EffectDialog::Levels {
                        channel: LevelsChannelKind::Rgb,
                        settings: LevelsSettings::IDENTITY,
                    });
                }
                NativeCommand::Posterize => {
                    self.open_effect_dialog(EffectDialog::Posterize {
                        levels: DEFAULT_POSTERIZE_LEVELS,
                    });
                }
                NativeCommand::Threshold => {
                    self.open_effect_dialog(EffectDialog::Threshold {
                        level: DEFAULT_THRESHOLD_LEVEL,
                    });
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
            can_save: self.can_save(),
            can_undo: self.session.can_undo(),
            can_redo: self.session.can_redo(),
            has_selection: self.has_selection(),
            has_clipboard: self.clipboard.is_some(),
            text_focus: context.text_edit_focused(),
            tools_visible: self.tools_visible,
            colors_visible: self.colors_visible,
            layers_visible: self.layers_visible,
            history_visible: self.history_visible,
            float_panels: self.panels_floating,
            rulers_enabled: self.show_rulers,
        };
        if let Some(menu) = self.native_menu.as_ref() {
            menu.sync(state);
        }
    }

    fn menu_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("paint-r-menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    let new_response = menu_button(ui, &self.icons, Icon::NewDocument, "New", true);
                    focus_first_menu_item_on_keyboard_open(ui, &new_response);
                    if new_response.clicked() {
                        self.show_new_image_dialog();
                        ui.close();
                    }
                    if menu_button(ui, &self.icons, Icon::OpenFile, "Open PNG…", true).clicked() {
                        self.show_path_dialog(PathDialogKind::Open);
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            self.can_save(),
                            egui::Button::new("Save"),
                        )
                        .clicked()
                    {
                        self.save_active_document();
                        ui.close();
                    }
                    if ui.button("Save As…").clicked() {
                        self.show_save_as_dialog();
                        ui.close();
                    }
                    ui.separator();
                    if menu_button(ui, &self.icons, Icon::ExportFile, "Export PNG…", true).clicked()
                    {
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
                    let undo =
                        menu_button(ui, &self.icons, Icon::Undo, "Undo", self.session.can_undo());
                    focus_first_menu_item_on_keyboard_open(ui, &undo);
                    if undo.clicked() {
                        self.execute(Command::Undo, "Undid the last edit");
                        ui.close();
                    }
                    if menu_button(ui, &self.icons, Icon::Redo, "Redo", self.session.can_redo())
                        .clicked()
                    {
                        self.execute(Command::Redo, "Redid the next edit");
                        ui.close();
                    }
                    ui.separator();
                    let has_selection = self.has_selection();
                    let has_clipboard = self.clipboard.is_some();
                    if ui
                        .add_enabled(
                            has_selection,
                            egui::Button::new("Cut"),
                        )
                        .on_hover_text("Copy the selected pixels and erase them")
                        .clicked()
                    {
                        self.cut_to_clipboard(ui.ctx());
                        ui.close();
                    }
                    if ui
                        .button("Copy")
                        .on_hover_text("Copy the selection, or the whole active layer without one")
                        .clicked()
                    {
                        self.copy_to_clipboard(ui.ctx());
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            has_clipboard,
                            egui::Button::new("Paste"),
                        )
                        .on_hover_text("Paste clipboard pixels as a floating selection")
                        .clicked()
                    {
                        self.paste_clipboard(ui.ctx());
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            has_clipboard,
                            egui::Button::new("Paste Into New Layer"),
                        )
                        .on_hover_text(
                            "Paste clipboard pixels into a new layer of the active document",
                        )
                        .clicked()
                    {
                        self.paste_into_new_layer(ui.ctx());
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            has_clipboard,
                            egui::Button::new("Paste Into New Image"),
                        )
                        .on_hover_text("Create a new image sized to the clipboard pixels")
                        .clicked()
                    {
                        self.paste_into_new_image(ui.ctx());
                        ui.close();
                    }
                    ui.separator();
                    if ui
                        .add_enabled(
                            has_selection,
                            egui::Button::new("Erase Selection"),
                        )
                        .on_hover_text("Erase the selected pixels and deselect")
                        .clicked()
                    {
                        self.execute(Command::EraseSelection, "Erased the selected pixels");
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            has_selection,
                            egui::Button::new("Fill Selection"),
                        )
                        .on_hover_text(
                            "Fill the selection with the primary color; Shift+Backspace uses the secondary color",
                        )
                        .clicked()
                    {
                        let color = self.fill_color;
                        self.execute(
                            Command::FillSelection(color),
                            "Filled the selection with the primary color",
                        );
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            has_selection,
                            egui::Button::new("Invert Selection"),
                        )
                        .clicked()
                    {
                        self.execute(Command::InvertSelection, "Inverted the selection");
                        ui.close();
                    }
                    if ui.button("Select All").clicked() {
                        self.execute(Command::SelectAll, "Selected the whole canvas");
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            has_selection,
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
                    ui.checkbox(&mut self.show_rulers, "Rulers");
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
                    let add = menu_button(ui, &self.icons, Icon::AddLayer, "Add New Layer", true);
                    focus_first_menu_item_on_keyboard_open(ui, &add);
                    if add.clicked() {
                        self.add_layer();
                        ui.close();
                    }
                    if menu_button(
                        ui,
                        &self.icons,
                        Icon::LayerDelete,
                        "Delete Layer",
                        layer_count > 1,
                    )
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
                    if menu_button(
                        ui,
                        &self.icons,
                        Icon::LayerDuplicate,
                        "Duplicate Layer",
                        true,
                    )
                    .clicked()
                    {
                        self.execute(
                            Command::DuplicateLayer {
                                index: active_layer_index,
                            },
                            "Duplicated the active layer",
                        );
                        ui.close();
                    }
                    if menu_button(
                        ui,
                        &self.icons,
                        Icon::LayerMerge,
                        "Merge Layer Down",
                        active_layer_index > 0,
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
                    if menu_button(
                        ui,
                        &self.icons,
                        if active_layer_visible {
                            Icon::Eye
                        } else {
                            Icon::EyeOff
                        },
                        "Toggle Layer Visibility",
                        layer_count > 1,
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
                    for (label, icon, offset) in [
                        (
                            "Move Layer to Top",
                            Icon::LayerUp,
                            layer_count as i32 - 1 - active_layer_index as i32,
                        ),
                        ("Move Layer Up", Icon::LayerUp, 1),
                        ("Move Layer Down", Icon::LayerDown, -1),
                        (
                            "Move Layer to Bottom",
                            Icon::LayerDown,
                            -(active_layer_index as i32),
                        ),
                    ] {
                        if menu_button(ui, &self.icons, icon, label, true).clicked() {
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
                    if menu_button(
                        ui,
                        &self.icons,
                        Icon::LayerSettings,
                        "Layer Properties…",
                        true,
                    )
                    .clicked()
                    {
                        self.open_layer_properties(active_layer_index);
                        ui.close();
                    }
                });
                ui.menu_button("Adjustments", |ui| {
                    // Paint.NET orders Adjustment effects alphabetically by display name.
                    let auto_level = ui.button("Auto-Level…");
                    focus_first_menu_item_on_keyboard_open(ui, &auto_level);
                    if auto_level.clicked() {
                        self.open_effect_dialog(EffectDialog::AutoLevel);
                        ui.close();
                    }
                    if ui.button("Black and White…").clicked() {
                        self.open_effect_dialog(EffectDialog::BlackAndWhite {
                            red_weight: DEFAULT_BLACK_AND_WHITE_RED,
                            green_weight: DEFAULT_BLACK_AND_WHITE_GREEN,
                            blue_weight: DEFAULT_BLACK_AND_WHITE_BLUE,
                        });
                        ui.close();
                    }
                    if ui.button("Brightness / Contrast…").clicked() {
                        self.open_effect_dialog(EffectDialog::BrightnessContrast {
                            brightness: 0,
                            contrast: 0,
                        });
                        ui.close();
                    }
                    disabled_effect_item(ui, "Curves…");
                    disabled_effect_item(ui, "Exposure…");
                    disabled_effect_item(ui, "Highlights and Shadows…");
                    if ui.button("Hue / Saturation…").clicked() {
                        self.open_effect_dialog(EffectDialog::HueSaturation {
                            hue: 0,
                            saturation: 0,
                            lightness: 0,
                        });
                        ui.close();
                    }
                    disabled_effect_item(ui, "Invert Alpha");
                    if ui.button("Invert Colors").clicked() {
                        self.execute(Command::Invert, "Inverted colors on the active layer");
                        ui.close();
                    }
                    if ui.button("Levels…").clicked() {
                        self.open_effect_dialog(EffectDialog::Levels {
                            channel: LevelsChannelKind::Rgb,
                            settings: LevelsSettings::IDENTITY,
                        });
                        ui.close();
                    }
                    if ui.button("Posterize…").clicked() {
                        self.open_effect_dialog(EffectDialog::Posterize {
                            levels: DEFAULT_POSTERIZE_LEVELS,
                        });
                        ui.close();
                    }
                    if ui.button("Sepia").clicked() {
                        self.execute(
                            Command::ApplyEffect(Effect::Sepia),
                            "Applied Sepia to the selection",
                        );
                        ui.close();
                    }
                    disabled_effect_item(ui, "Temperature and Tint…");
                    if ui.button("Threshold…").clicked() {
                        self.open_effect_dialog(EffectDialog::Threshold {
                            level: DEFAULT_THRESHOLD_LEVEL,
                        });
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
                        if ui.button("Motion Blur…").clicked() {
                            self.open_effect_dialog(EffectDialog::MotionBlur {
                                angle: DEFAULT_MOTION_BLUR_ANGLE,
                                distance: DEFAULT_MOTION_BLUR_DISTANCE,
                            });
                            ui.close();
                        }
                        if ui.button("Gaussian Blur…").clicked() {
                            self.open_gaussian_blur_dialog();
                            ui.close();
                        }
                        for label in ["Radial Blur", "Unfocus", "Zoom Blur"] {
                            disabled_effect_item(ui, label);
                        }
                    });
                    ui.menu_button("Distort", |ui| {
                        for label in ["Bulge", "Dents", "Pixelate", "Twist"] {
                            disabled_effect_item(ui, label);
                        }
                    });
                    ui.menu_button("Noise", |ui| {
                        if ui.button("Add Noise…").clicked() {
                            self.open_effect_dialog(EffectDialog::AddNoise {
                                intensity: DEFAULT_NOISE_INTENSITY,
                                seed: DEFAULT_NOISE_SEED,
                            });
                            ui.close();
                        }
                        for label in ["Median", "Reduce Noise"] {
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
                        ui.separator();
                        if ui.button("Sharpen…").clicked() {
                            self.open_effect_dialog(EffectDialog::Sharpen {
                                amount: DEFAULT_SHARPEN_AMOUNT,
                            });
                            ui.close();
                        }
                        for label in ["Glow", "Soften Portrait", "Vignette"] {
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
                    ui.label("N New | O Open | S Save | Shift+S Save As | Shift+E Export");
                    ui.label("A Select All | D Deselect | I Invert Selection | Shift+I Invert Colors");
                    ui.label("Delete Erase Selection | Backspace Fill Selection | Shift+Backspace Secondary");
                    ui.label("V Paste | Shift+V Paste Into New Layer | Alt+V Paste Into New Image");
                    ui.label("F Fill layer | R Rectangle selection");
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
            let icon_size = egui::vec2(30.0, 28.0);
            if icon_button(ui, &self.icons, Icon::NewDocument, "New document", true).clicked() {
                self.request_new();
            }
            if icon_button(ui, &self.icons, Icon::OpenFile, "Open PNG file", true).clicked() {
                self.show_path_dialog(PathDialogKind::Open);
            }
            if vector_icon_button_sized(ui, VectorIcon::Save, "Save document", true, icon_size)
                .clicked()
            {
                if self.document_path.is_some() {
                    self.save_active_document();
                } else {
                    self.show_save_as_dialog();
                }
            }
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
            let has_selection = self.has_selection();
            let has_clipboard = self.clipboard.is_some();
            if vector_icon_button_sized(
                ui,
                VectorIcon::Cut,
                "Cut to clipboard",
                has_selection,
                icon_size,
            )
            .clicked()
            {
                self.cut_to_clipboard(ui.ctx());
            }
            if vector_icon_button_sized(ui, VectorIcon::Copy, "Copy to clipboard", true, icon_size)
                .clicked()
            {
                self.copy_to_clipboard(ui.ctx());
            }
            if vector_icon_button_sized(
                ui,
                VectorIcon::Paste,
                "Paste from clipboard",
                has_clipboard,
                icon_size,
            )
            .clicked()
            {
                self.paste_clipboard(ui.ctx());
            }
            if vector_icon_button_sized(
                ui,
                VectorIcon::Crop,
                "Crop to selection",
                has_selection,
                icon_size,
            )
            .clicked()
            {
                self.execute(Command::CropToSelection, "Cropped to the selection");
            }
            if vector_icon_button_sized(
                ui,
                VectorIcon::Deselect,
                "Deselect all",
                has_selection,
                icon_size,
            )
            .clicked()
            {
                self.execute(Command::ClearSelection, "Selection cleared");
            }
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
            if icon_button(ui, &self.icons, Icon::Undo, "Undo", self.session.can_undo()).clicked() {
                self.execute(Command::Undo, "Undid the last edit");
            }
            if icon_button(ui, &self.icons, Icon::Redo, "Redo", self.session.can_redo()).clicked() {
                self.execute(Command::Redo, "Redid the next edit");
            }
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
            if vector_toggle_button(
                ui,
                VectorIcon::Grid,
                "Toggle grid",
                true,
                self.show_grid,
                icon_size,
            )
            .clicked()
            {
                self.show_grid = !self.show_grid;
            }
            if vector_toggle_button(
                ui,
                VectorIcon::Rulers,
                "Toggle rulers",
                true,
                self.show_rulers,
                icon_size,
            )
            .clicked()
            {
                self.show_rulers = !self.show_rulers;
            }
            if icon_button(ui, &self.icons, Icon::ExportFile, "Export PNG file", true).clicked() {
                self.show_path_dialog(PathDialogKind::Export);
            }
            if self.panels_floating {
                let mut pending_close = None;
                for index in 0..self.tabs.len() {
                    let active = index == self.active_tab;
                    let title = self.tab_display_title(index);
                    let thumbnail = if active {
                        self.document_thumbnail.clone()
                    } else {
                        self.tabs[index]
                            .as_ref()
                            .and_then(|tab| tab.thumbnail.clone())
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
            .default_size(TOOLS_PANEL_WIDTH)
            .min_size(TOOLS_PANEL_WIDTH)
            .max_size(TOOLS_PANEL_WIDTH)
            .resizable(false)
            .show(ui, |ui| self.tools_content(ui));
    }

    fn tools_content(&mut self, ui: &mut egui::Ui) {
        if self.tools_floating {
            if panel_header(ui, TOOLS_PANEL_TITLE, "Close Tools window") {
                self.tools_close_requested = true;
            }
        } else {
            ui.add_space(4.0);
            ui.strong(TOOLS_PANEL_TITLE);
            ui.separator();
        }
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in TOOLBOX.chunks(2) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 1.0;
                    for spec in row {
                        let selected = self.active_tool == spec.tool;
                        let tooltip = spec.entry.tooltip();
                        let clicked = tool_button_with_tooltip(
                            ui,
                            &self.icons,
                            spec.icon,
                            spec.entry.label,
                            &tooltip,
                            true,
                            selected,
                        )
                        .clicked();
                        if clicked {
                            self.select_tool(spec.tool);
                        }
                    }
                });
            }
        });
    }

    /// Switches tools with the same commit side effects as clicking a toolbox cell.
    fn select_tool(&mut self, tool: Tool) {
        if self.active_tool != tool {
            if tool == Tool::Picker {
                self.previous_tool_before_picker = Some(self.active_tool);
            }
            if self.active_tool == Tool::Text {
                self.commit_text_preview();
            }
            self.finalize_floating_selection();
            self.move_drag_origin = None;
            self.move_drag_current = None;
        }
        self.active_tool = tool;
        self.picking_color = tool == Tool::Picker;
    }

    fn selection_combine_options(&mut self, ui: &mut egui::Ui) {
        for mode in SelectionCombineMode::ALL {
            if selection_mode_button(ui, mode, self.selection_combine_mode == mode).clicked() {
                self.selection_combine_mode = mode;
                self.status = format!("Selection mode: {}", mode.label());
            }
        }
        if selection_fields_button(ui, self.selection_fields_open).clicked() {
            self.selection_fields_open = !self.selection_fields_open;
        }
        if self.selection_fields_open {
            selection_input(ui, "Selection left", &mut self.selection_input.left);
            selection_input(ui, "Selection top", &mut self.selection_input.top);
            selection_input(ui, "Selection right", &mut self.selection_input.right);
            selection_input(ui, "Selection bottom", &mut self.selection_input.bottom);
            if ui.button("Apply selection").clicked() {
                self.select_rectangle();
            }
        }
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
                Tool::Move
                    | Tool::MoveSelection
                    | Tool::Picker
                    | Tool::Line
                    | Tool::Fill
                    | Tool::Gradient
                    | Tool::Text
                    | Tool::CloneStamp
                    | Tool::Recolor
            ) {
                self.grouped_tool_options(ui);
                ui.add_space(2.0);
                return;
            }
            ui.horizontal_wrapped(|ui| match self.active_tool {
                Tool::Select => {
                    self.selection_combine_options(ui);
                }
                Tool::Lasso => {
                    self.selection_combine_options(ui);
                }
                Tool::EllipseSelect => {
                    self.selection_combine_options(ui);
                }
                Tool::MagicWand => {
                    self.selection_combine_options(ui);
                    ui.separator();
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
                }
                Tool::Zoom => {
                    ui.label("Zoom");
                    ui.label(format!("{}%", (self.zoom * 100.0).round() as u32));
                    for (label, zoom) in [("50%", 0.5), ("100%", 1.0), ("200%", 2.0), ("400%", 4.0)]
                    {
                        if labeled_button(ui, label, label).clicked() {
                            self.zoom = zoom;
                        }
                    }
                    if labeled_button(ui, "Fit", "Fit canvas").clicked() {
                        self.fit_canvas_requested = true;
                    }
                }
                Tool::Pencil | Tool::Brush | Tool::Eraser => {
                    if self.active_tool == Tool::Pencil {
                        ui.label("1 px");
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
                    if self.active_tool != Tool::Eraser {
                        let color = self.fill_color;
                        ui.colored_label(
                            egui::Color32::from_rgba_unmultiplied(
                                color.r, color.g, color.b, color.a,
                            ),
                            "Primary color",
                        );
                    }
                }
                Tool::Shapes => {
                    ui.label("Shape type");
                    ui.selectable_value(&mut self.shape_mode, ShapeMode::Rectangle, "Rectangle");
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
                }
                Tool::Text | Tool::CloneStamp | Tool::Fill | Tool::Gradient | Tool::Recolor => {}
                Tool::Pan => {}
                Tool::Move | Tool::MoveSelection | Tool::Picker | Tool::Line => {}
            });
            ui.add_space(2.0);
        });
    }

    /// Tool Options for tools whose controls need separate rows instead of one wrapped row.
    fn grouped_tool_options(&mut self, ui: &mut egui::Ui) {
        match self.active_tool {
            Tool::Move => self.move_tool_options(ui),
            Tool::MoveSelection => self.move_selection_tool_options(ui),
            Tool::Picker => self.picker_tool_options(ui),
            Tool::Line => self.line_tool_options(ui),
            Tool::Fill => self.fill_tool_options(ui),
            Tool::Gradient => self.gradient_tool_options(ui),
            Tool::Text => self.text_tool_options(ui),
            Tool::CloneStamp => self.clone_tool_options(ui),
            Tool::Recolor => self.recolor_tool_options(ui),
            _ => {}
        }
    }

    fn move_tool_options(&mut self, ui: &mut egui::Ui) {
        let floating = self.floating_selection.is_some();
        let previewing_layer = self.session.has_preview();
        let finishable = floating || previewing_layer;
        // Keep this row present even when nothing is floating: adding/removing it mid-drag
        // changes the tool strip height and shifts the canvas under the pointer.
        // Reference ToolConfigStrip order: resampling section first, Commit/Finish last.
        tool_option_row(ui, |ui| {
            ui.label("Sampling:");
            for mode in RESAMPLE_MODES {
                if mode == ResampleMode::Anisotropic {
                    ui.add_enabled(false, egui::Button::new(resample_label(mode)))
                        .on_disabled_hover_text(
                            "Anisotropic sampling only differs under non-uniform scaling, which \
                             Move does not perform. It is disabled rather than approximated.",
                        );
                    continue;
                }
                let selected = self.resample_mode == mode;
                let response = ui.selectable_label(selected, resample_label(mode));
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::Role::Button,
                        true,
                        selected,
                        resample_label(mode),
                    )
                });
                if response.on_hover_text(resample_hint(mode)).clicked() {
                    self.set_resample_mode(mode);
                }
            }
            ui.separator();
            if ui
                .add_enabled(finishable, egui::Button::new("Finish"))
                .on_hover_text("Place the moved pixels\nShortcut key: Enter")
                .clicked()
            {
                self.finish_move();
            }
        });
    }

    /// Applies a sampling mode and refreshes an already-lifted whole-layer preview.
    fn set_resample_mode(&mut self, mode: ResampleMode) {
        self.resample_mode = mode;
        if self.floating_selection.is_none() && self.session.has_preview() {
            let (dx, dy) = self.layer_move_offset;
            match self.session.preview_translate_resampled(dx, dy, mode) {
                Ok(_) => self.texture_dirty = true,
                Err(error) => self.status = format!("Move preview failed: {error}"),
            }
        }
    }

    fn finish_move(&mut self) {
        if self.floating_selection.is_some() {
            self.finalize_floating_selection();
        } else if self.session.has_preview() {
            self.finish_layer_move_preview();
        }
    }

    fn finish_layer_move_preview(&mut self) {
        match self.session.finish_preview() {
            Ok(true) => {
                self.history_entries
                    .push("Moved selected pixels".to_owned());
                self.redo_entries.clear();
                self.texture_dirty = true;
                self.status = "Placed the moved layer".to_owned();
            }
            Ok(false) => {}
            Err(error) => self.status = format!("Could not place the moved layer: {error}"),
        }
        self.layer_move_offset = (0.0, 0.0);
        self.layer_move_base = (0.0, 0.0);
        self.move_drag_origin = None;
        self.move_drag_current = None;
    }

    fn move_selection_tool_options(&mut self, ui: &mut egui::Ui) {
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
    }

    fn picker_tool_options(&mut self, ui: &mut egui::Ui) {
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
                PickerSampleSize::Average11x11,
                PickerSampleSize::Average31x31,
                PickerSampleSize::Average51x51,
            ] {
                ui.selectable_value(&mut self.picker_sample_size, size, size.label());
            }
        });
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
    }

    fn fill_tool_options(&mut self, ui: &mut egui::Ui) {
        tool_option_row(ui, |ui| {
            ui.label("Flood Mode");
            if option_selectable(ui, !self.fill_global, "Contiguous", "Contiguous") {
                self.fill_global = false;
            }
            if option_selectable(ui, self.fill_global, "Global", "Global") {
                self.fill_global = true;
            }
            ui.separator();
            ui.label("Fill");
            ui.label("Solid Color");
            let color = self.fill_color;
            ui.colored_label(
                egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a),
                "Primary color",
            );
        });
        tool_option_row(ui, |ui| {
            ui.label("Sampling");
            for sampling in MagicWandSampling::ALL {
                let (text, accessible) = match sampling {
                    MagicWandSampling::ActiveLayer => ("Layer", "Sampling: Layer"),
                    MagicWandSampling::Image => ("Image", "Sampling: Image"),
                };
                if option_selectable(ui, self.fill_sampling == sampling, text, accessible) {
                    self.fill_sampling = sampling;
                }
            }
            ui.separator();
            let tolerance_label = ui.label("Tolerance");
            ui.add(
                egui::DragValue::new(&mut self.fill_tolerance)
                    .range(0..=100)
                    .suffix("%"),
            )
            .labelled_by(tolerance_label.id);
        });
        tool_option_row(ui, |ui| {
            tool_blend_mode_row(ui, &mut self.fill_blend);
        });
    }

    fn gradient_tool_options(&mut self, ui: &mut egui::Ui) {
        tool_option_row(ui, |ui| {
            ui.label("Gradient type");
            for (text, kind) in [
                ("Linear", GradientType::Linear),
                ("Reflected", GradientType::Reflected),
                ("Diamond", GradientType::Diamond),
                ("Radial", GradientType::Radial),
                ("Conical", GradientType::Conical),
            ] {
                if option_selectable(ui, self.gradient_type == kind, text, text) {
                    self.gradient_type = kind;
                }
            }
        });
        tool_option_row(ui, |ui| {
            ui.checkbox(&mut self.gradient_alpha_only, "Transparency only")
                .on_hover_text(
                    "Replace the layer alpha with the gradient alpha; RGB colors stay unchanged",
                );
            ui.separator();
            ui.label("Repeat mode");
            if option_selectable(
                ui,
                self.gradient_repeat == GradientRepeat::None,
                "No Repeat",
                "No Repeat",
            ) {
                self.gradient_repeat = GradientRepeat::None;
            }
            if option_selectable(
                ui,
                self.gradient_repeat == GradientRepeat::Repeat,
                "Repeat",
                "Repeat",
            ) {
                self.gradient_repeat = GradientRepeat::Repeat;
            }
        });
        tool_option_row(ui, |ui| {
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
        });
        tool_option_row(ui, |ui| {
            tool_blend_mode_row(ui, &mut self.gradient_blend);
        });
    }

    fn recolor_tool_options(&mut self, ui: &mut egui::Ui) {
        tool_option_row(ui, |ui| {
            let width_label = ui.label("Width");
            ui.add(
                egui::DragValue::new(&mut self.brush_size)
                    .range(1..=64)
                    .suffix(" px"),
            )
            .labelled_by(width_label.id);
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
        });
        tool_option_row(ui, |ui| {
            let tolerance_label = ui.label("Tolerance");
            ui.add(
                egui::DragValue::new(&mut self.recolor_tolerance)
                    .range(0..=100)
                    .suffix("%"),
            )
            .labelled_by(tolerance_label.id);
            ui.separator();
            ui.label("Sampling");
            for sampling in MagicWandSampling::ALL {
                let (text, accessible) = match sampling {
                    MagicWandSampling::ActiveLayer => ("Layer", "Sampling: Layer"),
                    MagicWandSampling::Image => ("Image", "Sampling: Image"),
                };
                if option_selectable(ui, self.recolor_sampling == sampling, text, accessible) {
                    self.recolor_sampling = sampling;
                }
            }
        });
        tool_option_row(ui, |ui| {
            let (target, replacement) = (self.secondary_color, self.fill_color);
            ui.colored_label(
                egui::Color32::from_rgba_unmultiplied(target.r, target.g, target.b, target.a),
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
            if panel_header(ui, LAYERS_PANEL_TITLE, "Close Layers window") {
                self.layers_close_requested = true;
            }
        } else {
            ui.heading(LAYERS_PANEL_TITLE);
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
                    // Seven 23 px buttons, centered like Paint.NET's flow tool strip.
                    let pad = ((ui.available_width() - 161.0) / 2.0).max(0.0);
                    ui.add_space(pad);
                    ui.spacing_mut().item_spacing.x = 0.0;
                    if panel_button(
                        ui,
                        &self.icons,
                        Icon::AddLayer,
                        "Add Layer",
                        "Add a new layer",
                        true,
                    )
                    .clicked()
                    {
                        self.add_layer();
                    }
                    if panel_button(
                        ui,
                        &self.icons,
                        Icon::LayerDelete,
                        "Delete Layer",
                        "Delete the active layer",
                        can_delete,
                    )
                    .clicked()
                    {
                        pending = Some(Command::RemoveLayer {
                            index: active_index,
                        });
                    }
                    if panel_button(
                        ui,
                        &self.icons,
                        Icon::LayerDuplicate,
                        "Duplicate Layer",
                        "Duplicate the active layer",
                        true,
                    )
                    .clicked()
                    {
                        pending = Some(Command::DuplicateLayer {
                            index: active_index,
                        });
                    }
                    if panel_button(
                        ui,
                        &self.icons,
                        Icon::LayerMerge,
                        "Merge Layer Down",
                        "Merge the active layer down",
                        can_merge,
                    )
                    .clicked()
                    {
                        pending = Some(Command::MergeLayerDown {
                            index: active_index,
                        });
                    }
                    if panel_button(
                        ui,
                        &self.icons,
                        Icon::LayerUp,
                        "Move Layer Up",
                        "Move the active layer up",
                        can_up,
                    )
                    .clicked()
                    {
                        pending = Some(Command::MoveLayer {
                            index: active_index,
                            offset: 1,
                        });
                    }
                    if panel_button(
                        ui,
                        &self.icons,
                        Icon::LayerDown,
                        "Move Layer Down",
                        "Move the active layer down",
                        can_down,
                    )
                    .clicked()
                    {
                        pending = Some(Command::MoveLayer {
                            index: active_index,
                            offset: -1,
                        });
                    }
                    if panel_button(
                        ui,
                        &self.icons,
                        Icon::LayerSettings,
                        "Layer Properties",
                        "Open layer properties",
                        true,
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
            if panel_header(ui, HISTORY_PANEL_TITLE, "Close History window") {
                self.history_close_requested = true;
            }
        } else {
            ui.heading(HISTORY_PANEL_TITLE);
            ui.separator();
        }
        egui::Panel::bottom("paint-r-history-buttons")
            .frame(egui::Frame::NONE)
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    if panel_button(
                        ui,
                        &self.icons,
                        Icon::Undo,
                        "Undo History",
                        "Undo the last edit",
                        self.session.can_undo(),
                    )
                    .clicked()
                    {
                        self.execute(Command::Undo, "Undid the last edit");
                    }
                    if panel_button(
                        ui,
                        &self.icons,
                        Icon::Redo,
                        "Redo History",
                        "Redo the next edit",
                        self.session.can_redo(),
                    )
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
            if panel_header(ui, COLORS_PANEL_TITLE, "Close Colors window") {
                self.colors_close_requested = true;
            }
        } else {
            ui.heading(COLORS_PANEL_TITLE);
            ui.separator();
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
                        area.min + egui::vec2(left_width - 22.0, 0.0),
                        egui::vec2(22.0, 22.0),
                    );
                    let mut more_ui = ui.new_child(egui::UiBuilder::new().max_rect(more_rect));
                    let more_icon = if self.colors_expanded {
                        VectorIcon::ChevronUp
                    } else {
                        VectorIcon::ChevronDown
                    };
                    let more_label = if self.colors_expanded { "Less" } else { "More" };
                    if vector_icon_button(&mut more_ui, more_icon, more_label, true).clicked() {
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
                let right_width = 170.0;
                let right_rect = egui::Rect::from_min_size(
                    egui::pos2(content.right() - right_width, content.top()),
                    egui::vec2(right_width, content.height()),
                );
                let mut right_ui = ui.new_child(egui::UiBuilder::new().max_rect(right_rect));
                let right_bottom = {
                    let ui = &mut right_ui;
                    ui.vertical(|ui| {
                        ui.set_width(right_width);
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
                                    [124.0, 20.0],
                                    egui::TextEdit::singleline(&mut self.hex_input)
                                        .desired_width(124.0),
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
        let screen = context.viewport_rect();
        let layout = default_panel_layout(screen, self.panels_floating, self.colors_expanded);
        let tools_pos = layout.tools.min;
        let history_pos = layout.history.min;
        let colors_pos = layout.colors.min;
        let colors_size = layout.colors.size();
        let layers_pos = layout.layers.min;
        if self.tools_visible && self.tools_floating {
            let mut open = self.tools_visible;
            let frame = floating_window_frame(&context, self.tools_window_rect);
            if let Some(inner) = egui::Window::new(TOOLS_PANEL_TITLE)
                .open(&mut open)
                .title_bar(false)
                .frame(frame)
                .collapsible(false)
                .default_pos(tools_pos)
                .min_width(TOOLS_PANEL_WIDTH)
                .max_width(TOOLS_PANEL_WIDTH)
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
            if let Some(inner) = egui::Window::new(HISTORY_PANEL_TITLE)
                .open(&mut open)
                .title_bar(false)
                .frame(frame)
                .collapsible(false)
                .default_pos(history_pos)
                .fixed_size(egui::vec2(HISTORY_PANEL_SIZE[0], HISTORY_PANEL_SIZE[1]))
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
            if let Some(inner) = egui::Window::new(COLORS_PANEL_TITLE)
                .open(&mut open)
                .title_bar(false)
                .frame(frame)
                .collapsible(false)
                .default_pos(colors_pos)
                .fixed_size(egui::vec2(colors_size[0], colors_size[1]))
                .scroll(false)
                .show(&context, |ui| self.colors_content(ui))
            {
                self.colors_window_rect = Some(inner.response.rect);
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
            if let Some(inner) = egui::Window::new(LAYERS_PANEL_TITLE)
                .open(&mut open)
                .title_bar(false)
                .frame(frame)
                .collapsible(false)
                .default_pos(layers_pos)
                .fixed_size(egui::vec2(LAYERS_PANEL_SIZE[0], LAYERS_PANEL_SIZE[1]))
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

    fn effect_dialog_window(&mut self, context: &egui::Context) {
        let Some(mut dialog) = self.effect_dialog.take() else {
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
            self.apply_effect_dialog(dialog);
        } else if cancel || !open {
            let title = dialog.title();
            self.cancel_pending_effect();
            self.status = format!("Canceled {title}");
        } else {
            let effect = dialog.effect();
            if self.effect_dialog_previewed != Some(effect) {
                match self.session.preview_effect(effect) {
                    Ok(_) => {
                        self.effect_dialog_previewed = Some(effect);
                        self.texture_dirty = true;
                    }
                    Err(error) => {
                        self.status = format!("Effect preview failed: {error}");
                    }
                }
            }
            self.effect_dialog = Some(dialog);
        }
    }

    fn open_effect_dialog(&mut self, dialog: EffectDialog) {
        self.close_effect_preview();
        self.finish_active_preview();
        self.finalize_floating_selection();
        self.gaussian_blur_open = false;
        self.session.begin_preview();
        self.effect_dialog_previewed = None;
        self.effect_dialog = Some(dialog);
        self.texture_dirty = true;
    }

    fn apply_effect_dialog(&mut self, dialog: EffectDialog) {
        let effect = dialog.effect();
        let label = effect.history_label();
        self.cancel_pending_effect();
        self.execute(Command::ApplyEffect(effect), format!("Applied {label}"));
    }

    fn cancel_pending_effect(&mut self) {
        self.session.cancel_preview();
        self.effect_dialog_previewed = None;
        self.texture_dirty = true;
    }

    fn close_effect_preview(&mut self) {
        if self.effect_dialog.take().is_some() {
            self.cancel_pending_effect();
        }
    }

    fn open_gaussian_blur_dialog(&mut self) {
        self.close_effect_preview();
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
        self.new_image_dialog(context);
        self.resize_image_dialog(context);
        self.canvas_size_dialog(context);
    }

    fn new_image_dialog(&mut self, context: &egui::Context) {
        let Some(mut dialog) = self.new_image_dialog.take() else {
            return;
        };
        let limits = self.session.limits();
        let mut apply = false;
        let mut cancel =
            context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let mut open = true;
        egui::Window::new("New Image")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(320.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(context, |ui| {
                ui.label("Create a new image in its own document tab.");
                ui.separator();
                ui.horizontal(|ui| {
                    let label = ui.label("New image width");
                    ui.add(egui::TextEdit::singleline(&mut dialog.width).desired_width(72.0))
                        .labelled_by(label.id);
                    ui.label("pixels");
                });
                ui.horizontal(|ui| {
                    let label = ui.label("New image height");
                    ui.add(egui::TextEdit::singleline(&mut dialog.height).desired_width(72.0))
                        .labelled_by(label.id);
                    ui.label("pixels");
                });
                ui.separator();
                ui.strong("Background");
                ui.horizontal_wrapped(|ui| {
                    for (background, label) in NEW_IMAGE_BACKGROUNDS {
                        ui.radio_value(&mut dialog.background, background, label);
                    }
                });
                let validated = dialog.validated_size(limits);
                ui.separator();
                match &validated {
                    Ok(size) => {
                        ui.label(format!(
                            "New image: {} × {} pixels, {} background",
                            size.width(),
                            size.height(),
                            background_label(
                                dialog
                                    .background
                                    .color(self.fill_color, self.secondary_color)
                            )
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
        if cancel || !open {
            self.status = "New Image canceled".to_owned();
            return;
        }
        if !apply {
            self.new_image_dialog = Some(dialog);
            return;
        }
        let Ok(size) = dialog.validated_size(limits) else {
            self.new_image_dialog = Some(dialog);
            return;
        };
        let background = dialog
            .background
            .color(self.fill_color, self.secondary_color);
        self.create_new_document(size, background);
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
        egui::Panel::bottom("paint-r-status")
            .exact_size(STATUS_BAR_HEIGHT)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let dirty = self.session.dirty();
                    let (dot, _) =
                        ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                    ui.painter().circle_filled(
                        dot.center(),
                        4.0,
                        if dirty {
                            egui::Color32::from_rgb(240, 170, 60)
                        } else {
                            egui::Color32::from_rgb(90, 170, 110)
                        },
                    );
                    let dirty_label = if dirty { "Modified" } else { "Clean" };
                    ui.label(egui::RichText::new(dirty_label).strong().color(if dirty {
                        egui::Color32::from_rgb(240, 190, 100)
                    } else {
                        egui::Color32::from_gray(190)
                    }))
                    .on_hover_text(if dirty {
                        "The document has unsaved changes"
                    } else {
                        "The document has no unsaved changes"
                    });
                    ui.separator();
                    ui.add(egui::Label::new(&self.status).truncate());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let size = self.session.document().size();
                        ui.label(format!(
                            "{} × {} {}",
                            self.unit.format_value(f64::from(size.width())),
                            self.unit.format_value(f64::from(size.height())),
                            self.unit.suffix()
                        ))
                        .on_hover_text("Document size");
                        ui.separator();
                        if vector_icon_button(ui, VectorIcon::FitWindow, "Zoom to window", true)
                            .clicked()
                        {
                            self.fit_canvas_requested = true;
                        }
                        if vector_icon_button(ui, VectorIcon::ZoomIn, "Zoom in", true).clicked() {
                            self.zoom = (self.zoom + 0.25).min(8.0);
                        }
                        ui.label(format!("{}%", (self.zoom * 100.0).round() as u32))
                            .on_hover_text("Zoom level");
                        if vector_icon_button(ui, VectorIcon::ZoomOut, "Zoom out", true).clicked() {
                            self.zoom = (self.zoom - 0.25).max(0.1);
                        }
                        let units = ui.menu_button(self.unit.suffix(), |ui| {
                            ui.radio_value(&mut self.unit, CanvasUnit::Pixels, "Pixels");
                            ui.radio_value(&mut self.unit, CanvasUnit::Inches, "Inches");
                            ui.radio_value(&mut self.unit, CanvasUnit::Centimeters, "Centimeters");
                        });
                        units.response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::Role::Button,
                                true,
                                format!("Units: {}", self.unit.suffix()),
                            )
                        });
                        units.response.on_hover_text("Measurement units");
                        if let Some(point) = self.cursor_point {
                            ui.label(format!(
                                "{}, {} {}",
                                self.unit.format_value(f64::from(point.x)),
                                self.unit.format_value(f64::from(point.y)),
                                self.unit.suffix()
                            ))
                            .on_hover_text("Cursor position");
                        } else {
                            ui.label(format!("—, — {}", self.unit.suffix()))
                                .on_hover_text("Cursor position");
                        }
                    });
                });
            });
    }

    fn canvas_panel(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            let size = self.session.document().size();
            if self.fit_canvas_requested {
                let available = ui.available_size();
                let gutter = if self.show_rulers {
                    RULER_THICKNESS
                } else {
                    0.0
                };
                self.zoom = ((available.x - gutter - 24.0).max(32.0) / size.width() as f32)
                    .min((available.y - gutter - 24.0).max(32.0) / size.height() as f32)
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
                    let gutter = if self.show_rulers {
                        RULER_THICKNESS
                    } else {
                        0.0
                    };
                    self.zoom = ((available.x - gutter - 24.0).max(32.0)
                        / selection.width() as f32)
                        .min((available.y - gutter - 24.0).max(32.0) / selection.height() as f32)
                        .clamp(0.05, 8.0);
                }
                self.zoom_to_selection_requested = false;
            }
            let texture = self.texture.clone();
            let zoom = self.zoom;
            let picking = self.picking_color;
            let ruler_thickness = self.show_rulers.then_some(RULER_THICKNESS);
            self.ruler_layout = None;
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
                        let (rect, response, block_rect) = if let Some(thickness) = ruler_thickness
                        {
                            let block = egui::vec2(display.x + thickness, display.y + thickness);
                            let (block_rect, _) =
                                ui.allocate_exact_size(block, egui::Sense::hover());
                            let rect = egui::Rect::from_min_size(
                                block_rect.min + egui::vec2(thickness, thickness),
                                display,
                            );
                            let response = ui.interact(
                                rect,
                                ui.scope_id().with("paint-r-canvas-surface"),
                                sense,
                            );
                            (rect, response, block_rect)
                        } else {
                            let (rect, response) = ui.allocate_exact_size(display, sense);
                            (rect, response, rect)
                        };
                        response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::Role::Image,
                                true,
                                "Composed document canvas",
                            )
                        });
                        let response = response.on_hover_cursor(egui::CursorIcon::Crosshair);
                        if ruler_thickness.is_some() {
                            let layout = crate::rulers::layout(self.unit, zoom, rect.min, size);
                            crate::rulers::paint(ui, block_rect, rect, &layout);
                            self.ruler_layout = Some(layout);
                        }
                        if ui.is_rect_visible(rect) {
                            paint_checkerboard(ui.painter(), rect);
                            egui::Image::from_texture(&texture).paint_at(ui, rect);
                        }
                        if let Some(floating) = &self.floating_selection {
                            let min = rect.min
                                + egui::vec2(
                                    (floating.floating.bounds.left() as f64 + floating.offset_x)
                                        as f32
                                        * zoom,
                                    (floating.floating.bounds.top() as f64 + floating.offset_y)
                                        as f32
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
                        let hide_selection_for_move = dragging_selection
                            && matches!(self.active_tool, Tool::Move | Tool::MoveSelection);
                        if !hide_selection_for_move
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
                            && let Some((source_x, source_y)) = self.clone_marker_point()
                        {
                            let marker = rect.min
                                + egui::vec2(source_x as f32 * zoom, source_y as f32 * zoom);
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
                                        sampling: self.fill_sampling,
                                        blend: self.fill_blend,
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
                                self.text_editor_focus_requested = true;
                                self.text_editor_focus_release = false;
                                self.status = format!(
                                    "Text at ({}, {}): type to edit, Enter places, Shift+Enter adds \
                                     a line, Escape cancels",
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
                            let canvas_position = || {
                                pointer.and_then(|position| {
                                    canvas_position_at(position, rect, zoom, size)
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
                                if self.active_tool == Tool::Gradient {
                                    self.session.begin_preview();
                                    self.preview_live_gradient();
                                }
                                if self.active_tool == Tool::Move {
                                    self.move_drag_origin = canvas_position();
                                    self.move_drag_current = self.move_drag_origin;
                                    if self.session.document().selection().is_some() {
                                        self.lift_floating_selection(ui.ctx());
                                        if let Some(floating) = self.floating_selection.as_mut() {
                                            floating.base_x = floating.offset_x;
                                            floating.base_y = floating.offset_y;
                                        }
                                    } else {
                                        if !self.session.has_preview() {
                                            self.layer_move_offset = (0.0, 0.0);
                                            self.session.begin_preview();
                                        }
                                        self.layer_move_base = self.layer_move_offset;
                                    }
                                }
                            }
                            if response.dragged()
                                && let Some(point) = point()
                            {
                                let drag_point_changed = self.drag_current != Some(point);
                                self.drag_current = Some(point);
                                let previous_move_position = self.move_drag_current;
                                if self.active_tool == Tool::Move
                                    && let Some(position) = canvas_position()
                                {
                                    self.move_drag_current = Some(position);
                                }
                                let move_position_changed =
                                    self.move_drag_current != previous_move_position;
                                if self.active_tool == Tool::Move
                                    && let (Some(origin), Some(current)) =
                                        (self.move_drag_origin, self.move_drag_current)
                                    && let Some(floating) = self.floating_selection.as_mut()
                                {
                                    floating.offset_x = floating.base_x + (current.0 - origin.0);
                                    floating.offset_y = floating.base_y + (current.1 - origin.1);
                                }
                                if collects_points && self.paint_points.last() != Some(&point) {
                                    self.paint_points.push(point);
                                    if is_paint {
                                        self.preview_live_stroke();
                                    }
                                }
                                if self.active_tool == Tool::Gradient && drag_point_changed {
                                    self.preview_live_gradient();
                                }
                                if self.active_tool == Tool::Move
                                    && self.floating_selection.is_none()
                                    && move_position_changed
                                {
                                    self.preview_live_translate();
                                }
                            }
                            if let (Some(start), Some(end)) = (self.drag_start, self.drag_current) {
                                let start_pos = rect.min
                                    + egui::vec2(start.x as f32 * zoom, start.y as f32 * zoom);
                                let end_pos = rect.min
                                    + egui::vec2(
                                        (end.x + 1) as f32 * zoom,
                                        (end.y + 1) as f32 * zoom,
                                    );
                                let marquee = egui::Rect::from_two_pos(start_pos, end_pos);
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
                                    ui.painter().add(egui::Shape::convex_polygon(
                                        ellipse,
                                        egui::Color32::from_rgba_unmultiplied(34, 112, 200, 50),
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
                                    ui.painter().add(egui::Shape::Path(egui::epaint::PathShape {
                                        points: path,
                                        closed: true,
                                        fill: egui::Color32::from_rgba_unmultiplied(
                                            34, 112, 200, 50,
                                        ),
                                        stroke: egui::Stroke::new(1.5, egui::Color32::LIGHT_BLUE)
                                            .into(),
                                    }));
                                } else if self.active_tool == Tool::Move {
                                    // The floating image already shows the exact pixel shape.
                                    // A fast bounding guide avoids re-tracing a large mask each
                                    // frame while the user drags the lifted pixels.
                                    if let Some(floating) = &self.floating_selection {
                                        let min = rect.min
                                            + egui::vec2(
                                                (floating.floating.bounds.left() as f64
                                                    + floating.offset_x)
                                                    as f32
                                                    * zoom,
                                                (floating.floating.bounds.top() as f64
                                                    + floating.offset_y)
                                                    as f32
                                                    * zoom,
                                            );
                                        let max = rect.min
                                            + egui::vec2(
                                                (floating.floating.bounds.right() as f64
                                                    + floating.offset_x)
                                                    as f32
                                                    * zoom,
                                                (floating.floating.bounds.bottom() as f64
                                                    + floating.offset_y)
                                                    as f32
                                                    * zoom,
                                            );
                                        paint_selection_rect_boundary(
                                            ui.painter(),
                                            egui::Rect::from_min_max(min, max),
                                            phase,
                                        );
                                    }
                                } else if self.active_tool == Tool::MoveSelection {
                                    let dx = i64::from(end.x) - i64::from(start.x);
                                    let dy = i64::from(end.y) - i64::from(start.y);
                                    if let Some(selection) =
                                        self.session.document().selection_model()
                                        && let Ok(translated) = selection.translated(dx, dy)
                                    {
                                        // Move Selection has no floating image: keep the real shape.
                                        paint_selection_boundary(
                                            ui.painter(),
                                            rect,
                                            zoom,
                                            &translated,
                                            phase,
                                        );
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
                                    Tool::Gradient => {
                                        self.session.cancel_preview();
                                        self.texture_dirty = true;
                                        Some((
                                            Command::Gradient {
                                                start,
                                                end,
                                                from: color,
                                                to: self.secondary_color,
                                                options: GradientOptions::new(
                                                    self.gradient_type,
                                                    self.gradient_repeat,
                                                )
                                                .with_alpha_only(self.gradient_alpha_only)
                                                .with_blend(self.gradient_blend),
                                            },
                                            "Painted a gradient".to_owned(),
                                        ))
                                    }
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
                                                sampling: self.recolor_sampling,
                                            },
                                            "Recolored matching pixels".to_owned(),
                                        ))
                                    }
                                    Tool::Move => {
                                        let (dx, dy) =
                                            match (self.move_drag_origin, self.move_drag_current) {
                                                (Some(origin), Some(current)) => (
                                                    (current.0 - origin.0).round() as i64,
                                                    (current.1 - origin.1).round() as i64,
                                                ),
                                                _ => (
                                                    i64::from(end.x) - i64::from(start.x),
                                                    i64::from(end.y) - i64::from(start.y),
                                                ),
                                            };
                                        self.move_drag_origin = None;
                                        self.move_drag_current = None;
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
                                            self.status =
                                                "Moved the layer; click Finish to place it"
                                                    .to_owned();
                                            None
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
                        }
                        self.inline_text_editor(ui, rect, zoom);
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
                PathDialogKind::SaveAs | PathDialogKind::Export => match self.session.compose() {
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
            PathDialogKind::SaveAs => {
                let path = PathBuf::from(path);
                match self.session.save_png_atomic(&path) {
                    Ok(()) => {
                        self.document_title = file_title(&path);
                        self.document_path = Some(path.clone());
                        self.texture_dirty = true;
                        self.status = format!("Saved PNG: {}", path.display());
                        self.path_dialog = None;
                    }
                    Err(error) => {
                        dialog.focus_path = true;
                        self.status = format!("Save failed: {error}");
                    }
                }
            }
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
            || self.new_image_dialog.is_some()
            || self.resize_dialog.is_some()
            || self.canvas_size_dialog.is_some()
            || self.pending_discard.is_some()
            || self.effect_dialog.is_some()
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
        // Paint.NET's Commit button advertises Enter as "Finish" while Move is previewing.
        if self.active_tool == Tool::Move
            && (self.floating_selection.is_some() || self.session.has_preview())
            && context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            self.finish_move();
            return;
        }
        let command = egui::Modifiers::COMMAND;
        let command_shift = command | egui::Modifiers::SHIFT;
        let command_alt = command | egui::Modifiers::ALT;

        // Paint.NET reserves Ctrl+Shift+C for Copy Merged, which is not implemented here; consume
        // it so a plain Copy does not run instead.
        if shortcut_pressed(context, command_shift, egui::Key::C) {
            return;
        }
        if shortcut_pressed(context, command_shift, egui::Key::V) {
            self.paste_into_new_layer(context);
            return;
        }
        if shortcut_pressed(context, command_alt, egui::Key::V) {
            self.paste_into_new_image(context);
            return;
        }
        if shortcut_pressed(context, command, egui::Key::C) {
            self.copy_to_clipboard(context);
            return;
        }
        if shortcut_pressed(context, command, egui::Key::X) {
            self.cut_to_clipboard(context);
            return;
        }
        if shortcut_pressed(context, command, egui::Key::V) {
            self.paste_clipboard(context);
            return;
        }

        if context
            .input_mut(|input| input.consume_key(egui::Modifiers::SHIFT, egui::Key::Backspace))
        {
            if self.has_selection() {
                let color = self.secondary_color;
                self.execute(
                    Command::FillSelection(color),
                    "Filled the selection with the secondary color",
                );
            }
            return;
        }
        if context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Backspace))
        {
            if self.has_selection() {
                let color = self.fill_color;
                self.execute(
                    Command::FillSelection(color),
                    "Filled the selection with the primary color",
                );
            }
            return;
        }
        if context.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Delete)) {
            if self.has_selection() {
                self.execute(Command::EraseSelection, "Erased the selected pixels");
            }
            return;
        }

        if shortcut_pressed(context, command, egui::Key::Q)
            || shortcut_pressed(context, command, egui::Key::W)
        {
            self.request_quit(context);
            return;
        }
        if shortcut_pressed(context, command_shift, egui::Key::S) {
            self.show_save_as_dialog();
            return;
        }
        if shortcut_pressed(context, command, egui::Key::S) {
            if self.document_path.is_some() {
                self.save_active_document();
            } else {
                self.show_save_as_dialog();
            }
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
        if shortcut_pressed(context, command_shift, egui::Key::I) {
            self.execute(Command::Invert, "Inverted colors on the active layer");
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
            if self.has_selection() {
                self.execute(Command::InvertSelection, "Inverted the selection");
            }
            return;
        }
        if shortcut_pressed(context, command, egui::Key::A) {
            self.execute(Command::SelectAll, "Selected the whole canvas");
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
        if shortcut_pressed(context, command, egui::Key::D) {
            if self.has_selection() {
                self.execute(Command::ClearSelection, "Selection cleared");
            }
            return;
        }
        if shortcut_pressed(context, command, egui::Key::Z) {
            self.execute(Command::Undo, "Undid the last edit");
            return;
        }
        self.handle_tool_hotkeys(context);
    }

    /// Reference plain-key tool shortcuts. Tools sharing a key cycle in toolbox order.
    fn handle_tool_hotkeys(&mut self, context: &egui::Context) {
        if context.input(|input| input.pointer.any_down()) {
            return;
        }
        for shortcut in TOOL_HOTKEYS {
            let key = hotkey_key(shortcut);
            let reverse = context.input_mut(|input| {
                if input.consume_key(egui::Modifiers::SHIFT, key) {
                    Some(true)
                } else if input.consume_key(egui::Modifiers::NONE, key) {
                    Some(false)
                } else {
                    None
                }
            });
            if let Some(reverse) = reverse {
                self.cycle_tool_hotkey(shortcut, reverse);
                return;
            }
        }
    }

    fn cycle_tool_hotkey(&mut self, shortcut: char, reverse: bool) {
        let candidates: Vec<usize> = TOOLBOX
            .iter()
            .enumerate()
            .filter(|(_, spec)| spec.entry.shortcut == shortcut)
            .map(|(index, _)| index)
            .collect();
        if candidates.is_empty() {
            return;
        }
        let current = TOOLBOX
            .iter()
            .position(|spec| spec.tool == self.active_tool)
            .and_then(|index| candidates.iter().position(|candidate| *candidate == index));
        let next = match current {
            Some(index) if reverse => (index + candidates.len() - 1) % candidates.len(),
            Some(index) => (index + 1) % candidates.len(),
            None if reverse => candidates.len() - 1,
            None => 0,
        };
        let tool = TOOLBOX[candidates[next]].tool;
        self.select_tool(tool);
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
        self.handle_inline_text_keys(context);
        self.handle_keyboard_shortcuts(context);
        self.handle_close_request(context);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.text_editor_focus_release {
            ui.ctx().memory_mut(|memory| {
                memory.surrender_focus(inline_text_editor_id());
                memory.surrender_focus(text_content_field_id());
            });
            self.text_editor_focus_release = false;
        }
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
        self.effect_dialog_window(ui.ctx());
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

/// One cell in the Paint.NET two-column toolbox.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ToolboxEntry {
    /// Accessible button label, matching the reference tool name.
    pub label: &'static str,
    /// Reference hotkey. Tools sharing a key cycle in toolbox order.
    pub shortcut: char,
    /// Tooltip hint describing the shortcut or the cycle it belongs to.
    pub hint: &'static str,
}

impl ToolboxEntry {
    /// Two-line hover text: the tool name and its reference shortcut hint.
    pub fn tooltip(self) -> String {
        format!("{}\nShortcut: {}", self.label, self.hint)
    }
}

struct ToolSpec {
    entry: ToolboxEntry,
    tool: Tool,
    icon: Icon,
}

macro_rules! tool_spec {
    ($tool:ident, $icon:expr, $label:expr, $shortcut:expr, $hint:expr) => {
        ToolSpec {
            entry: ToolboxEntry {
                label: $label,
                shortcut: $shortcut,
                hint: $hint,
            },
            tool: Tool::$tool,
            icon: $icon,
        }
    };
}

/// Exact Paint.NET `DocumentWorkspace` static-constructor order: 19 enabled tools.
const TOOLBOX: [ToolSpec; 19] = [
    tool_spec!(
        Select,
        Icon::SelectRectangle,
        "Rectangle Select",
        'S',
        "S cycles selection tools"
    ),
    tool_spec!(
        Move,
        Icon::Move,
        "Move Selected Pixels",
        'M',
        "M cycles move tools"
    ),
    tool_spec!(
        Lasso,
        Icon::LassoSelect,
        "Lasso Select",
        'S',
        "S cycles selection tools"
    ),
    tool_spec!(
        MoveSelection,
        Icon::MoveSelection,
        "Move Selection",
        'M',
        "M cycles move tools"
    ),
    tool_spec!(
        EllipseSelect,
        Icon::Ellipse,
        "Ellipse Select",
        'S',
        "S cycles selection tools"
    ),
    tool_spec!(Zoom, Icon::Zoom, "Zoom", 'Z', "Z"),
    tool_spec!(
        MagicWand,
        Icon::MagicWand,
        "Magic Wand",
        'S',
        "S cycles selection tools"
    ),
    tool_spec!(Pan, Icon::Pan, "Pan", 'H', "H"),
    tool_spec!(Fill, Icon::Fill, "Paint Bucket", 'F', "F"),
    tool_spec!(Gradient, Icon::Gradient, "Gradient", 'G', "G"),
    tool_spec!(Brush, Icon::Brush, "Paintbrush", 'B', "B"),
    tool_spec!(Eraser, Icon::Eraser, "Eraser", 'E', "E"),
    tool_spec!(Pencil, Icon::Pencil, "Pencil", 'P', "P"),
    tool_spec!(Picker, Icon::EyeDropper, "Color Picker", 'K', "K"),
    tool_spec!(CloneStamp, Icon::CloneStamp, "Clone Stamp", 'L', "L"),
    tool_spec!(Recolor, Icon::Recolor, "Recolor", 'R', "R"),
    tool_spec!(Text, Icon::Text, "Text", 'T', "T"),
    tool_spec!(
        Line,
        Icon::Line,
        "Line / Curve",
        'O',
        "O cycles line and shapes"
    ),
    tool_spec!(
        Shapes,
        Icon::Shapes,
        "Shapes",
        'O',
        "O cycles line and shapes"
    ),
];

/// Every reference toolbox hotkey, in the order Shift reverses each cycle.
const TOOL_HOTKEYS: [char; 14] = [
    'S', 'M', 'Z', 'H', 'F', 'G', 'B', 'E', 'P', 'K', 'L', 'R', 'T', 'O',
];

fn hotkey_key(shortcut: char) -> egui::Key {
    match shortcut {
        'S' => egui::Key::S,
        'M' => egui::Key::M,
        'Z' => egui::Key::Z,
        'H' => egui::Key::H,
        'F' => egui::Key::F,
        'G' => egui::Key::G,
        'B' => egui::Key::B,
        'E' => egui::Key::E,
        'P' => egui::Key::P,
        'K' => egui::Key::K,
        'L' => egui::Key::L,
        'R' => egui::Key::R,
        'T' => egui::Key::T,
        'O' => egui::Key::O,
        _ => unreachable!("every toolbox hotkey has an egui key"),
    }
}

const RESAMPLE_MODES: [ResampleMode; 5] = [
    ResampleMode::NearestNeighbor,
    ResampleMode::Bilinear,
    ResampleMode::MultisampleBilinear,
    ResampleMode::Anisotropic,
    ResampleMode::Bicubic,
];

/// Reference labels from `MoveToolInterpolationMode`; `HighQualityCubic` displays as
/// "Bicubic" and is Paint.NET's default.
const fn resample_label(mode: ResampleMode) -> &'static str {
    match mode {
        ResampleMode::NearestNeighbor => "Nearest Neighbor",
        ResampleMode::Bilinear => "Bilinear",
        ResampleMode::MultisampleBilinear => "Multisample Bilinear",
        ResampleMode::Anisotropic => "Anisotropic",
        ResampleMode::Bicubic => "Bicubic",
    }
}

const fn resample_hint(mode: ResampleMode) -> &'static str {
    match mode {
        ResampleMode::NearestNeighbor => "Snap to whole source pixels; no blending",
        ResampleMode::Bilinear => "Bilinear filtering with premultiplied alpha",
        ResampleMode::MultisampleBilinear => "Four bilinear samples per destination pixel",
        ResampleMode::Anisotropic => "Not supported by translate-only Move",
        ResampleMode::Bicubic => "Catmull-Rom bicubic; Paint.NET's default mode",
    }
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
    Average11x11,
    Average31x31,
    Average51x51,
}

impl PickerSampleSize {
    const fn side(self) -> u32 {
        match self {
            Self::Point => 1,
            Self::Average3x3 => 3,
            Self::Average5x5 => 5,
            Self::Average11x11 => 11,
            Self::Average31x31 => 31,
            Self::Average51x51 => 51,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Point => "1 px",
            Self::Average3x3 => "3×3 average",
            Self::Average5x5 => "5×5 average",
            Self::Average11x11 => "11×11 average",
            Self::Average31x31 => "31×31 average",
            Self::Average51x51 => "51×51 average",
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
pub(crate) enum CanvasUnit {
    Pixels,
    Inches,
    Centimeters,
}

impl CanvasUnit {
    pub(crate) const fn suffix(self) -> &'static str {
        match self {
            Self::Pixels => "px",
            Self::Inches => "in",
            Self::Centimeters => "cm",
        }
    }

    pub(crate) fn convert(self, pixels: f64) -> f64 {
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
    SaveAs,
    Export,
}

impl PathDialogKind {
    const fn title(self) -> &'static str {
        match self {
            Self::Open => "Open PNG (flattened)",
            Self::SaveAs => "Save PNG As",
            Self::Export => "Export PNG (flattened)",
        }
    }

    const fn action(self) -> &'static str {
        match self {
            Self::Open => "Open PNG",
            Self::SaveAs => "Save",
            Self::Export => "Export PNG",
        }
    }

    const fn explanation(self) -> &'static str {
        match self {
            Self::Open => "Opening creates one flattened layer; source layers cannot be recovered.",
            Self::SaveAs => {
                "Save writes composed pixels to the PNG and marks the document clean; layers remain in this session but are not stored in PNG."
            }
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
enum LevelsChannelKind {
    Rgb,
    Red,
    Green,
    Blue,
}

impl LevelsChannelKind {
    const ALL: [Self; 4] = [Self::Rgb, Self::Red, Self::Green, Self::Blue];

    const fn label(self) -> &'static str {
        match self {
            Self::Rgb => "RGB",
            Self::Red => "Red",
            Self::Green => "Green",
            Self::Blue => "Blue",
        }
    }
}

fn levels_channel_mut(
    settings: &mut LevelsSettings,
    channel: LevelsChannelKind,
) -> &mut LevelsChannel {
    match channel {
        LevelsChannelKind::Rgb => &mut settings.rgb,
        LevelsChannelKind::Red => &mut settings.red,
        LevelsChannelKind::Green => &mut settings.green,
        LevelsChannelKind::Blue => &mut settings.blue,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EffectDialog {
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
    Levels {
        channel: LevelsChannelKind,
        settings: LevelsSettings,
    },
    Posterize {
        levels: u8,
    },
    BlackAndWhite {
        red_weight: u8,
        green_weight: u8,
        blue_weight: u8,
    },
    Threshold {
        level: u8,
    },
    MotionBlur {
        angle: i32,
        distance: u32,
    },
    Sharpen {
        amount: u32,
    },
    AddNoise {
        intensity: u32,
        seed: u64,
    },
}

impl EffectDialog {
    const fn title(self) -> &'static str {
        match self {
            Self::BrightnessContrast { .. } => "Brightness / Contrast",
            Self::HueSaturation { .. } => "Hue / Saturation",
            Self::AutoLevel => "Auto-Level",
            Self::Levels { .. } => "Levels",
            Self::Posterize { .. } => "Posterize",
            Self::BlackAndWhite { .. } => "Black and White",
            Self::Threshold { .. } => "Threshold",
            Self::MotionBlur { .. } => "Motion Blur",
            Self::Sharpen { .. } => "Sharpen",
            Self::AddNoise { .. } => "Add Noise",
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
            Self::Levels { settings, .. } => Effect::Levels(settings),
            Self::Posterize { levels } => Effect::Posterize { levels },
            Self::BlackAndWhite {
                red_weight,
                green_weight,
                blue_weight,
            } => Effect::BlackAndWhite {
                red_weight,
                green_weight,
                blue_weight,
            },
            Self::Threshold { level } => Effect::Threshold { level },
            Self::MotionBlur { angle, distance } => Effect::MotionBlur { angle, distance },
            Self::Sharpen { amount } => Effect::Sharpen { amount },
            Self::AddNoise { intensity, seed } => Effect::AddNoise { intensity, seed },
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
            Self::Levels { channel, settings } => {
                ui.label(
                    "Clip and stretch input tones, bend the midtones with gamma, then remap the \
                     output range.",
                );
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Channel");
                    for kind in LevelsChannelKind::ALL {
                        ui.radio_value(channel, kind, kind.label());
                    }
                });
                ui.separator();
                let levels = levels_channel_mut(settings, *channel);
                ui.add(
                    egui::Slider::new(&mut levels.input_black, 0..=levels.input_white)
                        .text("Input black"),
                );
                ui.add(
                    egui::Slider::new(&mut levels.input_white, levels.input_black..=255)
                        .text("Input white"),
                );
                ui.add(
                    egui::Slider::new(
                        &mut levels.gamma_hundredths,
                        MIN_LEVELS_GAMMA_HUNDREDTHS..=MAX_LEVELS_GAMMA_HUNDREDTHS,
                    )
                    .text("Gamma")
                    .custom_formatter(|value, _| format!("{:.2}", value / 100.0)),
                );
                ui.add(
                    egui::Slider::new(&mut levels.output_black, 0..=levels.output_white)
                        .text("Output black"),
                );
                ui.add(
                    egui::Slider::new(&mut levels.output_white, levels.output_black..=255)
                        .text("Output white"),
                );
                ui.label(
                    "RGB applies to every channel first; Red, Green, and Blue refine one channel.",
                );
            }
            Self::Posterize { levels } => {
                ui.label("Quantize each RGB channel to evenly spaced tonal levels.");
                ui.separator();
                ui.add(
                    egui::Slider::new(levels, MIN_POSTERIZE_LEVELS..=MAX_POSTERIZE_LEVELS)
                        .text("Levels"),
                );
                ui.label("2 levels produces pure black and white; 64 is nearly continuous.");
            }
            Self::BlackAndWhite {
                red_weight,
                green_weight,
                blue_weight,
            } => {
                ui.label(
                    "Convert the selection to black and white with adjustable channel weights.",
                );
                ui.separator();
                ui.add(
                    egui::Slider::new(red_weight, 0..=MAX_BLACK_AND_WHITE_WEIGHT)
                        .text("Red weight"),
                );
                ui.add(
                    egui::Slider::new(green_weight, 0..=MAX_BLACK_AND_WHITE_WEIGHT)
                        .text("Green weight"),
                );
                ui.add(
                    egui::Slider::new(blue_weight, 0..=MAX_BLACK_AND_WHITE_WEIGHT)
                        .text("Blue weight"),
                );
                ui.label(
                    "Weights are percentages of the luminance sum; setting all three to zero is \
                     a no-op.",
                );
            }
            Self::Threshold { level } => {
                ui.label(
                    "Map Rec. 601 luminance at or above the level to white, and below it to black.",
                );
                ui.separator();
                ui.add(egui::Slider::new(level, 0..=255).text("Threshold"));
                ui.label("Alpha is preserved; only the RGB channels become black or white.");
            }
            Self::MotionBlur { angle, distance } => {
                ui.label("Blur the selection along a direction by sampling a line of pixels.");
                ui.separator();
                ui.add(egui::Slider::new(angle, -180..=180).text("Angle"));
                ui.add(egui::Slider::new(distance, 0..=MAX_MOTION_BLUR_DISTANCE).text("Distance"));
                ui.label("A distance of zero leaves the selection unchanged.");
            }
            Self::Sharpen { amount } => {
                ui.label("Use an unsharp mask to accent edges in the selection.");
                ui.separator();
                ui.add(egui::Slider::new(amount, 0..=MAX_SHARPEN_AMOUNT).text("Amount"));
                ui.label("An amount of zero leaves the selection unchanged.");
            }
            Self::AddNoise { intensity, seed } => {
                ui.label("Add deterministic RGB noise without changing alpha.");
                ui.separator();
                ui.add(egui::Slider::new(intensity, 0..=MAX_NOISE_INTENSITY).text("Intensity"));
                ui.add(egui::Slider::new(seed, 0..=u64::from(u32::MAX)).text("Seed"));
                ui.label("A fixed seed reproduces the same preview and result.");
                ui.label("Zero intensity leaves the selection unchanged.");
            }
        }
    }
}

struct NewImageDialogState {
    width: String,
    height: String,
    background: CanvasFill,
}

impl NewImageDialogState {
    fn new() -> Self {
        Self {
            width: DEFAULT_WIDTH.to_string(),
            height: DEFAULT_HEIGHT.to_string(),
            background: CanvasFill::White,
        }
    }

    fn validated_size(&self, limits: ResourceLimits) -> Result<Size, String> {
        let width = parse_dimension(&self.width, "New image width")?;
        let height = parse_dimension(&self.height, "New image height")?;
        let size = Size::new(width, height)
            .map_err(|_| "Width and height must be at least 1 pixel".to_owned())?;
        if size.width() > limits.max_width || size.height() > limits.max_height {
            return Err(format!(
                "New image {}×{} exceeds the maximum {}×{}",
                size.width(),
                size.height(),
                limits.max_width,
                limits.max_height
            ));
        }
        let pixels = u64::from(size.width()) * u64::from(size.height());
        if pixels > limits.max_pixels {
            return Err(format!(
                "New image needs {pixels} pixels; the maximum is {}",
                limits.max_pixels
            ));
        }
        Ok(size)
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

impl CanvasFill {
    const fn color(self, primary: Bgra, secondary: Bgra) -> Bgra {
        match self {
            Self::Transparent => Bgra::TRANSPARENT,
            Self::White => Bgra::WHITE,
            Self::Black => Bgra::BLACK,
            Self::Primary => primary,
            Self::Secondary => secondary,
        }
    }
}

const CANVAS_FILLS: [(CanvasFill, &str); 5] = [
    (CanvasFill::Transparent, "Fill: transparent"),
    (CanvasFill::White, "Fill: white"),
    (CanvasFill::Black, "Fill: black"),
    (CanvasFill::Primary, "Fill: primary color"),
    (CanvasFill::Secondary, "Fill: secondary color"),
];

/// Paint.NET's `CanvasSizeFillColor` order, used by the New Image background selector.
const NEW_IMAGE_BACKGROUNDS: [(CanvasFill, &str); 5] = [
    (CanvasFill::Transparent, "Background: transparent"),
    (CanvasFill::Primary, "Background: primary color"),
    (CanvasFill::Secondary, "Background: secondary color"),
    (CanvasFill::White, "Background: white"),
    (CanvasFill::Black, "Background: black"),
];

const fn background_label(fill: Bgra) -> &'static str {
    match fill {
        Bgra::TRANSPARENT => "transparent",
        Bgra::WHITE => "opaque-white",
        Bgra::BLACK => "opaque-black",
        _ => "primary-color",
    }
}

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

/// Default desktop parking slots for the four floating panels.
///
/// Mirrors Paint.NET's `AppWorkspace.ResetFloatingForm`:
/// Tools top-left, History top-right, Layers bottom-right, Colors bottom-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelLayout {
    pub tools: egui::Rect,
    pub colors: egui::Rect,
    pub layers: egui::Rect,
    pub history: egui::Rect,
}

/// Computes the default floating-panel rectangles for a workspace viewport.
///
/// `panels_floating` mirrors the Window → Float Panels state. In the docked
/// layout the right dock occupies the screen edge, so History is parked beside
/// the expanded dock width to keep the dock's controls clear.
#[must_use]
pub fn default_panel_layout(
    screen: egui::Rect,
    panels_floating: bool,
    colors_expanded: bool,
) -> PanelLayout {
    let left = screen.left() + PANEL_SCREEN_MARGIN;
    let right = screen.right() - PANEL_SCREEN_MARGIN;
    let top = screen.top() + PANEL_TOP_OFFSET;
    let bottom = screen.bottom() - STATUS_BAR_HEIGHT - PANEL_SCREEN_MARGIN;
    let tools_top = top.max(180.0);
    let history_right = if panels_floating {
        right
    } else {
        screen.right() - COLORS_PANEL_MORE_SIZE[0] - PANEL_SCREEN_MARGIN
    };
    let colors_size = if colors_expanded {
        COLORS_PANEL_MORE_SIZE
    } else {
        COLORS_PANEL_LESS_SIZE
    };
    PanelLayout {
        tools: egui::Rect::from_min_size(
            egui::pos2(left, tools_top),
            egui::vec2(TOOLS_PANEL_WIDTH, 305.0),
        ),
        colors: egui::Rect::from_min_size(
            egui::pos2(left, bottom - colors_size[1]),
            egui::vec2(colors_size[0], colors_size[1]),
        ),
        layers: egui::Rect::from_min_size(
            egui::pos2(right - LAYERS_PANEL_SIZE[0], bottom - LAYERS_PANEL_SIZE[1]),
            egui::vec2(LAYERS_PANEL_SIZE[0], LAYERS_PANEL_SIZE[1]),
        ),
        history: egui::Rect::from_min_size(
            egui::pos2(history_right - HISTORY_PANEL_SIZE[0], top),
            egui::vec2(HISTORY_PANEL_SIZE[0], HISTORY_PANEL_SIZE[1]),
        ),
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

fn install_inline_text_fonts(context: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (family, bytes) in [
        (
            TEXT_FONT_SANS,
            &include_bytes!("../../raster/assets/fonts/Roboto-Variable.ttf")[..],
        ),
        (
            TEXT_FONT_SANS_ITALIC,
            &include_bytes!("../../raster/assets/fonts/Roboto-Italic-Variable.ttf")[..],
        ),
        (
            TEXT_FONT_MONO,
            &include_bytes!("../../raster/assets/fonts/RobotoMono-Variable.ttf")[..],
        ),
        (
            TEXT_FONT_MONO_ITALIC,
            &include_bytes!("../../raster/assets/fonts/RobotoMono-Italic-Variable.ttf")[..],
        ),
    ] {
        fonts.font_data.insert(
            family.to_owned(),
            Arc::new(egui::FontData::from_static(bytes)),
        );
        fonts.families.insert(
            egui::FontFamily::Name(family.into()),
            vec![family.to_owned()],
        );
    }
    context.set_fonts(fonts);
}

fn configure_style(context: &egui::Context) {
    install_inline_text_fonts(context);
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

fn file_title(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Untitled")
        .to_owned()
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

fn selection_fields_button(ui: &mut egui::Ui, open: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::click());
    let visuals = ui.style().interact_selectable(&response, open);
    if open || response.hovered() {
        ui.painter().rect_filled(rect, 3, visuals.bg_fill);
    }
    let color = ui.visuals().strong_text_color();
    let center = rect.center();
    for offset in [-4.0, 0.0, 4.0] {
        ui.painter()
            .circle_filled(center + egui::vec2(offset, 0.0), 1.3, color);
    }
    let response = response.on_hover_text("Selection fields");
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, true, open, "Selection fields")
    });
    response
}

fn selection_mode_button(
    ui: &mut egui::Ui,
    mode: SelectionCombineMode,
    selected: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::click());
    let visuals = ui.style().interact_selectable(&response, selected);
    let background = if selected || response.hovered() {
        visuals.bg_fill
    } else {
        egui::Color32::TRANSPARENT
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 3, background);
    if selected {
        painter.rect_stroke(
            rect,
            3,
            egui::Stroke::new(1.0, visuals.bg_stroke.color),
            egui::StrokeKind::Inside,
        );
    }
    let icon = rect.shrink(3.0);
    let outline = if selected {
        egui::Color32::WHITE
    } else {
        ui.visuals().strong_text_color()
    };
    paint_selection_mode_icon(painter, icon, mode, outline);
    let response = response.on_hover_text(format!("{} selection mode", mode.label()));
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, true, selected, mode.label())
    });
    response
}

fn paint_selection_mode_icon(
    painter: &egui::Painter,
    rect: egui::Rect,
    mode: SelectionCombineMode,
    outline: egui::Color32,
) {
    let side = (rect.width() * 0.62).round().max(8.0);
    let offset = (rect.width() - side).round().max(3.0);
    let a = egui::Rect::from_min_size(rect.min, egui::vec2(side, side));
    let b = egui::Rect::from_min_size(
        rect.min + egui::vec2(offset, offset),
        egui::vec2(side, side),
    );
    let fill = egui::Color32::from_rgb(120, 190, 255);
    let stroke = egui::Stroke::new(1.0, outline);
    let fill_rect = |target: egui::Rect| {
        painter.rect_filled(target, 0, fill);
    };
    match mode {
        SelectionCombineMode::Replace => fill_rect(a),
        SelectionCombineMode::Union => {
            fill_rect(a);
            fill_rect(b);
        }
        SelectionCombineMode::Subtract => {
            let overlap_left = b.left().max(a.left());
            let overlap_top = b.top().max(a.top());
            fill_rect(egui::Rect::from_min_max(
                a.min,
                egui::pos2(a.right(), overlap_top),
            ));
            fill_rect(egui::Rect::from_min_max(
                egui::pos2(a.left(), overlap_top),
                egui::pos2(overlap_left, a.bottom()),
            ));
        }
        SelectionCombineMode::Intersect => {
            fill_rect(a.intersect(b));
        }
        SelectionCombineMode::Xor => {
            let overlap = a.intersect(b);
            fill_rect(egui::Rect::from_min_max(
                a.min,
                egui::pos2(a.right(), overlap.top()),
            ));
            fill_rect(egui::Rect::from_min_max(
                egui::pos2(a.left(), overlap.top()),
                egui::pos2(overlap.left(), a.bottom()),
            ));
            fill_rect(egui::Rect::from_min_max(
                egui::pos2(overlap.right(), b.top()),
                egui::pos2(b.right(), b.bottom()),
            ));
            fill_rect(egui::Rect::from_min_max(
                egui::pos2(b.left(), overlap.bottom()),
                egui::pos2(b.right(), b.bottom()),
            ));
        }
    }
    painter.rect_stroke(a, 0, stroke, egui::StrokeKind::Inside);
    painter.rect_stroke(b, 0, stroke, egui::StrokeKind::Inside);
}

const fn blend_modes() -> [BlendMode; 4] {
    [
        BlendMode::Normal,
        BlendMode::Multiply,
        BlendMode::Screen,
        BlendMode::Overlay,
    ]
}

/// One Tool Options blend-mode button with a stable accessible name.
fn tool_blend_mode_option(ui: &mut egui::Ui, current: BlendMode, mode: BlendMode) -> bool {
    let accessible = match mode {
        BlendMode::Normal => "Blend mode: Normal",
        BlendMode::Multiply => "Blend mode: Multiply",
        BlendMode::Screen => "Blend mode: Screen",
        BlendMode::Overlay => "Blend mode: Overlay",
    };
    option_selectable(ui, current == mode, blend_mode_label(mode), accessible)
}

/// The `Blend Mode:` group shared by the fill, gradient, and text tool options.
fn tool_blend_mode_row(ui: &mut egui::Ui, current: &mut BlendMode) {
    ui.label("Blend Mode");
    for mode in blend_modes() {
        if tool_blend_mode_option(ui, *current, mode) {
            *current = mode;
        }
    }
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
    ui.add(egui::Slider::new(value, 0..=u8::MAX).text("Layer opacity"))
}

fn panel_header(ui: &mut egui::Ui, title: &str, close_label: &'static str) -> bool {
    let mut closed = false;
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).strong().size(13.0));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            closed = window_close_button(ui, close_label);
        });
    });
    ui.separator();
    closed
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
        Command::FillSelection(_) => Some("Fill Selection".to_owned()),
        Command::EraseSelection => Some("Erase Selection".to_owned()),
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
        | Command::SelectAll
        | Command::InvertSelection
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

fn color_image_from_surface(surface: &Surface) -> Option<egui::ColorImage> {
    let rgba = surface.to_rgba_bytes().ok()?;
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [surface.width() as usize, surface.height() as usize],
        &rgba,
    ))
}

/// Converts an image pasted from another application into clipboard-local surface content.
fn external_clipboard_surface(context: &egui::Context) -> Option<Surface> {
    let image = context.input(|input| {
        input.events.iter().find_map(|event| match event {
            egui::Event::PasteImage(image) => Some(image.clone()),
            _ => None,
        })
    })?;
    let [width, height] = image.size;
    let size = Size::new(u32::try_from(width).ok()?, u32::try_from(height).ok()?).ok()?;
    let pixels = image
        .pixels
        .iter()
        .map(|pixel| Bgra::from_rgba_bytes(pixel.to_srgba_unmultiplied()))
        .collect();
    Surface::from_pixels(size, pixels).ok()
}

fn canvas_point_at(position: egui::Pos2, rect: egui::Rect, zoom: f32, size: Size) -> Option<Point> {
    let x = ((position.x - rect.left()) / zoom).floor();
    let y = ((position.y - rect.top()) / zoom).floor();
    if x < 0.0 || y < 0.0 || x >= size.width() as f32 || y >= size.height() as f32 {
        return None;
    }
    Some(Point::new(x as u32, y as u32))
}

/// Continuous canvas coordinates for subpixel Move drags.
fn canvas_position_at(
    position: egui::Pos2,
    rect: egui::Rect,
    zoom: f32,
    size: Size,
) -> Option<(f64, f64)> {
    let x = f64::from(position.x - rect.left()) / f64::from(zoom);
    let y = f64::from(position.y - rect.top()) / f64::from(zoom);
    if x < 0.0 || y < 0.0 || x >= f64::from(size.width()) || y >= f64::from(size.height()) {
        return None;
    }
    Some((x, y))
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
