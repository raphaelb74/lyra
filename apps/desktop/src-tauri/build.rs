//! Generates Tauri's context and the permission set for our commands.
//! Every command must be listed here or the capability check rejects it.

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let attributes =
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(&[
            "get_app_info",
            "list_devices",
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
            "list_artists",
            "artist_albums",
            "list_tracks",
            "list_playlists",
            "create_playlist",
            "rename_playlist",
            "delete_playlist",
            "playlist_entries",
            "add_to_playlist",
            "remove_from_playlist",
            "move_in_playlist",
            "get_settings",
            "set_settings",
            "get_volume",
            "set_volume",
            "get_diagnostics",
            "probe_device",
        ]));
    tauri_build::try_build(attributes)?;
    Ok(())
}
