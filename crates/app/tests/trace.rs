use paint_app::{
    CommandTrace, ExpectationField, ResourceLimits, TraceError, execute_trace,
    execute_trace_with_limits,
};
use paint_core::Bgra;
use sha2::{Digest, Sha256};

#[test]
fn checked_in_invert_trace_matches_independent_hash() {
    let output = execute_trace(include_str!(
        "../../../tests/e2e/traces/invert-800x600.json"
    ))
    .unwrap();

    assert_eq!(
        (output.surface.width(), output.surface.height()),
        (800, 600)
    );
    assert!(
        output
            .surface
            .pixels()
            .iter()
            .all(|pixel| *pixel == Bgra::BLACK)
    );
    assert_eq!(
        output.snapshot.decoded_rgba_sha256,
        "a7aff0f8e3f44663c369e1e75a981b90b66949586d71035771921871d67e334e"
    );
    assert!(output.snapshot.dirty);
    assert!(output.snapshot.can_undo);
    assert!(!output.snapshot.can_redo);
}

#[test]
fn checked_in_undo_trace_returns_to_clean_baseline() {
    let output = execute_trace(include_str!(
        "../../../tests/e2e/traces/undo-invert-2x2.json"
    ))
    .unwrap();

    assert!(
        output
            .surface
            .pixels()
            .iter()
            .all(|pixel| *pixel == Bgra::WHITE)
    );
    assert_eq!(
        output.snapshot.decoded_rgba_sha256,
        "5ac6a5945f16500911219129984ba8b387a06f24fe383ce4e81a73294065461b"
    );
    assert!(!output.snapshot.dirty);
    assert!(!output.snapshot.can_undo);
    assert!(output.snapshot.can_redo);
}

#[test]
fn trace_executes_every_command_and_add_layer_defaults() {
    let trace = r#"{
        "format":"paint.r.command-trace",
        "version":1,
        "initial":{"width":2,"height":1,"background":{"bgra":[255,255,255,255]}},
        "commands":[
            {"op":"select_rect","rect":{"left":0,"top":0,"right":1,"bottom":1}},
            {"op":"fill","color":{"bgra":[0,0,0,255]}},
            {"op":"clear_selection"},
            {"op":"invert"},
            {"op":"add_layer","name":"顶层 🎨"},
            {"op":"set_layer_visibility","index":1,"visible":false},
            {"op":"set_layer_opacity","index":1,"opacity":200},
            {"op":"set_active_layer","index":0},
            {"op":"invert"},
            {"op":"undo"},
            {"op":"redo"}
        ],
        "expect":{
            "width":2,
            "height":1,
            "layer_count":2,
            "active_layer_index":0,
            "dirty":true,
            "can_undo":true,
            "can_redo":false
        }
    }"#;

    let output = execute_trace(trace).unwrap();
    assert_eq!(
        output.surface.pixels(),
        &[Bgra::BLACK, Bgra::WHITE],
        "selection, fill, invert, undo, and redo determine exact pixels"
    );
    assert_eq!(output.snapshot.layers.len(), 2);
    assert_eq!(output.snapshot.layers[1].name, "顶层 🎨");
    assert_eq!(output.snapshot.layers[1].opacity, 200);
    assert!(!output.snapshot.layers[1].visible);
    assert_eq!(output.snapshot.selection, None);

    let defaults = execute_trace(
        r#"{
            "format":"paint.r.command-trace","version":1,
            "initial":{"width":1,"height":1,"background":{"bgra":[1,2,3,255]}},
            "commands":[{"op":"add_layer","name":"defaults"}]
        }"#,
    )
    .unwrap();
    assert_eq!(defaults.snapshot.layers[1].opacity, 255);
    assert!(defaults.snapshot.layers[1].visible);
}

#[test]
fn custom_background_and_hash_use_row_major_rgba_not_bgra() {
    let output = execute_trace(
        r#"{
            "format":"paint.r.command-trace","version":1,
            "initial":{"width":1,"height":1,"background":{"bgra":[1,2,3,4]}},
            "commands":[]
        }"#,
    )
    .unwrap();

    assert_eq!(output.surface.pixels(), &[Bgra::new(1, 2, 3, 4)]);
    let expected = hex::encode(Sha256::digest([3, 2, 1, 4]));
    assert_eq!(output.snapshot.decoded_rgba_sha256, expected);
    assert!(!output.snapshot.dirty);
}

