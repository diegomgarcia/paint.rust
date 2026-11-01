use std::collections::HashMap;

use egui::{
    Color32, Context, Response, Role, Sense, Stroke, StrokeKind, TextureHandle, TextureOptions, Ui,
    WidgetInfo, vec2,
};

/// Project-owned button chrome with Lucide ISC reference glyphs.
/// The PNG assets are high-resolution renders of the listed Lucide SVGs.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub(crate) enum Icon {
    NewDocument,
    OpenFile,
    ExportFile,
    Undo,
    Redo,
    SelectRectangle,
    LassoSelect,
    Zoom,
    MagicWand,
    MoveSelection,
    CloneStamp,
    Text,
    Shapes,
    Freeform,
    Fill,
    RectangleShape,
    Pencil,
    Brush,
    Eraser,
    Line,
    Ellipse,
    Gradient,
    Recolor,
    Move,
    Pan,
    SwapColors,
    ResetColors,
    EyeDropper,
    AddLayer,
    LayerDelete,
    LayerDuplicate,
    LayerMerge,
    LayerUp,
    LayerDown,
    LayerSettings,
    Eye,
    EyeOff,
    PanelTools,
    PanelColors,
    PanelLayers,
    PanelHistory,
}

pub(crate) struct IconTextures {
    textures: HashMap<Icon, TextureHandle>,
}

impl IconTextures {
    pub(crate) fn load(context: &Context) -> Self {
        let icons = [
            Icon::NewDocument,
            Icon::OpenFile,
            Icon::ExportFile,
            Icon::Undo,
            Icon::Redo,
            Icon::SelectRectangle,
            Icon::LassoSelect,
            Icon::Zoom,
            Icon::MagicWand,
            Icon::MoveSelection,
            Icon::CloneStamp,
            Icon::Text,
            Icon::Shapes,
            Icon::Freeform,
            Icon::Fill,
            Icon::RectangleShape,
            Icon::Pencil,
            Icon::Brush,
            Icon::Eraser,
            Icon::Line,
            Icon::Ellipse,
            Icon::Gradient,
            Icon::Recolor,
            Icon::Move,
            Icon::Pan,
            Icon::SwapColors,
            Icon::ResetColors,
            Icon::EyeDropper,
            Icon::AddLayer,
            Icon::LayerDelete,
            Icon::LayerDuplicate,
            Icon::LayerMerge,
            Icon::LayerUp,
            Icon::LayerDown,
            Icon::LayerSettings,
            Icon::Eye,
            Icon::EyeOff,
            Icon::PanelTools,
            Icon::PanelColors,
            Icon::PanelLayers,
            Icon::PanelHistory,
        ];
        let mut textures = HashMap::with_capacity(icons.len());
        for icon in icons {
            let surface = paint_image_io::decode_png(icon_png(icon))
                .expect("embedded Lucide icon PNG must decode");
            let rgba = surface
                .to_rgba_bytes()
                .expect("embedded Lucide icon surface must convert to RGBA");
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [surface.width() as usize, surface.height() as usize],
                &rgba,
            );
            let texture = context.load_texture(
                format!("paint-r-icon-{}", icon_name(icon)),
                image,
                TextureOptions::LINEAR,
            );
            textures.insert(icon, texture);
        }
        Self { textures }
    }

    fn get(&self, icon: Icon) -> &TextureHandle {
        self.textures
            .get(&icon)
            .expect("every Icon has an embedded texture")
    }
}

pub(crate) fn icon_button(
    ui: &mut Ui,
    icons: &IconTextures,
    icon: Icon,
    label: &'static str,
    enabled: bool,
) -> Response {
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            let (rect, response) = ui.allocate_exact_size(vec2(32.0, 28.0), Sense::click());
            response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
            if ui.is_rect_visible(rect) {
                paint_button(ui, icons, rect, icon, enabled, false, &response);
            }
            response
        })
        .inner;
    response.on_hover_text(label)
}

