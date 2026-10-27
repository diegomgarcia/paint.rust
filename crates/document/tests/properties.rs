use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_document::{Document, DocumentError, Layer, LayerId, PixelPatch, Revision};
use proptest::prelude::*;

fn pixel() -> impl Strategy<Value = Bgra> {
    any::<[u8; 4]>().prop_map(Bgra::from_bgra_bytes)
}

fn surface() -> impl Strategy<Value = Surface> {
    (1..=5u32, 1..=5u32).prop_flat_map(|(width, height)| {
        let len = (width * height) as usize;
        proptest::collection::vec(pixel(), len).prop_map(move |pixels| {
            Surface::from_pixels(
                Size::new(width, height).expect("nonzero strategy dimensions"),
                pixels,
            )
            .expect("strategy length matches")
        })
    })
}

fn surface_selection_and_after() -> impl Strategy<Value = (Surface, Rect, Vec<Bgra>)> {
    surface().prop_flat_map(|surface| {
        let width = surface.width();
        let height = surface.height();
        (0..width, 0..height)
            .prop_flat_map(move |(left, top)| {
                (
                    1..=width - left,
                    1..=height - top,
                    0..=8_i64,
                    0..=8_i64,
                    0..=4_i64,
                    0..=4_i64,
                )
                    .prop_map(
                        move |(
                            clipped_width,
                            clipped_height,
                            left_padding,
                            top_padding,
                            right_padding,
                            bottom_padding,
                        )| {
                            let clipped_right = i64::from(left + clipped_width);
                            let clipped_bottom = i64::from(top + clipped_height);
                            Rect::new(
                                i64::from(left) - left_padding,
                                i64::from(top) - top_padding,
                                clipped_right + right_padding,
                                clipped_bottom + bottom_padding,
                            )
                            .expect("constructed selection has positive dimensions")
                        },
                    )
            })
            .prop_flat_map(move |selection| {
                let clipped = selection
                    .clip_to(surface.size())
                    .expect("constructed selection overlaps the surface");
                let len = (clipped.width() * clipped.height()) as usize;
                let generated_surface = surface.clone();
                proptest::collection::vec(pixel(), len)
                    .prop_map(move |after| (generated_surface.clone(), selection, after))
            })
    })
}

fn document_for(surface: Surface) -> Document {
    Document::from_layers(
        surface.size(),
        vec![Layer::new(LayerId::new(1), "property 🖌️", surface)],
    )
    .expect("one matching layer")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn valid_patch_and_inverse_restore_pixels_and_preserve_outside_regions(
        case in surface_selection_and_after(),
    ) {
        let (surface, selection, after) = case;
        let rect = selection
            .clip_to(surface.size())
            .expect("strategy guarantees an overlapping selection");
        let before = surface.read_region(rect).expect("clipped input region");
        let original_surface = surface.clone();
        let mut document = document_for(surface);
        let patch = PixelPatch::new(
            LayerId::new(1),
            Revision::new(0),
            rect,
            before,
            after.clone(),
        )
        .expect("lengths match clipped region");

        document.apply_patch(&patch).expect("before pixels and revision match");
        let changed = document.active_layer().surface();
        prop_assert!(changed.region_matches(rect, &after).expect("valid changed region"));
        for y in 0..changed.height() {
            for x in 0..changed.width() {
                let point = Point::new(x, y);
                if !rect.contains(point) {
                    prop_assert_eq!(changed.get(point), original_surface.get(point));
                }
            }
        }

        let inverse = patch.inverse().expect("revision zero has an inverse");
        document.apply_patch(&inverse).expect("inverse matches changed state");
        prop_assert_eq!(document.active_layer().surface(), &original_surface);
        prop_assert_eq!(document.active_layer().revision(), Revision::new(2));
    }

    #[test]
    fn stale_and_before_mismatch_failures_never_mutate_document(
        surface in surface(),
        expected_revision in 1..=4u64,
    ) {
        let rect = surface.size().rect();
        let before = surface.pixels().to_vec();
        let after: Vec<Bgra> = surface.pixels().iter().map(|pixel| pixel.inverted_rgb()).collect();
        let original_surface = surface.clone();
        let mut document = document_for(surface);

        let stale = PixelPatch::new(
            LayerId::new(1),
            Revision::new(expected_revision),
            rect,
            before.clone(),
            after.clone(),
        )
        .expect("structurally valid stale patch");
        let snapshot = document.clone();
        let stale_rejected = matches!(
            document.apply_patch(&stale),
            Err(DocumentError::StalePatch { .. })
        );
        prop_assert!(stale_rejected);
        prop_assert_eq!(&document, &snapshot);

        let mut wrong_before = before;
        wrong_before[0].b ^= 1;
        let mismatch = PixelPatch::new(
            LayerId::new(1),
            Revision::new(0),
            rect,
            wrong_before,
            after,
        )
        .expect("structurally valid mismatch");
        let mismatch_rejected = matches!(
            document.apply_patch(&mismatch),
            Err(DocumentError::BeforePixelsMismatch { .. })
        );
        prop_assert!(mismatch_rejected);
        prop_assert_eq!(&document, &snapshot);
        prop_assert_eq!(document.active_layer().surface(), &original_surface);
    }
}
