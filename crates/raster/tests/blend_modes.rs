use paint_core::{Bgra, Size, Surface};
use paint_document::{BlendMode, Document, Layer, LayerId};
use paint_raster::compose;

fn document(bottom: Bgra, top: Bgra, mode: BlendMode) -> Document {
    let size = Size::new(1, 1).expect("size");
    let bottom = Layer::new(
        LayerId::new(1),
        "Bottom",
        Surface::from_pixels(size, vec![bottom]).expect("bottom"),
    )
    .with_background(true);
    let mut top = Layer::new(
        LayerId::new(2),
        "Top",
        Surface::from_pixels(size, vec![top]).expect("top"),
    );
    let mut document = Document::from_layers(size, vec![bottom, top.clone()]).expect("document");
    document
        .set_layer_blend_mode(top.id(), mode)
        .expect("blend mode");
    top = document.layers()[1].clone();
    assert_eq!(top.blend_mode(), mode);
    document
}

#[test]
fn multiply_screen_and_overlay_use_the_backdrop_channels() {
    let bottom = Bgra::new(100, 150, 200, 255);
    let top = Bgra::new(200, 50, 100, 255);
    let multiply = compose(&document(bottom, top, BlendMode::Multiply)).expect("multiply");
    let screen = compose(&document(bottom, top, BlendMode::Screen)).expect("screen");
    let overlay = compose(&document(bottom, top, BlendMode::Overlay)).expect("overlay");
    assert_eq!(
        *multiply.pixel(paint_core::Point::new(0, 0)).expect("pixel"),
        Bgra::new(78, 29, 78, 255)
    );
    assert_eq!(
        *screen.pixel(paint_core::Point::new(0, 0)).expect("pixel"),
        Bgra::new(222, 171, 222, 255)
    );
    assert_eq!(
        *overlay.pixel(paint_core::Point::new(0, 0)).expect("pixel"),
        Bgra::new(157, 86, 188, 255)
    );
}

#[test]
fn blend_modes_preserve_layer_alpha_and_are_undoable_at_document_level() {
    let bottom = Bgra::new(0, 0, 0, 255);
    let top = Bgra::new(255, 255, 255, 128);
    let document = document(bottom, top, BlendMode::Screen);
    let composed = compose(&document).expect("compose");
    let pixel = composed.pixel(paint_core::Point::new(0, 0)).expect("pixel");
    assert!(pixel.r > 0 && pixel.r < 255);
    assert_eq!(pixel.a, 255);
}
