use paint_core::{Bgra, Rect, Size, Surface};
use paint_document::{Document, Layer, LayerId};
use serde::Deserialize;

use crate::{
    AppSession, Command, ExpectationError, ExpectationField, ResourceLimitError, ResourceLimits,
    SemanticSnapshot, SessionError, TraceError,
};

pub const TRACE_FORMAT: &str = "paint.r.command-trace";
pub const TRACE_VERSION: u32 = 1;
pub const MAX_TRACE_DIMENSION: u32 = 16_384;

/// Custom initial canvas used by a version 1 trace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InitialDocument {
    pub width: u32,
    pub height: u32,
    pub background: Bgra,
}

/// Optional exact final-state assertions. Absent fields are not checked.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Expectations {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub decoded_rgba_sha256: Option<String>,
    pub layer_count: Option<u64>,
    pub active_layer_index: Option<u64>,
    pub dirty: Option<bool>,
    pub can_undo: Option<bool>,
    pub can_redo: Option<bool>,
}

/// A fully parsed and statically validated command trace.
#[derive(Clone, Debug)]
pub struct CommandTrace {
    pub initial: InitialDocument,
    pub commands: Vec<Command>,
    pub expect: Option<Expectations>,
    limits: ResourceLimits,
}

impl CommandTrace {
    pub fn new(initial: InitialDocument, commands: Vec<Command>) -> Self {
        Self {
            initial,
            commands,
            expect: None,
            limits: ResourceLimits::default(),
        }
    }

    pub fn with_expectations(mut self, expectations: Expectations) -> Self {
        self.expect = Some(expectations);
        self
    }

    pub fn with_limits(mut self, limits: ResourceLimits) -> Self {
        self.limits = limits;
        self
    }

    pub const fn limits(&self) -> ResourceLimits {
        self.limits
    }

    /// Parses JSON with serde, rejecting unknown data and enforcing default resource bounds.
    pub fn from_json(input: &str) -> Result<Self, TraceError> {
        Self::from_json_with_limits(input, ResourceLimits::default())
    }

    /// Parses and statically validates every command before creating a document.
    pub fn from_json_with_limits(input: &str, limits: ResourceLimits) -> Result<Self, TraceError> {
        if input.len() > limits.max_trace_bytes {
            return Err(ResourceLimitError::TraceBytes {
                actual: input.len(),
                max: limits.max_trace_bytes,
            }
            .into());
        }
        let raw: RawTrace = serde_json::from_str(input)?;
        if raw.format != TRACE_FORMAT {
            return Err(TraceError::UnsupportedFormat { format: raw.format });
        }
        if raw.version != TRACE_VERSION {
            return Err(TraceError::UnsupportedVersion {
                version: raw.version,
            });
        }
        validate_initial(raw.initial.width, raw.initial.height, &limits)?;
        if raw.commands.len() > limits.max_commands {
            return Err(ResourceLimitError::Commands {
                actual: raw.commands.len(),
                max: limits.max_commands,
            }
            .into());
        }
        if let Some(expectations) = &raw.expect {
            validate_expectations(expectations)?;
        }

        let mut commands = Vec::new();
        commands
            .try_reserve_exact(raw.commands.len())
            .map_err(|_| ResourceLimitError::Commands {
                actual: raw.commands.len(),
                max: limits.max_commands,
            })?;
        for (index, raw_command) in raw.commands.into_iter().enumerate() {
            validate_wire_command_fields(&raw_command, index)?;
            let wire: WireCommand = serde_json::from_value(raw_command)
                .map_err(|source| TraceError::InvalidCommand { index, source })?;
            let command = wire.into_command(index)?;
            validate_command(&command, index, &limits)?;
            commands.push(command);
        }

        Ok(Self {
            initial: InitialDocument {
                width: raw.initial.width,
                height: raw.initial.height,
                background: raw.initial.background.into_bgra(),
            },
            commands,
            expect: raw.expect,
            limits,
        })
    }

