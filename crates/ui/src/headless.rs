//! Real trace execution and deterministic output for the windowless product CLI.

use std::error::Error;
use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::Path;

use paint_app::{ResourceLimits, SemanticSnapshot, execute_trace};
use serde_json::json;

const USAGE: &str =
    "usage: paint-ui --headless-smoke --trace <trace.json> --output-dir <directory>";

/// A CLI, trace, encoding, validation, or output failure.
#[derive(Debug)]
pub struct HeadlessError {
    message: String,
}

impl HeadlessError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn invalid_cli() -> Self {
        Self::new(USAGE)
    }
}

impl fmt::Display for HeadlessError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for HeadlessError {}

/// Runs the exact headless CLI and returns the one JSON object to print on success.
///
/// This function has no eframe, window, or GPU dependency. Every other argument shape is
/// rejected, so malformed invocations cannot fall through to native startup.
pub fn run_cli(arguments: impl IntoIterator<Item = OsString>) -> Result<String, HeadlessError> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    if arguments.len() != 5
        || arguments[0] != "--headless-smoke"
        || arguments[1] != "--trace"
        || arguments[2].is_empty()
        || arguments[3] != "--output-dir"
        || arguments[4].is_empty()
    {
        return Err(HeadlessError::invalid_cli());
    }

    run(Path::new(&arguments[2]), Path::new(&arguments[4]))
}

/// Executes a trace and atomically replaces each output after all data is prepared.
///
/// The two files are individually atomic, **not** one transactional pair: if the second
/// replacement fails after the first commits, prior outputs are not rolled back. All trace,
/// expectation, encoding, serialization, and validation failures occur before either write.
pub fn run(trace_path: &Path, output_directory: &Path) -> Result<String, HeadlessError> {
    let input = read_trace(trace_path)?;
    let output = execute_trace(&input)
        .map_err(|error| HeadlessError::new(format!("failed to execute trace: {error}")))?;

    let png = paint_image_io::encode_png(&output.surface)
        .map_err(|error| HeadlessError::new(format!("failed to encode composite PNG: {error}")))?;
    let decoded = paint_image_io::decode_png(&png).map_err(|error| {
        HeadlessError::new(format!("failed to validate encoded composite PNG: {error}"))
    })?;
    if decoded != output.surface {
        return Err(HeadlessError::new(
            "encoded composite PNG did not round-trip to the composed surface",
        ));
    }

    let mut snapshot_bytes = serde_json::to_vec_pretty(&output.snapshot)
        .map_err(|error| HeadlessError::new(format!("failed to serialize snapshot: {error}")))?;
    snapshot_bytes.push(b'\n');
    let validated_snapshot: SemanticSnapshot = serde_json::from_slice(&snapshot_bytes)
        .map_err(|error| HeadlessError::new(format!("failed to validate snapshot: {error}")))?;
    if validated_snapshot != output.snapshot {
        return Err(HeadlessError::new(
            "serialized snapshot did not round-trip to the session snapshot",
        ));
    }
    let report = serde_json::to_string(&json!({
        "status": "ok",
        "width": output.snapshot.width,
        "height": output.snapshot.height,
        "layer_count": output.snapshot.layer_count,
        "active_layer_index": output.snapshot.active_layer_index,
        "dirty": output.snapshot.dirty,
        "can_undo": output.snapshot.can_undo,
        "can_redo": output.snapshot.can_redo,
        "decoded_rgba_sha256": output.snapshot.decoded_rgba_sha256,
    }))
    .map_err(|error| HeadlessError::new(format!("failed to serialize result: {error}")))?;

    fs::create_dir_all(output_directory).map_err(|error| {
        HeadlessError::new(format!(
            "failed to create output directory {}: {error}",
            output_directory.display()
        ))
    })?;
    paint_image_io::atomic_write(output_directory.join("composite.png"), &png)
        .map_err(|error| HeadlessError::new(format!("failed to write composite.png: {error}")))?;
    paint_image_io::atomic_write(output_directory.join("snapshot.json"), &snapshot_bytes)
        .map_err(|error| HeadlessError::new(format!("failed to write snapshot.json: {error}")))?;

    Ok(report)
}

fn read_trace(path: &Path) -> Result<String, HeadlessError> {
    let file = File::open(path).map_err(|error| {
        HeadlessError::new(format!("failed to open trace {}: {error}", path.display()))
    })?;
    let maximum = ResourceLimits::default().max_trace_bytes;
    let bounded = u64::try_from(maximum).unwrap_or(u64::MAX).saturating_add(1);
    let mut bytes = Vec::new();
    file.take(bounded)
        .read_to_end(&mut bytes)
        .map_err(|error| read_error(path, error))?;
    if bytes.len() > maximum {
        return Err(HeadlessError::new(format!(
            "trace input exceeds the {maximum}-byte limit"
        )));
    }
    String::from_utf8(bytes)
        .map_err(|_| HeadlessError::new("trace input is not valid UTF-8".to_owned()))
}

fn read_error(path: &Path, error: io::Error) -> HeadlessError {
    HeadlessError::new(format!("failed to read trace {}: {error}", path.display()))
}
