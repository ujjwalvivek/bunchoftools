use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use bunch_core::{
    AdapterIdentity, CapabilityDescriptor, EncodingMode, ExecutorKind, ImageFormat, ImageProbe,
    ImageResizeParameters, MetadataPolicy, OperationKind, ResizeMode,
};

use crate::{CancellationToken, write_crash_safe};

pub(crate) const ADAPTER_ID: &str = "imagemagick.image-v2";
const ADAPTER_VERSION: &str = "2";
const POLICY: &str = include_str!("imagemagick-policy.xml");
const PROCESS_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_DIAGNOSTIC_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct ImageMagick {
    executable: PathBuf,
    version: String,
    policy_directory: PathBuf,
    temporary_directory: PathBuf,
}

#[derive(Debug)]
struct ScopedPolicy {
    directory: PathBuf,
}

impl Drop for ScopedPolicy {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

#[derive(Debug)]
pub(crate) enum ProcessOutcome {
    Completed,
    Cancelled,
}

impl ImageMagick {
    pub(crate) fn discover() -> Result<Self, String> {
        static DISCOVERY: OnceLock<Result<ImageMagick, String>> = OnceLock::new();
        DISCOVERY.get_or_init(Self::discover_uncached).clone()
    }

    fn discover_uncached() -> Result<Self, String> {
        let executable = std::env::var_os("BUNCH_IMAGEMAGICK_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("magick"));
        let policy_directory = ensure_policy_directory()?;
        let temporary_directory = std::env::temp_dir()
            .join("bunch-of-tools")
            .join("imagemagick-pixels");
        fs::create_dir_all(&temporary_directory).map_err(|error| {
            format!("could not create ImageMagick temporary directory: {error}")
        })?;

        let mut command = Command::new(&executable);
        configure_command(&mut command, &policy_directory, &temporary_directory);
        let output = command
            .arg("-version")
            .output()
            .map_err(|error| format!("ImageMagick 7 was not found: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "ImageMagick version check failed: {}",
                bounded_text(&output.stderr)
            ));
        }
        let first_line = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned();
        let version = parse_version(&first_line)
            .ok_or_else(|| format!("unrecognized ImageMagick version output: {first_line}"))?;
        if !version.starts_with("7.") {
            return Err(format!(
                "ImageMagick {version} is unsupported; the M2 adapter requires version 7"
            ));
        }

        let engine = Self {
            executable,
            version,
            policy_directory,
            temporary_directory,
        };
        engine.verify_policy()?;
        Ok(engine)
    }

    pub(crate) fn capability(&self) -> CapabilityDescriptor {
        CapabilityDescriptor {
            operation: OperationKind::ImageResize,
            executor: ExecutorKind::Native,
            adapter: self.identity(),
            supports_progress: false,
            supports_cancellation: true,
        }
    }

    pub(crate) fn identity(&self) -> AdapterIdentity {
        AdapterIdentity {
            id: ADAPTER_ID.to_owned(),
            version: ADAPTER_VERSION.to_owned(),
            engine_name: "ImageMagick".to_owned(),
            engine_version: self.version.clone(),
        }
    }

    pub(crate) fn version(&self) -> &str {
        &self.version
    }

    pub(crate) fn executable(&self) -> &Path {
        &self.executable
    }

    pub(crate) fn probe(&self, path: &Path) -> Result<ImageProbe, String> {
        let path = validate_regular_file(path)?;
        let file_size = fs::metadata(&path)
            .map_err(|error| format!("could not inspect input size: {error}"))?
            .len();
        let (mut command, _policy) = self.command_with_paths([path.as_path()], [])?;
        command.arg("identify");
        add_limits(&mut command);
        let output = command
            .arg("-quiet")
            .arg("-format")
            .arg("%m\t%w\t%h\t%[colorspace]\t%[orientation]\t%[channels]\t%n\t%[profiles]\n")
            .arg(&path)
            .output()
            .map_err(|error| format!("could not start ImageMagick probe: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "ImageMagick could not probe this image: {}",
                bounded_text(&output.stderr)
            ));
        }
        parse_probe(&output.stdout, file_size)
    }

