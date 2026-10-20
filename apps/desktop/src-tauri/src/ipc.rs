use serde::Serialize;

/// Response of the `get_app_info` command.
#[derive(Debug, Serialize)]
pub struct AppInfo {
    name: &'static str,
    version: &'static str,
    platform: &'static str,
}

impl AppInfo {
    pub fn current() -> Self {
        Self {
            name: "Lyra",
            version: env!("CARGO_PKG_VERSION"),
            platform: std::env::consts::OS,
        }
    }
}
