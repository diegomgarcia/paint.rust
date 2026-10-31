use paint_core::{Bgra, Rect, Size, Surface, SurfaceError};
use paint_document::{
    BlendMode, Document, DocumentError, Layer, LayerId, PatchError, PixelPatch, Revision,
};

fn size(width: u32, height: u32) -> Size {
    Size::new(width, height).expect("test dimensions")
}

fn layer(id: u64, name: &str, width: u32, height: u32, color: Bgra) -> Layer {
    Layer::new(
        LayerId::new(id),
        name,
        Surface::filled(size(width, height), color).expect("test surface"),
    )
}

#[test]
fn document_tracks_order_active_selection_unicode_and_revisions() {
    let mut document = Document::new(size(2, 1)).expect("default document");
    let background_id = LayerId::new(1);
    assert_eq!(document.active_layer_id(), background_id);
    assert!(document.active_layer().is_background());
    assert_eq!(
        document.active_layer().surface().pixels(),
        &[Bgra::WHITE; 2]
    );
    assert_eq!(document.revision(), Revision::new(0));

    let layer_id = document.add_layer("Layer 🎨 レイヤー").expect("new layer");
    assert_eq!(layer_id, LayerId::new(2));
    assert_eq!(document.active_layer_id(), layer_id);
    assert_eq!(document.revision(), Revision::new(1));
    assert_eq!(document.layers()[0].id(), background_id);
    assert_eq!(document.layers()[1].id(), layer_id);
    assert_eq!(document.active_layer().name(), "Layer 🎨 レイヤー");

    assert!(
        document
            .set_layer_name(layer_id, "真実 🖌️")
            .expect("rename")
    );
    assert_eq!(document.active_layer().name(), "真実 🖌️");
    assert_eq!(document.active_layer().revision(), Revision::new(1));
    assert!(document.set_layer_opacity(layer_id, 127).expect("opacity"));
    assert_eq!(document.active_layer().opacity(), 127);
    assert_eq!(document.active_layer().revision(), Revision::new(2));
    assert!(
        !document
            .set_layer_opacity(layer_id, 127)
            .expect("no-op opacity")
    );
    assert_eq!(document.active_layer().revision(), Revision::new(2));
    assert!(
        document
            .set_layer_visibility(layer_id, false)
            .expect("visibility")
    );
    assert!(!document.active_layer().is_visible());
    assert!(
        document
            .set_layer_background(layer_id, true)
            .expect("background state")
    );
    assert!(document.active_layer().is_background());

    let selection = Rect::new(-4, -2, 1, 1).expect("signed selection");
    assert!(document.set_selection(Some(selection)).expect("selection"));
    assert_eq!(document.selection(), Some(selection));
    assert!(document.clear_selection().expect("clear selection"));
    assert_eq!(document.selection(), None);
    assert!(!document.clear_selection().expect("already clear"));
    assert!(
        document
            .set_active_layer(background_id)
            .expect("active layer")
    );
    assert_eq!(document.active_layer_id(), background_id);

    let snapshot = document.clone();
    assert!(matches!(
        document.set_layer_blend_mode(layer_id, BlendMode::Multiply),
        Err(DocumentError::UnsupportedBlendMode {
            layer_id: actual_id,
            mode: BlendMode::Multiply,
        }) if actual_id == layer_id
    ));
    assert_eq!(document, snapshot);
}

#[test]
fn document_construction_validates_layers_ids_and_sizes() {
    assert!(matches!(
        Document::from_layers(size(1, 1), Vec::new()),
        Err(DocumentError::EmptyDocument)
    ));
    assert!(matches!(
        Document::from_layers(
            size(1, 1),
            vec![
                layer(7, "one", 1, 1, Bgra::BLACK),
                layer(7, "duplicate", 1, 1, Bgra::WHITE),
            ],
        ),
        Err(DocumentError::DuplicateLayerId { layer_id }) if layer_id == LayerId::new(7)
    ));
    assert!(matches!(
        Document::from_layers(size(2, 2), vec![layer(7, "wrong size", 1, 1, Bgra::BLACK)],),
        Err(DocumentError::LayerSizeMismatch { .. })
    ));

    let document = Document::from_layers(
        size(1, 1),
        vec![
            layer(10, "bottom", 1, 1, Bgra::BLACK),
            layer(20, "top", 1, 1, Bgra::WHITE),
        ],
    )
    .expect("valid layers");
    assert_eq!(document.active_layer_id(), LayerId::new(10));
}

