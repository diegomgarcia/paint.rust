use std::sync::{Mutex, OnceLock};

use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

static QUEUE: OnceLock<Mutex<Vec<NativeCommand>>> = OnceLock::new();

fn queue() -> &'static Mutex<Vec<NativeCommand>> {
    QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeCommand {
    New,
    Open,
    Export,
    Quit,
    Undo,
    Redo,
    Deselect,
    ZoomIn,
    ZoomOut,
    ActualSize,
    FitCanvas,
    FlipHorizontal,
    FlipVertical,
    Invert,
    Grayscale,
    Sepia,
    GaussianBlur,
    BrightnessContrast,
    HueSaturation,
    AutoLevel,
    ToggleTools,
    ToggleColors,
    ToggleLayers,
    ToggleHistory,
    ToggleFloatPanels,
    About,
}

impl NativeCommand {
    fn from_id(id: &str) -> Option<Self> {
        Some(match id {
            "file.new" => Self::New,
            "file.open" => Self::Open,
            "file.export" => Self::Export,
            "file.quit" => Self::Quit,
            "edit.undo" => Self::Undo,
            "edit.redo" => Self::Redo,
            "edit.deselect" => Self::Deselect,
            "view.zoom_in" => Self::ZoomIn,
            "view.zoom_out" => Self::ZoomOut,
            "view.actual" => Self::ActualSize,
            "view.fit" => Self::FitCanvas,
            "layers.flip_h" => Self::FlipHorizontal,
            "layers.flip_v" => Self::FlipVertical,
            "adjust.invert" => Self::Invert,
            "effects.grayscale" => Self::Grayscale,
            "effects.sepia" => Self::Sepia,
            "effects.gaussian_blur" => Self::GaussianBlur,
            "adjust.brightness_contrast" => Self::BrightnessContrast,
            "adjust.hue_saturation" => Self::HueSaturation,
            "adjust.auto_level" => Self::AutoLevel,
            "window.tools" => Self::ToggleTools,
            "window.colors" => Self::ToggleColors,
            "window.layers" => Self::ToggleLayers,
            "window.history" => Self::ToggleHistory,
            "window.float" => Self::ToggleFloatPanels,
            "help.about" => Self::About,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NativeMenuState {
    pub can_undo: bool,
    pub can_redo: bool,
    pub has_selection: bool,
    pub tools_visible: bool,
    pub colors_visible: bool,
    pub layers_visible: bool,
    pub history_visible: bool,
    pub float_panels: bool,
}

pub(crate) struct NativeMenu {
    _menu: Menu,
    undo: MenuItem,
    redo: MenuItem,
    deselect: MenuItem,
    tools: CheckMenuItem,
    colors: CheckMenuItem,
    layers: CheckMenuItem,
    history: CheckMenuItem,
    float_panels: CheckMenuItem,
}

impl NativeMenu {
    pub(crate) fn install(context: egui::Context) -> Option<Self> {
        objc2::MainThreadMarker::new()?;
        let menu = Menu::new();
        let app = Submenu::new("Paint.R", true);
        let about = PredefinedMenuItem::about(Some("About Paint.R"), None);
        let quit = MenuItem::with_id(
            "file.quit",
            "Quit Paint.R",
            true,
            Some(Accelerator::new(Modifiers::META, Code::KeyQ)),
        );
        app.append_items(&[&about, &PredefinedMenuItem::separator(), &quit])
            .ok()?;

        let file = Submenu::new("File", true);
        let new = MenuItem::with_id("file.new", "New", true, None);
        let open = MenuItem::with_id("file.open", "Open PNG…", true, None);
        let export = MenuItem::with_id("file.export", "Export PNG…", true, None);
        file.append_items(&[&new, &open, &export]).ok()?;

        let edit = Submenu::new("Edit", true);
        let undo = MenuItem::with_id("edit.undo", "Undo", false, None);
        let redo = MenuItem::with_id("edit.redo", "Redo", false, None);
        let deselect = MenuItem::with_id(
            "edit.deselect",
            "Deselect",
            false,
            Some(Accelerator::new(Modifiers::META, Code::KeyD)),
        );
        edit.append_items(&[&undo, &redo, &PredefinedMenuItem::separator(), &deselect])
            .ok()?;

        let view = Submenu::new("View", true);
        let zoom_in = MenuItem::with_id("view.zoom_in", "Zoom In", true, None);
        let zoom_out = MenuItem::with_id("view.zoom_out", "Zoom Out", true, None);
        let actual = MenuItem::with_id("view.actual", "Actual Size", true, None);
        let fit = MenuItem::with_id("view.fit", "Fit Canvas", true, None);
        view.append_items(&[&zoom_in, &zoom_out, &actual, &fit])
            .ok()?;

        let layers = Submenu::new("Layers", true);
        let flip_h = MenuItem::with_id(
            "layers.flip_h",
            "Flip Active Layer Horizontally",
            true,
            None,
        );
        let flip_v = MenuItem::with_id("layers.flip_v", "Flip Active Layer Vertically", true, None);
        layers.append_items(&[&flip_h, &flip_v]).ok()?;

        let adjustments = Submenu::new("Adjustments", true);
        let invert = MenuItem::with_id("adjust.invert", "Invert Colors", true, None);
        let brightness_contrast = MenuItem::with_id(
            "adjust.brightness_contrast",
            "Brightness / Contrast…",
            true,
            None,
        );
        let hue_saturation =
            MenuItem::with_id("adjust.hue_saturation", "Hue / Saturation…", true, None);
        let auto_level = MenuItem::with_id("adjust.auto_level", "Auto-Level…", true, None);
        adjustments
            .append_items(&[
                &invert,
                &PredefinedMenuItem::separator(),
                &brightness_contrast,
                &hue_saturation,
                &auto_level,
            ])
            .ok()?;

        let effects = Submenu::new("Effects", true);
        let artistic = Submenu::new("Artistic", true);
        let ink_sketch =
            MenuItem::with_id("effects.artistic.ink_sketch", "Ink Sketch", false, None);
        let oil_painting =
            MenuItem::with_id("effects.artistic.oil_painting", "Oil Painting", false, None);
        let pencil_sketch = MenuItem::with_id(
            "effects.artistic.pencil_sketch",
            "Pencil Sketch",
            false,
            None,
        );
        artistic
            .append_items(&[&ink_sketch, &oil_painting, &pencil_sketch])
            .ok()?;

        let blurs = Submenu::new("Blurs", true);
        let gaussian_blur =
            MenuItem::with_id("effects.gaussian_blur", "Gaussian Blur…", true, None);
        let motion_blur = MenuItem::with_id("effects.blurs.motion", "Motion Blur", false, None);
        let radial_blur = MenuItem::with_id("effects.blurs.radial", "Radial Blur", false, None);
        let unfocus = MenuItem::with_id("effects.blurs.unfocus", "Unfocus", false, None);
        blurs
            .append_items(&[&gaussian_blur, &motion_blur, &radial_blur, &unfocus])
            .ok()?;

        let distort = Submenu::new("Distort", true);
        let bulge = MenuItem::with_id("effects.distort.bulge", "Bulge", false, None);
        let dents = MenuItem::with_id("effects.distort.dents", "Dents", false, None);
        let pixelate = MenuItem::with_id("effects.distort.pixelate", "Pixelate", false, None);
        let twist = MenuItem::with_id("effects.distort.twist", "Twist", false, None);
        distort
            .append_items(&[&bulge, &dents, &pixelate, &twist])
            .ok()?;

        let noise = Submenu::new("Noise", true);
        let add_noise = MenuItem::with_id("effects.noise.add", "Add Noise", false, None);
        let median = MenuItem::with_id("effects.noise.median", "Median", false, None);
        let reduce_noise = MenuItem::with_id("effects.noise.reduce", "Reduce Noise", false, None);
        noise
            .append_items(&[&add_noise, &median, &reduce_noise])
            .ok()?;

        let photo = Submenu::new("Photo", true);
        let grayscale = MenuItem::with_id("effects.grayscale", "Grayscale", true, None);
        let sepia = MenuItem::with_id("effects.sepia", "Sepia", true, None);
        let glow = MenuItem::with_id("effects.photo.glow", "Glow", false, None);
        let sharpen = MenuItem::with_id("effects.photo.sharpen", "Sharpen", false, None);
        let soft_portrait = MenuItem::with_id(
            "effects.photo.soften_portrait",
            "Soften Portrait",
            false,
            None,
        );
        let vignette = MenuItem::with_id("effects.photo.vignette", "Vignette", false, None);
        photo
            .append_items(&[
                &grayscale,
                &sepia,
                &PredefinedMenuItem::separator(),
                &glow,
                &sharpen,
                &soft_portrait,
                &vignette,
            ])
            .ok()?;

        let render = Submenu::new("Render", true);
        let clouds = MenuItem::with_id("effects.render.clouds", "Clouds", false, None);
        let julia = MenuItem::with_id("effects.render.julia", "Julia Fractal", false, None);
        let mandelbrot = MenuItem::with_id(
            "effects.render.mandelbrot",
            "Mandelbrot Fractal",
            false,
            None,
        );
        render.append_items(&[&clouds, &julia, &mandelbrot]).ok()?;

        let stylize = Submenu::new("Stylize", true);
        let edge_detect = MenuItem::with_id("effects.stylize.edge", "Edge Detect", false, None);
        let emboss = MenuItem::with_id("effects.stylize.emboss", "Emboss", false, None);
        let outline = MenuItem::with_id("effects.stylize.outline", "Outline", false, None);
        let relief = MenuItem::with_id("effects.stylize.relief", "Relief", false, None);
        stylize
            .append_items(&[&edge_detect, &emboss, &outline, &relief])
            .ok()?;

        effects
            .append_items(&[
                &artistic, &blurs, &distort, &noise, &photo, &render, &stylize,
            ])
            .ok()?;

        let window = Submenu::new("Window", true);
        let tools = CheckMenuItem::with_id("window.tools", "Tools", true, true, None);
        let colors = CheckMenuItem::with_id("window.colors", "Colors", true, true, None);
        let layer_window = CheckMenuItem::with_id("window.layers", "Layers", true, true, None);
        let history = CheckMenuItem::with_id("window.history", "History", true, true, None);
        let float_panels = CheckMenuItem::with_id("window.float", "Float Panels", true, true, None);
        window
            .append_items(&[
                &tools,
                &colors,
                &layer_window,
                &history,
                &PredefinedMenuItem::separator(),
                &float_panels,
            ])
            .ok()?;

        let help = Submenu::new("Help", true);
        let about_item = MenuItem::with_id("help.about", "About Paint.R", true, None);
        help.append(&about_item).ok()?;

        menu.append_items(&[
            &app,
            &file,
            &edit,
            &view,
            &layers,
            &adjustments,
            &effects,
            &window,
            &help,
        ])
        .ok()?;
        menu.init_for_nsapp();
        MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
            if let Some(command) = NativeCommand::from_id(&event.id.0) {
                if let Ok(mut queue) = queue().lock() {
                    queue.push(command);
                }
                context.request_repaint();
            }
        }));

        Some(Self {
            _menu: menu,
            undo,
            redo,
            deselect,
            tools,
            colors,
            layers: layer_window,
            history,
            float_panels,
        })
    }

    pub(crate) fn poll() -> Vec<NativeCommand> {
        queue()
            .lock()
            .map(|mut queue| std::mem::take(&mut *queue))
            .unwrap_or_default()
    }

    pub(crate) fn sync(&self, state: NativeMenuState) {
        self.undo.set_enabled(state.can_undo);
        self.redo.set_enabled(state.can_redo);
        self.deselect.set_enabled(state.has_selection);
        self.tools.set_checked(state.tools_visible);
        self.colors.set_checked(state.colors_visible);
        self.layers.set_checked(state.layers_visible);
        self.history.set_checked(state.history_visible);
        self.float_panels.set_checked(state.float_panels);
    }
}

#[cfg(test)]
mod tests {
    use super::{NativeCommand, NativeCommand::*};

