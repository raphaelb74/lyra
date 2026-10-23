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
            "set_shuffle",
            "enqueue_tracks",
            "queue_move",
            "queue_remove",
            "clear_queue",
            "set_repeat",
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
            "set_volume_db",
            "set_muted",
            "get_diagnostics",
            "probe_device",
            "album_editor",
            "save_album_edits",
            "undo_edit",
            "choose_artwork",
            "reset_artwork",
            "propose_fixes",
            "apply_fixes",
            "match_candidates",
            "match_release",
            "match_cover",
            "apply_match",
        ]));
    // The window icon is embedded at compile time: on Linux, Tauri takes the
    // first PNG in `bundle.icon` (tauri.conf.json), so 256 px comes first to
    // stay sharp on HiDPI panels. Rebuild when any icon changes.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::try_build(attributes)?;
    Ok(())
}
