#![doc = "Single-writer application sessions, traces, snapshots, and inverse-based history."]

mod command;
mod error;
mod persistence;
mod session;
mod snapshot;
mod trace;

pub use command::Command;
pub use error::{ExpectationError, ExpectationField, ResourceLimitError, SessionError, TraceError};
pub use session::{AppSession, ResourceLimits};
pub use snapshot::{
    LayerSnapshot, SNAPSHOT_FORMAT, SNAPSHOT_VERSION, SelectionSnapshot, SemanticSnapshot,
};
pub use trace::{
    CommandTrace, Expectations, InitialDocument, MAX_TRACE_DIMENSION, TRACE_FORMAT, TRACE_VERSION,
    TraceOutput, execute_trace, execute_trace_with_limits,
};