    #[test]
    fn every_native_menu_id_maps_to_its_command() {
        let ids = [
            ("file.new", New),
            ("file.open", Open),
            ("file.export", Export),
            ("file.quit", Quit),
            ("edit.undo", Undo),
            ("edit.redo", Redo),
            ("edit.deselect", Deselect),
            ("view.zoom_in", ZoomIn),
            ("view.zoom_out", ZoomOut),
            ("view.actual", ActualSize),
            ("view.fit", FitCanvas),
            ("layers.flip_h", FlipHorizontal),
            ("layers.flip_v", FlipVertical),
            ("adjust.invert", Invert),
            ("effects.grayscale", Grayscale),
            ("effects.sepia", Sepia),
            ("effects.gaussian_blur", GaussianBlur),
            ("adjust.brightness_contrast", BrightnessContrast),
            ("adjust.hue_saturation", HueSaturation),
            ("adjust.auto_level", AutoLevel),
            ("window.tools", ToggleTools),
            ("window.colors", ToggleColors),
            ("window.layers", ToggleLayers),
            ("window.history", ToggleHistory),
            ("window.float", ToggleFloatPanels),
            ("help.about", About),
        ];
        for (id, command) in ids {
            assert_eq!(NativeCommand::from_id(id), Some(command), "{id}");
        }
        assert_eq!(NativeCommand::from_id("unknown"), None);
    }
}
