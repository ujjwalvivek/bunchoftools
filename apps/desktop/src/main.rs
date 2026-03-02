#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod assets;
mod settings;
mod theme;
mod views;

fn main() {
    #[cfg(windows)]
    if run_picker_helper() {
        return;
    }
    app::run();
}

#[cfg(windows)]
fn run_picker_helper() -> bool {
    let Some(mode) = std::env::args().nth(1) else {
        return false;
    };
    let paths = match mode.as_str() {
        "--bunch-pick-files" => rfd::FileDialog::new()
            .set_title("Import images")
            .add_filter(
                "Images",
                &[
                    "avif", "bmp", "gif", "heic", "heif", "ico", "j2k", "jp2", "jpeg", "jpg",
                    "jxl", "png", "psd", "qoi", "tga", "tif", "tiff", "webp",
                ],
            )
            .pick_files()
            .unwrap_or_default(),
        "--bunch-pick-folder" => rfd::FileDialog::new()
            .set_title("Choose export folder")
            .pick_folder()
            .into_iter()
            .collect(),
        _ => return false,
    };
    match serde_json::to_string(&paths) {
        Ok(json) => println!("{json}"),
        Err(error) => eprintln!("{error}"),
    }
    true
}
