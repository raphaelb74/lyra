mod commands;
mod ipc;

use audio_alsa::AlsaSink;
use commands::{AppState, is_playable};
use ipc::{DroppedFile, Status, file_name};
use player_core::Player;
use tauri::{DragDropEvent, Emitter, Manager, RunEvent, WindowEvent};

/// Event carrying a [`Status`] after every playback state change.
const STATUS_EVENT: &str = "player-status";
/// Event carrying the playable [`DroppedFile`]s of a drop (possibly none).
const DROP_EVENT: &str = "files-dropped";

pub fn run() -> tauri::Result<()> {
    let app = tauri::Builder::default()
        .setup(|app| {
            let handle = app.handle().clone();
            let player = Player::new(AlsaSink::new(), move |status| {
                // A closed window has nobody to notify.
                let _ = handle.emit(STATUS_EVENT, Status::from(status));
            })?;
            app.manage(AppState::new(player));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = event {
                let playable: Vec<_> = paths
                    .iter()
                    .filter(|path| is_playable(path))
                    .cloned()
                    .collect();
                let files: Vec<DroppedFile> = playable
                    .iter()
                    .map(|path| DroppedFile {
                        path: path.display().to_string(),
                        name: file_name(path),
                    })
                    .collect();
                window.state::<AppState>().allow(playable);
                let _ = window.emit(DROP_EVENT, files);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::list_devices,
            commands::play,
            commands::pause,
            commands::resume,
            commands::stop,
            commands::seek,
            commands::get_status,
            commands::get_position,
        ])
        .build(tauri::generate_context!())?;
    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            // Managed state is not dropped at exit: release the DAC and
            // restore its volume explicitly.
            if let Some(state) = handle.try_state::<AppState>() {
                state.player.shutdown();
            }
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_info_crosses_the_ipc_handler() {
        let app = tauri::test::mock_builder()
            .invoke_handler(tauri::generate_handler![commands::get_app_info])
            .build(tauri::generate_context!())
            .unwrap();
        let webview = tauri::WebviewWindowBuilder::new(&app, "main", tauri::WebviewUrl::default())
            .build()
            .unwrap();
        tauri::test::assert_ipc_response(
            &webview,
            tauri::webview::InvokeRequest {
                cmd: "get_app_info".into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: "tauri://localhost".parse().unwrap(),
                body: tauri::ipc::InvokeBody::default(),
                headers: tauri::http::HeaderMap::default(),
                invoke_key: tauri::test::INVOKE_KEY.into(),
            },
            Ok(serde_json::json!({
                "name": "Lyra",
                "version": env!("CARGO_PKG_VERSION"),
                "platform": std::env::consts::OS,
            })),
        );
    }
}
