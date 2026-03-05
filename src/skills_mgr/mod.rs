use anyhow::Result;
use include_dir::{include_dir, Dir};
use std::path::Path;

static SKILLS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/skills");
static AGENTS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/agents");

/// Copy all embedded skill files to the target directory.
/// Returns the number of files written.
pub fn install_bundled_skills(target_dir: &Path) -> Result<usize> {
    let mut count = 0;
    copy_dir_recursive(&SKILLS_DIR, target_dir, &mut count)?;
    Ok(count)
}

/// Copy all embedded agent files to the target directory.
/// Returns the number of files written.
pub fn install_bundled_agents(target_dir: &Path) -> Result<usize> {
    let mut count = 0;
    copy_dir_recursive(&AGENTS_DIR, target_dir, &mut count)?;
    Ok(count)
}

fn copy_dir_recursive(dir: &Dir, target: &Path, count: &mut usize) -> Result<()> {
    for file in dir.files() {
        let dest = target.join(file.path());
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, file.contents())?;
        *count += 1;
    }

    for subdir in dir.dirs() {
        copy_dir_recursive(subdir, target, count)?;
    }

    Ok(())
}

/// List installed skill directories (those starting with `spex-`).
pub fn list_installed_skills(skills_dir: &Path) -> Result<Vec<String>> {
    if !skills_dir.exists() {
        return Ok(vec![]);
    }

    let mut skills = vec![];
    for entry in std::fs::read_dir(skills_dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        if entry.file_type()?.is_dir() && name.starts_with("spex-") {
            skills.push(name);
        }
    }
    skills.sort();
    Ok(skills)
}
