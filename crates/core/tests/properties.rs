use paint_core::{Bgra, Point, Rect, Size, Surface};
use proptest::prelude::*;

fn pixel() -> impl Strategy<Value = Bgra> {
    any::<[u8; 4]>().prop_map(Bgra::from_bgra_bytes)
}

fn surface() -> impl Strategy<Value = Surface> {
    (1..=7u32, 1..=7u32).prop_flat_map(|(width, height)| {
        let len = (width * height) as usize;
        proptest::collection::vec(pixel(), len).prop_map(move |pixels| {
            Surface::from_pixels(
                Size::new(width, height).expect("strategy uses nonzero dimensions"),
                pixels,
            )
            .expect("strategy buffer length matches dimensions")
        })
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn rgba_and_bgra_pixel_round_trips_are_lossless(bytes in any::<[u8; 4]>()) {
        let bgra = Bgra::from_bgra_bytes(bytes);
        prop_assert_eq!(Bgra::from_rgba(bgra.to_rgba()), bgra);
        prop_assert_eq!(Bgra::from_rgba_bytes(bgra.to_rgba_bytes()), bgra);
        prop_assert_eq!(Bgra::from_bgra_bytes(bgra.to_bgra_bytes()), bgra);
    }

    #[test]
    fn surface_byte_round_trips_preserve_pixels(surface in surface()) {
        let bgra = surface.to_bgra_bytes().expect("checked BGRA allocation");
        let rgba = surface.to_rgba_bytes().expect("checked RGBA allocation");
        prop_assert_eq!(Surface::from_bgra_bytes(surface.size(), &bgra).expect("valid BGRA"), surface.clone());
        prop_assert_eq!(Surface::from_rgba_bytes(surface.size(), &rgba).expect("valid RGBA"), surface);
    }

    #[test]
    fn every_surface_index_is_row_major_and_boundaries_are_rejected(surface in surface()) {
        let width = surface.width();
        let height = surface.height();
        for y in 0..height {
            for x in 0..width {
                let point = Point::new(x, y);
                let index = (y * width + x) as usize;
                prop_assert_eq!(surface.index(point), Ok(index));
                prop_assert_eq!(surface.get(point), Some(&surface.pixels()[index]));
            }
        }
        prop_assert!(surface.get(Point::new(width, 0)).is_none());
        prop_assert!(surface.get(Point::new(0, height)).is_none());
    }

    #[test]
    fn rectangle_clipping_matches_independent_edge_arithmetic(
        width in 1..=8u32,
        height in 1..=8u32,
        x in -10..10i64,
        y in -10..10i64,
        rect_width in 1..=12u32,
        rect_height in 1..=12u32,
    ) {
        let size = Size::new(width, height).expect("nonzero");
        let rect = Rect::from_xywh(x, y, rect_width, rect_height).expect("small checked rectangle");
        let actual = rect.intersection(size.rect());
        let left = x.max(0);
        let top = y.max(0);
        let right = (x + i64::from(rect_width)).min(i64::from(width));
        let bottom = (y + i64::from(rect_height)).min(i64::from(height));
        if left < right && top < bottom {
            let clipped = actual.expect("edges overlap");
            prop_assert_eq!((clipped.left(), clipped.top(), clipped.right(), clipped.bottom()), (left, top, right, bottom));
            prop_assert!(clipped.is_within(rect));
            prop_assert!(clipped.is_within(size.rect()));
            let surface = Surface::new(size).expect("small surface");
            let pixels = surface.read_region(clipped).expect("clipped region is in bounds");
            prop_assert_eq!(pixels.len(), (clipped.width() * clipped.height()) as usize);
        } else {
            prop_assert_eq!(actual, None);
        }
    }
}