    pub(crate) fn validate_resize_input(&self, path: &Path) -> Result<ImageProbe, String> {
        let probe = self.probe(path)?;
        if !matches!(
            probe.format.as_str(),
            "AVIF"
                | "BMP"
                | "GIF"
                | "HEIC"
                | "HEIF"
                | "ICO"
                | "ICON"
                | "J2K"
                | "JP2"
                | "JPEG"
                | "JPG"
                | "JXL"
                | "PNG"
                | "PSD"
                | "QOI"
                | "TGA"
                | "TIFF"
                | "WEBP"
        ) {
            return Err(format!(
                "{} input is not in the qualified static-image set",
                probe.format
            ));
        }
        if probe.frame_count != 1 {
            return Err(format!(
                "animated or multi-image inputs are not supported by image.resize ({} frames)",
                probe.frame_count
            ));
        }
        if !probe.within_resource_limits() {
            return Err(format!(
                "image exceeds M2 limits ({}x{}, {} frame(s))",
                probe.width, probe.height, probe.frame_count
            ));
        }
        Ok(probe)
    }

    pub(crate) fn validate_parameters_for_probe(
        &self,
        parameters: &ImageResizeParameters,
        probe: &ImageProbe,
    ) -> Result<(), String> {
        if let Some(crop) = parameters.crop {
            let right = crop
                .x
                .checked_add(crop.width)
                .ok_or_else(|| "crop horizontal bounds overflow".to_owned())?;
            let bottom = crop
                .y
                .checked_add(crop.height)
                .ok_or_else(|| "crop vertical bounds overflow".to_owned())?;
            if right > probe.width || bottom > probe.height {
                return Err(format!(
                    "crop {}x{}+{}+{} exceeds the {}x{} source",
                    crop.width, crop.height, crop.x, crop.y, probe.width, probe.height
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn resize(
        &self,
        input: &Path,
        output: &Path,
        parameters: &ImageResizeParameters,
        cancellation: &CancellationToken,
    ) -> Result<ProcessOutcome, String> {
        let input = validate_regular_file(input)?;
        let (mut command, _policy) = self.command_with_paths([input.as_path()], [output])?;
        add_limits(&mut command);
        command.arg(&input).arg("-auto-orient");
        add_transform_arguments(&mut command, parameters);
        if parameters.metadata == MetadataPolicy::Strip {
            command.arg("-strip");
        }
        add_encoding_arguments(&mut command, parameters);
        command.arg(format!(
            "{}:{}",
            coder(parameters.format),
            output.to_string_lossy()
        ));

        run_cancellable(command, cancellation, PROCESS_TIMEOUT)
    }

    pub(crate) fn preview(
        &self,
        input: &Path,
        output: &Path,
        max_width: u32,
        max_height: u32,
        cancellation: &CancellationToken,
    ) -> Result<ProcessOutcome, String> {
        let input = validate_regular_file(input)?;
        let input_frame = format!("{}[0]", input.to_string_lossy());
        let (mut command, _policy) = self.command_with_paths([input.as_path()], [output])?;
        add_limits(&mut command);
        command
            .arg(input_frame)
            .args(["-auto-orient", "-thumbnail"])
            .arg(format!("{max_width}x{max_height}>"))
            .arg("-strip")
            .arg(format!("PNG:{}", output.to_string_lossy()));
        run_cancellable(command, cancellation, Duration::from_secs(30))
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        configure_command(
            &mut command,
            &self.policy_directory,
            &self.temporary_directory,
        );
        command
    }

    fn command_with_paths<'a>(
        &self,
        read_paths: impl IntoIterator<Item = &'a Path>,
        write_paths: impl IntoIterator<Item = &'a Path>,
    ) -> Result<(Command, ScopedPolicy), String> {
        let policy = create_scoped_policy(read_paths, write_paths, &self.temporary_directory)?;
        let mut command = Command::new(&self.executable);
        configure_command(&mut command, &policy.directory, &self.temporary_directory);
        Ok((command, policy))
    }

    fn verify_policy(&self) -> Result<(), String> {
        let mut command = self.command();
        let output = command
            .args(["-list", "policy"])
            .output()
            .map_err(|error| format!("could not inspect ImageMagick policy: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "ImageMagick policy check failed: {}",
                bounded_text(&output.stderr)
            ));
        }
        let policy = String::from_utf8_lossy(&output.stdout);
        if !policy.contains("bunch-policy-version")
            || !policy.contains("Delegate")
            || !policy.contains("Path")
        {
            return Err("Bunch ImageMagick security policy was not loaded".to_owned());
        }
        Ok(())
    }
}

fn ensure_policy_directory() -> Result<PathBuf, String> {
    let directory = std::env::temp_dir()
        .join("bunch-of-tools")
        .join("imagemagick-policy-v1");
    fs::create_dir_all(&directory)
        .map_err(|error| format!("could not create ImageMagick policy directory: {error}"))?;
    let path = directory.join("policy.xml");
    let current = fs::read_to_string(&path).unwrap_or_default();
    if current != POLICY {
        write_crash_safe(&path, POLICY.as_bytes())?;
    }
    Ok(directory)
}

fn create_scoped_policy<'a>(
    read_paths: impl IntoIterator<Item = &'a Path>,
    write_paths: impl IntoIterator<Item = &'a Path>,
    temporary_directory: &Path,
) -> Result<ScopedPolicy, String> {
    static NEXT_POLICY: AtomicU64 = AtomicU64::new(1);
    let id = NEXT_POLICY.fetch_add(1, Ordering::Relaxed);
    let directory = std::env::temp_dir().join("bunch-of-tools").join(format!(
        "imagemagick-job-policy-{}-{id}",
        std::process::id()
    ));
    fs::create_dir_all(&directory)
        .map_err(|error| format!("could not create per-job ImageMagick policy: {error}"))?;

    let mut path_rules =
        String::from("  <policy domain=\"path\" rights=\"none\" pattern=\"*\" />\n");
    for path in read_paths {
        path_rules.push_str(&path_policy("read", path));
    }
    for path in write_paths {
        path_rules.push_str(&path_policy("write", path));
    }
    let temporary_pattern = format!(
        "{}\\\\*",
        glob_escape(&temporary_directory.to_string_lossy())
    );
    path_rules.push_str(&format!(
        "  <policy domain=\"path\" rights=\"read | write\" pattern=\"{}\" />\n",
        xml_escape(&temporary_pattern)
    ));
    let policy = POLICY.replace("</policymap>", &format!("{path_rules}</policymap>"));
    if let Err(error) = write_crash_safe(&directory.join("policy.xml"), policy.as_bytes()) {
        let _ = fs::remove_dir_all(&directory);
        return Err(error);
    }
    Ok(ScopedPolicy { directory })
}

fn path_policy(rights: &str, path: &Path) -> String {
    format!(
        "  <policy domain=\"path\" rights=\"{rights}\" pattern=\"{}\" />\n",
        xml_escape(&glob_escape(&path.to_string_lossy()))
    )
}

fn glob_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() * 2);
    for character in value.chars() {
        if matches!(character, '\\' | '*' | '?' | '[' | ']' | '{' | '}') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn configure_command(command: &mut Command, policy: &Path, temporary: &Path) {
    command
        .env("MAGICK_CONFIGURE_PATH", policy)
        .env("MAGICK_TEMPORARY_PATH", temporary)
        .stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
}

fn add_limits(command: &mut Command) {
    command.args([
        "-limit",
        "memory",
        "256MiB",
        "-limit",
        "map",
        "512MiB",
        "-limit",
        "disk",
        "1GiB",
        "-limit",
        "area",
        "40MP",
        "-limit",
        "width",
        "8192",
        "-limit",
        "height",
        "8192",
        "-limit",
        "list-length",
        "32",
        "-limit",
        "thread",
        "4",
        "-limit",
        "time",
        "120",
    ]);
}

fn validate_regular_file(path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::metadata(path).map_err(|error| {
        format!(
            "input file is not accessible at {}: {error}",
            path.display()
        )
    })?;
    if !metadata.is_file() {
        return Err(format!("input is not a regular file: {}", path.display()));
    }
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("could not canonicalize input path: {error}"))?;
    Ok(normalize_windows_path(canonical))
}

#[cfg(windows)]
fn normalize_windows_path(path: PathBuf) -> PathBuf {
    let value = path.to_string_lossy();
    if let Some(remainder) = value.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{remainder}"));
    }
    value
        .strip_prefix(r"\\?\")
        .map_or(path.clone(), PathBuf::from)
}

#[cfg(not(windows))]
fn normalize_windows_path(path: PathBuf) -> PathBuf {
    path
}

fn parse_version(line: &str) -> Option<String> {
    line.split_once("ImageMagick ")?
        .1
        .split_whitespace()
        .next()
        .map(str::to_owned)
}

fn parse_probe(output: &[u8], file_size: u64) -> Result<ImageProbe, String> {
    let text = String::from_utf8(output.to_vec())
        .map_err(|_| "ImageMagick probe output was not UTF-8".to_owned())?;
    let first_line = text
        .lines()
        .next()
        .ok_or_else(|| "ImageMagick probe returned no image properties".to_owned())?;
    let fields = first_line.split('\t').collect::<Vec<_>>();
    if fields.len() != 8 {
        return Err(format!(
            "ImageMagick probe returned {} fields instead of 8",
            fields.len()
        ));
    }
    let format = fields[0].to_ascii_uppercase();
    let width = parse_u32(fields[1], "width")?;
    let height = parse_u32(fields[2], "height")?;
    let frame_count = parse_u32(fields[6], "frame count")?;
    let channels = fields[5].to_ascii_lowercase();
    let profile_names = fields[7]
        .split([',', ' '])
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect();
    Ok(ImageProbe {
        media_type: media_type(&format).to_owned(),
        format,
        width,
        height,
        file_size,
        colorspace: fields[3].to_owned(),
        orientation: fields[4].to_owned(),
        has_alpha: channels.contains("rgba")
            || channels.contains("graya")
            || channels.contains("cmyka")
            || channels.contains("alpha"),
        frame_count,
        profile_names,
    })
}

fn parse_u32(value: &str, field: &str) -> Result<u32, String> {
    value
        .parse()
        .map_err(|error| format!("invalid {field} in ImageMagick probe: {error}"))
}

fn media_type(format: &str) -> &'static str {
    match format {
        "PNG" => "image/png",
        "JPEG" | "JPG" => "image/jpeg",
        "WEBP" => "image/webp",
        "GIF" => "image/gif",
        "AVIF" => "image/avif",
        "BMP" => "image/bmp",
        "HEIC" | "HEIF" => "image/heic",
        "ICO" | "ICON" => "image/x-icon",
        "J2K" | "JP2" => "image/jp2",
        "JXL" => "image/jxl",
        "PSD" => "image/vnd.adobe.photoshop",
        "QOI" => "image/qoi",
        "TGA" => "image/x-tga",
        "TIFF" => "image/tiff",
        _ => "application/octet-stream",
    }
}

