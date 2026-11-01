//! Native save panel with an in-panel file-format popup.
//!
//! macOS uses `NSSavePanel` plus an accessory `NSPopUpButton`; other platforms
//! use the native `rfd` dialog with one filter per format.

use std::path::{Path, PathBuf};

/// One selectable format in the native save panel.
pub(crate) struct FormatOption {
    pub label: String,
    pub extensions: Vec<String>,
    pub extension: String,
}

#[cfg(target_os = "macos")]
use objc2::MainThreadOnly;
#[cfg(target_os = "macos")]
use objc2_app_kit::{NSModalResponseOK, NSPopUpButton, NSSavePanel, NSTextField, NSView};
#[cfg(target_os = "macos")]
use objc2_foundation::{MainThreadMarker, NSArray, NSPoint, NSRect, NSSize, NSString, NSURL};

/// Shows the native save panel and returns the chosen path plus selected format index.
#[cfg(target_os = "macos")]
pub(crate) fn save_panel(
    title: &str,
    current: &Path,
    options: &[FormatOption],
) -> Option<(PathBuf, usize)> {
    let mtm = MainThreadMarker::new()?;
    let panel = NSSavePanel::savePanel(mtm);
    panel.setTitle(Some(&NSString::from_str(title)));
    panel.setCanCreateDirectories(true);

    let popup = NSPopUpButton::initWithFrame_pullsDown(
        NSPopUpButton::alloc(mtm),
        NSRect::new(NSPoint::ZERO, NSSize::new(300.0, 24.0)),
        false,
    );
    for option in options {
        popup.addItemWithTitle(&NSString::from_str(&option.label));
    }
    let current_index = current
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(|extension| {
            options.iter().position(|option| {
                option
                    .extensions
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(extension))
            })
        })
        .unwrap_or(0);
    popup.selectItemAtIndex(current_index as isize);

    let accessory = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::ZERO, NSSize::new(330.0, 32.0)),
    );
    let label = NSTextField::labelWithString(&NSString::from_str("File type:"), mtm);
    label.setFrame(NSRect::new(NSPoint::ZERO, NSSize::new(64.0, 20.0)));
    popup.setFrame(NSRect::new(
        NSPoint::new(66.0, 0.0),
        NSSize::new(260.0, 24.0),
    ));
    accessory.addSubview(&label);
    accessory.addSubview(&popup);
    panel.setAccessoryView(Some(&accessory));

    let mut allowed = Vec::new();
    for option in options {
        for extension in &option.extensions {
            allowed.push(NSString::from_str(extension));
        }
    }
    let allowed_refs: Vec<&NSString> = allowed.iter().map(|value| &**value).collect();
    #[allow(deprecated)]
    panel.setAllowedFileTypes(Some(&NSArray::from_slice(&allowed_refs)));

    let name = current
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| format!("Untitled.{}", options[0].extension));
    panel.setNameFieldStringValue(&NSString::from_str(&name));
    if let Some(parent) = current.parent().filter(|parent| parent.is_dir()) {
        let url = NSURL::fileURLWithPath_isDirectory(
            &NSString::from_str(&parent.display().to_string()),
            true,
        );
        panel.setDirectoryURL(Some(&url));
    }

    if panel.runModal() != NSModalResponseOK {
        return None;
    }
    let selected = popup.indexOfSelectedItem().max(0) as usize;
    let option = options.get(selected).unwrap_or(&options[0]);
    let path = panel
        .URL()
        .and_then(|url| url.path())
        .map(|path| PathBuf::from(path.to_string()))?;
    let mut path = path;
    if path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| {
            !option
                .extensions
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(extension))
        })
    {
        path.set_extension(&option.extension);
    }
    Some((path, selected))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn save_panel(
    title: &str,
    current: &Path,
    options: &[FormatOption],
) -> Option<(PathBuf, usize)> {
    let mut dialog = rfd::FileDialog::new().set_title(title);
    for option in options {
        let extensions: Vec<&str> = option.extensions.iter().map(String::as_str).collect();
        dialog = dialog.add_filter(&option.label, &extensions);
    }
    if let Some(parent) = current.parent().filter(|parent| parent.is_dir()) {
        dialog = dialog.set_directory(parent);
    }
    if let Some(name) = current.file_name().and_then(|name| name.to_str()) {
        dialog = dialog.set_file_name(name);
    }
    let mut path = dialog.save_file()?;
    let index = path
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(|extension| {
            options.iter().position(|option| {
                option
                    .extensions
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(extension))
            })
        })
        .unwrap_or(0);
    if path.extension().is_none() {
        path.set_extension(&options[index].extension);
    }
    Some((path, index))
}

/// Shows the native open panel with the supported image extensions.
pub(crate) fn open_panel(title: &str, current: &Path) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title(title)
        .add_filter("Paint.Rust document", &["pdr"])
        .add_filter("Paint.NET document", &["pdn"])
        .add_filter(
            "Images",
            &[
                "png", "jpg", "jpeg", "jpe", "jfif", "exif", "tif", "tiff", "bmp", "dib", "tga",
                "dds", "gif", "webp",
            ],
        );
    if let Some(parent) = current.parent().filter(|parent| parent.is_dir()) {
        dialog = dialog.set_directory(parent);
    }
    dialog.pick_file()
}
