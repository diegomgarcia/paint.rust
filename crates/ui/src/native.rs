//! Native eframe startup. Headless CLI handling never calls this module.

use crate::PaintApp;

pub fn run() -> eframe::Result<()> {
    eframe::run_native(
        "Paint.Rust Development Preview",
        native_options(),
        Box::new(|creation_context| Ok(Box::new(PaintApp::new(creation_context)))),
    )
}

fn native_options() -> eframe::NativeOptions {
    let viewport = || {
        egui::ViewportBuilder::default()
            .with_inner_size([1_280.0, 900.0])
            .with_min_inner_size([960.0, 700.0])
    };
    #[cfg(target_os = "macos")]
    {
        use winit::platform::macos::EventLoopBuilderExtMacOS as _;
        let mut options = eframe::NativeOptions {
            viewport: viewport(),
            ..eframe::NativeOptions::default()
        };
        // The default native menu terminates directly on Command-Q, bypassing
        // eframe's CloseRequested path and the document's dirty guard.
        options.event_loop_builder = Some(Box::new(|builder| {
            builder.with_default_menu(false);
        }));
        options
    }
    #[cfg(not(target_os = "macos"))]
    {
        eframe::NativeOptions {
            viewport: viewport(),
            ..eframe::NativeOptions::default()
        }
    }
}
