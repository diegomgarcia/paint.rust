//! Native eframe startup. Headless CLI handling never calls this module.

use crate::PaintApp;

pub fn run() -> eframe::Result<()> {
    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1_280.0, 900.0])
            .with_min_inner_size([960.0, 700.0]),
        ..eframe::NativeOptions::default()
    };
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::EventLoopBuilderExtMacOS as _;
        options.event_loop_builder = Some(Box::new(|builder| {
            // The default native menu terminates directly on Command-Q, bypassing
            // eframe's CloseRequested path and the document's dirty guard.
            builder.with_default_menu(false);
        }));
    }
    eframe::run_native(
        "Paint.R Development Preview",
        options,
        Box::new(|creation_context| Ok(Box::new(PaintApp::new(creation_context)))),
    )
}
