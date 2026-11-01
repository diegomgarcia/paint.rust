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
    Icon, IconTextures, MenuIcon, VectorIcon, icon_button, icon_button_at, menu_check_row,
    menu_row, panel_button, tool_button, tool_button_with_tooltip, vector_icon_button,
    vector_icon_button_sized, vector_icon_button_sized_with_tooltip, vector_toggle_button,
    visibility_button_at,
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
/// Paint.NET's ToolConfigStrip uses compact 24 px items on one row.
const TOOL_STRIP_HEIGHT: f32 = 24.0;

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
    tool_options_bounds: Option<egui::Rect>,
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
            tool_options_bounds: None,
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

    /// Screen bounds of the contextual ToolConfigStrip row, for compact-layout tests.
    #[doc(hidden)]
    pub const fn tool_options_bounds(&self) -> Option<egui::Rect> {
        self.tool_options_bounds
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
        let format = SaveFormatChoice::from_path(&path)
            .unwrap_or(SaveFormatChoice::Raster(paint_image_io::SaveFormat::Png));
        match format.save(&mut self.session, &path) {
            Ok(()) => {
                self.texture_dirty = true;
                self.status = format!("Saved {}: {}", format.short_label(), path.display());
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
        let family = self.text_font_family.label();
        strip_combo(
            ui,
            "text-font-family",
            "Font family",
            family,
            &mut self.text_font_family,
            &[
                StripComboItem::new(TextFontFamily::Sans, "Sans", "Font family: Sans", true),
                StripComboItem::new(TextFontFamily::Mono, "Mono", "Font family: Mono", true),
            ],
        );
        strip_drag(
            ui,
            "Size",
            &mut self.text_size,
            1..=paint_raster::MAX_TEXT_SIZE,
            "px",
        );
        if strip_toggle(ui, "Bold", "B", self.text_bold, "Bold") {
            self.text_bold = !self.text_bold;
        }
        if strip_toggle(ui, "Italic", "I", self.text_italic, "Italic") {
            self.text_italic = !self.text_italic;
        }
        for (text, alignment, accessible) in [
            ("L", TextAlignment::Left, "Align left"),
            ("C", TextAlignment::Center, "Align center"),
            ("R", TextAlignment::Right, "Align right"),
        ] {
            if strip_toggle(
                ui,
                accessible,
                text,
                self.text_alignment == alignment,
                accessible,
            ) {
                self.text_alignment = alignment;
            }
        }
        if strip_toggle(
            ui,
            "Antialiasing",
            "AA",
            self.text_antialias,
            "Antialiasing",
        ) {
            self.text_antialias = !self.text_antialias;
        }
        let blend = blend_mode_label(self.text_blend);
        strip_combo(
            ui,
            "text-blend",
            "Blend mode",
            blend,
            &mut self.text_blend,
            &BLEND_MENU_ITEMS,
        );
        strip_separator(ui);
        strip_swatch(ui, "Text color", self.fill_color);
        strip_separator(ui);
        let content_label = ui.label(egui::RichText::new("Content").size(12.0));
        let content_label = content_label.on_hover_text("Text content");
        ui.add(
            egui::TextEdit::multiline(&mut self.text_content)
                .id(text_content_field_id())
                .desired_rows(1)
                .desired_width(160.0)
                .hint_text("Click canvas and type"),
        )
        .labelled_by(content_label.id);
        if strip_icon_action(
            ui,
            VectorIcon::Check,
            "Place text",
            "Place the previewed text",
            self.session.has_preview(),
        ) {
            self.commit_text_preview();
        }
        if strip_icon_action(
            ui,
            VectorIcon::Close,
            "Cancel text",
            "Cancel the text preview (Esc)",
            true,
        ) {
            self.cancel_text_preview();
        }
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
        strip_drag(ui, "Size", &mut self.brush_size, 1..=64, "px");
        strip_drag(ui, "Hardness", &mut self.clone_hardness, 0..=100, "%");
        strip_drag(ui, "Spacing", &mut self.clone_spacing, 1..=100, "%");
        strip_separator(ui);
        if let Some(source) = self.clone_source {
            ui.label(
                egui::RichText::new(format!("Source: ({}, {})", source.x, source.y)).size(12.0),
            )
            .on_hover_text("Clone source pixel");
            if let Some((offset_x, offset_y)) = self.clone_offset {
                ui.label(
                    egui::RichText::new(format!("Offset: ({offset_x}, {offset_y})")).size(12.0),
                );
            }
        } else {
            ui.label(egui::RichText::new("No source").size(12.0))
                .on_hover_text("Alt-click the canvas to set a clone source");
        }
        if strip_icon_action(
            ui,
            VectorIcon::Close,
            "Clear clone source",
            "Clear the clone source",
            self.clone_source.is_some(),
        ) {
            self.clone_source = None;
            self.clone_offset = None;
        }
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
                        "Paint.Rust development preview · MIT first-party license".to_owned();
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
                    compact_menu(ui);
                    let new_response = menu_action(
                        ui,
                        &self.icons,
                        Icon::NewDocument,
                        "New",
                        Some(Shortcut::New),
                        true,
                    );
                    focus_first_menu_item_on_keyboard_open(ui, &new_response);
                    if new_response.clicked() {
                        self.show_new_image_dialog();
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        Icon::OpenFile,
                        "Open PNG…",
                        Some(Shortcut::Open),
                        true,
                    )
                    .clicked()
                    {
                        self.show_path_dialog(PathDialogKind::Open);
                        ui.close();
                    }
                    ui.separator();
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::Save,
                        "Save",
                        Some(Shortcut::Save),
                        self.can_save(),
                    )
                    .clicked()
                    {
                        self.save_active_document();
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::Save,
                        "Save As…",
                        Some(Shortcut::SaveAs),
                        true,
                    )
                    .clicked()
                    {
                        self.show_save_as_dialog();
                        ui.close();
                    }
                    ui.separator();
                    if menu_action(
                        ui,
                        &self.icons,
                        Icon::ExportFile,
                        "Export Image…",
                        Some(Shortcut::Export),
                        true,
                    )
                    .clicked()
                    {
                        self.show_path_dialog(PathDialogKind::Export);
                        ui.close();
                    }
                    ui.separator();
                    if menu_action(
                        ui,
                        &self.icons,
                        MenuIcon::None,
                        "Close Window",
                        Some(Shortcut::Close),
                        true,
                    )
                    .clicked()
                    {
                        self.request_quit(ui.ctx());
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        MenuIcon::None,
                        "Quit",
                        Some(Shortcut::Quit),
                        true,
                    )
                    .clicked()
                    {
                        self.request_quit(ui.ctx());
                        ui.close();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    compact_menu(ui);
                    let undo = menu_action(
                        ui,
                        &self.icons,
                        Icon::Undo,
                        "Undo",
                        Some(Shortcut::Undo),
                        self.session.can_undo(),
                    );
                    focus_first_menu_item_on_keyboard_open(ui, &undo);
                    if undo.clicked() {
                        self.execute(Command::Undo, "Undid the last edit");
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        Icon::Redo,
                        "Redo",
                        Some(Shortcut::Redo),
                        self.session.can_redo(),
                    )
                    .clicked()
                    {
                        self.execute(Command::Redo, "Redid the next edit");
                        ui.close();
                    }
                    ui.separator();
                    let has_selection = self.has_selection();
                    let has_clipboard = self.clipboard.is_some();
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::Cut,
                        "Cut",
                        Some(Shortcut::Cut),
                        has_selection,
                    )
                    .on_hover_text("Copy the selected pixels and erase them")
                    .clicked()
                    {
                        self.cut_to_clipboard(ui.ctx());
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::Copy,
                        "Copy",
                        Some(Shortcut::Copy),
                        true,
                    )
                    .on_hover_text("Copy the selection, or the whole active layer without one")
                    .clicked()
                    {
                        self.copy_to_clipboard(ui.ctx());
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::Paste,
                        "Paste",
                        Some(Shortcut::Paste),
                        has_clipboard,
                    )
                    .on_hover_text("Paste clipboard pixels as a floating selection")
                    .clicked()
                    {
                        self.paste_clipboard(ui.ctx());
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::Paste,
                        "Paste Into New Layer",
                        Some(Shortcut::PasteIntoNewLayer),
                        has_clipboard,
                    )
                    .on_hover_text("Paste clipboard pixels into a new layer of the active document")
                    .clicked()
                    {
                        self.paste_into_new_layer(ui.ctx());
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::Paste,
                        "Paste Into New Image",
                        Some(Shortcut::PasteIntoNewImage),
                        has_clipboard,
                    )
                    .on_hover_text("Create a new image sized to the clipboard pixels")
                    .clicked()
                    {
                        self.paste_into_new_image(ui.ctx());
                        ui.close();
                    }
                    ui.separator();
                    if menu_action(
                        ui,
                        &self.icons,
                        Icon::Eraser,
                        "Erase Selection",
                        Some(Shortcut::EraseSelection),
                        has_selection,
                    )
                    .on_hover_text("Erase the selected pixels and deselect")
                    .clicked()
                    {
                        self.execute(Command::EraseSelection, "Erased the selected pixels");
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        Icon::Fill,
                        "Fill Selection",
                        Some(Shortcut::FillSelection),
                        has_selection,
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
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::InvertSelection,
                        "Invert Selection",
                        Some(Shortcut::InvertSelection),
                        has_selection,
                    )
                    .clicked()
                    {
                        self.execute(Command::InvertSelection, "Inverted the selection");
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::SelectAll,
                        "Select All",
                        Some(Shortcut::SelectAll),
                        true,
                    )
                    .clicked()
                    {
                        self.execute(Command::SelectAll, "Selected the whole canvas");
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::Deselect,
                        "Deselect",
                        Some(Shortcut::Deselect),
                        has_selection,
                    )
                    .clicked()
                    {
                        self.execute(Command::ClearSelection, "Selection cleared");
                        ui.close();
                    }
                });
                ui.menu_button("View", |ui| {
                    compact_menu(ui);
                    let zoom_in = menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::ZoomIn,
                        "Zoom In",
                        None,
                        true,
                    );
                    focus_first_menu_item_on_keyboard_open(ui, &zoom_in);
                    if zoom_in.clicked() {
                        self.zoom = (self.zoom + 0.25).min(8.0);
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::ZoomOut,
                        "Zoom Out",
                        None,
                        true,
                    )
                    .clicked()
                    {
                        self.zoom = (self.zoom - 0.25).max(0.1);
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::FitWindow,
                        "Zoom to Window",
                        None,
                        true,
                    )
                    .clicked()
                    {
                        self.fit_canvas_requested = true;
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        Icon::SelectRectangle,
                        "Zoom to Selection",
                        None,
                        self.session.document().selection().is_some(),
                    )
                    .clicked()
                    {
                        self.zoom_to_selection_requested = true;
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        MenuIcon::None,
                        "Actual Size",
                        None,
                        true,
                    )
                    .clicked()
                    {
                        self.zoom = 1.0;
                        ui.close();
                    }
                    ui.separator();
                    if menu_check_row(
                        ui,
                        &self.icons,
                        VectorIcon::Grid,
                        "Grid",
                        self.show_grid,
                        true,
                    )
                    .clicked()
                    {
                        self.show_grid = !self.show_grid;
                    }
                    if menu_check_row(
                        ui,
                        &self.icons,
                        VectorIcon::Rulers,
                        "Rulers",
                        self.show_rulers,
                        true,
                    )
                    .clicked()
                    {
                        self.show_rulers = !self.show_rulers;
                    }
                    ui.separator();
                    ui.radio_value(&mut self.unit, CanvasUnit::Pixels, "Pixels");
                    ui.radio_value(&mut self.unit, CanvasUnit::Inches, "Inches");
                    ui.radio_value(&mut self.unit, CanvasUnit::Centimeters, "Centimeters");
                });
                ui.menu_button("Image", |ui| {
                    compact_menu(ui);
                    let has_selection = self.session.document().selection().is_some();
                    let image_layers = self.session.document().layers().len();
                    let crop = menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::Crop,
                        "Crop to Selection",
                        None,
                        has_selection,
                    );
                    focus_first_menu_item_on_keyboard_open(ui, &crop);
                    if crop.clicked() {
                        self.execute(
                            Command::CropToSelection,
                            "Cropped the image to the selection",
                        );
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::ResizeImage,
                        "Resize Image…",
                        None,
                        true,
                    )
                    .clicked()
                    {
                        self.show_resize_dialog();
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::CanvasSize,
                        "Canvas Size…",
                        None,
                        true,
                    )
                    .clicked()
                    {
                        self.show_canvas_size_dialog();
                        ui.close();
                    }
                    ui.separator();
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::FlipHorizontal,
                        "Flip Horizontal",
                        None,
                        true,
                    )
                    .clicked()
                    {
                        self.execute(
                            Command::FlipImageHorizontal,
                            "Flipped the image horizontally",
                        );
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::FlipVertical,
                        "Flip Vertical",
                        None,
                        true,
                    )
                    .clicked()
                    {
                        self.execute(Command::FlipImageVertical, "Flipped the image vertically");
                        ui.close();
                    }
                    ui.separator();
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::RotateClockwise,
                        "Rotate 90° Clockwise",
                        None,
                        true,
                    )
                    .clicked()
                    {
                        self.execute(
                            Command::Rotate90Clockwise,
                            "Rotated the image 90° clockwise",
                        );
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::RotateCounterClockwise,
                        "Rotate 90° Counter-Clockwise",
                        None,
                        true,
                    )
                    .clicked()
                    {
                        self.execute(
                            Command::Rotate90CounterClockwise,
                            "Rotated the image 90° counter-clockwise",
                        );
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::Rotate180,
                        "Rotate 180°",
                        None,
                        true,
                    )
                    .clicked()
                    {
                        self.execute(Command::Rotate180, "Rotated the image 180°");
                        ui.close();
                    }
                    ui.separator();
                    ui.add_enabled(false, egui::Button::new("Color Profile…"));
                    ui.separator();
                    if menu_action(
                        ui,
                        &self.icons,
                        Icon::LayerMerge,
                        "Flatten Image",
                        None,
                        image_layers > 1,
                    )
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
                    compact_menu(ui);
                    let can_move_up = active_layer_index + 1 < layer_count;
                    let can_move_down = active_layer_index > 0;
                    let add = menu_action(
                        ui,
                        &self.icons,
                        Icon::AddLayer,
                        "Add New Layer",
                        Some(Shortcut::AddLayer),
                        true,
                    );
                    focus_first_menu_item_on_keyboard_open(ui, &add);
                    if add.clicked() {
                        self.add_layer();
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        Icon::LayerDelete,
                        "Delete Layer",
                        None,
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
                    if menu_action(
                        ui,
                        &self.icons,
                        Icon::LayerDuplicate,
                        "Duplicate Layer",
                        None,
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
                    if menu_action(
                        ui,
                        &self.icons,
                        Icon::LayerMerge,
                        "Merge Layer Down",
                        None,
                        can_move_down,
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
                    if menu_action(
                        ui,
                        &self.icons,
                        if active_layer_visible {
                            Icon::Eye
                        } else {
                            Icon::EyeOff
                        },
                        "Toggle Layer Visibility",
                        None,
                        true,
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
                    menu_action(
                        ui,
                        &self.icons,
                        MenuIcon::None,
                        "Import From File…",
                        None,
                        false,
                    );
                    ui.separator();
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::FlipHorizontal,
                        "Flip Active Layer Horizontally",
                        None,
                        true,
                    )
                    .clicked()
                    {
                        self.execute(
                            Command::FlipLayerHorizontal,
                            "Flipped the active layer horizontally",
                        );
                        ui.close();
                    }
                    if menu_action(
                        ui,
                        &self.icons,
                        VectorIcon::FlipVertical,
                        "Flip Active Layer Vertically",
                        None,
                        true,
                    )
                    .clicked()
                    {
                        self.execute(
                            Command::FlipLayerVertical,
                            "Flipped the active layer vertically",
                        );
                        ui.close();
                    }
                    menu_action(
                        ui,
                        &self.icons,
                        MenuIcon::None,
                        "Rotate 180°",
                        None,
                        false,
                    );
                    menu_action(
                        ui,
                        &self.icons,
                        MenuIcon::None,
                        "Rotate / Zoom…",
                        None,
                        false,
                    );
                    ui.separator();
                    for (label, target, enabled) in [
                        (
                            "Go to Top Layer",
                            layer_count.saturating_sub(1),
                            can_move_up,
                        ),
                        (
                            "Go to Layer Above",
                            (active_layer_index + 1).min(layer_count - 1),
                            can_move_up,
                        ),
                        (
                            "Go to Layer Below",
                            active_layer_index.saturating_sub(1),
                            can_move_down,
                        ),
                        ("Go to Bottom Layer", 0, can_move_down),
                    ] {
                        if menu_action(
                            ui,
                            &self.icons,
                            MenuIcon::None,
                            label,
                            None,
                            enabled,
                        )
                        .clicked()
                        {
                            self.execute(
                                Command::SetActiveLayer { index: target },
                                "Selected a layer",
                            );
                            ui.close();
                        }
                    }
                    ui.separator();
                    for (label, icon, offset, enabled) in [
                        (
                            "Move Layer to Top",
                            Icon::LayerUp,
                            layer_count as i32 - 1 - active_layer_index as i32,
                            can_move_up,
                        ),
                        ("Move Layer Up", Icon::LayerUp, 1, can_move_up),
                        ("Move Layer Down", Icon::LayerDown, -1, can_move_down),
                        (
                            "Move Layer to Bottom",
                            Icon::LayerDown,
                            -(active_layer_index as i32),
                            can_move_down,
                        ),
                    ] {
                        if menu_action(ui, &self.icons, icon, label, None, enabled).clicked() {
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
                    if menu_action(
                        ui,
                        &self.icons,
                        Icon::LayerSettings,
                        "Layer Properties…",
                        None,
                        true,
                    )
                    .clicked()
                    {
                        self.open_layer_properties(active_layer_index);
                        ui.close();
                    }
                });
                ui.menu_button("Adjustments", |ui| {
                    compact_menu_spacing(ui);
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
                    compact_menu_spacing(ui);
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
                    compact_menu_spacing(ui);
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
                    compact_menu_spacing(ui);
                    ui.label("Paint.Rust development preview · MIT first-party license");
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
                ui.label("Paint.Rust development preview");
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
            if icon_button(ui, &self.icons, Icon::ExportFile, "Export image", true).clicked() {
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
            selection_input(ui, "Left", &mut self.selection_input.left);
            selection_input(ui, "Top", &mut self.selection_input.top);
            selection_input(ui, "Right", &mut self.selection_input.right);
            selection_input(ui, "Bottom", &mut self.selection_input.bottom);
            if strip_action_button(
                ui,
                "Apply",
                "Apply selection",
                "Apply the entered selection bounds",
                true,
            ) {
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
        egui::Panel::top("paint-r-tool-options")
            .frame(
                egui::Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(egui::Margin::symmetric(6, 3)),
            )
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 2.0);
                ui.spacing_mut().interact_size.y = TOOL_STRIP_HEIGHT;
                ui.spacing_mut().button_padding = egui::vec2(5.0, 2.0);
                let row = ui.horizontal(|ui| self.tool_config_strip(ui)).response.rect;
                self.tool_options_bounds = Some(row);
            });
    }

    /// One compact ToolConfigStrip row, in Paint.NET group order for every tool.
    fn tool_config_strip(&mut self, ui: &mut egui::Ui) {
        match self.active_tool {
            Tool::Select | Tool::Lasso | Tool::EllipseSelect => {
                self.selection_combine_options(ui);
            }
            Tool::MagicWand => {
                self.selection_combine_options(ui);
                strip_separator(ui);
                self.magic_wand_tool_options(ui);
            }
            Tool::Move => self.move_tool_options(ui),
            Tool::MoveSelection => self.move_selection_tool_options(ui),
            Tool::Picker => self.picker_tool_options(ui),
            Tool::Line => self.line_tool_options(ui),
            Tool::Fill => self.fill_tool_options(ui),
            Tool::Gradient => self.gradient_tool_options(ui),
            Tool::Text => self.text_tool_options(ui),
            Tool::CloneStamp => self.clone_tool_options(ui),
            Tool::Recolor => self.recolor_tool_options(ui),
            Tool::Brush | Tool::Eraser | Tool::Pencil => self.brush_tool_options(ui),
            Tool::Shapes => self.shape_tool_options(ui),
            Tool::Zoom => self.zoom_tool_options(ui),
            Tool::Pan => {
                ui.allocate_space(egui::vec2(24.0, TOOL_STRIP_HEIGHT));
            }
        }
    }

    fn magic_wand_tool_options(&mut self, ui: &mut egui::Ui) {
        let flood = if self.fill_global {
            "Global"
        } else {
            "Contiguous"
        };
        strip_combo(
            ui,
            "wand-flood",
            "Flood mode",
            flood,
            &mut self.fill_global,
            &[
                StripComboItem::new(false, "Contiguous", "Flood mode: Contiguous", true),
                StripComboItem::new(true, "Global", "Flood mode: Global", true),
            ],
        );
        strip_drag(ui, "Tolerance", &mut self.fill_tolerance, 0..=100, "%");
        strip_separator(ui);
        let sampling = sampling_short_label(self.magic_wand_sampling);
        strip_combo(
            ui,
            "wand-sampling",
            "Sampling",
            sampling,
            &mut self.magic_wand_sampling,
            &[
                StripComboItem::new(
                    MagicWandSampling::ActiveLayer,
                    "Layer",
                    "Sampling: Active Layer",
                    true,
                ),
                StripComboItem::new(MagicWandSampling::Image, "Image", "Sampling: Image", true),
            ],
        );
    }

    fn move_tool_options(&mut self, ui: &mut egui::Ui) {
        let finishable = self.floating_selection.is_some() || self.session.has_preview();
        let selected = resample_short_label(self.resample_mode);
        let response = strip_combo_button(ui, "move-sampling", "Sampling", selected);
        let mut chosen = None;
        egui::Popup::menu(&response).show(|ui| {
            ui.set_min_width(response.rect.width().max(120.0));
            for mode in RESAMPLE_MODES {
                if mode == ResampleMode::Anisotropic {
                    ui.add_enabled(false, egui::Button::new("Anisotropic"))
                        .on_disabled_hover_text(
                            "Anisotropic sampling only differs under non-uniform scaling, \
                             which Move does not perform. It is disabled rather than approximated.",
                        );
                    continue;
                }
                let selected = self.resample_mode == mode;
                let option = ui.selectable_label(selected, resample_short_label(mode));
                option.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::Role::Button,
                        true,
                        selected,
                        resample_label(mode),
                    )
                });
                if option.on_hover_text(resample_hint(mode)).clicked() {
                    chosen = Some(mode);
                    ui.close();
                }
            }
        });
        if let Some(mode) = chosen {
            self.set_resample_mode(mode);
        }
        strip_separator(ui);
        if strip_action_button(
            ui,
            "Finish",
            "Finish",
            "Place the moved pixels\nShortcut key: Enter",
            finishable,
        ) {
            self.finish_move();
        }
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
        strip_drag(
            ui,
            "Offset X",
            &mut self.move_selection_dx,
            -16_384..=16_384,
            "",
        );
        strip_drag(
            ui,
            "Offset Y",
            &mut self.move_selection_dy,
            -16_384..=16_384,
            "",
        );
        strip_separator(ui);
        let has_selection = self.session.document().selection().is_some();
        if strip_action_button(
            ui,
            "Apply",
            "Apply offset",
            "Move the selection by the entered offset",
            has_selection,
        ) {
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
        if strip_action_button(
            ui,
            "Cancel",
            "Cancel offset",
            "Discard the pending selection offset",
            pending,
        ) {
            self.move_selection_dx = 0;
            self.move_selection_dy = 0;
            self.status = "Canceled the pending selection offset".to_owned();
        }
    }

    fn picker_tool_options(&mut self, ui: &mut egui::Ui) {
        let source = match self.picker_sample_source {
            PickerSampleSource::Layer => "Layer",
            PickerSampleSource::Image => "Image",
        };
        strip_combo(
            ui,
            "picker-source",
            "Sampling",
            source,
            &mut self.picker_sample_source,
            &[
                StripComboItem::new(PickerSampleSource::Layer, "Layer", "Sampling: Layer", true),
                StripComboItem::new(
                    PickerSampleSource::Image,
                    "Image",
                    "Sampling: Composed image",
                    true,
                ),
            ],
        );
        strip_separator(ui);
        let size = picker_size_short_label(self.picker_sample_size);
        strip_combo(
            ui,
            "picker-size",
            "Sample size",
            size,
            &mut self.picker_sample_size,
            &[
                StripComboItem::new(PickerSampleSize::Point, "1 px", "Sample size: 1 px", true),
                StripComboItem::new(
                    PickerSampleSize::Average3x3,
                    "3×3",
                    "Sample size: 3×3 average",
                    true,
                ),
                StripComboItem::new(
                    PickerSampleSize::Average5x5,
                    "5×5",
                    "Sample size: 5×5 average",
                    true,
                ),
                StripComboItem::new(
                    PickerSampleSize::Average11x11,
                    "11×11",
                    "Sample size: 11×11 average",
                    true,
                ),
                StripComboItem::new(
                    PickerSampleSize::Average31x31,
                    "31×31",
                    "Sample size: 31×31 average",
                    true,
                ),
                StripComboItem::new(
                    PickerSampleSize::Average51x51,
                    "51×51",
                    "Sample size: 51×51 average",
                    true,
                ),
            ],
        );
        strip_separator(ui);
        let behavior = match self.picker_click_behavior {
            PickerClickBehavior::NoToolSwitch => "Stay",
            PickerClickBehavior::SwitchToPreviousTool => "Previous tool",
        };
        strip_combo(
            ui,
            "picker-behavior",
            "Click behavior",
            behavior,
            &mut self.picker_click_behavior,
            &[
                StripComboItem::new(
                    PickerClickBehavior::NoToolSwitch,
                    "Stay",
                    "Click behavior: Do not switch",
                    true,
                ),
                StripComboItem::new(
                    PickerClickBehavior::SwitchToPreviousTool,
                    "Previous tool",
                    "Click behavior: Switch to previous tool",
                    true,
                ),
            ],
        );
    }

    fn line_tool_options(&mut self, ui: &mut egui::Ui) {
        strip_drag(ui, "Width", &mut self.brush_size, 1..=64, "px");
        strip_combo(
            ui,
            "line-start-cap",
            "Start cap",
            line_cap_label(self.line_start_cap),
            &mut self.line_start_cap,
            &line_cap_items(),
        );
        strip_combo(
            ui,
            "line-dash",
            "Dash style",
            line_dash_label(self.line_dash),
            &mut self.line_dash,
            &[
                StripComboItem::new(LineDash::Solid, "Solid", "Dash style: Solid", true),
                StripComboItem::new(LineDash::Dash, "Dash", "Dash style: Dash", true),
                StripComboItem::new(LineDash::Dot, "Dot", "Dash style: Dot", true),
            ],
        );
        strip_combo(
            ui,
            "line-end-cap",
            "End cap",
            line_cap_label(self.line_end_cap),
            &mut self.line_end_cap,
            &end_cap_items(),
        );
        strip_separator(ui);
        strip_swatch(ui, "Line color", self.fill_color);
    }

    fn fill_tool_options(&mut self, ui: &mut egui::Ui) {
        let flood = if self.fill_global {
            "Global"
        } else {
            "Contiguous"
        };
        strip_combo(
            ui,
            "fill-flood",
            "Flood mode",
            flood,
            &mut self.fill_global,
            &[
                StripComboItem::new(false, "Contiguous", "Flood mode: Contiguous", true),
                StripComboItem::new(true, "Global", "Flood mode: Global", true),
            ],
        );
        strip_drag(ui, "Tolerance", &mut self.fill_tolerance, 0..=100, "%");
        strip_separator(ui);
        let sampling = sampling_short_label(self.fill_sampling);
        strip_combo(
            ui,
            "fill-sampling",
            "Sampling",
            sampling,
            &mut self.fill_sampling,
            &sampling_items(),
        );
        strip_separator(ui);
        let blend = blend_mode_label(self.fill_blend);
        strip_combo(
            ui,
            "fill-blend",
            "Blend mode",
            blend,
            &mut self.fill_blend,
            &BLEND_MENU_ITEMS,
        );
        strip_separator(ui);
        strip_swatch(ui, "Fill color", self.fill_color);
    }

    fn gradient_tool_options(&mut self, ui: &mut egui::Ui) {
        strip_combo(
            ui,
            "gradient-type",
            "Gradient type",
            gradient_type_label(self.gradient_type),
            &mut self.gradient_type,
            &gradient_type_items(),
        );
        strip_separator(ui);
        strip_swatch(ui, "Gradient start color", self.fill_color);
        strip_swatch(ui, "Gradient end color", self.secondary_color);
        strip_separator(ui);
        let blend = blend_mode_label(self.gradient_blend);
        strip_combo(
            ui,
            "gradient-blend",
            "Blend mode",
            blend,
            &mut self.gradient_blend,
            &BLEND_MENU_ITEMS,
        );
        strip_separator(ui);
        let repeat = gradient_repeat_label(self.gradient_repeat);
        strip_combo(
            ui,
            "gradient-repeat",
            "Repeat mode",
            repeat,
            &mut self.gradient_repeat,
            &[
                StripComboItem::new(GradientRepeat::None, "None", "Repeat mode: No Repeat", true),
                StripComboItem::new(
                    GradientRepeat::Repeat,
                    "Repeat",
                    "Repeat mode: Repeat",
                    true,
                ),
            ],
        );
        if strip_toggle(
            ui,
            "Transparency only",
            "α",
            self.gradient_alpha_only,
            "Transparency only: replace layer alpha, keep RGB",
        ) {
            self.gradient_alpha_only = !self.gradient_alpha_only;
        }
    }

    fn recolor_tool_options(&mut self, ui: &mut egui::Ui) {
        strip_drag(ui, "Size", &mut self.brush_size, 1..=64, "px");
        strip_drag(ui, "Hardness", &mut self.brush_hardness, 0..=100, "%");
        strip_drag(ui, "Spacing", &mut self.brush_spacing, 1..=100, "%");
        strip_separator(ui);
        strip_drag(ui, "Tolerance", &mut self.recolor_tolerance, 0..=100, "%");
        strip_separator(ui);
        let sampling = sampling_short_label(self.recolor_sampling);
        strip_combo(
            ui,
            "recolor-sampling",
            "Sampling",
            sampling,
            &mut self.recolor_sampling,
            &sampling_items(),
        );
        strip_separator(ui);
        strip_swatch(ui, "Recolor target color", self.secondary_color);
        strip_swatch(ui, "Recolor replacement color", self.fill_color);
    }

    fn brush_tool_options(&mut self, ui: &mut egui::Ui) {
        if self.active_tool == Tool::Pencil {
            ui.label(egui::RichText::new("1 px").size(12.0));
            strip_separator(ui);
            strip_swatch(ui, "Pencil color", self.fill_color);
            return;
        }
        strip_drag(ui, "Size", &mut self.brush_size, 1..=64, "px");
        strip_drag(ui, "Hardness", &mut self.brush_hardness, 0..=100, "%");
        strip_drag(ui, "Spacing", &mut self.brush_spacing, 1..=100, "%");
        if self.active_tool == Tool::Brush {
            strip_separator(ui);
            strip_swatch(ui, "Brush color", self.fill_color);
        }
    }

    fn shape_tool_options(&mut self, ui: &mut egui::Ui) {
        strip_combo(
            ui,
            "shape-type",
            "Shape type",
            shape_mode_label(self.shape_mode),
            &mut self.shape_mode,
            &[
                StripComboItem::new(
                    ShapeMode::Rectangle,
                    "Rectangle",
                    "Shape type: Rectangle",
                    true,
                ),
                StripComboItem::new(ShapeMode::Ellipse, "Ellipse", "Shape type: Ellipse", true),
            ],
        );
        strip_combo(
            ui,
            "shape-fill",
            "Fill style",
            shape_style_label(self.shape_style),
            &mut self.shape_style,
            &[
                StripComboItem::new(ShapeStyle::Outline, "Outline", "Fill style: Outline", true),
                StripComboItem::new(ShapeStyle::Fill, "Fill", "Fill style: Fill", true),
                StripComboItem::new(ShapeStyle::Both, "Both", "Fill style: Both", true),
            ],
        );
        strip_drag(ui, "Border", &mut self.shape_border_width, 1..=64, "px");
        strip_separator(ui);
        strip_swatch(ui, "Shape color", self.fill_color);
    }

    fn zoom_tool_options(&mut self, ui: &mut egui::Ui) {
        let percent = format!("{}%", (self.zoom * 100.0).round() as u32);
        let mut zoom = self.zoom;
        if strip_combo(
            ui,
            "zoom-level",
            "Zoom",
            &percent,
            &mut zoom,
            &[
                StripComboItem::new(0.5, "50%", "Zoom 50%", true),
                StripComboItem::new(1.0, "100%", "Zoom 100%", true),
                StripComboItem::new(2.0, "200%", "Zoom 200%", true),
                StripComboItem::new(4.0, "400%", "Zoom 400%", true),
            ],
        ) {
            self.zoom = zoom;
        }
        strip_separator(ui);
        if strip_action_button(
            ui,
            "Fit",
            "Fit canvas",
            "Fit the canvas in the window",
            true,
        ) {
            self.fit_canvas_requested = true;
        }
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
                let label = ui.label("File path");
                let response = ui
                    .horizontal(|ui| {
                        let response = ui
                            .add_sized(
                                egui::vec2(360.0, 20.0),
                                egui::TextEdit::singleline(&mut dialog.path),
                            )
                            .labelled_by(label.id);
                        if ui.button("Browse…").clicked()
                            && let Some(path) = native_path_dialog(
                                kind,
                                dialog.format,
                                Path::new(&dialog.path),
                            )
                        {
                            dialog.path = path.display().to_string();
                            dialog.preview = None;
                            dialog.preview_error = None;
                            preview_requested = true;
                        }
                        response
                    })
                    .inner;
                if dialog.focus_path {
                    response.request_focus();
                    dialog.focus_path = false;
                }
                if response.changed() {
                    if let Some(format) = SaveFormatChoice::from_path(Path::new(dialog.path.trim()))
                        && (matches!(kind, PathDialogKind::SaveAs)
                            || matches!(format, SaveFormatChoice::Raster(_)))
                    {
                        dialog.format = format;
                    }
                    dialog.preview = None;
                    dialog.preview_error = None;
                }
                if !matches!(kind, PathDialogKind::Open) {
                    let choices = if matches!(kind, PathDialogKind::Export) {
                        &SaveFormatChoice::EXPORT_ALL[..]
                    } else {
                        &SaveFormatChoice::ALL[..]
                    };
                    ui.horizontal(|ui| {
                        ui.label("File type");
                        let before = dialog.format;
                        let combo = egui::ComboBox::from_id_salt("paint-r-save-format")
                            .selected_text(dialog.format.label())
                            .show_ui(ui, |ui| {
                                for choice in choices {
                                    ui.selectable_value(
                                        &mut dialog.format,
                                        *choice,
                                        choice.label(),
                                    );
                                }
                            });
                        combo.response.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::Role::ComboBox,
                                true,
                                "File type",
                            )
                        });
                        if dialog.format != before {
                            apply_extension(&mut dialog.path, dialog.format);
                            dialog.preview = None;
                            dialog.preview_error = None;
                        }
                    });
                    if dialog.format == SaveFormatChoice::Pdn
                        && matches!(kind, PathDialogKind::SaveAs)
                    {
                        ui.label("PDN stores layers, opacity and visibility; interoperability is experimental.");
                    } else {
                        ui.label("Flat image; layers remain in the session but are not stored in this file.");
                        ui.label(
                            "HEIC, AVIF, JPEG XL and JPEG XR codecs are not bundled yet.",
                        );
                    }
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
                    ui.label(format!("Image preview: {}", preview.label));
                    ui.add(
                        egui::Image::from_texture(&preview.texture)
                            .maintain_aspect_ratio(true)
                            .fit_to_exact_size(egui::vec2(PREVIEW_WIDTH, PREVIEW_HEIGHT))
                            .alt_text(format!("Preview thumbnail: {}", preview.label)),
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
                let format = dialog.format;
                let mut path = PathBuf::from(path);
                if path.extension().is_none() {
                    path.set_extension(format.extension());
                }
                match format.save(&mut self.session, &path) {
                    Ok(()) => {
                        self.document_title = file_title(&path);
                        self.document_path = Some(path.clone());
                        self.texture_dirty = true;
                        self.status = format!("Saved {}: {}", format.short_label(), path.display());
                        self.path_dialog = None;
                    }
                    Err(error) => {
                        dialog.focus_path = true;
                        self.status = format!("Save failed: {error}");
                    }
                }
            }
            PathDialogKind::Export => {
                let format = dialog.format;
                let mut path = PathBuf::from(path);
                if path.extension().is_none() {
                    path.set_extension(format.extension());
                }
                match format.export(&self.session, &path) {
                    Ok(()) => {
                        self.path_dialog = None;
                        self.status = format!(
                            "Exported flattened {} without changing the layered session: {}",
                            format.short_label(),
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
                "Closing Paint.Rust will discard the modified document and its undo history."
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
        let zoom_in = egui::KeyboardShortcut::new(command, egui::Key::Plus);
        let zoom_in_secondary = egui::KeyboardShortcut::new(command, egui::Key::Equals);
        let zoom_out = egui::KeyboardShortcut::new(command, egui::Key::Minus);
        let zoom_reset = egui::KeyboardShortcut::new(command, egui::Key::Num0);
        if context.input_mut(|input| input.consume_shortcut(&zoom_reset)) {
            self.zoom = 1.0;
            self.status = "Zoom set to 100%".to_owned();
            return;
        }
        if context.input_mut(|input| {
            input.consume_shortcut(&zoom_in) || input.consume_shortcut(&zoom_in_secondary)
        }) {
            self.zoom = (self.zoom + 0.25).min(8.0);
            self.status = format!("Zoomed canvas to {}%", (self.zoom * 100.0).round());
            return;
        }
        if context.input_mut(|input| input.consume_shortcut(&zoom_out)) {
            self.zoom = (self.zoom - 0.25).max(0.1);
            self.status = format!("Zoomed canvas to {}%", (self.zoom * 100.0).round());
            return;
        }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SaveFormatChoice {
    Pdn,
    Raster(paint_image_io::SaveFormat),
}

impl SaveFormatChoice {
    const ALL: [Self; 9] = [
        Self::Pdn,
        Self::Raster(paint_image_io::SaveFormat::Png),
        Self::Raster(paint_image_io::SaveFormat::Jpeg),
        Self::Raster(paint_image_io::SaveFormat::Tiff),
        Self::Raster(paint_image_io::SaveFormat::Bmp),
        Self::Raster(paint_image_io::SaveFormat::Tga),
        Self::Raster(paint_image_io::SaveFormat::Dds),
        Self::Raster(paint_image_io::SaveFormat::Gif),
        Self::Raster(paint_image_io::SaveFormat::WebP),
    ];
    const EXPORT_ALL: [Self; 8] = [
        Self::Raster(paint_image_io::SaveFormat::Png),
        Self::Raster(paint_image_io::SaveFormat::Jpeg),
        Self::Raster(paint_image_io::SaveFormat::Tiff),
        Self::Raster(paint_image_io::SaveFormat::Bmp),
        Self::Raster(paint_image_io::SaveFormat::Tga),
        Self::Raster(paint_image_io::SaveFormat::Dds),
        Self::Raster(paint_image_io::SaveFormat::Gif),
        Self::Raster(paint_image_io::SaveFormat::WebP),
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Pdn => "Paint.NET (*.pdn)",
            Self::Raster(format) => format.label(),
        }
    }

    const fn short_label(self) -> &'static str {
        match self {
            Self::Pdn => "PDN",
            Self::Raster(paint_image_io::SaveFormat::Png) => "PNG",
            Self::Raster(paint_image_io::SaveFormat::Jpeg) => "JPEG",
            Self::Raster(paint_image_io::SaveFormat::Tiff) => "TIFF",
            Self::Raster(paint_image_io::SaveFormat::Bmp) => "BMP",
            Self::Raster(paint_image_io::SaveFormat::Tga) => "TGA",
            Self::Raster(paint_image_io::SaveFormat::Dds) => "DDS",
            Self::Raster(paint_image_io::SaveFormat::Gif) => "GIF",
            Self::Raster(paint_image_io::SaveFormat::WebP) => "WebP",
        }
    }

    const fn extension(self) -> &'static str {
        match self {
            Self::Pdn => "pdn",
            Self::Raster(format) => format.extension(),
        }
    }

    const fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Pdn => &["pdn"],
            Self::Raster(format) => format.extensions(),
        }
    }

    fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "pdn" => Some(Self::Pdn),
            extension => paint_image_io::SaveFormat::from_extension(extension).map(Self::Raster),
        }
    }

    fn save(
        self,
        session: &mut paint_app::AppSession,
        path: &Path,
    ) -> Result<(), paint_app::SessionError> {
        match self {
            Self::Pdn => session.save_pdn_atomic(path),
            Self::Raster(format) => session.save_image_atomic(path, format),
        }
    }

    fn export(
        self,
        session: &paint_app::AppSession,
        path: &Path,
    ) -> Result<(), paint_app::SessionError> {
        match self {
            Self::Pdn => unreachable!("PDN is a layered save, not a flat export"),
            Self::Raster(format) => session.export_image_atomic(path, format),
        }
    }
}

