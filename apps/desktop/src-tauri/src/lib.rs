//! Minimal Tauri 2 host for LoConv.
//!
//! Only the IPC version command is registered in this increment. The host does
//! not expose shell, filesystem, HTTP, updater, or arbitrary URL access to the
//! frontend, and it performs no network activity.

/// Return the IPC version owned by `loconv-contracts` through the engine.
#[tauri::command]
fn get_api_version() -> u32 {
    loconv_engine::api_version()
}

/// Run the native application host.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_api_version])
        .run(tauri::generate_context!())
        .expect("failed to run LoConv");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_returns_contract_ipc_version() {
        assert_eq!(get_api_version(), 1);
        assert_eq!(get_api_version(), loconv_contracts::API_VERSION);
        assert_eq!(get_api_version(), loconv_engine::contracts::API_VERSION);
    }
}
