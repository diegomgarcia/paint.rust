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
        let deselect = MenuItem::with_id("edit.deselect", "Deselect", false, None);
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
        adjustments.append(&invert).ok()?;

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