fn apply_extension(path: &mut String, format: SaveFormatChoice) {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return;
    }
    let mut buffer = PathBuf::from(trimmed);
    let matches = buffer
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|_| {
            SaveFormatChoice::from_path(&buffer).is_some_and(|current| current == format)
        });
    if !matches {
        buffer.set_extension(format.extension());
        *path = buffer.display().to_string();
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
            Self::SaveAs => "Save As",
            Self::Export => "Export Image",
        }
    }

    const fn action(self) -> &'static str {
        match self {
            Self::Open => "Open PNG",
            Self::SaveAs => "Save",
            Self::Export => "Export",
        }
    }

    const fn explanation(self) -> &'static str {
        match self {
            Self::Open => "Opening creates one flattened layer; source layers cannot be recovered.",
            Self::SaveAs => {
                "PDN preserves layers. PNG, JPEG, TIFF, BMP, TGA, DDS, GIF and WebP write composed pixels; layers remain in this session but are not stored in those files."
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
    format: SaveFormatChoice,
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

fn native_path_dialog(
    kind: PathDialogKind,
    format: SaveFormatChoice,
    current: &Path,
) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new();
    if let Some(parent) = current.parent().filter(|parent| parent.is_dir()) {
        dialog = dialog.set_directory(parent);
    }
    if matches!(kind, PathDialogKind::Open) {
        dialog = dialog
            .set_title("Open PNG")
            .add_filter("PNG image", &["png"]);
        return dialog.pick_file();
    }
    let default_name = current
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("Untitled.{}", format.extension()));
    let mut path = dialog
        .set_title(if matches!(kind, PathDialogKind::SaveAs) {
            "Save As"
        } else {
            "Export Image"
        })
        .set_file_name(default_name)
        .add_filter(format.short_label(), format.extensions())
        .save_file()?;
    if path.extension().is_none() {
        path.set_extension(format.extension());
    }
    Some(path)
}

