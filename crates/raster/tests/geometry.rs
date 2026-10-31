use paint_core::{Bgra, Rect, Size, Surface};
use paint_document::{Document, Layer, LayerId};
use paint_raster::{Rotation, canvas_surfaces, crop_surfaces, resize_surfaces, rotate_surfaces};

fn size(width: u32, height: u32) -> Size {
    Size::new(width, height).expect("nonzero test size")
}

fn layer(id: u64, name: &str, width: u32, height: u32, pixels: Vec<Bgra>) -> Layer {
    let surface = Surface::from_pixels(size(width, height), pixels).expect("test surface");
    Layer::new(LayerId::new(id), name, surface)
}

fn document(width: u32, height: u32, layers: Vec<Layer>) -> Document {
    Document::from_layers(size(width, height), layers).expect("test document")
}

fn pixel(surface: &Surface, x: u32, y: u32) -> Bgra {
    *surface
        .pixel(paint_core::Point::new(x, y))
        .expect("in-bounds pixel")
}

fn distinct(width: u32, height: u32, offset: u8) -> Vec<Bgra> {
    (0..width * height)
        .map(|index| {
            let value = u8::try_from(index % 200).expect("small index");
            Bgra::new(
                value,
                value.wrapping_add(offset),
                value.wrapping_add(2),
                255,
            )
        })
        .collect()
}

#[test]
fn crop_copies_the_exact_selection_window_from_every_layer() {
    let first = vec![
        Bgra::new(0, 0, 0, 255),
        Bgra::new(1, 0, 0, 255),
        Bgra::new(2, 0, 0, 255),
        Bgra::new(3, 0, 0, 255),
        Bgra::new(4, 0, 0, 255),
        Bgra::new(5, 0, 0, 255),
    ];
    let second = first
        .iter()
        .map(|pixel| Bgra::new(pixel.b.wrapping_add(10), 0, 0, 255))
        .collect();
    let document = document(
        3,
        2,
        vec![
            layer(1, "Background", 3, 2, first),
            layer(2, "Overlay", 3, 2, second),
        ],
    );

    let bounds = Rect::new(1, 0, 3, 2).expect("bounds");
    let surfaces = crop_surfaces(&document, bounds).expect("crop surfaces");
    assert_eq!(surfaces.len(), 2);
    assert_eq!(surfaces[0].size(), size(2, 2));
    assert_eq!(
        surfaces[0].pixels(),
        &[
            Bgra::new(1, 0, 0, 255),
            Bgra::new(2, 0, 0, 255),
            Bgra::new(4, 0, 0, 255),
            Bgra::new(5, 0, 0, 255),
        ]
    );
    assert_eq!(
        surfaces[1].pixels(),
        &[
            Bgra::new(11, 0, 0, 255),
            Bgra::new(12, 0, 0, 255),
            Bgra::new(14, 0, 0, 255),
            Bgra::new(15, 0, 0, 255),
        ]
    );
}

#[test]
fn resize_keeps_constant_images_constant_and_preserves_layer_order() {
    let red = Bgra::new(200, 30, 10, 255);
    let blue = Bgra::new(10, 30, 200, 128);
    let document = document(
        1,
        1,
        vec![
            layer(1, "Background", 1, 1, vec![red]),
            layer(2, "Overlay", 1, 1, vec![blue]),
        ],
    );

    let surfaces = resize_surfaces(&document, size(3, 2)).expect("resized surfaces");
    assert_eq!(surfaces.len(), 2);
    assert_eq!(surfaces[0].size(), size(3, 2));
    assert_eq!(surfaces[0].pixels(), &[red; 6]);
    assert_eq!(surfaces[1].pixels(), &[blue; 6]);
}

#[test]
fn resize_downscale_uses_bilinear_pixel_centre_sampling() {
    let pixels = vec![
        Bgra::new(0, 0, 0, 255),
        Bgra::new(40, 0, 0, 255),
        Bgra::new(80, 0, 0, 255),
        Bgra::new(120, 0, 0, 255),
    ];
    let document = document(2, 2, vec![layer(1, "Layer", 2, 2, pixels)]);

    let surfaces = resize_surfaces(&document, size(1, 1)).expect("resized surface");
    assert_eq!(surfaces[0].pixels(), &[Bgra::new(60, 0, 0, 255)]);
}

#[test]
fn resize_upscale_uses_the_nearest_source_pixels_at_centres() {
    let pixels = vec![
        Bgra::new(20, 0, 0, 255),
        Bgra::new(40, 0, 0, 255),
        Bgra::new(60, 0, 0, 255),
        Bgra::new(80, 0, 0, 255),
    ];
    let document = document(2, 2, vec![layer(1, "Layer", 2, 2, pixels)]);

    let surfaces = resize_surfaces(&document, size(4, 4)).expect("resized surface");
    let surface = &surfaces[0];
    assert_eq!(surface.size(), size(4, 4));
    // Destination centres map to source centres: the corner quadrants are exact.
    assert_eq!(pixel(surface, 0, 0), Bgra::new(20, 0, 0, 255));
    assert_eq!(pixel(surface, 3, 0), Bgra::new(40, 0, 0, 255));
    assert_eq!(pixel(surface, 0, 3), Bgra::new(60, 0, 0, 255));
    assert_eq!(pixel(surface, 3, 3), Bgra::new(80, 0, 0, 255));
}

