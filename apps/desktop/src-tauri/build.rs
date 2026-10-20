//! Generates Tauri's context and the permission set for our commands.
//! Every command must be listed here or the capability check rejects it.

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let attributes = tauri_build::Attributes::new()
        .app_manifest(tauri_build::AppManifest::new().commands(&["get_app_info"]));
    tauri_build::try_build(attributes)?;
    Ok(())
}
