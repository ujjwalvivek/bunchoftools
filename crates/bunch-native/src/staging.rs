use std::fs;
use std::path::{Path, PathBuf};

use bunch_core::CollisionPolicy;

pub(crate) struct StagedOutput {
    pub final_path: PathBuf,
    pub staging_path: PathBuf,
    collision: CollisionPolicy,
}

impl StagedOutput {
    pub(crate) fn prepare(
        directory: &Path,
        output_name: &str,
        collision: CollisionPolicy,
        job_tag: &str,
    ) -> Result<Self, String> {
        if !directory.is_dir() {
            return Err(format!(
                "output directory does not exist: {}",
                directory.display()
            ));
        }
        if Path::new(output_name)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(output_name)
        {
            return Err("output name must be a single valid filename".to_owned());
        }
        if collision == CollisionPolicy::Choose {
            return Err("choose collision policy requires an interactive destination".to_owned());
        }

        let requested = directory.join(output_name);
        let final_path = match collision {
            CollisionPolicy::Increment => incremented_path(&requested)?,
            CollisionPolicy::Fail if requested.exists() => {
                return Err(format!("output already exists: {}", requested.display()));
            }
            CollisionPolicy::Fail | CollisionPolicy::Replace => requested,
            CollisionPolicy::Choose => unreachable!("handled above"),
        };
        let extension = final_path
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("image");
        let staging_path = directory.join(format!(".bunch-{job_tag}.partial.{extension}"));
        if staging_path.exists() {
            fs::remove_file(&staging_path).map_err(|error| {
                format!(
                    "could not clear stale staging output {}: {error}",
                    staging_path.display()
                )
            })?;
        }
        Ok(Self {
            final_path,
            staging_path,
            collision,
        })
    }

    pub(crate) fn publish(&self) -> Result<(), String> {
        if !self.staging_path.is_file() {
            return Err("engine did not produce the staged output".to_owned());
        }
        match self.collision {
            CollisionPolicy::Replace if self.final_path.exists() => self.publish_replacing(),
            CollisionPolicy::Fail | CollisionPolicy::Increment | CollisionPolicy::Replace => {
                if self.final_path.exists() {
                    return Err(format!(
                        "output appeared before publication: {}",
                        self.final_path.display()
                    ));
                }
                fs::rename(&self.staging_path, &self.final_path)
                    .map_err(|error| format!("could not publish output: {error}"))
            }
            CollisionPolicy::Choose => Err("interactive output was not resolved".to_owned()),
        }
    }

    fn publish_replacing(&self) -> Result<(), String> {
        let backup = self.final_path.with_extension("bunch-backup");
        if backup.exists() {
            fs::remove_file(&backup)
                .map_err(|error| format!("could not remove stale output backup: {error}"))?;
        }
        fs::rename(&self.final_path, &backup)
            .map_err(|error| format!("could not stage existing output for replacement: {error}"))?;
        match fs::rename(&self.staging_path, &self.final_path) {
            Ok(()) => {
                fs::remove_file(&backup).map_err(|error| {
                    format!("output published but backup cleanup failed: {error}")
                })?;
                Ok(())
            }
            Err(error) => {
                let _ = fs::rename(&backup, &self.final_path);
                Err(format!("could not publish replacement output: {error}"))
            }
        }
    }

    pub(crate) fn cleanup(&self) {
        let _ = fs::remove_file(&self.staging_path);
    }
}

fn incremented_path(requested: &Path) -> Result<PathBuf, String> {
    if !requested.exists() {
        return Ok(requested.to_path_buf());
    }
    let parent = requested.parent().unwrap_or_else(|| Path::new("."));
    let stem = requested
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "output filename has no usable stem".to_owned())?;
    let extension = requested.extension().and_then(|value| value.to_str());
    for suffix in 2..=9_999 {
        let name = extension.map_or_else(
            || format!("{stem} ({suffix})"),
            |extension| format!("{stem} ({suffix}).{extension}"),
        );
        let candidate = parent.join(name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("could not resolve an available incremented output name".to_owned())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn test_directory(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("bunch-staging-{label}-{nonce}"));
        fs::create_dir_all(&path).expect("test directory");
        path
    }

    #[test]
    fn increment_never_replaces_an_existing_output() {
        let directory = test_directory("increment");
        fs::write(directory.join("image.webp"), b"original").expect("existing output");
        let staged =
            StagedOutput::prepare(&directory, "image.webp", CollisionPolicy::Increment, "test")
                .expect("staging");
        assert_eq!(
            staged.final_path.file_name().and_then(|name| name.to_str()),
            Some("image (2).webp")
        );
        fs::write(&staged.staging_path, b"new").expect("staged output");
        staged.publish().expect("publish");
        assert_eq!(
            fs::read(directory.join("image.webp")).expect("original"),
            b"original"
        );
        assert_eq!(fs::read(staged.final_path).expect("new"), b"new");
        fs::remove_dir_all(directory).expect("cleanup");
    }
}