fn coder(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "PNG",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::Webp => "WEBP",
        ImageFormat::Avif => "AVIF",
        ImageFormat::Bmp => "BMP",
        ImageFormat::Ico => "ICO",
        ImageFormat::Jp2 => "JP2",
        ImageFormat::Jxl => "JXL",
        ImageFormat::Qoi => "QOI",
        ImageFormat::Tga => "TGA",
        ImageFormat::Tiff => "TIFF",
    }
}

fn resize_geometry(parameters: &ImageResizeParameters) -> String {
    let mode = match parameters.mode {
        ResizeMode::Fit => "",
        ResizeMode::Fill => "^",
        ResizeMode::Stretch => "!",
    };
    let upscale = if parameters.mode == ResizeMode::Fit && !parameters.allow_upscale {
        ">"
    } else {
        ""
    };
    format!(
        "{}x{}{}{}",
        parameters.width, parameters.height, mode, upscale
    )
}

fn add_transform_arguments(command: &mut Command, parameters: &ImageResizeParameters) {
    if let Some(crop) = parameters.crop {
        command
            .arg("-crop")
            .arg(format!(
                "{}x{}+{}+{}",
                crop.width, crop.height, crop.x, crop.y
            ))
            .arg("+repage");
    }
    if parameters.rotation_degrees != 0 {
        command
            .args(["-background", "none", "-rotate"])
            .arg(parameters.rotation_degrees.to_string());
    }
    if parameters.flip_horizontal {
        command.arg("-flop");
    }
    if parameters.flip_vertical {
        command.arg("-flip");
    }

    let adjustments = parameters.adjustments;
    if adjustments.brightness != 0 || adjustments.contrast != 0 {
        command.arg("-brightness-contrast").arg(format!(
            "{}x{}",
            adjustments.brightness, adjustments.contrast
        ));
    }
    if adjustments.saturation != 0 || adjustments.hue != 0 {
        command.arg("-modulate").arg(format!(
            "100,{},{}",
            100 + adjustments.saturation,
            100 + adjustments.hue
        ));
    }
    if adjustments.blur_tenths > 0 {
        command.arg("-blur").arg(format!(
            "0x{:.1}",
            f32::from(adjustments.blur_tenths) / 10.0
        ));
    }
    if adjustments.sharpen_tenths > 0 {
        command.arg("-sharpen").arg(format!(
            "0x{:.1}",
            f32::from(adjustments.sharpen_tenths) / 10.0
        ));
    }

    command.arg("-resize").arg(resize_geometry(parameters));
    if parameters.mode == ResizeMode::Fill {
        command
            .args(["-gravity", "center", "-background", "none"])
            .arg("-extent")
            .arg(format!("{}x{}", parameters.width, parameters.height));
    } else if parameters.pad_to_size {
        let background = if parameters.format.preserves_alpha() {
            "none".to_owned()
        } else {
            format!("#{:06x}", parameters.background_rgb)
        };
        command
            .args(["-gravity", "center", "-background"])
            .arg(background)
            .arg("-extent")
            .arg(format!("{}x{}", parameters.width, parameters.height));
    }
    if !parameters.format.preserves_alpha() {
        command
            .args(["-background"])
            .arg(format!("#{:06x}", parameters.background_rgb))
            .args(["-alpha", "remove", "-alpha", "off"]);
    }
}

