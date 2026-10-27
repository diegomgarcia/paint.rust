#![doc = "Isolated plugin-host protocols, transport, and lifecycle management."]

mod broker;
pub mod protocol;

pub use broker::{BrokerOptions, HostError, PluginHost, RequestId};
