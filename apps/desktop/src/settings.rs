use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use bunch_core::{EncodingMode, ImageAdjustments, ImageFormat, MetadataPolicy, ResizeMode};
use serde::{Deserialize, Serialize};

const SETTINGS_SCHEMA_VERSION: u32 = 5;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub(crate) struct DesktopSettings {
    pub schema_version: u32,
    pub image_width: u32,
    pub image_height: u32,
    pub image_format: ImageFormat,
    pub image_quality: u8,
    pub resize_mode: ResizeMode,
    pub allow_upscale: bool,
    #[serde(default)]
    pub pad_to_size: bool,
    #[serde(default)]
    pub rotation_degrees: i16,
    #[serde(default)]
    pub flip_horizontal: bool,
    #[serde(default)]
    pub flip_vertical: bool,
    #[serde(default)]
    pub adjustments: ImageAdjustments,
    #[serde(default = "default_background_rgb")]
    pub background_rgb: u32,
    #[serde(default)]
    pub encoding: EncodingMode,
    pub metadata_policy: MetadataPolicy,
}

impl Default for DesktopSettings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            image_width: 1_280,
            image_height: 720,
            image_format: ImageFormat::Webp,
            image_quality: 82,
            resize_mode: ResizeMode::Fit,
            allow_upscale: true,
            pad_to_size: false,
            rotation_degrees: 0,
            flip_horizontal: false,
            flip_vertical: false,
            adjustments: ImageAdjustments::default(),
            background_rgb: default_background_rgb(),
            encoding: EncodingMode::Automatic,
            metadata_policy: MetadataPolicy::Strip,
        }
    }
}

impl DesktopSettings {
    fn sanitize(&mut self) {
        self.image_width = self.image_width.clamp(1, 8_192);
        self.image_height = self.image_height.clamp(1, 8_192);
        self.image_quality = self.image_quality.clamp(1, 100);
        self.rotation_degrees = self.rotation_degrees.clamp(-180, 180);
        self.adjustments.brightness = self.adjustments.brightness.clamp(-100, 100);
        self.adjustments.contrast = self.adjustments.contrast.clamp(-100, 100);
        self.adjustments.saturation = self.adjustments.saturation.clamp(-100, 100);
        self.adjustments.hue = self.adjustments.hue.clamp(-180, 180);
        self.adjustments.blur_tenths = self.adjustments.blur_tenths.min(200);
        self.adjustments.sharpen_tenths = self.adjustments.sharpen_tenths.min(200);
        self.background_rgb = self.background_rgb.min(0xffffff);
        self.allow_upscale = true;
        self.pad_to_size = false;
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
    let mut value: serde_json::Value =
        serde_json::from_str(source).map_err(|error| format!("invalid settings JSON: {error}"))?;
    let version = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "settings are missing schema_version".to_owned())?;
    if version == 1 {
        return Ok(DesktopSettings::default());
    }
    if version == 2 {
        value["schema_version"] = SETTINGS_SCHEMA_VERSION.into();
    } else if version == 3 {
        value["schema_version"] = SETTINGS_SCHEMA_VERSION.into();
        value["pad_to_size"] = false.into();
        if value
            .get("background_rgb")
            .and_then(serde_json::Value::as_u64)
            == Some(0xffffff)
        {
            value["background_rgb"] = default_background_rgb().into();
        }
    } else if version == 4 {
        value["schema_version"] = SETTINGS_SCHEMA_VERSION.into();
        value["allow_upscale"] = true.into();
        value["pad_to_size"] = false.into();
    } else if version != u64::from(SETTINGS_SCHEMA_VERSION) {
        return Err(format!(
            "settings schema {version} is not supported; this build supports {SETTINGS_SCHEMA_VERSION}"
        ));
    }
    let mut settings: DesktopSettings =
        serde_json::from_value(value).map_err(|error| error.to_string())?;
    settings.sanitize();
    Ok(settings)
}

const fn default_background_rgb() -> u32 {
    0x000000
}

fn settings_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("BUNCH_SETTINGS_PATH") {
        return Ok(PathBuf::from(path));
    }
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
        let settings = parse(
            r#"{"schema_version":5,"image_width":99999,"image_height":0,"image_format":"webp","image_quality":0,"resize_mode":"fit","allow_upscale":false,"metadata_policy":"strip"}"#,
        )
        .expect("settings");
        assert_eq!(settings.image_width, 8_192);
        assert_eq!(settings.image_height, 1);
        assert_eq!(settings.image_quality, 1);
        assert!(settings.allow_upscale);
        assert!(!settings.pad_to_size);
    }

    #[test]
    fn m1_settings_migrate_to_image_defaults() {
        let settings =
            parse(r#"{"schema_version":1,"simulation_duration_ms":1200}"#).expect("migration");
        assert_eq!(settings, DesktopSettings::default());
    }

    #[test]
    fn m2_preview_settings_migrate_away_from_implicit_white_padding() {
        let settings = parse(
            r#"{"schema_version":3,"image_width":1280,"image_height":720,"image_format":"webp","image_quality":82,"resize_mode":"fit","allow_upscale":true,"pad_to_size":true,"background_rgb":16777215,"metadata_policy":"strip"}"#,
        )
        .expect("migration");
        assert_eq!(settings.schema_version, SETTINGS_SCHEMA_VERSION);
        assert!(!settings.pad_to_size);
        assert_eq!(settings.background_rgb, 0x000000);
    }

    #[test]
    fn old_workbench_settings_migrate_to_bar_free_resize_policy() {
        let settings = parse(
            r#"{"schema_version":4,"image_width":1280,"image_height":720,"image_format":"webp","image_quality":82,"resize_mode":"fit","allow_upscale":false,"pad_to_size":true,"background_rgb":0,"encoding":"lossy","metadata_policy":"strip"}"#,
        )
        .expect("migration");
        assert!(settings.allow_upscale);
        assert!(!settings.pad_to_size);
    }

    #[test]
    fn future_settings_are_rejected() {
        assert!(parse(r#"{"schema_version":99,"simulation_duration_ms":1000}"#).is_err());
    }
}
