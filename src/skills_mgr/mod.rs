use anyhow::Result;
use include_dir::{include_dir, Dir};
use std::path::Path;

static AGENTS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/agents");

/// Copy all embedded agent files to the target directory, renaming them with
/// the given extension. Pass `"md"` for OpenCode, `"agent.md"` for Copilot.
/// Returns the number of files written.
pub fn install_bundled_agents(target_dir: &Path, extension: &str) -> Result<usize> {
    let mut count = 0;
    copy_dir_recursive(&AGENTS_DIR, target_dir, extension, &mut count)?;
    Ok(count)
}

/// Returns the bundled agent file stems compiled into the binary.
pub fn bundled_agent_names() -> Vec<String> {
    let mut names = Vec::new();
    collect_agent_names(&AGENTS_DIR, &mut names);
    names.sort();
    names.dedup();
    names
}

fn collect_agent_names(dir: &Dir, names: &mut Vec<String>) {
    for file in dir.files() {
        if file.path().extension().and_then(|ext| ext.to_str()) == Some("md") {
            if let Some(stem) = file.path().file_stem().and_then(|stem| stem.to_str()) {
                names.push(stem.to_string());
            }
        }
    }

    for subdir in dir.dirs() {
        collect_agent_names(subdir, names);
    }
}

fn copy_dir_recursive(dir: &Dir, target: &Path, extension: &str, count: &mut usize) -> Result<()> {
    for file in dir.files() {
        // Only process .md source files
        if file.path().extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }

        // Build destination path: replace the source extension with the target extension
        let stem = file
            .path()
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let new_filename = format!("{}.{}", stem, extension);

        // Preserve any subdirectory structure
        let dest = if let Some(parent) = file.path().parent() {
            if parent == Path::new("") {
                target.join(&new_filename)
            } else {
                target.join(parent).join(&new_filename)
            }
        } else {
            target.join(&new_filename)
        };

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, file.contents())?;
        *count += 1;
    }

    for subdir in dir.dirs() {
        copy_dir_recursive(subdir, target, extension, count)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_agent_names_returns_non_empty_list() {
        let names = bundled_agent_names();
        assert!(!names.is_empty(), "must have at least one bundled agent");
    }

    #[test]
    fn install_bundled_agents_with_md_extension() {
        let dir = tempfile::tempdir().unwrap();
        let count = install_bundled_agents(dir.path(), "md").unwrap();
        assert!(count > 0);

        // All installed files must end in .md (not .agent.md)
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                let name = path.file_name().unwrap().to_string_lossy().to_string();
                assert!(
                    name.ends_with(".md"),
                    "expected .md extension, got: {}",
                    name
                );
            }
        }
    }

    #[test]
    fn install_bundled_agents_with_agent_md_extension() {
        let dir = tempfile::tempdir().unwrap();
        let count = install_bundled_agents(dir.path(), "agent.md").unwrap();
        assert!(count > 0);

        // All installed files must end in .agent.md
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                let name = path.file_name().unwrap().to_string_lossy().to_string();
                assert!(
                    name.ends_with(".agent.md"),
                    "expected .agent.md extension, got: {}",
                    name
                );
            }
        }
    }
}
