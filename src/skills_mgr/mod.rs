use anyhow::Result;
use include_dir::{include_dir, Dir};
use std::path::Path;

static AGENTS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/agents");
static SKILLS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/skills");

/// Fields in the YAML frontmatter that are OpenCode-specific and should be
/// stripped when installing for hosts that don't recognise them (e.g. Copilot CLI).
const OPENCODE_ONLY_FIELDS: &[&str] = &["mode", "temperature", "permission"];

/// Copy all embedded agent files to the target directory, renaming them with
/// the given extension. Pass `"md"` for OpenCode, `"agent.md"` for Copilot.
/// When installing as `.agent.md`, OpenCode-specific frontmatter fields are
/// stripped so Copilot CLI does not emit warnings about unknown fields.
/// Returns the number of files written.
pub fn install_bundled_agents(target_dir: &Path, extension: &str) -> Result<usize> {
    let mut count = 0;
    copy_dir_recursive(&AGENTS_DIR, target_dir, extension, &mut count)?;
    Ok(count)
}

/// Copy bundled skill directories (`skills/<slug>/SKILL.md`) into the target
/// skills root (`~/.agents/skills/<slug>/SKILL.md`). Returns the number installed.
pub fn install_bundled_skills(target_root: &Path) -> Result<usize> {
    let mut count = 0;
    for subdir in SKILLS_DIR.dirs() {
        let slug = subdir
            .path()
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        if slug.is_empty() {
            continue;
        }

        let skill_file = subdir.files().find(|f| {
            f.path()
                .file_name()
                .and_then(|s| s.to_str())
                == Some("SKILL.md")
        });
        let Some(skill_file) = skill_file else {
            continue;
        };

        let dest_dir = target_root.join(slug);
        std::fs::create_dir_all(&dest_dir)?;
        let dest = dest_dir.join("SKILL.md");
        std::fs::write(&dest, skill_file.contents())?;
        count += 1;
    }
    Ok(count)
}

/// Returns bundled skill slugs compiled into the binary.
pub fn bundled_skill_names() -> Vec<String> {
    let mut names: Vec<String> = SKILLS_DIR
        .dirs()
        .filter_map(|d| d.path().file_name().and_then(|s| s.to_str()).map(str::to_string))
        .collect();
    names.sort();
    names
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

        // Strip OpenCode-only frontmatter fields when installing for other hosts
        let contents = if extension == "agent.md" {
            let raw = std::str::from_utf8(file.contents()).unwrap_or_default();
            strip_frontmatter_fields(raw, OPENCODE_ONLY_FIELDS)
        } else {
            String::from_utf8_lossy(file.contents()).into_owned()
        };

        std::fs::write(&dest, contents.as_bytes())?;
        *count += 1;
    }

    for subdir in dir.dirs() {
        copy_dir_recursive(subdir, target, extension, count)?;
    }

    Ok(())
}

