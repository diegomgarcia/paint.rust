use paint_core::{Bgra, GeometryError, Point, Rect, Rgba, Size, Surface, SurfaceError};

#[test]
fn dimensions_and_allocation_sizes_are_checked() {
    assert!(matches!(
        Size::new(0, 1),
        Err(GeometryError::ZeroDimension { .. })
    ));
    assert!(matches!(
        Size::new(1, 0),
        Err(GeometryError::ZeroDimension { .. })
    ));

    let size = Size::new(3, 2).expect("nonzero dimensions");
    assert_eq!(size.checked_pixel_count(), Ok(6));
    assert_eq!(size.checked_stride(), Ok(12));
    assert_eq!(size.checked_buffer_len(), Ok(24));

    let huge = Size::new(u32::MAX, u32::MAX).expect("dimensions themselves are representable");
    assert!(matches!(
        huge.checked_buffer_len(),
        Err(GeometryError::AllocationSizeOverflow { .. })
    ));
    assert!(matches!(
        Surface::new(huge),
        Err(SurfaceError::Geometry(
            GeometryError::AllocationSizeOverflow { .. }
        ))
    ));
}

#[test]
fn rectangles_are_checked_half_open_and_clip_exactly() {
    let rect = Rect::new(1, 2, 4, 6).expect("valid rectangle");
    assert_eq!(rect.width(), 3);
    assert_eq!(rect.height(), 4);
    assert_eq!(rect.size(), Size::new(3, 4).expect("valid size"));
    assert!(rect.contains(Point::new(1, 2)));
    assert!(rect.contains(Point::new(3, 5)));
    assert!(!rect.contains(Point::new(4, 5)));
    assert!(!rect.contains(Point::new(3, 6)));

    assert!(matches!(
        Rect::new(1, 1, 1, 2),
        Err(GeometryError::InvalidRectangle { .. })
    ));
    assert!(matches!(
        Rect::new(2, 1, 1, 2),
        Err(GeometryError::InvalidRectangle { .. })
    ));
    assert!(matches!(
        Rect::new(i64::MIN, 0, i64::MAX, 1),
        Err(GeometryError::RectangleTooLarge { .. })
    ));
    assert!(matches!(
        Rect::from_xywh(i64::MAX - 1, 0, 4, 1),
        Err(GeometryError::CoordinateOverflow { .. })
    ));

    let canvas = Size::new(4, 3).expect("canvas").rect();
    let clipped = Rect::new(-2, 1, 3, 8)
        .expect("selection")
        .intersection(canvas)
        .expect("overlap");
    assert_eq!(clipped, Rect::new(0, 1, 3, 3).expect("expected clip"));
    assert_eq!(
        Rect::new(8, 8, 9, 9).expect("outside").intersection(canvas),
        None
    );
}

#[test]
fn bgra_and_rgba_conversion_is_explicit_and_lossless() {
    let bgra = Bgra::new(1, 2, 3, 4);
    let rgba = Rgba::new(3, 2, 1, 4);
    assert_eq!(bgra.to_rgba(), rgba);
    assert_eq!(bgra.to_rgba_bytes(), [3, 2, 1, 4]);
    assert_eq!(Bgra::from_rgba(rgba), bgra);
    assert_eq!(Bgra::from_rgba_bytes([3, 2, 1, 4]), bgra);
    assert_eq!(bgra.to_bgra_bytes(), [1, 2, 3, 4]);

    let inverted = bgra.inverted_rgb();
    assert_eq!(inverted, Bgra::new(254, 253, 252, 4));
    let transparent = Bgra::new(9, 8, 7, 0).inverted_rgb();
    assert_eq!(transparent, Bgra::new(246, 247, 248, 0));
}