impl PathDialog {
    fn new(kind: PathDialogKind) -> Self {
        Self::with_path(kind, String::new())
    }

    fn with_path(kind: PathDialogKind, path: String) -> Self {
        let browser_dir = browser_start_dir(&path);
        Self {
            kind,
            format: SaveFormatChoice::from_path(Path::new(&path))
                .unwrap_or(SaveFormatChoice::Raster(paint_image_io::SaveFormat::Png)),
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

/// One compact dropdown choice. `accessible` is what screen readers and tests see.
#[derive(Clone, Copy)]
struct StripComboItem<T> {
    value: T,
    visual: &'static str,
    accessible: &'static str,
    enabled: bool,
}

impl<T> StripComboItem<T> {
    const fn new(value: T, visual: &'static str, accessible: &'static str, enabled: bool) -> Self {
        Self {
            value,
            visual,
            accessible,
            enabled,
        }
    }
}

const BLEND_MENU_ITEMS: [StripComboItem<BlendMode>; 4] = [
    StripComboItem::new(BlendMode::Normal, "Normal", "Blend mode: Normal", true),
    StripComboItem::new(
        BlendMode::Multiply,
        "Multiply",
        "Blend mode: Multiply",
        true,
    ),
    StripComboItem::new(BlendMode::Screen, "Screen", "Blend mode: Screen", true),
    StripComboItem::new(BlendMode::Overlay, "Overlay", "Blend mode: Overlay", true),
];

/// A compact Paint.NET-style dropdown that keeps its group name as the accessible name.
fn strip_combo<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    id_salt: &'static str,
    name: &'static str,
    selected_text: &str,
    current: &mut T,
    items: &[StripComboItem<T>],
) -> bool {
    let response = strip_combo_button(ui, id_salt, name, selected_text);
    let mut changed = false;
    egui::Popup::menu(&response).show(|ui| {
        ui.set_min_width(response.rect.width().max(112.0));
        for item in items {
            let selected = *current == item.value;
            if !item.enabled {
                ui.add_enabled(false, egui::Button::new(item.visual))
                    .on_disabled_hover_text(item.accessible);
                continue;
            }
            let option = ui.selectable_label(selected, item.visual);
            option.widget_info(|| {
                egui::WidgetInfo::selected(egui::Role::Button, true, selected, item.accessible)
            });
            if option.clicked() {
                if !selected {
                    *current = item.value;
                    changed = true;
                }
                ui.close();
            }
        }
    });
    changed
}

/// The visible part of a compact dropdown: selected value plus a chevron.
fn strip_combo_button(
    ui: &mut egui::Ui,
    id_salt: &'static str,
    name: &'static str,
    selected_text: &str,
) -> egui::Response {
    let response = ui
        .push_id(id_salt, |ui| {
            ui.add(
                egui::Button::new(egui::RichText::new(selected_text.to_owned()).size(12.0))
                    .min_size(egui::vec2(0.0, TOOL_STRIP_HEIGHT))
                    .small(),
            )
        })
        .inner;
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::new(egui::Role::ComboBox);
        info.label = Some(name.to_owned());
        info.current_text_value = Some(selected_text.to_owned());
        info
    });
    response.on_hover_text(format!("{name}: {selected_text}"))
}