pub(crate) fn icon_button_at(
    ui: &mut Ui,
    icons: &IconTextures,
    rect: egui::Rect,
    icon: Icon,
    label: &'static str,
) -> Response {
    let response = ui.interact(
        rect,
        ui.scope_id().with(("paint-r-icon", label)),
        Sense::click(),
    );
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, label));
    if ui.is_rect_visible(rect) {
        paint_button(ui, icons, rect, icon, true, false, &response);
    }
    response.on_hover_text(label)
}

pub(crate) fn visibility_button_at(
    ui: &mut Ui,
    icons: &IconTextures,
    rect: egui::Rect,
    visible: bool,
    label: &str,
) -> (Response, bool) {
    let response = ui.interact(
        rect,
        ui.scope_id().with(("paint-r-layer-visibility", label)),
        Sense::click(),
    );
    response.widget_info(|| WidgetInfo::selected(Role::CheckBox, true, visible, label));
    if ui.is_rect_visible(rect) {
        let icon_rect = egui::Rect::from_center_size(rect.center(), vec2(18.0, 18.0));
        egui::Image::from_texture(icons.get(if visible { Icon::Eye } else { Icon::EyeOff }))
            .tint(if visible {
                Color32::WHITE
            } else {
                Color32::from_gray(140)
            })
            .paint_at(ui, icon_rect.shrink(1.0));
    }
    let toggled = response.clicked()
        || (response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Space)));
    (response.on_hover_text(label), toggled)
}

pub(crate) fn tool_button(
    ui: &mut Ui,
    icons: &IconTextures,
    icon: Icon,
    label: &'static str,
    enabled: bool,
    selected: bool,
) -> Response {
    tool_button_with_tooltip(ui, icons, icon, label, label, enabled, selected)
}

/// A tool button whose hover tooltip can carry a shortcut hint while its accessible name
/// stays the plain Paint.NET tool label.
pub(crate) fn tool_button_with_tooltip(
    ui: &mut Ui,
    icons: &IconTextures,
    icon: Icon,
    label: &'static str,
    tooltip: &str,
    enabled: bool,
    selected: bool,
) -> Response {
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            let (rect, response) = ui.allocate_exact_size(vec2(25.0, 25.0), Sense::click());
            response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, selected, label));
            if ui.is_rect_visible(rect) {
                paint_button(ui, icons, rect, icon, enabled, selected, &response);
            }
            response
        })
        .inner;
    response.on_hover_text(tooltip.to_owned())
}

/// Panel footer buttons use Paint.NET's 23 × 22 tool strip item metrics.
pub(crate) fn panel_button(
    ui: &mut Ui,
    icons: &IconTextures,
    icon: Icon,
    label: &'static str,
    tooltip: &'static str,
    enabled: bool,
) -> Response {
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            let (rect, response) = ui.allocate_exact_size(vec2(23.0, 22.0), Sense::click());
            response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
            if ui.is_rect_visible(rect) {
                paint_button(ui, icons, rect, icon, enabled, false, &response);
            }
            response
        })
        .inner;
    response.on_hover_text(tooltip)
}

/// A menu row with the same icon used by the corresponding toolbar or panel action.
pub(crate) fn menu_button(
    ui: &mut Ui,
    icons: &IconTextures,
    icon: Icon,
    text: &str,
    enabled: bool,
) -> Response {
    let tint = if enabled {
        Color32::from_gray(235)
    } else {
        ui.visuals().weak_text_color()
    };
    let image = egui::Image::from_texture(icons.get(icon))
        .fit_to_exact_size(vec2(16.0, 16.0))
        .tint(tint);
    ui.add_enabled(enabled, egui::Button::image_and_text(image, text))
}

/// Small chrome glyphs that have no dedicated Lucide asset. They are drawn as
/// first-party vector shapes so no proprietary bitmaps enter the repository.
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum VectorIcon {
    ChevronDown,
    ChevronUp,
    FitWindow,
    ZoomIn,
    ZoomOut,
    Save,
    Cut,
    Copy,
    Paste,
    Crop,
    Deselect,
    Grid,
    Rulers,
}

