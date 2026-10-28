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
    Zoom,
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
    EyeDropper,
    AddLayer,
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
            Icon::Zoom,
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
            Icon::EyeDropper,
            Icon::AddLayer,
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
            let (rect, response) = ui.allocate_exact_size(vec2(38.0, 32.0), Sense::click());
            response.widget_info(|| WidgetInfo::labeled(Role::Button, enabled, label));
            if ui.is_rect_visible(rect) {
                paint_button(ui, icons, rect, icon, enabled, false, &response);
            }
            response
        })
        .inner;
    response.on_hover_text(label)
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
            let (rect, response) = ui.allocate_exact_size(vec2(46.0, 42.0), Sense::click());
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
    let icon_inset = if selected { 7.0 } else { 6.0 };
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
    painter.rect_filled(rect, 5, fill);
    if stroke != Stroke::NONE {
        painter.rect_stroke(rect, 5, stroke, StrokeKind::Inside);
    }
    let color = if enabled {
        Color32::from_gray(235)
    } else {
        ui.visuals().weak_text_color()
    };
    painter.image(
        icons.get(icon).id(),
        rect.shrink(icon_inset),
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
        Icon::Zoom => include_bytes!("../assets/lucide/zoom.png"),
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
        Icon::EyeDropper => include_bytes!("../assets/lucide/picker.png"),
        Icon::AddLayer => include_bytes!("../assets/lucide/add-layer.png"),
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
        Icon::Zoom => "zoom",
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
        Icon::EyeDropper => "picker",
        Icon::AddLayer => "add-layer",
    }
}
