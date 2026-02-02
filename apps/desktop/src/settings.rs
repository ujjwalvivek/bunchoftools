use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const SETTINGS_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesktopSettings {
    pub schema_version: u32,
    pub simulation_duration_ms: u64,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            simulation_duration_ms: 1_200,
        }
    }
}

impl DesktopSettings {
    fn sanitize(&mut self) {
        self.simulation_duration_ms = self.simulation_duration_ms.clamp(100, 60_000);
    }
}

pub(crate) fn load() -> Result<DesktopSettings, String> {
    let path = settings_path()?;
    if !path.exists() {
        return Ok(DesktopSettings::default());
    }
    let source = std::fs::read_to_string(&path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    parse(&source)
}

pub(crate) struct SettingsWorker {
    sender: mpsc::Sender<DesktopSettings>,
}

impl SettingsWorker {
    pub(crate) fn start() -> Result<Self, String> {
        let path = settings_path()?;
        let (sender, receiver) = mpsc::channel::<DesktopSettings>();
        thread::Builder::new()
            .name("bunch-settings-writer".to_owned())
            .spawn(move || {
                while let Ok(mut latest) = receiver.recv() {
                    while let Ok(next) = receiver.recv_timeout(Duration::from_millis(200)) {
                        latest = next;
                    }
                    latest.sanitize();
                    if let Ok(serialized) = serde_json::to_vec_pretty(&latest) {
                        let _ = bunch_native::write_crash_safe(&path, &serialized);
                    }
                }
            })
            .map_err(|error| format!("could not start settings worker: {error}"))?;
        Ok(Self { sender })
    }

    pub(crate) fn queue(&self, settings: DesktopSettings) {
        let _ = self.sender.send(settings);
    }
}

fn parse(source: &str) -> Result<DesktopSettings, String> {
    let value: serde_json::Value =
        serde_json::from_str(source).map_err(|error| format!("invalid settings JSON: {error}"))?;
    let version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "settings are missing schema_version".to_owned())?;
    if version != u64::from(SETTINGS_SCHEMA_VERSION) {
        return Err(format!(
            "settings schema {version} is not supported; this build supports {SETTINGS_SCHEMA_VERSION}"
        ));
    }
    let mut settings: DesktopSettings =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    settings.sanitize();
    Ok(settings)
}

fn settings_path() -> Result<PathBuf, String> {
    #[cfg(windows)]
    {
        let root = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| "LOCALAPPDATA is unavailable".to_owned())?;
        Ok(root.join("BunchOfTools").join("settings.json"))
    }
    #[cfg(not(windows))]
    {
        let root = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "HOME is unavailable".to_owned())?;
        Ok(root.join(".config/bunchoftools/settings.json"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_are_versioned_and_sanitized() {
        let settings =
            parse(r#"{"schema_version":1,"simulation_duration_ms":1}"#).expect("settings");
        assert_eq!(settings.simulation_duration_ms, 100);
    }

    #[test]
    fn future_settings_are_rejected() {
        assert!(parse(r#"{"schema_version":99,"simulation_duration_ms":1000}"#).is_err());
    }
}