/// A small numeric field with a short caption and an explicit accessible label.
fn strip_drag<Num: egui::emath::Numeric>(
    ui: &mut egui::Ui,
    label: &'static str,
    value: &mut Num,
    range: std::ops::RangeInclusive<Num>,
    suffix: &'static str,
) -> egui::Response {
    let caption = ui.label(egui::RichText::new(label).size(12.0));
    ui.add(
        egui::DragValue::new(value)
            .range(range)
            .speed(1.0)
            .suffix(suffix)
            .min_size(egui::vec2(0.0, TOOL_STRIP_HEIGHT)),
    )
    .labelled_by(caption.id)
}

/// A compact icon-only toggle with its accessible name and tooltip preserved.
fn strip_toggle(
    ui: &mut egui::Ui,
    label: &'static str,
    text: &str,
    selected: bool,
    tooltip: &'static str,
) -> bool {
    let response = ui.add(
        egui::Button::new(egui::RichText::new(text).size(12.0))
            .selected(selected)
            .min_size(egui::vec2(24.0, TOOL_STRIP_HEIGHT))
            .small(),
    );
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    response.on_hover_text(tooltip).clicked()
}

/// A compact text action with an accessible name that can differ from the short caption.
fn strip_action_button(
    ui: &mut egui::Ui,
    text: &str,
    accessible: &'static str,
    tooltip: &'static str,
    enabled: bool,
) -> bool {
    let response = ui.add_enabled(
        enabled,
        egui::Button::new(egui::RichText::new(text).size(12.0))
            .min_size(egui::vec2(0.0, TOOL_STRIP_HEIGHT))
            .small(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, accessible));
    response.on_hover_text(tooltip).clicked()
}

