use anyhow::Result;
use include_dir::{include_dir, Dir};
use std::path::Path;

static AGENTS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/agents");

/// Copy all embedded agent files to the target directory.
/// Returns the number of files written.
pub fn install_bundled_agents(target_dir: &Path) -> Result<usize> {
    let mut count = 0;
    copy_dir_recursive(&AGENTS_DIR, target_dir, &mut count)?;
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