fn add_encoding_arguments(command: &mut Command, parameters: &ImageResizeParameters) {
    if parameters.encoding == EncodingMode::Lossless {
        match parameters.format {
            ImageFormat::Webp => {
                command.args(["-define", "webp:lossless=true"]);
            }
            ImageFormat::Avif | ImageFormat::Jxl => {
                command.arg("-quality").arg("100");
            }
            _ => {}
        }
    } else if parameters.format.has_quality_control() {
        command.arg("-quality").arg(parameters.quality.to_string());
    }
}

fn run_cancellable(
    mut command: Command,
    cancellation: &CancellationToken,
    timeout: Duration,
) -> Result<ProcessOutcome, String> {
    command.stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start ImageMagick: {error}"))?;
    let stderr = child.stderr.take();
    let stderr_reader = thread::spawn(move || drain_bounded(stderr));
    let started = Instant::now();
    enum WaitOutcome {
        Status(ExitStatus),
        Cancelled,
        TimedOut,
    }

    let outcome = loop {
        if cancellation.is_cancelled() {
            terminate(&mut child);
            break WaitOutcome::Cancelled;
        }
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("could not poll ImageMagick: {error}"))?
        {
            break WaitOutcome::Status(status);
        }
        if started.elapsed() >= timeout {
            terminate(&mut child);
            break WaitOutcome::TimedOut;
        }
        thread::sleep(Duration::from_millis(20));
    };
    let diagnostic = stderr_reader
        .join()
        .unwrap_or_else(|_| "ImageMagick diagnostic reader failed".to_owned());
    match outcome {
        WaitOutcome::Status(status) if status.success() => Ok(ProcessOutcome::Completed),
        WaitOutcome::Status(status) => {
            Err(format!("ImageMagick exited with {status}: {diagnostic}"))
        }
        WaitOutcome::Cancelled => Ok(ProcessOutcome::Cancelled),
        WaitOutcome::TimedOut => Err(format!("ImageMagick exceeded the time limit: {diagnostic}")),
    }
}