/// A first-party icon action sized for the compact strip.
fn strip_icon_action(
    ui: &mut egui::Ui,
    icon: VectorIcon,
    label: &'static str,
    tooltip: &'static str,
    enabled: bool,
) -> bool {
    vector_icon_button_sized_with_tooltip(
        ui,
        icon,
        label,
        tooltip,
        enabled,
        egui::vec2(24.0, TOOL_STRIP_HEIGHT),
    )
    .clicked()
}

/// A read-only color swatch. Colors live in the Colors panel; this is only a compact preview.
fn strip_swatch(ui: &mut egui::Ui, name: &'static str, color: Bgra) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(16.0, TOOL_STRIP_HEIGHT), egui::Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, name));
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(
            rect,
            2,
            egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a),
        );
        painter.rect_stroke(
            rect,
            2,
            egui::Stroke::new(1.0, egui::Color32::from_gray(70)),
            egui::StrokeKind::Inside,
        );
    }
    response.on_hover_text(format!("{name}: {}", format_hex_color(color)))
}

/// A slim vertical divider between logical groups in the single row.
fn strip_separator(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 18.0), egui::Sense::hover());
    ui.painter().line_segment(
        [rect.center_top(), rect.center_bottom()],
        egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
    );
}

const fn resample_short_label(mode: ResampleMode) -> &'static str {
    match mode {
        ResampleMode::NearestNeighbor => "Nearest",
        ResampleMode::Bilinear => "Bilinear",
        ResampleMode::MultisampleBilinear => "Multisample",
        ResampleMode::Anisotropic => "Anisotropic",
        ResampleMode::Bicubic => "Bicubic",
    }
}

