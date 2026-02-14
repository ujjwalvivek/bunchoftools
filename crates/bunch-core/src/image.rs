use serde::{Deserialize, Serialize};

pub const MAX_IMAGE_DIMENSION: u32 = 8_192;
pub const MAX_IMAGE_PIXELS: u64 = 40_000_000;
pub const MAX_IMAGE_FRAMES: u32 = 32;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ImageProbe {
    pub format: String,
    pub media_type: String,
    pub width: u32,
    pub height: u32,
    pub file_size: u64,
    pub colorspace: String,
    pub orientation: String,
    pub has_alpha: bool,
    pub frame_count: u32,
    #[serde(default)]
    pub profile_names: Vec<String>,
}

impl ImageProbe {
    #[must_use]
    pub const fn pixel_count(&self) -> u64 {
        self.width as u64 * self.height as u64
    }

    #[must_use]
    pub const fn within_resource_limits(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.width <= MAX_IMAGE_DIMENSION
            && self.height <= MAX_IMAGE_DIMENSION
            && self.pixel_count() <= MAX_IMAGE_PIXELS
            && self.frame_count > 0
            && self.frame_count <= MAX_IMAGE_FRAMES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(width: u32, height: u32, frame_count: u32) -> ImageProbe {
        ImageProbe {
            format: "PNG".to_owned(),
            media_type: "image/png".to_owned(),
            width,
            height,
            file_size: 100,
            colorspace: "sRGB".to_owned(),
            orientation: "Undefined".to_owned(),
            has_alpha: true,
            frame_count,
            profile_names: Vec::new(),
        }
    }

    #[test]
    fn hostile_dimensions_and_frame_counts_are_rejected() {
        assert!(probe(1_920, 1_080, 1).within_resource_limits());
        assert!(!probe(MAX_IMAGE_DIMENSION + 1, 100, 1).within_resource_limits());
        assert!(!probe(8_000, 8_000, 1).within_resource_limits());
        assert!(!probe(100, 100, MAX_IMAGE_FRAMES + 1).within_resource_limits());
    }
}