    /// Executes against a fresh session without mutating the parsed trace or any external state.
    pub fn execute(&self) -> Result<TraceOutput, TraceError> {
        validate_initial(self.initial.width, self.initial.height, &self.limits)?;
        if self.commands.len() > self.limits.max_commands {
            return Err(ResourceLimitError::Commands {
                actual: self.commands.len(),
                max: self.limits.max_commands,
            }
            .into());
        }
        for (index, command) in self.commands.iter().enumerate() {
            validate_command(command, index, &self.limits)?;
        }
        if let Some(expectations) = &self.expect {
            validate_expectations(expectations)?;
        }

        let size = Size::new(self.initial.width, self.initial.height).map_err(|_| {
            TraceError::InvalidDimensions {
                width: self.initial.width,
                height: self.initial.height,
            }
        })?;
        let surface = Surface::filled(size, self.initial.background).map_err(SessionError::from)?;
        let background = Layer::new(LayerId::new(1), "Background", surface).with_background(true);
        let document = Document::from_layers(size, vec![background]).map_err(SessionError::from)?;
        let mut session = AppSession::with_limits(document, self.limits)?;
        for (index, command) in self.commands.iter().cloned().enumerate() {
            session
                .execute(command)
                .map_err(|source| TraceError::Command { index, source })?;
        }
        let (surface, snapshot) = session.compose_and_snapshot()?;
        if let Some(expectations) = &self.expect {
            check_expectations(expectations, &snapshot)?;
        }
        Ok(TraceOutput { surface, snapshot })
    }
}

/// Pure headless result: the exact composed pixels and their corresponding semantic snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceOutput {
    pub surface: Surface,
    pub snapshot: SemanticSnapshot,
}

pub fn execute_trace(input: &str) -> Result<TraceOutput, TraceError> {
    CommandTrace::from_json(input)?.execute()
}