#[test]
fn layer_insert_move_remove_and_ids_remain_stable() {
    let mut document = Document::from_layers(
        size(1, 1),
        vec![
            layer(10, "bottom", 1, 1, Bgra::BLACK),
            layer(20, "top", 1, 1, Bgra::WHITE),
        ],
    )
    .expect("document");
    document
        .insert_layer(1, layer(5, "middle", 1, 1, Bgra::TRANSPARENT))
        .expect("insert");
    assert_eq!(
        document
            .layers()
            .iter()
            .map(|layer| layer.id().get())
            .collect::<Vec<_>>(),
        vec![10, 5, 20]
    );
    assert_eq!(document.active_layer_id(), LayerId::new(5));

    document.move_layer(LayerId::new(20), 0).expect("move");
    assert_eq!(
        document
            .layers()
            .iter()
            .map(|layer| layer.id().get())
            .collect::<Vec<_>>(),
        vec![20, 10, 5]
    );
    let removed = document
        .remove_layer(LayerId::new(5))
        .expect("remove active");
    assert_eq!(removed.name(), "middle");
    assert_eq!(document.active_layer_id(), LayerId::new(10));

    let added = document.add_layer("new after removal").expect("new ID");
    assert_eq!(added, LayerId::new(21));
    document.remove_layer(LayerId::new(10)).expect("remove");
    document.remove_layer(added).expect("remove");
    let snapshot = document.clone();
    assert!(matches!(
        document.remove_layer(LayerId::new(20)),
        Err(DocumentError::CannotRemoveLastLayer)
    ));
    assert_eq!(document, snapshot);
}

#[test]
fn forward_and_inverse_patches_validate_and_restore_exact_pixels() {
    let original = vec![
        Bgra::new(1, 2, 3, 4),
        Bgra::new(5, 6, 7, 8),
        Bgra::new(9, 10, 11, 12),
        Bgra::new(13, 14, 15, 16),
    ];
    let surface = Surface::from_pixels(size(2, 2), original.clone()).expect("surface");
    let mut document = Document::from_layers(
        size(2, 2),
        vec![Layer::new(LayerId::new(9), "pixels", surface)],
    )
    .expect("document");
    let rect = Rect::new(0, 0, 2, 1).expect("patch rectangle");
    let before = original[..2].to_vec();
    let after = vec![Bgra::new(20, 21, 22, 23), Bgra::new(24, 25, 26, 27)];
    let patch = PixelPatch::new(
        LayerId::new(9),
        Revision::new(0),
        rect,
        before.clone(),
        after.clone(),
    )
    .expect("valid patch");
    let inverse = patch.inverse().expect("inverse");
    assert_eq!(inverse.expected_revision(), Revision::new(1));
    assert_eq!(inverse.before(), after);
    assert_eq!(inverse.after(), before);
    let consuming_inverse = patch.clone().into_inverse().expect("consuming inverse");
    assert_eq!(consuming_inverse, inverse);

    assert_eq!(document.apply_patch(&patch), Ok(Revision::new(1)));
    assert!(
        document
            .active_layer()
            .surface()
            .region_matches(rect, &after)
            .expect("after pixels")
    );
    assert_eq!(document.active_layer().revision(), Revision::new(1));
    assert_eq!(document.revision(), Revision::new(1));

    assert_eq!(document.apply_inverse(&patch), Ok(Revision::new(2)));
    assert_eq!(document.active_layer().surface().pixels(), original);
    assert_eq!(document.active_layer().revision(), Revision::new(2));
    let forward_again = inverse.inverse().expect("forward again");
    assert_eq!(forward_again.expected_revision(), Revision::new(2));
    document
        .apply_patch(&forward_again)
        .expect("reapply forward");
    assert!(
        document
            .active_layer()
            .surface()
            .region_matches(rect, &after)
            .expect("after again")
    );
}

#[test]
fn stale_patch_is_rejected_without_any_mutation() {
    let mut document =
        Document::from_layers(size(1, 1), vec![layer(3, "pixels", 1, 1, Bgra::BLACK)])
            .expect("document");
    let patch = PixelPatch::new(
        LayerId::new(3),
        Revision::new(0),
        Rect::new(0, 0, 1, 1).expect("rect"),
        vec![Bgra::BLACK],
        vec![Bgra::WHITE],
    )
    .expect("patch");
    document.apply_patch(&patch).expect("first application");
    let snapshot = document.clone();
    assert!(matches!(
        document.apply_patch(&patch),
        Err(DocumentError::StalePatch {
            expected: Revision { .. },
            actual: Revision { .. },
            ..
        })
    ));
    assert_eq!(document, snapshot);
}

#[test]
fn mismatched_before_pixels_are_rejected_without_any_mutation() {
    let mut document =
        Document::from_layers(size(2, 1), vec![layer(3, "pixels", 2, 1, Bgra::BLACK)])
            .expect("document");
    let patch = PixelPatch::new(
        LayerId::new(3),
        Revision::new(0),
        Rect::new(0, 0, 2, 1).expect("rect"),
        vec![Bgra::BLACK, Bgra::WHITE],
        vec![Bgra::WHITE, Bgra::WHITE],
    )
    .expect("structurally valid patch");
    let snapshot = document.clone();
    assert!(matches!(
        document.apply_patch(&patch),
        Err(DocumentError::BeforePixelsMismatch { .. })
    ));
    assert_eq!(document, snapshot);
}