pub(crate) fn vector_icon_button(
    ui: &mut Ui,
    icon: VectorIcon,
    label: &'static str,
    enabled: bool,
) -> Response {
    vector_icon_button_sized(ui, icon, label, enabled, vec2(22.0, 20.0))
}

pub(crate) fn vector_toggle_button(
    ui: &mut Ui,
    icon: VectorIcon,
    label: &'static str,
    enabled: bool,
    selected: bool,
    size: egui::Vec2,
) -> Response {
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            let (rect, response) = ui.allocate_exact_size(size, Sense::click());
            response.widget_info(|| WidgetInfo::selected(Role::Button, enabled, selected, label));
            if ui.is_rect_visible(rect) {
                let widgets = &ui.style().visuals.widgets;
                let fill = if !enabled {
                    Color32::TRANSPARENT
                } else if selected {
                    ui.style().visuals.selection.bg_fill
                } else if response.is_pointer_button_down_on() {
                    widgets.active.bg_fill
                } else if response.hovered() {
                    widgets.hovered.bg_fill
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter().rect_filled(rect, 4, fill);
                paint_vector_icon(ui, rect, icon, enabled);
            }
            response
        })
        .inner;
    response.on_hover_text(label)
}

pub(crate) fn vector_icon_button_sized(
    ui: &mut Ui,
    icon: VectorIcon,
    label: &'static str,
    enabled: bool,
    size: egui::Vec2,
) -> Response {
    let response = ui
        .add_enabled_ui(enabled, |ui| {
            let (rect, response) = ui.allocate_exact_size(size, Sense::click());
            response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
            if ui.is_rect_visible(rect) {
                let widgets = &ui.style().visuals.widgets;
                let fill = if !enabled {
                    Color32::TRANSPARENT
                } else if response.is_pointer_button_down_on() {
                    widgets.active.bg_fill
                } else if response.hovered() {
                    widgets.hovered.bg_fill
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter().rect_filled(rect, 4, fill);
                paint_vector_icon(ui, rect, icon, enabled);
            }
            response
        })
        .inner;
    response.on_hover_text(label)
}

fn paint_vector_icon(ui: &Ui, rect: egui::Rect, icon: VectorIcon, enabled: bool) {
    let color = if enabled {
        Color32::from_gray(235)
    } else {
        ui.visuals().weak_text_color()
    };
    let stroke = Stroke::new(1.6, color);
    let center = rect.center();
    let painter = ui.painter();
    match icon {
        VectorIcon::ChevronDown => {
            painter.line_segment([center + vec2(-4.0, -2.0), center + vec2(0.0, 2.5)], stroke);
            painter.line_segment([center + vec2(0.0, 2.5), center + vec2(4.0, -2.0)], stroke);
        }
        VectorIcon::ChevronUp => {
            painter.line_segment([center + vec2(-4.0, 2.0), center + vec2(0.0, -2.5)], stroke);
            painter.line_segment([center + vec2(0.0, -2.5), center + vec2(4.0, 2.0)], stroke);
        }
        VectorIcon::FitWindow => {
            let r = egui::Rect::from_center_size(center, vec2(12.0, 10.0));
            let corner = 3.0;
            for (start, end) in [
                (r.left_top(), r.left_top() + vec2(corner, 0.0)),
                (r.left_top(), r.left_top() + vec2(0.0, corner)),
                (r.right_top(), r.right_top() + vec2(-corner, 0.0)),
                (r.right_top(), r.right_top() + vec2(0.0, corner)),
                (r.left_bottom(), r.left_bottom() + vec2(corner, 0.0)),
                (r.left_bottom(), r.left_bottom() + vec2(0.0, -corner)),
                (r.right_bottom(), r.right_bottom() + vec2(-corner, 0.0)),
                (r.right_bottom(), r.right_bottom() + vec2(0.0, -corner)),
            ] {
                painter.line_segment([start, end], stroke);
            }
        }
        VectorIcon::ZoomIn | VectorIcon::ZoomOut => {
            let radius = 4.5;
            let lens_center = center + vec2(-1.5, -1.5);
            painter.circle_stroke(lens_center, radius, stroke);
            painter.line_segment(
                [lens_center + vec2(3.2, 3.2), lens_center + vec2(6.2, 6.2)],
                stroke,
            );
            painter.line_segment(
                [lens_center + vec2(-2.0, 0.0), lens_center + vec2(2.0, 0.0)],
                stroke,
            );
            if icon == VectorIcon::ZoomIn {
                painter.line_segment(
                    [lens_center + vec2(0.0, -2.0), lens_center + vec2(0.0, 2.0)],
                    stroke,
                );
            }
        }
        VectorIcon::Save => {
            let body = egui::Rect::from_center_size(center, vec2(13.0, 13.0));
            painter.rect_stroke(body, 2.0, stroke, StrokeKind::Inside);
            painter.rect_stroke(
                egui::Rect::from_min_size(body.min + vec2(3.0, 2.0), vec2(7.0, 4.0)),
                0.0,
                stroke,
                StrokeKind::Inside,
            );
            painter.rect_filled(
                egui::Rect::from_min_size(body.min + vec2(4.0, 8.0), vec2(5.0, 3.0)),
                0.0,
                color,
            );
        }
        VectorIcon::Cut => {
            let radius = 2.0;
            painter.circle_stroke(center + vec2(-3.5, 4.5), radius, stroke);
            painter.circle_stroke(center + vec2(3.5, 4.5), radius, stroke);
            painter.line_segment([center + vec2(-2.5, 3.0), center + vec2(3.0, -5.0)], stroke);
            painter.line_segment([center + vec2(2.5, 3.0), center + vec2(-3.0, -5.0)], stroke);
        }
        VectorIcon::Copy => {
            painter.rect_stroke(
                egui::Rect::from_min_size(center + vec2(-6.0, -6.0), vec2(9.0, 10.0)),
                1.0,
                stroke,
                StrokeKind::Inside,
            );
            painter.rect_stroke(
                egui::Rect::from_min_size(center + vec2(-2.0, -2.0), vec2(9.0, 10.0)),
                1.0,
                stroke,
                StrokeKind::Inside,
            );
        }
        VectorIcon::Paste => {
            let body = egui::Rect::from_center_size(center, vec2(12.0, 14.0));
            painter.rect_stroke(body, 2.0, stroke, StrokeKind::Inside);
            painter.rect_stroke(
                egui::Rect::from_min_size(center + vec2(-3.5, -8.5), vec2(7.0, 3.5)),
                1.0,
                stroke,
                StrokeKind::Inside,
            );
        }
        VectorIcon::Crop => {
            painter.line_segment(
                [center + vec2(-6.0, -8.0), center + vec2(-6.0, 6.0)],
                stroke,
            );
            painter.line_segment([center + vec2(-6.0, 6.0), center + vec2(8.0, 6.0)], stroke);
            painter.line_segment(
                [center + vec2(-2.0, -6.0), center + vec2(6.0, -6.0)],
                stroke,
            );
            painter.line_segment([center + vec2(6.0, -6.0), center + vec2(6.0, 8.0)], stroke);
        }
        VectorIcon::Deselect => {
            let r = egui::Rect::from_center_size(center, vec2(14.0, 12.0));
            for (a, b) in [
                (r.left_top(), r.left_top() + vec2(3.0, 0.0)),
                (r.left_top(), r.left_top() + vec2(0.0, 3.0)),
                (r.right_top(), r.right_top() + vec2(-3.0, 0.0)),
                (r.right_top(), r.right_top() + vec2(0.0, 3.0)),
                (r.left_bottom(), r.left_bottom() + vec2(3.0, 0.0)),
                (r.left_bottom(), r.left_bottom() + vec2(0.0, -3.0)),
                (r.right_bottom(), r.right_bottom() + vec2(-3.0, 0.0)),
                (r.right_bottom(), r.right_bottom() + vec2(0.0, -3.0)),
            ] {
                painter.line_segment([a, b], stroke);
            }
            painter.line_segment([center + vec2(-4.0, -4.0), center + vec2(4.0, 4.0)], stroke);
        }
        VectorIcon::Grid => {
            let r = egui::Rect::from_center_size(center, vec2(14.0, 14.0));
            painter.rect_stroke(r, 0.0, stroke, StrokeKind::Inside);
            painter.line_segment(
                [
                    egui::pos2(r.center().x, r.top()),
                    egui::pos2(r.center().x, r.bottom()),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(r.left(), r.center().y),
                    egui::pos2(r.right(), r.center().y),
                ],
                stroke,
            );
        }
        VectorIcon::Rulers => {
            let r = egui::Rect::from_center_size(center, vec2(16.0, 9.0));
            painter.rect_stroke(r, 1.0, stroke, StrokeKind::Inside);
            for i in 1..4 {
                let x = r.left() + r.width() * i as f32 / 4.0;
                painter.line_segment(
                    [egui::pos2(x, r.top()), egui::pos2(x, r.top() + 3.0)],
                    stroke,
                );
            }
        }
    }
}

fn paint_button(
    ui: &Ui,
    icons: &IconTextures,
    rect: egui::Rect,
    icon: Icon,
    enabled: bool,
    selected: bool,
    response: &Response,
) {
    let icon_inset = (rect.width() * 0.12).clamp(2.0, 4.0);
    let widgets = &ui.style().visuals.widgets;
    let (fill, stroke) = if !enabled {
        (Color32::TRANSPARENT, Stroke::NONE)
    } else if response.is_pointer_button_down_on() {
        (widgets.active.bg_fill, widgets.active.bg_stroke)
    } else if selected {
        (
            Color32::from_rgb(31, 78, 116),
            Stroke::new(1.5, Color32::from_rgb(100, 178, 235)),
        )
    } else if response.hovered() {
        (widgets.hovered.bg_fill, widgets.hovered.bg_stroke)
    } else {
        (Color32::TRANSPARENT, Stroke::NONE)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 4, fill);
    if stroke != Stroke::NONE {
        painter.rect_stroke(rect, 4, stroke, StrokeKind::Inside);
    }
    let color = if enabled {
        Color32::from_gray(235)
    } else {
        ui.visuals().weak_text_color()
    };
    let icon_size = (rect.width().min(rect.height()) - icon_inset * 2.0).max(1.0);
    let icon_rect = egui::Rect::from_center_size(rect.center(), egui::vec2(icon_size, icon_size));
    painter.image(
        icons.get(icon).id(),
        icon_rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        color,
    );
}

fn icon_png(icon: Icon) -> &'static [u8] {
    match icon {
        Icon::NewDocument => include_bytes!("../assets/lucide/new-document.png"),
        Icon::OpenFile => include_bytes!("../assets/lucide/open-file.png"),
        Icon::ExportFile => include_bytes!("../assets/lucide/export-file.png"),
        Icon::Undo => include_bytes!("../assets/lucide/undo.png"),
        Icon::Redo => include_bytes!("../assets/lucide/redo.png"),
        Icon::SelectRectangle => include_bytes!("../assets/lucide/select.png"),
        Icon::LassoSelect => include_bytes!("../assets/lucide/lasso-select.png"),
        Icon::Zoom => include_bytes!("../assets/lucide/zoom.png"),
        Icon::MagicWand => include_bytes!("../assets/lucide/wand.png"),
        Icon::MoveSelection => include_bytes!("../assets/lucide/move-selection.png"),
        Icon::CloneStamp => include_bytes!("../assets/lucide/clone-stamp.png"),
        Icon::Text => include_bytes!("../assets/lucide/text.png"),
        Icon::Shapes => include_bytes!("../assets/lucide/shapes.png"),
        Icon::Freeform => include_bytes!("../assets/lucide/freeform.png"),
        Icon::Fill => include_bytes!("../assets/lucide/fill.png"),
        Icon::RectangleShape => include_bytes!("../assets/lucide/rectangle-shape.png"),
        Icon::Pencil => include_bytes!("../assets/lucide/pencil.png"),
        Icon::Brush => include_bytes!("../assets/lucide/brush.png"),
        Icon::Eraser => include_bytes!("../assets/lucide/eraser.png"),
        Icon::Line => include_bytes!("../assets/lucide/line.png"),
        Icon::Ellipse => include_bytes!("../assets/lucide/ellipse.png"),
        Icon::Gradient => include_bytes!("../assets/lucide/gradient.png"),
        Icon::Recolor => include_bytes!("../assets/lucide/recolor.png"),
        Icon::Move => include_bytes!("../assets/lucide/move.png"),
        Icon::Pan => include_bytes!("../assets/lucide/pan.png"),
        Icon::SwapColors => include_bytes!("../assets/lucide/swap.png"),
        Icon::ResetColors => include_bytes!("../assets/lucide/reset.png"),
        Icon::EyeDropper => include_bytes!("../assets/lucide/picker.png"),
        Icon::AddLayer => include_bytes!("../assets/lucide/add-layer.png"),
        Icon::LayerDelete => include_bytes!("../assets/lucide/layer-delete.png"),
        Icon::LayerDuplicate => include_bytes!("../assets/lucide/layer-duplicate.png"),
        Icon::LayerMerge => include_bytes!("../assets/lucide/layer-merge.png"),
        Icon::LayerUp => include_bytes!("../assets/lucide/layer-up.png"),
        Icon::LayerDown => include_bytes!("../assets/lucide/layer-down.png"),
        Icon::LayerSettings => include_bytes!("../assets/lucide/layer-settings.png"),
        Icon::Eye => include_bytes!("../assets/lucide/eye.png"),
        Icon::EyeOff => include_bytes!("../assets/lucide/eye-off.png"),
        Icon::PanelTools => include_bytes!("../assets/lucide/panel-tools.png"),
        Icon::PanelColors => include_bytes!("../assets/lucide/panel-colors.png"),
        Icon::PanelLayers => include_bytes!("../assets/lucide/panel-layers.png"),
        Icon::PanelHistory => include_bytes!("../assets/lucide/panel-history.png"),
    }
}