const fn sampling_short_label(sampling: MagicWandSampling) -> &'static str {
    match sampling {
        MagicWandSampling::ActiveLayer => "Layer",
        MagicWandSampling::Image => "Image",
    }
}

const fn picker_size_short_label(size: PickerSampleSize) -> &'static str {
    match size {
        PickerSampleSize::Point => "1 px",
        PickerSampleSize::Average3x3 => "3×3",
        PickerSampleSize::Average5x5 => "5×5",
        PickerSampleSize::Average11x11 => "11×11",
        PickerSampleSize::Average31x31 => "31×31",
        PickerSampleSize::Average51x51 => "51×51",
    }
}

const fn line_cap_label(cap: LineCap) -> &'static str {
    match cap {
        LineCap::Flat => "None",
        LineCap::Rounded => "Rounded",
        LineCap::Arrow => "Arrow",
    }
}

const fn line_dash_label(dash: LineDash) -> &'static str {
    match dash {
        LineDash::Solid => "Solid",
        LineDash::Dash => "Dash",
        LineDash::Dot => "Dot",
    }
}

const fn gradient_type_label(kind: GradientType) -> &'static str {
    match kind {
        GradientType::Linear => "Linear",
        GradientType::Reflected => "Reflected",
        GradientType::Diamond => "Diamond",
        GradientType::Radial => "Radial",
        GradientType::Conical => "Conical",
    }
}