pub fn execute_trace_with_limits(
    input: &str,
    limits: ResourceLimits,
) -> Result<TraceOutput, TraceError> {
    CommandTrace::from_json_with_limits(input, limits)?.execute()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTrace {
    format: String,
    version: u32,
    initial: RawInitial,
    commands: Vec<serde_json::Value>,
    expect: Option<Expectations>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInitial {
    width: u32,
    height: u32,
    background: WireColor,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireColor {
    bgra: [u8; 4],
}

impl WireColor {
    const fn into_bgra(self) -> Bgra {
        Bgra::from_bgra_bytes(self.bgra)
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireRect {
    left: i64,
    top: i64,
    right: i64,
    bottom: i64,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum WireCommand {
    Invert,
    Undo,
    Redo,
    ClearSelection,
    SelectRect {
        rect: WireRect,
    },
    Fill {
        color: WireColor,
    },
    AddLayer {
        name: String,
        #[serde(default = "default_opacity")]
        opacity: u8,
        #[serde(default = "default_visible")]
        visible: bool,
    },
    SetActiveLayer {
        index: usize,
    },
    SetLayerVisibility {
        index: usize,
        visible: bool,
    },
    SetLayerOpacity {
        index: usize,
        opacity: u8,
    },
}

impl WireCommand {
    fn into_command(self, index: usize) -> Result<Command, TraceError> {
        match self {
            Self::Invert => Ok(Command::Invert),
            Self::Undo => Ok(Command::Undo),
            Self::Redo => Ok(Command::Redo),
            Self::ClearSelection => Ok(Command::ClearSelection),
            Self::SelectRect { rect } => Rect::new(rect.left, rect.top, rect.right, rect.bottom)
                .map(Command::SelectRect)
                .map_err(|source| TraceError::InvalidRectangle { index, source }),
            Self::Fill { color } => Ok(Command::Fill(color.into_bgra())),
            Self::AddLayer {
                name,
                opacity,
                visible,
            } => Ok(Command::AddLayer {
                name,
                opacity,
                visible,
            }),
            Self::SetActiveLayer { index: layer } => Ok(Command::SetActiveLayer { index: layer }),
            Self::SetLayerVisibility {
                index: layer,
                visible,
            } => Ok(Command::SetLayerVisibility {
                index: layer,
                visible,
            }),
            Self::SetLayerOpacity {
                index: layer,
                opacity,
            } => Ok(Command::SetLayerOpacity {
                index: layer,
                opacity,
            }),
        }
    }
}

const fn default_opacity() -> u8 {
    u8::MAX
}

const fn default_visible() -> bool {
    true
}

fn validate_wire_command_fields(value: &serde_json::Value, index: usize) -> Result<(), TraceError> {
    let (Some(object), Some(operation)) = (
        value.as_object(),
        value.get("op").and_then(serde_json::Value::as_str),
    ) else {
        return Ok(());
    };
    let allowed: &[&str] = match operation {
        "invert" | "undo" | "redo" | "clear_selection" => &["op"],
        "select_rect" => &["op", "rect"],
        "fill" => &["op", "color"],
        "add_layer" => &["op", "name", "opacity", "visible"],
        "set_active_layer" => &["op", "index"],
        "set_layer_visibility" => &["op", "index", "visible"],
        "set_layer_opacity" => &["op", "index", "opacity"],
        _ => return Ok(()),
    };
    for field in object.keys() {
        if !allowed.contains(&field.as_str()) {
            let source = <serde_json::Error as serde::de::Error>::custom(format!(
                "unknown field `{field}` for operation `{operation}`"
            ));
            return Err(TraceError::InvalidCommand { index, source });
        }
    }
    Ok(())
}

fn validate_initial(width: u32, height: u32, limits: &ResourceLimits) -> Result<(), TraceError> {
    if width == 0 || height == 0 {
        return Err(TraceError::InvalidDimensions { width, height });
    }
    let max_width = limits.max_width.min(MAX_TRACE_DIMENSION);
    let max_height = limits.max_height.min(MAX_TRACE_DIMENSION);
    if width > max_width || height > max_height {
        return Err(ResourceLimitError::Dimensions {
            width,
            height,
            max_width,
            max_height,
        }
        .into());
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > limits.max_pixels {
        return Err(ResourceLimitError::Pixels {
            actual: pixels,
            max: limits.max_pixels,
        }
        .into());
    }
    Ok(())
}

fn validate_command(
    command: &Command,
    index: usize,
    limits: &ResourceLimits,
) -> Result<(), TraceError> {
    match command {
        Command::SelectRect(rect)
            if rect.left() < 0 || rect.top() < 0 || rect.right() < 1 || rect.bottom() < 1 =>
        {
            Err(TraceError::InvalidRectangleCoordinates {
                index,
                left: rect.left(),
                top: rect.top(),
                right: rect.right(),
                bottom: rect.bottom(),
            })
        }
        Command::AddLayer { name, .. } if name.is_empty() => {
            Err(TraceError::EmptyLayerName { index })
        }
        Command::AddLayer { name, .. } if name.len() > limits.max_layer_name_bytes => {
            Err(TraceError::CommandResourceLimit {
                index,
                source: ResourceLimitError::LayerNameBytes {
                    actual: name.len(),
                    max: limits.max_layer_name_bytes,
                },
            })
        }
        _ => Ok(()),
    }
}

fn validate_expectations(expectations: &Expectations) -> Result<(), TraceError> {
    if expectations.width == Some(0) || expectations.height == Some(0) {
        return Err(TraceError::InvalidExpectation {
            message: "expected width and height must be nonzero".to_owned(),
        });
    }
    if let Some(hash) = &expectations.decoded_rgba_sha256 {
        let valid = hash.len() == 64
            && hash
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        if !valid {
            return Err(TraceError::InvalidExpectation {
                message: "decoded_rgba_sha256 must contain exactly 64 lowercase hex digits"
                    .to_owned(),
            });
        }
    }
    Ok(())
}

fn check_expectations(
    expectations: &Expectations,
    snapshot: &SemanticSnapshot,
) -> Result<(), ExpectationError> {
    if let Some(expected) = expectations.width {
        check_expectation(ExpectationField::Width, expected, snapshot.width)?;
    }
    if let Some(expected) = expectations.height {
        check_expectation(ExpectationField::Height, expected, snapshot.height)?;
    }
    if let Some(expected) = &expectations.decoded_rgba_sha256 {
        check_expectation(
            ExpectationField::DecodedRgbaSha256,
            expected,
            &snapshot.decoded_rgba_sha256,
        )?;
    }
    if let Some(expected) = expectations.layer_count {
        check_expectation(
            ExpectationField::LayerCount,
            expected,
            snapshot.layer_count as u64,
        )?;
    }
    if let Some(expected) = expectations.active_layer_index {
        check_expectation(
            ExpectationField::ActiveLayerIndex,
            expected,
            snapshot.active_layer_index as u64,
        )?;
    }
    if let Some(expected) = expectations.dirty {
        check_expectation(ExpectationField::Dirty, expected, snapshot.dirty)?;
    }
    if let Some(expected) = expectations.can_undo {
        check_expectation(ExpectationField::CanUndo, expected, snapshot.can_undo)?;
    }
    if let Some(expected) = expectations.can_redo {
        check_expectation(ExpectationField::CanRedo, expected, snapshot.can_redo)?;
    }
    Ok(())
}

fn check_expectation<T>(
    field: ExpectationField,
    expected: T,
    actual: T,
) -> Result<(), ExpectationError>
where
    T: PartialEq + ToString,
{
    if expected == actual {
        return Ok(());
    }
    Err(ExpectationError {
        field,
        expected: expected.to_string(),
        actual: actual.to_string(),
    })
}