#[test]
fn out_of_bounds_patch_is_rejected_without_any_mutation() {
    let mut document =
        Document::from_layers(size(1, 1), vec![layer(3, "pixels", 1, 1, Bgra::BLACK)])
            .expect("document");
    let patch = PixelPatch::new(
        LayerId::new(3),
        Revision::new(0),
        Rect::new(-1, 0, 1, 1).expect("rect"),
        vec![Bgra::BLACK; 2],
        vec![Bgra::WHITE; 2],
    )
    .expect("patch dimensions are internally consistent");
    let snapshot = document.clone();
    assert!(matches!(
        document.apply_patch(&patch),
        Err(DocumentError::Surface(
            SurfaceError::RectangleOutOfBounds { .. }
        ))
    ));
    assert_eq!(document, snapshot);
}

#[test]
fn patch_lengths_and_inverse_revision_overflow_are_explicit_errors() {
    let rect = Rect::new(0, 0, 2, 1).expect("rect");
    assert!(matches!(
        PixelPatch::new(
            LayerId::new(1),
            Revision::new(0),
            rect,
            vec![Bgra::BLACK],
            vec![Bgra::WHITE; 2],
        ),
        Err(PatchError::BeforeLength {
            expected: 2,
            actual: 1
        })
    ));
    assert!(matches!(
        PixelPatch::new(
            LayerId::new(1),
            Revision::new(0),
            rect,
            vec![Bgra::BLACK; 2],
            vec![Bgra::WHITE; 1],
        ),
        Err(PatchError::AfterLength {
            expected: 2,
            actual: 1
        })
    ));

    let at_max = PixelPatch::new(
        LayerId::new(1),
        Revision::MAX,
        Rect::new(0, 0, 1, 1).expect("tiny"),
        vec![Bgra::BLACK],
        vec![Bgra::WHITE],
    )
    .expect("valid patch");
    assert!(matches!(
        at_max.inverse(),
        Err(PatchError::RevisionOverflow)
    ));
}

#[test]
fn replace_geometry_preserves_identity_metadata_and_active_layer() {
    let original_size = size(2, 2);
    let background = layer(1, "Background", 2, 2, Bgra::WHITE).with_background(true);
    let overlay = layer(2, "Overlay", 2, 2, Bgra::BLACK)
        .with_opacity(77)
        .with_visibility(false);
    let mut document = Document::from_layers(original_size, vec![background, overlay])
        .expect("two-layer document");
    document
        .set_active_layer(LayerId::new(2))
        .expect("select overlay");
    document
        .set_selection(Some(Rect::new(-1, 0, 2, 2).expect("signed selection")))
        .expect("selection");
    let before_revision = document.revision();
    let before_layer_revisions: Vec<_> = document.layers().iter().map(Layer::revision).collect();

    let new_size = size(3, 1);
    let surfaces = vec![
        Surface::filled(new_size, Bgra::new(9, 8, 7, 255)).expect("background surface"),
        Surface::filled(new_size, Bgra::new(1, 2, 3, 4)).expect("overlay surface"),
    ];
    document
        .replace_geometry(new_size, surfaces)
        .expect("replace geometry");

    assert_eq!(document.size(), new_size);
    assert_eq!(document.canvas_rect(), new_size.rect());
    assert_eq!(document.layers().len(), 2);
    assert_eq!(document.layers()[0].id(), LayerId::new(1));
    assert_eq!(document.layers()[1].id(), LayerId::new(2));
    assert_eq!(document.layers()[0].name(), "Background");
    assert_eq!(document.layers()[1].name(), "Overlay");
    assert!(document.layers()[0].is_background());
    assert!(!document.layers()[1].is_background());
    assert_eq!(document.layers()[1].opacity(), 77);
    assert!(!document.layers()[1].is_visible());
    assert_eq!(document.active_layer_id(), LayerId::new(2));
    assert_eq!(
        document.selection(),
        Some(Rect::new(-1, 0, 2, 2).expect("selection retained"))
    );
    assert_eq!(
        document.layers()[0].surface().pixels(),
        &[Bgra::new(9, 8, 7, 255); 3]
    );
    assert_eq!(document.layers()[0].surface().size(), new_size);
    assert!(document.revision() > before_revision);
    for (index, layer) in document.layers().iter().enumerate() {
        assert!(layer.revision() > before_layer_revisions[index]);
    }
    let added = document
        .add_layer("after resize")
        .expect("shrink to minimum");
    assert_eq!(added, LayerId::new(3), "next layer ID is preserved");
}

#[test]
fn replace_geometry_rejects_bad_surface_counts_and_sizes_without_mutation() {
    let mut document = Document::new(size(2, 2)).expect("default document");
    let snapshot = document.clone();

    assert!(matches!(
        document.replace_geometry(size(1, 1), Vec::new()),
        Err(DocumentError::SurfaceCountMismatch {
            expected: 1,
            actual: 0
        })
    ));
    assert_eq!(document, snapshot);

    let wrong_size = vec![Surface::new(size(1, 1)).expect("wrong-size surface")];
    assert!(matches!(
        document.replace_geometry(size(3, 3), wrong_size),
        Err(DocumentError::LayerSizeMismatch {
            layer_id: LayerId(1),
            expected,
            actual,
        }) if expected == size(3, 3) && actual == size(1, 1)
    ));
    assert_eq!(document, snapshot);
}