const fn gradient_repeat_label(repeat: GradientRepeat) -> &'static str {
    match repeat {
        GradientRepeat::None => "None",
        GradientRepeat::Repeat => "Repeat",
    }
}

const fn shape_mode_label(mode: ShapeMode) -> &'static str {
    match mode {
        ShapeMode::Rectangle => "Rectangle",
        ShapeMode::Ellipse => "Ellipse",
    }
}

const fn shape_style_label(style: ShapeStyle) -> &'static str {
    match style {
        ShapeStyle::Outline => "Outline",
        ShapeStyle::Fill => "Fill",
        ShapeStyle::Both => "Both",
    }
}

fn sampling_items() -> [StripComboItem<MagicWandSampling>; 2] {
    [
        StripComboItem::new(
            MagicWandSampling::ActiveLayer,
            "Layer",
            "Sampling: Layer",
            true,
        ),
        StripComboItem::new(MagicWandSampling::Image, "Image", "Sampling: Image", true),
    ]
}

fn line_cap_items() -> [StripComboItem<LineCap>; 3] {
    [
        StripComboItem::new(LineCap::Flat, "None", "Start cap: None", true),
        StripComboItem::new(LineCap::Rounded, "Rounded", "Start cap: Rounded", true),
        StripComboItem::new(LineCap::Arrow, "Arrow", "Start cap: Arrow", true),
    ]
}

