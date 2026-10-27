#![doc = "Desktop editor UI, real headless trace execution, and semantic usability-test hooks."]

mod app;
pub mod headless;
pub mod native;

pub use app::PaintApp;
