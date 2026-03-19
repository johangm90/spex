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