#[test]
fn positive_out_of_canvas_rectangles_clip_and_empty_clips_are_noops() {
    let output = execute_trace(
        r#"{
            "format":"paint.r.command-trace","version":1,
            "initial":{"width":2,"height":2,"background":{"bgra":[255,255,255,255]}},
            "commands":[
                {"op":"select_rect","rect":{"left":1,"top":1,"right":4,"bottom":3}},
                {"op":"fill","color":{"bgra":[0,0,0,255]}}
            ]
        }"#,
    )
    .unwrap();
    assert_eq!(
        output.surface.pixels(),
        &[Bgra::WHITE, Bgra::WHITE, Bgra::WHITE, Bgra::BLACK]
    );

    let noop = execute_trace(
        r#"{
            "format":"paint.r.command-trace","version":1,
            "initial":{"width":2,"height":2,"background":{"bgra":[255,255,255,255]}},
            "commands":[
                {"op":"select_rect","rect":{"left":4,"top":4,"right":5,"bottom":5}},
                {"op":"invert"},
                {"op":"fill","color":{"bgra":[0,0,0,255]}}
            ],
            "expect":{"dirty":false,"can_undo":false,"can_redo":false}
        }"#,
    )
    .unwrap();
    assert!(
        noop.surface
            .pixels()
            .iter()
            .all(|pixel| *pixel == Bgra::WHITE)
    );
    assert!(noop.snapshot.selection.is_some());
}

#[test]
fn malformed_json_and_unknown_top_level_fields_are_rejected() {
    assert!(matches!(execute_trace("{"), Err(TraceError::Json(_))));
    let unknown_field = r#"{
        "format":"paint.r.command-trace","version":1,
        "initial":{"width":1,"height":1,"background":{"bgra":[0,0,0,255]}},
        "commands":[],"mystery":true
    }"#;
    assert!(matches!(
        execute_trace(unknown_field),
        Err(TraceError::Json(_))
    ));
}

#[test]
fn unknown_format_and_version_have_typed_errors() {
    let wrong_format = r#"{
        "format":"other","version":1,
        "initial":{"width":1,"height":1,"background":{"bgra":[0,0,0,255]}},
        "commands":[]
    }"#;
    assert!(matches!(
        execute_trace(wrong_format),
        Err(TraceError::UnsupportedFormat { .. })
    ));

    let wrong_version = r#"{
        "format":"paint.r.command-trace","version":2,
        "initial":{"width":1,"height":1,"background":{"bgra":[0,0,0,255]}},
        "commands":[]
    }"#;
    assert!(matches!(
        execute_trace(wrong_version),
        Err(TraceError::UnsupportedVersion { version: 2 })
    ));
}

#[test]
fn unknown_commands_and_command_fields_report_their_command_index() {
    let cases = [
        r#"{"op":"invert"},{"op":"future_command"}"#,
        r#"{"op":"invert"},{"op":"redo","extra":1}"#,
        r#"{"op":"invert"},{"op":"fill","color":{"bgra":[0,0,0,255],"extra":1}}"#,
        r#"{"op":"invert"},{"op":"select_rect","rect":{"left":0,"top":0,"right":1,"bottom":1,"extra":1}}"#,
    ];
    for commands in cases {
        let trace = format!(
            r#"{{"format":"paint.r.command-trace","version":1,
            "initial":{{"width":1,"height":1,"background":{{"bgra":[0,0,0,255]}}}},
            "commands":[{commands}]}}"#
        );
        let error = execute_trace(&trace).unwrap_err();
        assert_eq!(error.command_index(), Some(1), "trace: {trace}");
        assert!(matches!(error, TraceError::InvalidCommand { .. }));
    }
}

#[test]
fn malformed_colors_and_empty_names_are_command_indexed() {
    let cases = [
        r#"{"op":"fill","color":{"bgra":[0,0,0]}}"#,
        r#"{"op":"fill","color":{"bgra":[0,0,0,256]}}"#,
        r#"{"op":"add_layer","name":""}"#,
    ];
    for command in cases {
        let trace = format!(
            r#"{{"format":"paint.r.command-trace","version":1,
            "initial":{{"width":1,"height":1,"background":{{"bgra":[0,0,0,255]}}}},
            "commands":[{{"op":"clear_selection"}},{command}]}}"#
        );
        let error = execute_trace(&trace).unwrap_err();
        assert_eq!(error.command_index(), Some(1), "trace: {trace}");
    }
}

#[test]
fn invalid_rectangles_are_rejected_before_execution() {
    let rectangles = [
        r#"{"left":0,"top":0,"right":0,"bottom":1}"#,
        r#"{"left":1,"top":0,"right":0,"bottom":1}"#,
        r#"{"left":-1,"top":0,"right":1,"bottom":1}"#,
        r#"{"left":0,"top":-1,"right":1,"bottom":1}"#,
        r#"{"left":0,"top":0,"right":1,"bottom":0}"#,
    ];
    for rect in rectangles {
        let trace = format!(
            r#"{{"format":"paint.r.command-trace","version":1,
            "initial":{{"width":1,"height":1,"background":{{"bgra":[0,0,0,255]}}}},
            "commands":[{{"op":"invert"}},{{"op":"select_rect","rect":{rect}}}]}}"#
        );
        let error = execute_trace(&trace).unwrap_err();
        assert_eq!(error.command_index(), Some(1), "rectangle: {rect}");
    }
}

