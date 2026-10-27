# Verified egui/eframe/egui_kittest 0.37 integration notes

These findings came from local 0.37.0 source inspection and a disposable Rust 1.95 probe, not from an older-version tutorial.

## Native app

`eframe::App` uses required `fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame)`; optional `logic` runs before `ui`. Native creation remains `eframe::run_native(..., Box::new(|cc| Ok(Box::new(App::new(cc)))))`. All three 0.37 crates require Rust 1.95.

## Semantic usability harness

Enable dev-dependency `egui_kittest = { version = "0.37", features = ["eframe"] }` for tests. The production trace-based `--headless-smoke` in `TRACE_PROTOCOL.md` does not need to ship kittest; it runs the real session without constructing a UI. Keep kittest as a dev-dependency unless a production flag actually uses it.

```rust,ignore
use egui_kittest::{Harness, kittest::Queryable as _};
use eframe::egui::Role;

let mut harness = Harness::new_eframe(|cc| PaintApp::new(cc));
harness.get_by_role_and_label(Role::Button, "Apply").click();
harness.run();
```

`Harness::builder().with_size(...).build_eframe(...)` is also available. The eframe harness closure returns the concrete app directly, not native's `Result<Box<dyn App>>`. Construction runs an initial pass. `state()`/`state_mut()` expose the concrete app.

Queries include `get_by_label`, `get_by_label_contains`, `get_by_role`, `get_by_role_and_label`, `get_by_value`, and scoped `get` variants. There is no `get_by_text`. Re-query after frames. `query_*` returns `None` for no match but still panics on multiple matches; scope ambiguous menu queries through their parent.

Use physical `click()` for usability evidence; `click_accesskit()` may activate invisible/non-hit-tested controls and mask defects. Menus require click, frame, re-query, then item click. Focus with `focus()`, run, and verify `is_focused()` on the re-queried node. `type_text()` does not focus by itself. Keyboard APIs include `key_press`, `key_down/up`, and `key_combination`.

`run()` settles and panics beyond its step limit; use `step()`/`run_steps(n)` for intentionally continuous repaint. Settled runs include accessibility checks for unnamed visible input widgets. Do not disable these checks to obtain green output.

## Limitations

The harness executes real `App::logic` + `App::ui` headlessly, but not native winit/window integration, OS dialogs/clipboard, persistence restore, raw-input hooks, `on_exit`, or custom GPU rendering. Those require separate native/package tests. Screenshot rendering needs `wgpu` + `snapshot` and an adapter; GPU pixels are not cross-platform correctness oracles.

Linux compile-time backend requirements remain even for headless tests. Keep eframe's x11/wayland features and native development packages in CI. A rendererless eframe build had a macOS 0.37 re-export issue in the probe; do not spend time on a rendererless product configuration.

Local source roots used by the research agent:

- `~/.cargo/registry/src/*/eframe-0.37.0/src/epi.rs` and `src/lib.rs`
- `~/.cargo/registry/src/*/egui_kittest-0.37.0/src/{lib,builder,app_kind,node,accessibility}.rs`
- `~/.cargo/registry/src/*/kittest-0.5.0/src/{query,filter,node}.rs`

The disposable probe passed one real stateful test covering role/label queries, a button, scoped menus, focus, text input/value, Tab, and app state. That probe is not a Paint.R usability result; the product lane must implement and run its own tests.