#[test]
fn surface_uses_contiguous_row_major_pixels_and_checked_indices() {
    let size = Size::new(3, 2).expect("size");
    let pixels = (0..6).map(|n| Bgra::new(n, n + 1, n + 2, n + 3)).collect();
    let mut surface = Surface::from_pixels(size, pixels).expect("matching buffer");

    assert_eq!(surface.stride(), 12);
    assert_eq!(surface.len(), 6);
    assert_eq!(surface.index(Point::new(2, 1)), Ok(5));
    assert_eq!(surface.pixel(Point::new(0, 0)), Ok(&Bgra::new(0, 1, 2, 3)));
    assert_eq!(surface.pixel(Point::new(2, 1)), Ok(&Bgra::new(5, 6, 7, 8)));
    assert_eq!(surface.row(1).expect("row").len(), 3);
    assert!(matches!(
        surface.index(Point::new(3, 0)),
        Err(SurfaceError::OutOfBounds { .. })
    ));
    assert!(surface.get(Point::new(0, 2)).is_none());

    surface
        .set_pixel(Point::new(1, 1), Bgra::new(90, 91, 92, 93))
        .expect("in bounds");
    assert_eq!(surface.pixels()[4], Bgra::new(90, 91, 92, 93));

    let tiny = Surface::filled(Size::new(1, 1).expect("tiny"), Bgra::WHITE).expect("tiny surface");
    assert_eq!(tiny.pixels(), &[Bgra::WHITE]);
}

#[test]
fn surface_rejects_mismatched_external_buffers() {
    let size = Size::new(2, 2).expect("size");
    assert!(matches!(
        Surface::from_pixels(size, vec![Bgra::TRANSPARENT; 3]),
        Err(SurfaceError::PixelLength {
            expected: 4,
            actual: 3
        })
    ));
    assert!(matches!(
        Surface::from_bgra_bytes(size, &[0; 15]),
        Err(SurfaceError::BufferLength {
            expected: 16,
            actual: 15
        })
    ));
    assert!(matches!(
        Surface::from_rgba_bytes(size, &[0; 17]),
        Err(SurfaceError::BufferLength {
            expected: 16,
            actual: 17
        })
    ));
}

#[test]
fn surface_regions_are_row_major_and_failed_replacement_is_atomic() {
    let size = Size::new(3, 3).expect("size");
    let original = (0..9).map(|n| Bgra::new(n, 0, 0, 255)).collect::<Vec<_>>();
    let mut surface = Surface::from_pixels(size, original.clone()).expect("surface");
    let rect = Rect::new(1, 1, 3, 3).expect("region");

    assert_eq!(
        surface.read_region(rect).expect("region pixels"),
        vec![original[4], original[5], original[7], original[8]]
    );
    assert!(
        surface
            .region_matches(rect, &[original[4], original[5], original[7], original[8]])
            .expect("matching region")
    );

    let before_failure = surface.clone();
    assert!(surface.replace_region(rect, &[Bgra::WHITE; 3]).is_err());
    assert_eq!(surface, before_failure);
    let outside = Rect::new(2, 2, 4, 3).expect("outside region");
    assert!(surface.replace_region(outside, &[Bgra::WHITE]).is_err());
    assert_eq!(surface, before_failure);

    let replacement = [
        Bgra::WHITE,
        Bgra::BLACK,
        Bgra::TRANSPARENT,
        Bgra::new(1, 2, 3, 4),
    ];
    surface
        .replace_region(rect, &replacement)
        .expect("replacement");
    assert!(
        surface
            .region_matches(rect, &replacement)
            .expect("new pixels")
    );
    assert_eq!(surface.pixels()[0], original[0]);
    assert_eq!(surface.pixels()[3], original[3]);
    assert_eq!(surface.pixels()[6], original[6]);
}

#[test]
fn surface_bgra_and_rgba_byte_round_trips_preserve_all_channels() {
    let size = Size::new(2, 1).expect("size");
    let pixels = vec![Bgra::new(1, 2, 3, 0), Bgra::new(4, 5, 6, 128)];
    let surface = Surface::from_pixels(size, pixels).expect("surface");

    let bgra = surface.to_bgra_bytes().expect("BGRA bytes");
    assert_eq!(bgra, vec![1, 2, 3, 0, 4, 5, 6, 128]);
    assert_eq!(
        Surface::from_bgra_bytes(size, &bgra).expect("BGRA round trip"),
        surface
    );

    let rgba = surface.to_rgba_bytes().expect("RGBA bytes");
    assert_eq!(rgba, vec![3, 2, 1, 0, 6, 5, 4, 128]);
    assert_eq!(
        Surface::from_rgba_bytes(size, &rgba).expect("RGBA round trip"),
        surface
    );
}