/// Remove specific top-level YAML frontmatter fields (and their indented sub-lines)
/// from a markdown document. The document must start with `---\n`.
/// Fields not present are silently ignored. The rest of the document is unchanged.
fn strip_frontmatter_fields(content: &str, fields: &[&str]) -> String {
    // Must start with a YAML frontmatter block
    if !content.starts_with("---") {
        return content.to_string();
    }

    let mut lines = content.lines().peekable();
    let mut result = Vec::new();

    // Emit the opening `---`
    let Some(first) = lines.next() else {
        return content.to_string();
    };
    result.push(first.to_string());

    let mut skip_indented = false;

    for line in lines.by_ref() {
        // Closing delimiter — stop frontmatter processing
        if line == "---" {
            result.push(line.to_string());
            break;
        }

        // Indented continuation of a skipped block (e.g. permission sub-keys)
        if skip_indented && (line.starts_with(' ') || line.starts_with('\t')) {
            continue;
        }
        skip_indented = false;

        // Check if this line is a field we want to strip
        let is_stripped = fields.iter().any(|field| {
            let prefix = format!("{}:", field);
            line == prefix.as_str() || line.starts_with(&format!("{} ", prefix))
        });

        if is_stripped {
            skip_indented = true;
            continue;
        }

        result.push(line.to_string());
    }

    // Emit the rest of the document (body after closing ---)
    for line in lines {
        result.push(line.to_string());
    }

    result.join("\n")
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
    fn bundled_skill_names_includes_grilling() {
        let names = bundled_skill_names();
        assert!(
            names.contains(&"grilling".to_string()),
            "grilling skill must be bundled"
        );
    }

    #[test]
    fn install_bundled_skills_writes_skill_md() {
        let dir = tempfile::tempdir().unwrap();
        let count = install_bundled_skills(dir.path()).unwrap();
        assert!(count > 0, "must install at least one bundled skill");

        let grilling = dir.path().join("grilling").join("SKILL.md");
        assert!(grilling.exists(), "grilling/SKILL.md must exist");
        let content = std::fs::read_to_string(&grilling).unwrap();
        assert!(
            content.contains("name: grilling"),
            "grilling skill frontmatter must be present"
        );
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

    #[test]
    fn install_agent_md_strips_opencode_fields() {
        let dir = tempfile::tempdir().unwrap();
        install_bundled_agents(dir.path(), "agent.md").unwrap();

        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let path = entry.unwrap().path();
            if !path.is_file() {
                continue;
            }
            let content = std::fs::read_to_string(&path).unwrap();
            // Only check files that have frontmatter
            if !content.starts_with("---") {
                continue;
            }
            let fm_end = content[3..].find("\n---").map(|i| i + 3).unwrap_or(0);
            let frontmatter = &content[..fm_end];
            for field in OPENCODE_ONLY_FIELDS {
                assert!(
                    !frontmatter.contains(&format!("\n{}:", field)),
                    "field '{}' should be stripped from {:?}",
                    field,
                    path.file_name().unwrap()
                );
            }
        }
    }

    #[test]
    fn install_md_preserves_opencode_fields() {
        let dir = tempfile::tempdir().unwrap();
        install_bundled_agents(dir.path(), "md").unwrap();

        // At least one .md file should still contain 'mode:' (OpenCode field preserved)
        let has_mode = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .any(|e| {
                std::fs::read_to_string(e.path())
                    .map(|c| c.contains("\nmode:"))
                    .unwrap_or(false)
            });
        assert!(has_mode, "OpenCode .md install must preserve 'mode:' field");
    }

    #[test]
    fn strip_frontmatter_fields_removes_scalar_field() {
        let input = "---\ndescription: hello\nmode: primary\ntemperature: 0.2\n---\nbody\n";
        let result = strip_frontmatter_fields(input, &["mode", "temperature"]);
        assert!(!result.contains("mode:"), "mode should be stripped");
        assert!(
            !result.contains("temperature:"),
            "temperature should be stripped"
        );
        assert!(
            result.contains("description: hello"),
            "description must remain"
        );
        assert!(result.contains("body"), "body must remain");
    }

    #[test]
    fn strip_frontmatter_fields_removes_nested_block_field() {
        let input = "---\ndescription: hi\npermission:\n  edit: allow\n  bash: allow\n---\nbody\n";
        let result = strip_frontmatter_fields(input, &["permission"]);
        assert!(
            !result.contains("permission:"),
            "permission should be stripped"
        );
        assert!(
            !result.contains("edit: allow"),
            "sub-keys should be stripped"
        );
        assert!(
            result.contains("description: hi"),
            "description must remain"
        );
        assert!(result.contains("body"), "body must remain");
    }

    #[test]
    fn strip_frontmatter_fields_is_noop_when_no_frontmatter() {
        let input = "# Just a markdown file\nno frontmatter here\n";
        let result = strip_frontmatter_fields(input, &["mode"]);
        assert_eq!(result, input);
    }

    #[test]
    fn strip_frontmatter_fields_is_noop_for_absent_fields() {
        let input = "---\ndescription: hello\n---\nbody\n";
        let result = strip_frontmatter_fields(input, &["mode", "temperature"]);
        assert!(result.contains("description: hello"));
        assert!(result.contains("body"));
    }
}
