//! Generates Tauri's context and the permission set for our commands.
//! Every command must be listed here or the capability check rejects it.

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let attributes =
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "get_app_info",
            "list_devices",
            "play",
            "pause",
            "resume",
            "stop",
            "seek",
            "get_status",
            "get_position",
            "play_tracks",
            "next",
            "previous",
            "skip_to",
            "get_queue",
            "track_details",
            "list_folders",
            "pick_folder",
            "remove_folder",
            "rescan",
            "get_scan_status",
            "list_albums",
            "album_tracks",
        ]));
    tauri_build::try_build(attributes)?;
    Ok(())
}
