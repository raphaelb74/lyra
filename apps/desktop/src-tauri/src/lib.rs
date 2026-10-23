mod commands;
mod id_match;
mod ipc;
mod mpris;
mod settings;
mod thumbs;
mod volume_watch;

use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};

use audio_alsa::AlsaSink;
use commands::{AppState, LIBRARY_CHANGED_EVENT, LIBRARY_NOTICE_EVENT, is_playable};
use ipc::{ScanReport, ScanStatus, Status, THUMB_PATH};
use library::{Library, ScanEvent, Scanner};
use tauri::http::{Response, StatusCode, header};
use tauri::{AppHandle, DragDropEvent, Emitter, Manager, RunEvent, WindowEvent};
use unda::Player;

/// Event carrying a [`Status`] after every playback state change.
const STATUS_EVENT: &str = "player-status";
/// Event carrying a [`ScanStatus`] while a scan runs and when it ends.
const SCAN_EVENT: &str = "scan-status";

/// Starts the engine, the library and the desktop integrations.
fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let sink = AlsaSink::new();
    let sink_stats = sink.stats();
    // Status changes also go to MPRIS (the desktop's media keys).
    let (notify_mpris, mpris_updates) = mpsc::channel();
    let player = Player::new(sink, move |status| {
        // A closed window has nobody to notify.
        let _ = handle.emit(STATUS_EVENT, Status::from(status));
        let _ = notify_mpris.send(());
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
    mpris::spawn(app.handle().clone(), mpris_updates)?;
    // Pick up changes made while Lyra was closed.
    if has_folders {
        scanner.request();
    }
    Ok(())
}

pub fn run() -> tauri::Result<()> {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(setup)
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
            commands::play_tracks,
            commands::next,
            commands::previous,
            commands::skip_to,
            commands::set_shuffle,
            commands::enqueue_tracks,
            commands::queue_move,
            commands::queue_remove,
            commands::clear_queue,
            commands::set_repeat,
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
            commands::set_volume_db,
            commands::get_diagnostics,
            commands::probe_device,
            commands::album_editor,
            commands::save_album_edits,
            commands::undo_edit,
            commands::choose_artwork,
            commands::reset_artwork,
            commands::propose_fixes,
            commands::apply_fixes,
            id_match::match_candidates,
            id_match::match_release,
            id_match::match_cover,
            id_match::apply_match,
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

/// Dropped folders join the library. Lyra plays from the library only, so a
/// dropped file gets a hint to add its folder instead.
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
    } else if !files.is_empty() {
        let hint = if files.iter().any(|path| is_playable(path)) {
            "Lyra plays music from your library: drop the folder that holds these files."
        } else {
            "Drop a folder of FLAC, WAV, DSF or DFF files to add it to your library."
        };
        let _ = app.emit(LIBRARY_NOTICE_EVENT, hint);
    }
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

/// Serves one cached artwork file, or its thumbnail for `thumb/<name>`.
/// Only names the cache creates are served, so a request cannot leave the
/// artwork directory.
fn artwork(art_dir: &Path, request: &str) -> Response<Vec<u8>> {
    let (name, small) = match request.strip_prefix(THUMB_PATH) {
        Some(name) => (name, true),
        None => (request, false),
    };
    let found = library::is_art_name(name)
        .then(|| {
            let path = if small {
                thumbs::thumbnail(art_dir, name)?
            } else {
                art_dir.join(name)
            };
            std::fs::read(path).ok()
        })
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
