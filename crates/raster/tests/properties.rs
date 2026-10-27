use paint_core::{Bgra, Point, Rect, Size, Surface};
use paint_document::{Document, Layer, LayerId};
use paint_raster::{compose, fill, invert, normal_over};
use proptest::prelude::*;

fn pixel() -> impl Strategy<Value = Bgra> {
    any::<[u8; 4]>().prop_map(Bgra::from_bgra_bytes)
}

fn surfaces() -> impl Strategy<Value = Vec<Surface>> {
    (1..=4u32, 1..=4u32).prop_flat_map(|(width, height)| {
        let len = (width * height) as usize;
        proptest::collection::vec(proptest::collection::vec(pixel(), len), 1..=3).prop_map(
            move |buffers| {
                buffers
                    .into_iter()
                    .map(|pixels| {
                        Surface::from_pixels(
                            Size::new(width, height).expect("nonzero strategy dimensions"),
                            pixels,
                        )
                        .expect("strategy length matches")
                    })
                    .collect()
            },
        )
    })
}

fn one_surface() -> impl Strategy<Value = Surface> {
    surfaces().prop_map(|mut surfaces| surfaces.remove(0))
}

fn document_for(surface: Surface) -> Document {
    Document::from_layers(
        surface.size(),
        vec![Layer::new(LayerId::new(1), "property", surface)],
    )
    .expect("matching layer")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]

    #[test]
    fn fill_patch_matches_independent_clipping_and_preserves_outside_pixels(
        surface in one_surface(),
        x in -4..6i64,
        y in -4..6i64,
        width in 1..=7u32,
        height in 1..=7u32,
        color in pixel(),
    ) {
        let requested = Rect::from_xywh(x, y, width, height).expect("small rectangle");
        let expected_clip = requested.clip_to(surface.size());
        let original = surface.clone();
        let mut document = document_for(surface);
        document.set_selection(Some(requested)).expect("valid selection");
        let patch = fill(&document, LayerId::new(1), color).expect("fill calculation");
        match (expected_clip, patch) {
            (None, None) => prop_assert_eq!(document.active_layer().surface(), &original),
            (Some(rect), None) => {
                let before = original.read_region(rect).expect("expected clip");
                prop_assert!(before.iter().all(|pixel| *pixel == color));
            }
            (Some(rect), Some(patch)) => {
                prop_assert_eq!(patch.rect(), rect);
                prop_assert_eq!(patch.before(), original.read_region(rect).expect("before"));
                prop_assert!(patch.after().iter().all(|pixel| *pixel == color));
                document.apply_patch(&patch).expect("generated patch is valid");
                let changed = document.active_layer().surface();
                prop_assert!(changed.region_matches(rect, patch.after()).expect("changed region"));
                for y in 0..changed.height() {
                    for x in 0..changed.width() {
                        let point = Point::new(x, y);
                        if !rect.contains(point) {
                            prop_assert_eq!(changed.get(point), original.get(point));
                        }
                    }
                }
            }
            (None, Some(_)) => prop_assert!(false, "empty clip cannot produce a patch"),
        }
    }

    #[test]
    fn invert_patch_preserves_alpha_and_restores_pixels_on_second_application(
        surface in one_surface(),
        x in -4..6i64,
        y in -4..6i64,
        width in 1..=7u32,
        height in 1..=7u32,
    ) {
        let requested = Rect::from_xywh(x, y, width, height).expect("small rectangle");
        let expected_clip = requested.clip_to(surface.size());
        let original = surface.clone();
        let mut document = document_for(surface);
        document.set_selection(Some(requested)).expect("valid selection");
        let patch = invert(&document, LayerId::new(1)).expect("invert calculation");
        if let Some(rect) = expected_clip {
            let patch = patch.expect("invert always changes a nonempty RGB region");
            prop_assert_eq!(patch.rect(), rect);
            for (before, after) in patch.before().iter().zip(patch.after()) {
                prop_assert_eq!(after.a, before.a);
                prop_assert_eq!(after.b, 255 - before.b);
                prop_assert_eq!(after.g, 255 - before.g);
                prop_assert_eq!(after.r, 255 - before.r);
            }
            document.apply_patch(&patch).expect("first invert");
            let restore = invert(&document, LayerId::new(1))
                .expect("second invert calculation")
                .expect("nonempty region");
            document.apply_patch(&restore).expect("second invert");
            prop_assert_eq!(document.active_layer().surface(), &original);
        } else {
            prop_assert_eq!(patch, None);
            prop_assert_eq!(document.active_layer().surface(), &original);
        }
    }

    #[test]
    fn normal_over_boundary_invariants_hold_for_arbitrary_pixels(
        destination in pixel(),
        source in pixel(),
        opacity in any::<u8>(),
    ) {
        let transparent_source = Bgra { a: 0, ..source };
        prop_assert_eq!(normal_over(destination, transparent_source, opacity), destination);
        prop_assert_eq!(normal_over(destination, source, 0), destination);
        let opaque_source = Bgra { a: 255, ..source };
        prop_assert_eq!(normal_over(destination, opaque_source, 255), opaque_source);
    }

    #[test]
    fn single_layer_composition_has_explicit_rounded_alpha(
        surface in one_surface(),
        opacity in any::<u8>(),
    ) {
        let source_pixels = surface.pixels().to_vec();
        let layer = Layer::new(LayerId::new(1), "single", surface).with_opacity(opacity);
        let size = layer.surface().size();
        let document = Document::from_layers(size, vec![layer]).expect("single layer document");
        let composed = compose(&document).expect("Normal composition");
        for (source, output) in source_pixels.iter().zip(composed.pixels()) {
            let alpha = ((u32::from(source.a) * u32::from(opacity) + 127) / 255) as u8;
            if alpha == 0 {
                prop_assert_eq!(*output, Bgra::TRANSPARENT);
            } else {
                prop_assert_eq!(*output, Bgra::new(source.b, source.g, source.r, alpha));
            }
        }
    }

    #[test]
    fn hidden_layers_are_equivalent_to_removed_layers(
        surfaces in surfaces(),
        settings in proptest::collection::vec(any::<(bool, u8)>(), 3),
    ) {
        let layers = surfaces
            .iter()
            .enumerate()
            .map(|(index, surface)| {
                let (visible, opacity) = settings[index];
                Layer::new(LayerId::new(index as u64 + 1), "property", surface.clone())
                    .with_visibility(visible)
                    .with_opacity(opacity)
            })
            .collect::<Vec<_>>();
        let size = layers[0].surface().size();
        let document = Document::from_layers(size, layers.clone()).expect("all layers");
        let visible = layers.into_iter().filter(|layer| layer.is_visible()).collect::<Vec<_>>();
        let actual = compose(&document).expect("composition with hidden layers");
        if visible.is_empty() {
            prop_assert_eq!(actual.pixels(), vec![Bgra::TRANSPARENT; actual.len()]);
        } else {
            let visible_document = Document::from_layers(size, visible).expect("visible layers");
            let expected = compose(&visible_document).expect("visible-only composition");
            prop_assert_eq!(actual, expected);
        }
    }

    #[test]
    fn fully_opaque_top_layer_determines_composition(surfaces in surfaces()) {
        let size = surfaces[0].size();
        let mut layers = surfaces
            .into_iter()
            .enumerate()
            .map(|(index, surface)| Layer::new(LayerId::new(index as u64 + 1), "property", surface))
            .collect::<Vec<_>>();
        let last = layers.len() - 1;
        let mut top_surface = layers[last].surface().clone();
        for pixel in top_surface.pixels_mut() {
            pixel.a = 255;
        }
        let expected = top_surface.pixels().to_vec();
        layers[last] = Layer::new(LayerId::new(last as u64 + 1), "opaque top", top_surface);
        let document = Document::from_layers(size, layers).expect("layers");
        let composed = compose(&document).expect("composition");
        prop_assert_eq!(composed.pixels(), expected);
    }
}
