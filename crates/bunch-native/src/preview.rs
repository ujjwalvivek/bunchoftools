use std::fs;
use std::path::{Path, PathBuf};
use std::thread;

use async_channel::{Receiver, bounded};
use bunch_core::{ImageProbe, ImageResizeParameters};

use crate::CancellationToken;
use crate::adapters::imagemagick::{ImageMagick, ProcessOutcome};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImagePreview {
    pub generation: u64,
    pub source_path: PathBuf,
    pub encoded_path: PathBuf,
    pub result_path: PathBuf,
    pub source: ImageProbe,
    pub result: ImageProbe,
}

#[derive(Debug)]
pub struct PreviewHandle {
    receiver: Receiver<Result<ImagePreview, String>>,
    cancellation: CancellationToken,
}

impl PreviewHandle {
    #[must_use]
    pub fn receiver(&self) -> Receiver<Result<ImagePreview, String>> {
        self.receiver.clone()
    }

    #[must_use]
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }
}

pub fn request_image_preview(
    path: PathBuf,
    parameters: ImageResizeParameters,
    generation: u64,
    reusable_source: Option<(PathBuf, ImageProbe)>,
) -> Result<PreviewHandle, String> {
    let engine = ImageMagick::discover()?;
    let cancellation = CancellationToken::new();
    let thread_cancellation = cancellation.clone();
    let (sender, receiver) = bounded(1);
    thread::Builder::new()
        .name(format!("bunch-preview-{generation}"))
        .spawn(move || {
            let result = generate_preview(
                &engine,
                &path,
                &parameters,
                generation,
                reusable_source,
                &thread_cancellation,
            );
            let _ = sender.send_blocking(result);
        })
        .map_err(|error| format!("could not start preview worker: {error}"))?;
    Ok(PreviewHandle {
        receiver,
        cancellation,
    })
}

pub fn probe_image(path: &Path) -> Result<ImageProbe, String> {
    ImageMagick::discover()?.probe(path)
}

fn generate_preview(
    engine: &ImageMagick,
    input: &Path,
    parameters: &ImageResizeParameters,
    generation: u64,
    reusable_source: Option<(PathBuf, ImageProbe)>,
    cancellation: &CancellationToken,
) -> Result<ImagePreview, String> {
    let reusable_source = reusable_source.filter(|(path, _)| path.is_file());
    let source = reusable_source.as_ref().map_or_else(
        || engine.validate_resize_input(input),
        |(_, probe)| Ok(probe.clone()),
    )?;
    if !source.within_resource_limits() {
        return Err(format!(
            "preview rejected image outside resource limits ({}x{}, {} frame(s))",
            source.width, source.height, source.frame_count
        ));
    }
    engine.validate_parameters_for_probe(parameters, &source)?;
    let directory = std::env::temp_dir().join("bunch-of-tools").join("previews");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("could not create preview cache: {error}"))?;
    let process_id = std::process::id();
    let source_path = reusable_source.as_ref().map_or_else(
        || directory.join(format!("source-{process_id}-{generation}.png")),
        |(path, _)| path.clone(),
    );
    let encoded_path = directory.join(format!(
        "encoded-{process_id}-{generation}.{}",
        parameters.format.extension()
    ));
    let result_path = directory.join(format!("result-{process_id}-{generation}.png"));
    for path in [&encoded_path, &result_path] {
        let _ = fs::remove_file(path);
    }
    if reusable_source.is_none() {
        let _ = fs::remove_file(&source_path);
    }

    let generated = (|| {
        if reusable_source.is_none() {
            ensure_completed(engine.preview(input, &source_path, 1_600, 1_200, cancellation)?)?;
        }
        ensure_completed(engine.resize(input, &encoded_path, parameters, cancellation)?)?;
        let result = engine.probe(&encoded_path)?;
        ensure_completed(engine.preview(
            &encoded_path,
            &result_path,
            1_600,
            1_200,
            cancellation,
        )?)?;
        if !source_path.is_file() || !result_path.is_file() {
            return Err("ImageMagick did not produce both preview proxies".to_owned());
        }
        Ok(ImagePreview {
            generation,
            source_path: source_path.clone(),
            encoded_path: encoded_path.clone(),
            result_path: result_path.clone(),
            source,
            result,
        })
    })();
    if generated.is_err() {
        if reusable_source.is_none() {
            let _ = fs::remove_file(source_path);
        }
        let _ = fs::remove_file(encoded_path);
        let _ = fs::remove_file(result_path);
    }
    generated
}

fn ensure_completed(outcome: ProcessOutcome) -> Result<(), String> {
    match outcome {
        ProcessOutcome::Completed => Ok(()),
        ProcessOutcome::Cancelled => Err("preview generation was cancelled".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::{ColorType, ImageFormat as FixtureFormat};

    use super::*;

    #[test]
    fn preview_worker_returns_a_bounded_png_proxy_when_engine_is_available() {
        if ImageMagick::discover().is_err() {
            return;
        }
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("bunch-preview-test-{nonce}"));
        fs::create_dir_all(&directory).expect("directory");
        let input = directory.join("source.png");
        let pixels = vec![127_u8; 320 * 180 * 4];
        image::save_buffer_with_format(
            &input,
            &pixels,
            320,
            180,
            ColorType::Rgba8,
            FixtureFormat::Png,
        )
        .expect("fixture");
        let recipe = bunch_core::Recipe::image_resize();
        let bunch_core::Operation::ImageResize(parameters) = &recipe.steps[0].operation else {
            panic!("image recipe");
        };
        let handle = request_image_preview(input.clone(), parameters.clone(), 42, None)
            .expect("preview request");
        let preview = handle
            .receiver()
            .recv_blocking()
            .expect("preview result")
            .expect("preview success");
        assert_eq!(preview.generation, 42);
        assert_eq!((preview.source.width, preview.source.height), (320, 180));
        assert!(preview.source_path.is_file());
        assert!(preview.encoded_path.is_file());
        assert!(preview.result_path.is_file());
        assert_eq!(
            probe_image(&preview.result_path)
                .expect("proxy probe")
                .format,
            "PNG"
        );
        let reused_source = (preview.source_path.clone(), preview.source.clone());
        let next = request_image_preview(input, parameters.clone(), 43, Some(reused_source))
            .expect("reused preview request")
            .receiver()
            .recv_blocking()
            .expect("reused preview result")
            .expect("reused preview success");
        assert_eq!(next.source_path, preview.source_path);

        fs::remove_file(next.encoded_path).expect("next encoded preview cleanup");
        fs::remove_file(next.result_path).expect("next result proxy cleanup");
        assert!(preview.source_path.is_file());
        fs::remove_file(preview.source_path).expect("source proxy cleanup");
        fs::remove_file(preview.encoded_path).expect("encoded preview cleanup");
        fs::remove_file(preview.result_path).expect("result proxy cleanup");
        fs::remove_dir_all(directory).expect("fixture cleanup");
    }
}