#[test]
fn canvas_resize_places_content_using_anchor_offsets_and_fills_new_area() {
    let pixels = vec![
        Bgra::new(1, 0, 0, 255),
        Bgra::new(2, 0, 0, 255),
        Bgra::new(3, 0, 0, 255),
        Bgra::new(4, 0, 0, 255),
    ];
    let document = document(2, 2, vec![layer(1, "Layer", 2, 2, pixels)]);

    let top_left =
        canvas_surfaces(&document, size(4, 4), 0, 0, Bgra::TRANSPARENT).expect("top-left canvas");
    assert_eq!(pixel(&top_left[0], 0, 0), Bgra::new(1, 0, 0, 255));
    assert_eq!(pixel(&top_left[0], 1, 1), Bgra::new(4, 0, 0, 255));
    assert_eq!(pixel(&top_left[0], 2, 2), Bgra::TRANSPARENT);

    let bottom_right = canvas_surfaces(&document, size(4, 4), 2, 2, Bgra::TRANSPARENT)
        .expect("bottom-right canvas");
    assert_eq!(pixel(&bottom_right[0], 0, 0), Bgra::TRANSPARENT);
    assert_eq!(pixel(&bottom_right[0], 2, 2), Bgra::new(1, 0, 0, 255));
    assert_eq!(pixel(&bottom_right[0], 3, 3), Bgra::new(4, 0, 0, 255));

    let white = canvas_surfaces(&document, size(3, 3), 0, 0, Bgra::WHITE).expect("white fill");
    assert_eq!(pixel(&white[0], 2, 2), Bgra::WHITE);
}

#[test]
fn canvas_resize_shrink_crops_from_the_anchor_offset() {
    let pixels = distinct(3, 3, 1);
    let document = document(3, 3, vec![layer(1, "Layer", 3, 3, pixels.clone())]);

    let surfaces =
        canvas_surfaces(&document, size(2, 2), 0, 0, Bgra::TRANSPARENT).expect("shrunk canvas");
    assert_eq!(
        surfaces[0].pixels(),
        &[pixels[0], pixels[1], pixels[3], pixels[4]]
    );
}

#[test]
fn clockwise_rotation_swaps_dimensions_and_maps_pixels() {
    let a = Bgra::new(1, 0, 0, 255);
    let b = Bgra::new(2, 0, 0, 255);
    let c = Bgra::new(3, 0, 0, 255);
    let d = Bgra::new(4, 0, 0, 255);
    let e = Bgra::new(5, 0, 0, 255);
    let f = Bgra::new(6, 0, 0, 255);
    let document = document(2, 3, vec![layer(1, "Layer", 2, 3, vec![a, b, c, d, e, f])]);

    assert_eq!(
        Rotation::Rotate90Clockwise.rotated_size(document.size()),
        size(3, 2)
    );
    let surfaces = rotate_surfaces(&document, Rotation::Rotate90Clockwise).expect("rotate cw");
    assert_eq!(surfaces[0].size(), size(3, 2));
    assert_eq!(surfaces[0].pixels(), &[e, c, a, f, d, b]);
}

#[test]
fn counter_clockwise_rotation_swaps_dimensions_and_maps_pixels() {
    let a = Bgra::new(1, 0, 0, 255);
    let b = Bgra::new(2, 0, 0, 255);
    let c = Bgra::new(3, 0, 0, 255);
    let d = Bgra::new(4, 0, 0, 255);
    let e = Bgra::new(5, 0, 0, 255);
    let f = Bgra::new(6, 0, 0, 255);
    let document = document(2, 3, vec![layer(1, "Layer", 2, 3, vec![a, b, c, d, e, f])]);

    let surfaces =
        rotate_surfaces(&document, Rotation::Rotate90CounterClockwise).expect("rotate ccw");
    assert_eq!(surfaces[0].size(), size(3, 2));
    assert_eq!(surfaces[0].pixels(), &[b, d, f, a, c, e]);
}

#[test]
fn half_turn_rotation_keeps_dimensions_and_reverses_pixels() {
    let a = Bgra::new(1, 0, 0, 255);
    let b = Bgra::new(2, 0, 0, 255);
    let c = Bgra::new(3, 0, 0, 255);
    let d = Bgra::new(4, 0, 0, 255);
    let e = Bgra::new(5, 0, 0, 255);
    let f = Bgra::new(6, 0, 0, 255);
    let document = document(2, 3, vec![layer(1, "Layer", 2, 3, vec![a, b, c, d, e, f])]);

    assert_eq!(
        Rotation::Rotate180.rotated_size(document.size()),
        size(2, 3)
    );
    let surfaces = rotate_surfaces(&document, Rotation::Rotate180).expect("rotate 180");
    assert_eq!(surfaces[0].pixels(), &[f, e, d, c, b, a]);
}
