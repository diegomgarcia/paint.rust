use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use paint_core::Bgra;
use serde_json::{Value, json};
use tempfile::TempDir;

const BLACK_RGBA_SHA256: &str = "a7aff0f8e3f44663c369e1e75a981b90b66949586d71035771921871d67e334e";
const WHITE_2X2_RGBA_SHA256: &str =
    "5ac6a5945f16500911219129984ba8b387a06f24fe383ce4e81a73294065461b";

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_paint-ui")
}

fn checked_in_trace(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/e2e/traces")
        .join(name)
}

fn run(arguments: &[OsString]) -> Output {
    Command::new(binary())
        .args(arguments)
        .output()
        .expect("run actual paint-ui binary")
}

fn run_trace(trace: &Path, output: &Path) -> Output {
    run(&[
        OsString::from("--headless-smoke"),
        OsString::from("--trace"),
        trace.as_os_str().to_owned(),
        OsString::from("--output-dir"),
        output.as_os_str().to_owned(),
    ])
}

fn success_json(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "paint-ui failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "",
        "successful headless runs must not emit diagnostics"
    );
    let stdout = std::str::from_utf8(&output.stdout).expect("UTF-8 stdout");
    let report: Value = serde_json::from_str(stdout).expect("exactly one stdout JSON object");
    assert!(report.is_object());
    assert_eq!(report["status"], "ok");
    report
}

fn seed_prior_outputs(output: &Path) -> (Vec<u8>, Vec<u8>) {
    fs::create_dir_all(output).expect("create prior output directory");
    let png = b"previous-good-composite".to_vec();
    let snapshot = b"previous-good-snapshot".to_vec();
    fs::write(output.join("composite.png"), &png).expect("seed composite");
    fs::write(output.join("snapshot.json"), &snapshot).expect("seed snapshot");
    (png, snapshot)
}

fn assert_prior_outputs(output: &Path, prior: &(Vec<u8>, Vec<u8>)) {
    assert_eq!(
        fs::read(output.join("composite.png")).expect("read preserved composite"),
        prior.0
    );
    assert_eq!(
        fs::read(output.join("snapshot.json")).expect("read preserved snapshot"),
        prior.1
    );
}

fn trace_with(commands: Value, expect: Option<Value>) -> Vec<u8> {
    let mut trace = json!({
        "format": "paint.r.command-trace",
        "version": 1,
        "initial": {
            "width": 2,
            "height": 2,
            "background": { "bgra": [255, 255, 255, 255] }
        },
        "commands": commands,
    });
    if let Some(expect) = expect {
        trace["expect"] = expect;
    }
    serde_json::to_vec(&trace).expect("serialize test trace")
}

fn assert_rejected_without_clobber(trace_bytes: &[u8], diagnostic: &str) {
    let temporary = TempDir::new().expect("temporary directory");
    let trace = temporary.path().join("bad.json");
    fs::write(&trace, trace_bytes).expect("write bad trace");
    let output_directory = temporary.path().join("output");
    let prior = seed_prior_outputs(&output_directory);

    let output = run_trace(&trace, &output_directory);

    assert!(!output.status.success());
    assert_eq!(output.stdout, b"", "failed runs must not report success");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(diagnostic),
        "diagnostic {stderr:?} does not contain {diagnostic:?}"
    );
    assert_prior_outputs(&output_directory, &prior);
}

#[test]
fn invert_trace_writes_exact_outputs_and_repeat_is_byte_deterministic() {
    let temporary = TempDir::new().expect("temporary directory");
    let output_directory = temporary.path().join("nested/output");
    let trace = checked_in_trace("invert-800x600.json");

    let first = run_trace(&trace, &output_directory);
    let first_report = success_json(&first);
    let first_png = fs::read(output_directory.join("composite.png")).expect("read composite");
    let first_snapshot =
        fs::read(output_directory.join("snapshot.json")).expect("read snapshot bytes");
    let snapshot: Value = serde_json::from_slice(&first_snapshot).expect("snapshot JSON");

    assert_eq!(first_report["width"], 800);
    assert_eq!(first_report["height"], 600);
    assert_eq!(first_report["decoded_rgba_sha256"], BLACK_RGBA_SHA256);
    assert_eq!(
        snapshot,
        json!({
            "format": "paint.r.semantic-snapshot",
            "version": 1,
            "width": 800,
            "height": 600,
            "layer_count": 1,
            "active_layer_index": 0,
            "dirty": true,
            "can_undo": true,
            "can_redo": false,
            "layers": [{
                "name": "Background",
                "visible": true,
                "opacity": 255,
                "is_background": true,
                "blend_mode": "normal"
            }],
            "selection": null,
            "decoded_rgba_sha256": BLACK_RGBA_SHA256
        })
    );

    let decoded = paint_image_io::decode_png(&first_png).expect("independently decode output PNG");
    assert_eq!((decoded.width(), decoded.height()), (800, 600));
    assert!(decoded.pixels().iter().all(|pixel| *pixel == Bgra::BLACK));
    assert_eq!(
        decoded.to_rgba_bytes().expect("decoded RGBA"),
        [0, 0, 0, 255].repeat(800 * 600)
    );

    let second = run_trace(&trace, &output_directory);
    let second_report = success_json(&second);
    assert_eq!(second.stdout, first.stdout);
    assert_eq!(second_report, first_report);
    assert_eq!(
        fs::read(output_directory.join("composite.png")).expect("read repeated composite"),
        first_png
    );
    assert_eq!(
        fs::read(output_directory.join("snapshot.json")).expect("read repeated snapshot"),
        first_snapshot
    );
    let mut names = fs::read_dir(&output_directory)
        .expect("list output directory")
        .map(|entry| {
            entry
                .expect("directory entry")
                .file_name()
                .into_string()
                .expect("UTF-8 output name")
        })
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["composite.png", "snapshot.json"]);
}

