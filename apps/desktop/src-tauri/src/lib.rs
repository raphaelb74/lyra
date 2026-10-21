mod commands;
mod ipc;
mod settings;
mod volume_watch;

use std::path::Path;
use std::sync::{Arc, Mutex};

use audio_alsa::AlsaSink;
use commands::{AppState, LIBRARY_CHANGED_EVENT, LIBRARY_NOTICE_EVENT, is_playable};
use ipc::{DroppedFile, ScanReport, ScanStatus, Status, file_name};
use library::{Library, ScanEvent, Scanner};
use player_core::Player;
use tauri::http::{Response, StatusCode, header};
use tauri::{AppHandle, DragDropEvent, Emitter, Manager, RunEvent, WindowEvent};

/// Event carrying a [`Status`] after every playback state change.
const STATUS_EVENT: &str = "player-status";
/// Event carrying the playable [`DroppedFile`]s of a drop (possibly none).
const DROP_EVENT: &str = "files-dropped";
/// Event carrying a [`ScanStatus`] while a scan runs and when it ends.
const SCAN_EVENT: &str = "scan-status";

pub fn run() -> tauri::Result<()> {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let handle = app.handle().clone();
            let sink = AlsaSink::new();
            let sink_stats = sink.stats();
            let player = Player::new(sink, move |status| {
                // A closed window has nobody to notify.
                let _ = handle.emit(STATUS_EVENT, Status::from(status));
            })?;
            let db = app.path().app_data_dir()?.join("library.db");
            let art_dir = app.path().app_cache_dir()?.join("artwork");
            let library = Library::open(&db)?;
            let has_folders = !library.folders()?.is_empty();
            let scan_status = Arc::new(Mutex::new(ScanStatus::default()));
            let scanner = Scanner::new(db, art_dir.clone(), {
                let handle = app.handle().clone();
                let scan_status = Arc::clone(&scan_status);
                move |event| on_scan_event(&handle, &scan_status, event)
            });
            app.manage(AppState::new(
                player,
                sink_stats,
                library,
                scanner.clone(),
                scan_status,
                art_dir,
                app.path().app_config_dir()?.join("settings.json"),
            ));
            volume_watch::spawn(app.handle().clone())?;
            // Pick up changes made while Lyra was closed.
            if has_folders {
                scanner.request();
            }
            Ok(())
        })
        .register_asynchronous_uri_scheme_protocol("lyra-art", |context, request, responder| {
            let art_dir = context.app_handle().state::<AppState>().art_dir.clone();
            let name = request.uri().path().trim_start_matches('/').to_owned();
            tauri::async_runtime::spawn_blocking(move || {
                responder.respond(artwork(&art_dir, &name));
            });
        })
        .on_window_event(|window, event| {
            if let WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) = event {
                on_drop(window.app_handle(), paths);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::list_devices,
            commands::play,
            commands::play_tracks,
            commands::next,
            commands::previous,
            commands::skip_to,
            commands::get_queue,
            commands::track_details,
            commands::pause,
            commands::resume,
            commands::stop,
            commands::seek,
            commands::get_status,
            commands::get_position,
            commands::list_folders,
            commands::pick_folder,
            commands::remove_folder,
            commands::rescan,
            commands::get_scan_status,
            commands::list_albums,
            commands::album_tracks,
            commands::list_artists,
            commands::artist_albums,
            commands::list_tracks,
            commands::list_playlists,
            commands::create_playlist,
            commands::rename_playlist,
            commands::delete_playlist,
            commands::playlist_entries,
            commands::add_to_playlist,
            commands::remove_from_playlist,
            commands::move_in_playlist,
            commands::get_settings,
            commands::set_settings,
            commands::get_volume,
            commands::set_volume,
            commands::get_diagnostics,
            commands::probe_device,
        ])
        .build(tauri::generate_context!())?;
    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            // Managed state is not dropped at exit: release the DAC and
            // restore its volume explicitly.
            if let Some(state) = handle.try_state::<AppState>() {
                state.scanner.cancel();
                state.player.shutdown();
            }
        }
    });
    Ok(())
}

/// Dropped folders join the library; dropped audio files become playable.
fn on_drop(app: &AppHandle, paths: &[std::path::PathBuf]) {
    let state = app.state::<AppState>();
    let (folders, files): (Vec<_>, Vec<_>) = paths.iter().partition(|path| path.is_dir());
    let mut added = false;
    for folder in folders {
        match state.add_folder(folder) {
            Ok(()) => added = true,
            Err(message) => {
                let _ = app.emit(LIBRARY_NOTICE_EVENT, message);
            }
        }
    }
    if added {
        let _ = app.emit(LIBRARY_CHANGED_EVENT, ());
    }
    let playable: Vec<_> = files
        .into_iter()
        .filter(|path| is_playable(path))
        .cloned()
        .collect();
    if playable.is_empty() && added {
        return; // A folder drop, not a failed file drop.
    }
    let dropped: Vec<DroppedFile> = playable
        .iter()
        .map(|path| DroppedFile {
            path: path.display().to_string(),
            name: file_name(path),
        })
        .collect();
    state.allow(playable);
    let _ = app.emit(DROP_EVENT, dropped);
}

fn on_scan_event(app: &AppHandle, status: &Mutex<ScanStatus>, event: ScanEvent) {
    let (snapshot, changed) = {
        let mut status = commands::locked(status);
        let changed = match event {
            ScanEvent::Progress(progress) => {
                status.running = true;
                status.done = progress.done;
                status.total = progress.total;
                false
            }
            ScanEvent::Finished(Ok(summary)) => {
                status.running = false;
                status.error = None;
                status.last = Some(ScanReport::from(&summary));
                summary.added + summary.updated + summary.removed > 0
            }
            ScanEvent::Finished(Err(error)) => {
                status.running = false;
                status.error = Some(format!("Scan failed: {error}"));
                true
            }
        };
        (status.clone(), changed)
    };
    let _ = app.emit(SCAN_EVENT, snapshot);
    if changed {
        let _ = app.emit(LIBRARY_CHANGED_EVENT, ());
    }
}

/// Serves one cached artwork file. Only names the cache creates are served,
/// so a request cannot leave the artwork directory.
fn artwork(art_dir: &Path, name: &str) -> Response<Vec<u8>> {
    let found = library::is_art_name(name)
        .then(|| std::fs::read(art_dir.join(name)).ok())
        .flatten();
    let Some(bytes) = found else {
        return Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Vec::new())
            .unwrap_or_default();
    };
    let mime = match name.rsplit_once('.').map(|(_, extension)| extension) {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        _ => "image/jpeg",
    };
    Response::builder()
        .header(header::CONTENT_TYPE, mime)
        // Names change whenever the image does.
        .header(header::CACHE_CONTROL, "max-age=31536000, immutable")
        .body(bytes)
        .unwrap_or_default()
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
