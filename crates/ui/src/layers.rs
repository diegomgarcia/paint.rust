use paint_document::{Layer, Revision};

pub(crate) struct LayerThumbnail {
    pub(crate) revision: Revision,
    pub(crate) texture: egui::TextureHandle,
}

pub(crate) fn layer_thumbnail_texture(
    context: &egui::Context,
    layer: &Layer,
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

pub(crate) fn surface_thumbnail_texture(
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