#[test]
fn invalid_dimensions_and_resource_limits_fail_before_large_allocations() {
    for dimensions in [(0, 1), (1, 0), (16_385, 1), (1, 16_385)] {
        let trace = format!(
            r#"{{"format":"paint.r.command-trace","version":1,
            "initial":{{"width":{},"height":{},"background":{{"bgra":[0,0,0,255]}}}},
            "commands":[]}}"#,
            dimensions.0, dimensions.1
        );
        assert!(execute_trace(&trace).is_err(), "dimensions: {dimensions:?}");
    }

    let too_many_pixels = r#"{
        "format":"paint.r.command-trace","version":1,
        "initial":{"width":2,"height":2,"background":{"bgra":[0,0,0,255]}},
        "commands":[]
    }"#;
    let limits = ResourceLimits {
        max_pixels: 3,
        ..ResourceLimits::default()
    };
    assert!(matches!(
        execute_trace_with_limits(too_many_pixels, limits),
        Err(TraceError::ResourceLimit(_))
    ));

    let too_many_commands = r#"{
        "format":"paint.r.command-trace","version":1,
        "initial":{"width":1,"height":1,"background":{"bgra":[0,0,0,255]}},
        "commands":[{"op":"clear_selection"},{"op":"clear_selection"}]
    }"#;
    let limits = ResourceLimits {
        max_commands: 1,
        ..ResourceLimits::default()
    };
    assert!(matches!(
        execute_trace_with_limits(too_many_commands, limits),
        Err(TraceError::ResourceLimit(_))
    ));

    let limits = ResourceLimits {
        max_trace_bytes: 8,
        ..ResourceLimits::default()
    };
    assert!(matches!(
        execute_trace_with_limits(too_many_commands, limits),
        Err(TraceError::ResourceLimit(_))
    ));
}

#[test]
fn dynamic_command_failures_are_indexed_and_return_no_partial_output() {
    let trace = r#"{
        "format":"paint.r.command-trace","version":1,
        "initial":{"width":1,"height":1,"background":{"bgra":[255,255,255,255]}},
        "commands":[{"op":"invert"},{"op":"set_active_layer","index":4}]
    }"#;
    let error = execute_trace(trace).unwrap_err();
    assert_eq!(error.command_index(), Some(1));
    assert!(matches!(error, TraceError::Command { .. }));
}

#[test]
fn layer_and_name_resource_limits_are_enforced() {
    let add_layer = r#"{
        "format":"paint.r.command-trace","version":1,
        "initial":{"width":1,"height":1,"background":{"bgra":[0,0,0,255]}},
        "commands":[{"op":"add_layer","name":"second"}]
    }"#;
    let limits = ResourceLimits {
        max_layers: 1,
        ..ResourceLimits::default()
    };
    let error = execute_trace_with_limits(add_layer, limits).unwrap_err();
    assert_eq!(error.command_index(), Some(0));

    let limits = ResourceLimits {
        max_layer_name_bytes: 3,
        ..ResourceLimits::default()
    };
    let error = CommandTrace::from_json_with_limits(add_layer, limits).unwrap_err();
    assert_eq!(error.command_index(), Some(0));
}

#[test]
fn expectation_mismatches_identify_the_exact_field() {
    let trace = r#"{
        "format":"paint.r.command-trace","version":1,
        "initial":{"width":1,"height":1,"background":{"bgra":[0,0,0,255]}},
        "commands":[],
        "expect":{"width":2,"dirty":true}
    }"#;
    let error = execute_trace(trace).unwrap_err();
    match error {
        TraceError::Expectation(error) => {
            assert_eq!(error.field, ExpectationField::Width);
            assert_eq!(error.expected, "2");
            assert_eq!(error.actual, "1");
        }
        other => panic!("expected expectation error, got {other:?}"),
    }
}

#[test]
fn malformed_expectations_and_unknown_expectation_fields_are_rejected() {
    let bad_hash = r#"{
        "format":"paint.r.command-trace","version":1,
        "initial":{"width":1,"height":1,"background":{"bgra":[0,0,0,255]}},
        "commands":[],"expect":{"decoded_rgba_sha256":"ABCD"}
    }"#;
    assert!(matches!(
        execute_trace(bad_hash),
        Err(TraceError::InvalidExpectation { .. })
    ));

    let unknown = r#"{
        "format":"paint.r.command-trace","version":1,
        "initial":{"width":1,"height":1,"background":{"bgra":[0,0,0,255]}},
        "commands":[],"expect":{"selection":null}
    }"#;
    assert!(execute_trace(unknown).is_err());
}
