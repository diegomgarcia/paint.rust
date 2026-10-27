use paint_core::{Bgra, Size};
use paint_document::BlendMode;

const STOCK_PDN: &[u8] = include_bytes!("fixtures/paintnet-5.1.12-invert-2x2.pdn");
const STOCK_REOPENED_PNG: &[u8] =
    include_bytes!("fixtures/paintnet-5.1.12-invert-2x2-reopened.png");

#[test]
fn authentic_stock_pdn_decodes_and_composites_like_stock_reopen() {
    let decoded = paint_pdn3::decode_pdn(STOCK_PDN).expect("decode authentic Paint.NET 5.1.12 PDN");
    assert_eq!(decoded.producer_version, "5.112.9563.32325");

    let document = &decoded.document;
    assert_eq!(document.size(), Size::new(2, 2).unwrap());
    assert_eq!(document.layers().len(), 1);
    let layer = &document.layers()[0];
    assert_eq!(layer.name(), "Background");
    assert!(layer.is_visible());
    assert!(layer.is_background());
    assert_eq!(layer.opacity(), u8::MAX);
    assert_eq!(layer.blend_mode(), BlendMode::Normal);
    assert_eq!(layer.surface().pixels(), &[Bgra::BLACK; 4]);

    let clone_composite = paint_raster::compose(document).expect("compose decoded stock document");
    let stock_composite =
        paint_image_io::decode_png(STOCK_REOPENED_PNG).expect("decode stock reopened PNG");
    assert_eq!(clone_composite, stock_composite);
    assert_eq!(stock_composite.pixels(), &[Bgra::BLACK; 4]);
}
