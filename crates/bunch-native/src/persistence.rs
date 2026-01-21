use std::fs;
use std::io::Write;
use std::path::Path;

use bunch_core::{Recipe, RecipeLoadError, parse_recipe_json};

pub fn load_recipe(path: &Path) -> Result<Recipe, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    parse_recipe_json(&contents).map_err(|error| format!("{}: {error}", path.display()))
}

pub fn save_recipe(path: &Path, recipe: &Recipe) -> Result<(), String> {
    let serialized = recipe.to_pretty_json().map_err(format_recipe_error)?;
    write_crash_safe(path, serialized.as_bytes())
}

pub fn write_crash_safe(path: &Path, contents: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("could not create {}: {error}", parent.display()))?;

    let temporary = path.with_extension("tmp");
    let backup = path.with_extension("bak");
    let _ = fs::remove_file(&temporary);
    let _ = fs::remove_file(&backup);

    let mut file = fs::File::create(&temporary)
        .map_err(|error| format!("could not create {}: {error}", temporary.display()))?;
    file.write_all(contents)
        .map_err(|error| format!("could not write {}: {error}", temporary.display()))?;
    file.sync_all()
        .map_err(|error| format!("could not flush {}: {error}", temporary.display()))?;
    drop(file);

    let had_existing = path.exists();
    if had_existing {
        fs::rename(path, &backup).map_err(|error| {
            format!(
                "could not stage existing {} as {}: {error}",
                path.display(),
                backup.display()
            )
        })?;
    }

    if let Err(error) = fs::rename(&temporary, path) {
        if had_existing {
            let _ = fs::rename(&backup, path);
        }
        let _ = fs::remove_file(&temporary);
        return Err(format!(
            "could not publish {} from {}: {error}",
            path.display(),
            temporary.display()
        ));
    }

    if had_existing {
        fs::remove_file(&backup)
            .map_err(|error| format!("could not remove {}: {error}", backup.display()))?;
    }
    Ok(())
}

fn format_recipe_error(error: RecipeLoadError) -> String {
    match error {
        RecipeLoadError::InvalidRecipe(issues) => issues
            .into_iter()
            .map(|issue| format!("{}: {}", issue.path, issue.message))
            .collect::<Vec<_>>()
            .join(" | "),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn test_directory(label: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "bunch-native-{label}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn recipe_write_replaces_atomically_and_cleans_auxiliary_files() {
        let directory = test_directory("replace");
        let path = directory.join("recipe.json");
        let mut recipe = Recipe::foundation_demo();
        save_recipe(&path, &recipe).expect("first save");
        recipe.name = "Updated foundation job".to_owned();
        save_recipe(&path, &recipe).expect("replacement save");

        assert_eq!(load_recipe(&path).expect("load"), recipe);
        assert!(!path.with_extension("tmp").exists());
        assert!(!path.with_extension("bak").exists());
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn future_recipe_is_rejected_without_modification() {
        let directory = test_directory("future");
        fs::create_dir_all(&directory).expect("directory");
        let path = directory.join("recipe.json");
        let original = b"{\"schema_version\":99,\"name\":\"future\"}";
        fs::write(&path, original).expect("fixture");

        assert!(load_recipe(&path).is_err());
        assert_eq!(fs::read(&path).expect("unchanged"), original);
        fs::remove_dir_all(directory).expect("cleanup");
    }
}
