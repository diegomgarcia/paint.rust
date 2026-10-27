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
        surface in surface(),
        x in -4..6i64,
        y in -4..6i64,
        width in 1..=7u32,
        height in 1..=7u32,
        mut generated_after in proptest::collection::vec(pixel(), 25),
    ) {
        let selection = Rect::from_xywh(x, y, width, height).expect("small rectangle");
        let Some(rect) = selection.clip_to(surface.size()) else {
            prop_assume!(false);
            return Ok(());
        };
        let before = surface.read_region(rect).expect("clipped input region");
        generated_after.truncate(before.len());
        let after = generated_after;
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