fn icon_name(icon: Icon) -> &'static str {
    match icon {
        Icon::NewDocument => "new-document",
        Icon::OpenFile => "open-file",
        Icon::ExportFile => "export-file",
        Icon::Undo => "undo",
        Icon::Redo => "redo",
        Icon::SelectRectangle => "select",
        Icon::LassoSelect => "lasso-select",
        Icon::Zoom => "zoom",
        Icon::MagicWand => "wand",
        Icon::MoveSelection => "move-selection",
        Icon::CloneStamp => "clone-stamp",
        Icon::Text => "text",
        Icon::Shapes => "shapes",
        Icon::Freeform => "freeform",
        Icon::Fill => "fill",
        Icon::RectangleShape => "rectangle-shape",
        Icon::Pencil => "pencil",
        Icon::Brush => "brush",
        Icon::Eraser => "eraser",
        Icon::Line => "line",
        Icon::Ellipse => "ellipse",
        Icon::Gradient => "gradient",
        Icon::Recolor => "recolor",
        Icon::Move => "move",
        Icon::Pan => "pan",
        Icon::SwapColors => "swap",
        Icon::ResetColors => "reset",
        Icon::EyeDropper => "picker",
        Icon::AddLayer => "add-layer",
        Icon::LayerDelete => "layer-delete",
        Icon::LayerDuplicate => "layer-duplicate",
        Icon::LayerMerge => "layer-merge",
        Icon::LayerUp => "layer-up",
        Icon::LayerDown => "layer-down",
        Icon::LayerSettings => "layer-settings",
        Icon::Eye => "eye",
        Icon::EyeOff => "eye-off",
        Icon::PanelTools => "panel-tools",
        Icon::PanelColors => "panel-colors",
        Icon::PanelLayers => "panel-layers",
        Icon::PanelHistory => "panel-history",
    }
}
