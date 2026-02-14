use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub enum OperationKind {
    #[serde(rename = "foundation.simulate")]
    FoundationSimulate,
    #[serde(rename = "image.resize")]
    ImageResize,
}

impl OperationKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FoundationSimulate => "foundation.simulate",
            Self::ImageResize => "image.resize",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImageFormat {
    Png,
    Jpeg,
    Webp,
    Avif,
    Bmp,
    Ico,
    Jp2,
    Jxl,
    Qoi,
    Tga,
    Tiff,
}

impl ImageFormat {
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Png => "png",
            Self::Jpeg => "jpg",
            Self::Webp => "webp",
            Self::Avif => "avif",
            Self::Bmp => "bmp",
            Self::Ico => "ico",
            Self::Jp2 => "jp2",
            Self::Jxl => "jxl",
            Self::Qoi => "qoi",
            Self::Tga => "tga",
            Self::Tiff => "tiff",
        }
    }

    #[must_use]
    pub const fn media_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg => "image/jpeg",
            Self::Webp => "image/webp",
            Self::Avif => "image/avif",
            Self::Bmp => "image/bmp",
            Self::Ico => "image/x-icon",
            Self::Jp2 => "image/jp2",
            Self::Jxl => "image/jxl",
            Self::Qoi => "image/qoi",
            Self::Tga => "image/x-tga",
            Self::Tiff => "image/tiff",
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Png => "PNG",
            Self::Jpeg => "JPEG",
            Self::Webp => "WebP",
            Self::Avif => "AVIF",
            Self::Bmp => "BMP",
            Self::Ico => "ICO",
            Self::Jp2 => "JPEG 2000",
            Self::Jxl => "JPEG XL",
            Self::Qoi => "QOI",
            Self::Tga => "TGA",
            Self::Tiff => "TIFF",
        }
    }

    #[must_use]
    pub const fn supports_lossless_choice(self) -> bool {
        matches!(self, Self::Webp | Self::Avif | Self::Jxl)
    }

    #[must_use]
    pub const fn has_quality_control(self) -> bool {
        matches!(
            self,
            Self::Jpeg | Self::Webp | Self::Avif | Self::Ico | Self::Jp2 | Self::Jxl | Self::Tiff
        )
    }

    #[must_use]
    pub const fn preserves_alpha(self) -> bool {
        !matches!(self, Self::Jpeg | Self::Bmp | Self::Jp2)
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResizeMode {
    Fit,
    Fill,
    Stretch,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MetadataPolicy {
    Strip,
    Preserve,
}

#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EncodingMode {
    #[default]
    Automatic,
    Lossy,
    Lossless,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CropParameters {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ImageAdjustments {
    pub brightness: i16,
    pub contrast: i16,
    pub saturation: i16,
    pub hue: i16,
    pub blur_tenths: u16,
    pub sharpen_tenths: u16,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ImageResizeParameters {
    pub width: u32,
    pub height: u32,
    pub mode: ResizeMode,
    pub allow_upscale: bool,
    #[serde(default)]
    pub pad_to_size: bool,
    #[serde(default)]
    pub crop: Option<CropParameters>,
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
    pub format: ImageFormat,
    pub quality: u8,
    #[serde(default)]
    pub encoding: EncodingMode,
    pub metadata: MetadataPolicy,
}

const fn default_background_rgb() -> u32 {
    0x000000
}

impl ImageResizeParameters {
    #[must_use]
    pub fn projected_dimensions(&self, source_width: u32, source_height: u32) -> (u32, u32) {
        let (mut width, mut height) = self.crop.map_or((source_width, source_height), |crop| {
            (crop.width, crop.height)
        });

        let radians = f64::from(self.rotation_degrees).to_radians().abs();
        if self.rotation_degrees.rem_euclid(360) != 0 {
            let cos = radians.cos().abs();
            let sin = radians.sin().abs();
            let rotated_width = (f64::from(width) * cos + f64::from(height) * sin).ceil();
            let rotated_height = (f64::from(width) * sin + f64::from(height) * cos).ceil();
            width = rotated_width.clamp(1.0, f64::from(u32::MAX)) as u32;
            height = rotated_height.clamp(1.0, f64::from(u32::MAX)) as u32;
        }

        match self.mode {
            ResizeMode::Fill | ResizeMode::Stretch => (self.width, self.height),
            ResizeMode::Fit if self.pad_to_size => (self.width, self.height),
            ResizeMode::Fit => {
                let scale_x = f64::from(self.width) / f64::from(width.max(1));
                let scale_y = f64::from(self.height) / f64::from(height.max(1));
                let mut scale = scale_x.min(scale_y);
                if !self.allow_upscale {
                    scale = scale.min(1.0);
                }
                (
                    (f64::from(width) * scale).round().max(1.0) as u32,
                    (f64::from(height) * scale).round().max(1.0) as u32,
                )
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SimulateParameters {
    pub duration_ms: u64,
    pub output_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fail_at_percent: Option<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "parameters")]
pub enum Operation {
    #[serde(rename = "foundation.simulate")]
    FoundationSimulate(SimulateParameters),
    #[serde(rename = "image.resize")]
    ImageResize(ImageResizeParameters),
}

impl Operation {
    #[must_use]
    pub const fn kind(&self) -> OperationKind {
        match self {
            Self::FoundationSimulate(_) => OperationKind::FoundationSimulate,
            Self::ImageResize(_) => OperationKind::ImageResize,
        }
    }
}