fn end_cap_items() -> [StripComboItem<LineCap>; 3] {
    [
        StripComboItem::new(LineCap::Flat, "None", "End cap: None", true),
        StripComboItem::new(LineCap::Rounded, "Rounded", "End cap: Rounded", true),
        StripComboItem::new(LineCap::Arrow, "Arrow", "End cap: Arrow", true),
    ]
}

fn gradient_type_items() -> [StripComboItem<GradientType>; 5] {
    [
        StripComboItem::new(
            GradientType::Linear,
            "Linear",
            "Gradient type: Linear",
            true,
        ),
        StripComboItem::new(
            GradientType::Reflected,
            "Reflected",
            "Gradient type: Reflected",
            true,
        ),
        StripComboItem::new(
            GradientType::Diamond,
            "Diamond",
            "Gradient type: Diamond",
            true,
        ),
        StripComboItem::new(
            GradientType::Radial,
            "Radial",
            "Gradient type: Radial",
            true,
        ),
        StripComboItem::new(
            GradientType::Conical,
            "Conical",
            "Gradient type: Conical",
            true,
        ),
    ]
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
        .on_disabled_hover_text("Not implemented in the Paint.Rust preview");
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

/// Paint.NET menu rows are compact: a 16 px icon column, 22 px rows, and tight groups.
const MENU_MIN_WIDTH: f32 = 284.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shortcut {
    New,
    Open,
    Save,
    SaveAs,
    Export,
    Close,
    Quit,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    PasteIntoNewLayer,
    PasteIntoNewImage,
    EraseSelection,
    FillSelection,
    InvertSelection,
    SelectAll,
    Deselect,
    AddLayer,
}

impl Shortcut {
    /// The Cmd/Ctrl convention of the host platform for the shortcut the app actually handles.
    fn text(self) -> &'static str {
        #[cfg(target_os = "macos")]
        let text = match self {
            Self::New => "Cmd+N",
            Self::Open => "Cmd+O",
            Self::Save => "Cmd+S",
            Self::SaveAs => "Cmd+Shift+S",
            Self::Export => "Cmd+Shift+E",
            Self::Close => "Cmd+W",
            Self::Quit => "Cmd+Q",
            Self::Undo => "Cmd+Z",
            Self::Redo => "Cmd+Shift+Z",
            Self::Cut => "Cmd+X",
            Self::Copy => "Cmd+C",
            Self::Paste => "Cmd+V",
            Self::PasteIntoNewLayer => "Cmd+Shift+V",
            Self::PasteIntoNewImage => "Cmd+Alt+V",
            Self::EraseSelection => "Delete",
            Self::FillSelection => "Backspace",
            Self::InvertSelection => "Cmd+I",
            Self::SelectAll => "Cmd+A",
            Self::Deselect => "Cmd+D",
            Self::AddLayer => "Cmd+Shift+L",
        };
        #[cfg(not(target_os = "macos"))]
        let text = match self {
            Self::New => "Ctrl+N",
            Self::Open => "Ctrl+O",
            Self::Save => "Ctrl+S",
            Self::SaveAs => "Ctrl+Shift+S",
            Self::Export => "Ctrl+Shift+E",
            Self::Close => "Ctrl+W",
            Self::Quit => "Ctrl+Q",
            Self::Undo => "Ctrl+Z",
            Self::Redo => "Ctrl+Shift+Z",
            Self::Cut => "Ctrl+X",
            Self::Copy => "Ctrl+C",
            Self::Paste => "Ctrl+V",
            Self::PasteIntoNewLayer => "Ctrl+Shift+V",
            Self::PasteIntoNewImage => "Ctrl+Alt+V",
            Self::EraseSelection => "Delete",
            Self::FillSelection => "Backspace",
            Self::InvertSelection => "Ctrl+I",
            Self::SelectAll => "Ctrl+A",
            Self::Deselect => "Ctrl+D",
            Self::AddLayer => "Ctrl+Shift+L",
        };
        text
    }
}

fn compact_menu(ui: &mut egui::Ui) {
    ui.set_min_width(MENU_MIN_WIDTH);
    compact_menu_spacing(ui);
    ui.spacing_mut().button_padding = egui::vec2(6.0, 2.0);
}

fn compact_menu_spacing(ui: &mut egui::Ui) {
    ui.spacing_mut().item_spacing.y = 2.0;
}

fn menu_action(
    ui: &mut egui::Ui,
    icons: &IconTextures,
    icon: impl Into<MenuIcon>,
    text: &str,
    shortcut: Option<Shortcut>,
    enabled: bool,
) -> egui::Response {
    menu_row(
        ui,
        icons,
        icon.into(),
        text,
        shortcut.map(Shortcut::text),
        enabled,
    )
}

#[cfg(test)]
mod tests {
    use super::Shortcut;

    #[test]
    fn shortcut_hints_follow_the_host_cmd_ctrl_convention() {
        let command = if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Ctrl"
        };
        assert_eq!(Shortcut::New.text(), format!("{command}+N"));
        assert_eq!(Shortcut::Save.text(), format!("{command}+S"));
        assert_eq!(Shortcut::SaveAs.text(), format!("{command}+Shift+S"));
        assert_eq!(Shortcut::Export.text(), format!("{command}+Shift+E"));
        assert_eq!(Shortcut::Redo.text(), format!("{command}+Shift+Z"));
        assert_eq!(
            Shortcut::PasteIntoNewLayer.text(),
            format!("{command}+Shift+V")
        );
        assert_eq!(
            Shortcut::PasteIntoNewImage.text(),
            format!("{command}+Alt+V")
        );
        assert_eq!(Shortcut::AddLayer.text(), format!("{command}+Shift+L"));
        assert_eq!(Shortcut::EraseSelection.text(), "Delete");
        assert_eq!(Shortcut::FillSelection.text(), "Backspace");
    }
}