#[test]
fn undo_trace_has_exact_white_pixels_snapshot_and_stdout() {
    let temporary = TempDir::new().expect("temporary directory");
    let output_directory = temporary.path().join("output");
    let output = run_trace(&checked_in_trace("undo-invert-2x2.json"), &output_directory);
    let report = success_json(&output);
    assert_eq!(report["decoded_rgba_sha256"], WHITE_2X2_RGBA_SHA256);

    let snapshot: Value = serde_json::from_slice(
        &fs::read(output_directory.join("snapshot.json")).expect("read snapshot"),
    )
    .expect("snapshot JSON");
    assert_eq!(snapshot["width"], 2);
    assert_eq!(snapshot["height"], 2);
    assert_eq!(snapshot["layer_count"], 1);
    assert_eq!(snapshot["active_layer_index"], 0);
    assert_eq!(snapshot["dirty"], false);
    assert_eq!(snapshot["can_undo"], false);
    assert_eq!(snapshot["can_redo"], true);
    assert_eq!(snapshot["selection"], Value::Null);
    assert_eq!(snapshot["decoded_rgba_sha256"], WHITE_2X2_RGBA_SHA256);

    let decoded = paint_image_io::decode_png(
        &fs::read(output_directory.join("composite.png")).expect("read composite"),
    )
    .expect("independently decode output PNG");
    assert_eq!(decoded.pixels(), &[Bgra::WHITE; 4]);
    assert_eq!(decoded.to_rgba_bytes().expect("decoded RGBA"), [255; 16]);
}

#[test]
fn unknown_command_is_nonzero_and_preserves_prior_outputs() {
    assert_rejected_without_clobber(
        &trace_with(json!([{"op": "invert"}, {"op": "not-a-command"}]), None),
        "invalid trace command at index 1",
    );
}

#[test]
fn invalid_rectangle_is_nonzero_and_preserves_prior_outputs() {
    assert_rejected_without_clobber(
        &trace_with(
            json!([{
                "op": "select_rect",
                "rect": {"left": 2, "top": 0, "right": 1, "bottom": 1}
            }]),
            None,
        ),
        "invalid rectangle in trace command 0",
    );
}

#[test]
fn expectation_mismatch_is_nonzero_and_preserves_prior_outputs() {
    assert_rejected_without_clobber(
        &trace_with(json!([{"op": "invert"}]), Some(json!({"can_undo": false}))),
        "expectation can_undo mismatch",
    );
}

#[test]
fn malformed_json_invalid_utf8_and_oversized_input_preserve_prior_outputs() {
    assert_rejected_without_clobber(b"{", "invalid trace JSON");
    assert_rejected_without_clobber(b"\xff\xfe", "not valid UTF-8");
    let oversized = vec![b' '; paint_app::ResourceLimits::default().max_trace_bytes + 1];
    assert_rejected_without_clobber(&oversized, "trace input exceeds");
}

#[test]
fn dynamic_command_failure_is_nonzero_and_preserves_prior_outputs() {
    assert_rejected_without_clobber(
        &trace_with(
            json!([{"op": "invert"}, {"op": "set_active_layer", "index": 9}]),
            None,
        ),
        "trace command 1 failed",
    );
}

#[test]
fn invalid_cli_fails_without_running_trace_or_touching_outputs() {
    let temporary = TempDir::new().expect("temporary directory");
    let trace = checked_in_trace("undo-invert-2x2.json");
    let output_directory = temporary.path().join("output");
    let prior = seed_prior_outputs(&output_directory);
    let headless = OsString::from("--headless-smoke");
    let trace_flag = OsString::from("--trace");
    let output_flag = OsString::from("--output-dir");
    let trace_path = trace.as_os_str().to_owned();
    let output_path = output_directory.as_os_str().to_owned();
    let cases = vec![
        vec![trace_flag.clone(), trace_path.clone()],
        vec![headless.clone()],
        vec![headless.clone(), trace_flag.clone(), trace_path.clone()],
        vec![
            headless.clone(),
            trace_flag.clone(),
            trace_path.clone(),
            output_flag.clone(),
        ],
        vec![
            headless.clone(),
            trace_flag.clone(),
            trace_path.clone(),
            output_flag.clone(),
            output_path.clone(),
            OsString::from("extra"),
        ],
        vec![
            headless.clone(),
            OsString::from("--unknown"),
            trace_flag,
            trace_path,
            output_flag,
            output_path,
        ],
    ];

    for arguments in cases {
        let output = run(&arguments);
        assert!(
            !output.status.success(),
            "arguments accepted: {arguments:?}"
        );
        assert_eq!(output.stdout, b"");
        assert!(!output.stderr.is_empty());
        assert_prior_outputs(&output_directory, &prior);
    }
}
