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

pub(crate) fn visibility_button(
    ui: &mut Ui,
    icons: &IconTextures,
    visible: bool,
    label: &str,
) -> (Response, bool) {
    let (rect, response) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
    response.widget_info(|| WidgetInfo::selected(Role::CheckBox, true, visible, label));
    if ui.is_rect_visible(rect) {
        egui::Image::from_texture(icons.get(if visible { Icon::Eye } else { Icon::EyeOff }))
            .tint(if visible {
                Color32::WHITE
            } else {
                Color32::from_gray(140)
            })
            .paint_at(ui, rect.shrink(1.0));
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
    response.on_hover_text(label)
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