fn terminate(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn drain_bounded(stderr: Option<impl Read>) -> String {
    let Some(mut stderr) = stderr else {
        return String::new();
    };
    let mut collected = Vec::new();
    let mut chunk = [0_u8; 4096];
    loop {
        match stderr.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                let remaining = MAX_DIAGNOSTIC_BYTES.saturating_sub(collected.len());
                collected.extend_from_slice(&chunk[..count.min(remaining)]);
            }
            Err(_) => break,
        }
    }
    bounded_text(&collected)
}

fn bounded_text(bytes: &[u8]) -> String {
    let limit = bytes.len().min(MAX_DIAGNOSTIC_BYTES);
    String::from_utf8_lossy(&bytes[..limit]).trim().to_owned()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use image::{ColorType, ImageFormat as FixtureFormat};

    use super::*;

    fn test_directory(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "bunch-adapter-{label}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).expect("test directory");
        directory
    }

    fn write_png(path: &Path, width: u32, height: u32, value: u8) {
        let pixels = vec![value; (u64::from(width) * u64::from(height) * 4) as usize];
        image::save_buffer_with_format(
            path,
            &pixels,
            width,
            height,
            ColorType::Rgba8,
            FixtureFormat::Png,
        )
        .expect("PNG fixture");
    }

    fn write_solid_red_png(path: &Path, width: u32, height: u32) {
        let pixels = [255_u8, 0, 0, 255].repeat((u64::from(width) * u64::from(height)) as usize);
        image::save_buffer_with_format(
            path,
            &pixels,
            width,
            height,
            ColorType::Rgba8,
            FixtureFormat::Png,
        )
        .expect("solid PNG fixture");
    }

    #[test]
    fn version_and_geometry_are_deterministic() {
        assert_eq!(
            parse_version("Version: ImageMagick 7.1.2-27 Q16-HDRI x64"),
            Some("7.1.2-27".to_owned())
        );
        let recipe = bunch_core::Recipe::image_resize();
        let bunch_core::Operation::ImageResize(mut parameters) = recipe.steps[0].operation.clone()
        else {
            panic!("image recipe");
        };
        assert_eq!(resize_geometry(&parameters), "1280x720");
        parameters.mode = ResizeMode::Fill;
        assert_eq!(resize_geometry(&parameters), "1280x720^");
        parameters.mode = ResizeMode::Stretch;
        assert_eq!(resize_geometry(&parameters), "1280x720!");
    }

    #[test]
    fn installed_engine_loads_the_bunch_policy_when_available() {
        let Ok(engine) = ImageMagick::discover() else {
            return;
        };
        assert!(engine.version().starts_with("7."));
    }

    #[test]
    fn cover_enlarges_to_fill_without_background_bars() {
        let Ok(engine) = ImageMagick::discover() else {
            return;
        };
        let directory = test_directory("cover-no-bars");
        let input = directory.join("input.png");
        let output = directory.join("output.png");
        write_solid_red_png(&input, 8, 8);

        let recipe = bunch_core::Recipe::image_resize();
        let bunch_core::Operation::ImageResize(mut parameters) = recipe.steps[0].operation.clone()
        else {
            panic!("image recipe");
        };
        parameters.width = 16;
        parameters.height = 8;
        parameters.mode = ResizeMode::Fill;
        parameters.allow_upscale = false;
        parameters.format = ImageFormat::Png;

        assert!(matches!(
            engine
                .resize(&input, &output, &parameters, &CancellationToken::new())
                .expect("cover resize"),
            ProcessOutcome::Completed
        ));
        let rendered = image::open(&output).expect("cover output").to_rgba8();
        assert_eq!(rendered.dimensions(), (16, 8));
        assert!(
            rendered
                .pixels()
                .all(|pixel| pixel.0[0] > 250 && pixel.0[1] < 5 && pixel.0[2] < 5)
        );
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn explicit_padding_is_transparent_for_alpha_outputs() {
        let Ok(engine) = ImageMagick::discover() else {
            return;
        };
        let directory = test_directory("transparent-padding");
        let input = directory.join("input.png");
        let output = directory.join("output.png");
        write_solid_red_png(&input, 8, 8);

        let recipe = bunch_core::Recipe::image_resize();
        let bunch_core::Operation::ImageResize(mut parameters) = recipe.steps[0].operation.clone()
        else {
            panic!("image recipe");
        };
        parameters.width = 16;
        parameters.height = 8;
        parameters.mode = ResizeMode::Fit;
        parameters.allow_upscale = false;
        parameters.pad_to_size = true;
        parameters.format = ImageFormat::Png;

        assert!(matches!(
            engine
                .resize(&input, &output, &parameters, &CancellationToken::new())
                .expect("padded resize"),
            ProcessOutcome::Completed
        ));
        let rendered = image::open(&output).expect("padded output").to_rgba8();
        assert_eq!(rendered.dimensions(), (16, 8));
        assert_eq!(rendered.get_pixel(0, 4).0[3], 0);
        assert_eq!(rendered.get_pixel(8, 4).0, [255, 0, 0, 255]);
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn policy_rejects_hostile_dimensions_and_unqualified_coders() {
        let Ok(engine) = ImageMagick::discover() else {
            return;
        };
        let directory = test_directory("policy");
        let too_wide = directory.join("too-wide.png");
        write_png(&too_wide, 8_193, 1, 255);
        assert!(engine.probe(&too_wide).is_err());

        let svg = directory.join("indirect.svg");
        fs::write(
            &svg,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect width="10" height="10"/></svg>"#,
        )
        .expect("SVG fixture");
        assert!(engine.probe(&svg).is_err());

        let allowed = directory.join("allowed.png");
        let undeclared = directory.join("undeclared.png");
        write_png(&allowed, 2, 2, 64);
        write_png(&undeclared, 2, 2, 192);
        let (mut command, _policy) = engine
            .command_with_paths([allowed.as_path()], [])
            .expect("scoped policy");
        command.arg("identify");
        add_limits(&mut command);
        let output = command
            .arg(&undeclared)
            .output()
            .expect("undeclared read probe");
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("security policy"));
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn animation_is_probed_but_rejected_by_static_resize() {
        let Ok(engine) = ImageMagick::discover() else {
            return;
        };
        let directory = test_directory("animation");
        let first = directory.join("first.png");
        let second = directory.join("second.png");
        let animation = directory.join("animation.gif");
        write_png(&first, 8, 8, 64);
        write_png(&second, 8, 8, 192);
        let status = Command::new(engine.executable())
            .arg(&first)
            .arg(&second)
            .arg(&animation)
            .status()
            .expect("GIF fixture");
        assert!(status.success());
        assert_eq!(engine.probe(&animation).expect("GIF probe").frame_count, 2);
        assert!(engine.validate_resize_input(&animation).is_err());
        fs::remove_dir_all(directory).expect("cleanup");
    }
}
