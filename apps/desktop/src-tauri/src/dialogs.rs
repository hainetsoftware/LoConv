//! Rust-only native selection. Paths never leave this module through dialog IPC.
use loconv_engine::errors::{self, EngineResult};
use std::path::PathBuf;

pub trait Dialogs: Send + Sync {
    fn choose(&self, directory: bool) -> EngineResult<Vec<PathBuf>>;
}

pub struct NativeDialogs;
impl Dialogs for NativeDialogs {
    fn choose(&self, directory: bool) -> EngineResult<Vec<PathBuf>> {
        choose(directory)
    }
}

#[cfg(target_os = "macos")]
fn choose(directory: bool) -> EngineResult<Vec<PathBuf>> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSModalResponseCancel, NSModalResponseOK, NSOpenPanel};
    let mtm = MainThreadMarker::new().ok_or_else(errors::internal)?;
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(directory);
    panel.setCanChooseFiles(!directory);
    panel.setAllowsMultipleSelection(!directory);
    // Do not let the panel fetch cloud placeholders on our behalf.
    panel.setCanDownloadUbiquitousContents(false);
    panel.setCanResolveUbiquitousConflicts(false);
    match panel.runModal() {
        response if response == NSModalResponseCancel => Ok(Vec::new()),
        response if response == NSModalResponseOK => panel
            .URLs()
            .iter()
            .map(|url| {
                if !url.isFileURL() {
                    return Err(errors::invalid_request());
                }
                url.path()
                    .map(|p| PathBuf::from(p.to_string()))
                    .ok_or_else(errors::invalid_request)
            })
            .collect(),
        _ => Err(errors::internal()),
    }
}

#[cfg(not(target_os = "macos"))]
fn choose(_directory: bool) -> EngineResult<Vec<PathBuf>> {
    // Other platforms need a native dialog and mount-locality adapter.
    Err(errors::tool_unavailable())
}
