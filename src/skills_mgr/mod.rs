use anyhow::Result;
use include_dir::{include_dir, Dir};
use std::path::Path;

use crate::tool_target::ToolTarget;

static SKILLS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/skills");
static AGENTS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/agents");

/// Copy all embedded skill files to the target directory.
/// Returns the number of files written.
pub fn install_bundled_skills(target_dir: &Path) -> Result<usize> {
    let mut count = 0;
    copy_dir_recursive(&SKILLS_DIR, target_dir, &mut count)?;
    Ok(count)
}

/// Copy all embedded agent files to the target directory, transforming the
/// YAML frontmatter for the target tool.
///
/// OpenCode and Copilot CLI have incompatible frontmatter schemas (see
/// `transform_agent_for_copilot` for details).  The source files in `agents/`
/// use the OpenCode canonical format; this function rewrites them on-the-fly
/// when installing for Copilot CLI.
///
/// Returns the number of files written.
pub fn install_bundled_agents(target_dir: &Path, tool: &ToolTarget) -> Result<usize> {
    let mut count = 0;
    install_agents_recursive(&AGENTS_DIR, target_dir, &mut count, tool)?;
    Ok(count)
}

// ── Internal helpers ─────────────────────────────────────────────────────────

fn install_agents_recursive(
    dir: &Dir,
    target: &Path,
    count: &mut usize,
    tool: &ToolTarget,
) -> Result<()> {
    for file in dir.files() {
        let dest = target.join(file.path());
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let raw = std::str::from_utf8(file.contents()).map_err(|e| {
            anyhow::anyhow!(
                "Agent file {:?} is not valid UTF-8: {}",
                file.path(),
                e
            )
        })?;

        let content = match tool {
            ToolTarget::CopilotCli => transform_agent_for_copilot(raw),
            ToolTarget::OpenCode => raw.to_string(),
        };

        std::fs::write(&dest, content.as_bytes())?;
        *count += 1;
    }

    for subdir in dir.dirs() {
        install_agents_recursive(subdir, target, count, tool)?;
    }

    Ok(())
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

/// Transform an agent markdown file's YAML frontmatter for GitHub Copilot CLI.
///
/// ## Schema differences
///
/// | Field           | OpenCode                                  | Copilot CLI                   |
/// |-----------------|-------------------------------------------|-------------------------------|
/// | `description`   | string (required)                         | string (required)             |
/// | `mode`          | `primary` \| `subagent` \| `all`          | **unknown — strip**           |
/// | `temperature`   | float                                     | **unknown — strip**           |
/// | `tools`         | object `{write: false, bash: false, …}`   | string array (whitelist)      |
/// | `permission`    | object with bash/task sub-keys            | **unknown — strip**           |
///
/// ## Derivation of the Copilot `tools` allowlist
///
/// Start from the full set `["read","search","execute","agent","edit","web","todo"]`
/// and remove entries based on the OpenCode source frontmatter:
///
/// - OpenCode `tools.write: false` or `tools.edit: false`  → remove `"edit"`
/// - OpenCode `tools.bash: false`                          → remove `"execute"`
/// - OpenCode `permission.task` wildcard is `deny`         → remove `"agent"`
///
/// If the resulting list equals the full set, the `tools` key is omitted
/// entirely (Copilot CLI defaults to all tools when absent).
fn transform_agent_for_copilot(content: &str) -> String {
    // ── Split on YAML frontmatter delimiters ─────────────────────────────────
    let Some(after_open) = content.strip_prefix("---\n") else {
        return content.to_string();
    };

    // Find the closing "---" separator
    let Some(fm_end) = after_open.find("\n---") else {
        return content.to_string();
    };

    let fm = &after_open[..fm_end];
    let after_sep = &after_open[fm_end + 4..]; // skip "\n---"
    let body = after_sep.strip_prefix('\n').unwrap_or(after_sep);

    // ── Parse the relevant OpenCode fields ───────────────────────────────────
    let mut description = String::new();
    let mut tools_write_disabled = false;
    let mut tools_edit_disabled = false;
    let mut tools_bash_disabled = false;
    let mut task_default_deny = false;

    // State machine: track which top-level / second-level block we're in.
    let mut top_key = "";
    let mut in_task_block = false;

    for line in fm.lines() {
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();

        match indent {
            0 => {
                // New top-level key — reset all sub-block context.
                in_task_block = false;

                if let Some(rest) = trimmed.strip_prefix("description:") {
                    description = rest.trim().trim_matches('"').to_string();
                    top_key = "description";
                } else if trimmed == "tools:" {
                    top_key = "tools";
                } else if trimmed == "permission:" {
                    top_key = "permission";
                } else {
                    top_key = "";
                }
            }

            2 => {
                match top_key {
                    "tools" => {
                        if let Some(v) = trimmed.strip_prefix("write:") {
                            tools_write_disabled = v.trim() == "false";
                        } else if let Some(v) = trimmed.strip_prefix("edit:") {
                            tools_edit_disabled = v.trim() == "false";
                        } else if let Some(v) = trimmed.strip_prefix("bash:") {
                            tools_bash_disabled = v.trim() == "false";
                        }
                    }
                    "permission" => {
                        if trimmed == "task:" {
                            in_task_block = true;
                        } else {
                            // e.g., "bash:" — leave the task block
                            in_task_block = false;
                        }
                    }
                    _ => {
                        in_task_block = false;
                    }
                }
            }

            4 => {
                // Third-level entries, only relevant inside permission.task.
                if in_task_block && trimmed.contains('*') && trimmed.contains("deny") {
                    task_default_deny = true;
                }
            }

            _ => {}
        }
    }

    // ── Derive Copilot tools allowlist ───────────────────────────────────────
    // Canonical order matches the Copilot CLI documentation tool-alias table.
    const ALL_TOOLS: &[&str] = &["read", "search", "execute", "agent", "edit", "web", "todo"];
    let mut tools: Vec<&str> = ALL_TOOLS.to_vec();

    if tools_write_disabled || tools_edit_disabled {
        tools.retain(|&t| t != "edit");
    }
    if tools_bash_disabled {
        tools.retain(|&t| t != "execute");
    }
    if task_default_deny {
        tools.retain(|&t| t != "agent");
    }

    // ── Assemble Copilot-compatible frontmatter ───────────────────────────────
    let mut new_fm = format!("description: \"{}\"", description);

    // Only emit `tools` when we're actually restricting something; if all
    // tools are allowed, omit the key (Copilot CLI default is all tools).
    if tools.len() < ALL_TOOLS.len() {
        let list = tools
            .iter()
            .map(|t| format!("\"{}\"", t))
            .collect::<Vec<_>>()
            .join(", ");
        new_fm.push_str(&format!("\ntools: [{}]", list));
    }

    format!("---\n{}\n---\n{}", new_fm, body)
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

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn orchestrate_src() -> &'static str {
        r#"---
description: "Delegate-only orchestrator."
mode: primary
temperature: 0.1
tools:
  write: false
  edit: false
permission:
  bash:
    "*": allow
    "git*": deny
  task:
    "*": allow
---
Load your skill.
"#
    }

    fn subagent_src() -> &'static str {
        r#"---
description: "QA verifier."
mode: subagent
temperature: 0.1
permission:
  bash:
    "*": allow
    "git push": deny
  task:
    "*": deny
---
Load your skill.
"#
    }
    #[test]
    fn orchestrate_strips_mode_temperature_permission_and_restricts_edit() {
        let out = transform_agent_for_copilot(orchestrate_src());
        assert!(!out.contains("mode:"), "mode should be stripped");
        assert!(!out.contains("temperature:"), "temperature should be stripped");
        assert!(!out.contains("permission:"), "permission should be stripped");
        // edit disabled by tools.write/edit false; agent kept (task: allow)
        assert!(out.contains("\"execute\""), "execute should be present");
        assert!(out.contains("\"agent\""), "agent should be present (task allow)");
        assert!(!out.contains("\"edit\""), "edit should be absent");
        // body preserved
        assert!(out.contains("Load your skill."), "body must be preserved");
    }

    #[test]
    fn subagent_strips_unknown_fields_and_removes_agent_tool() {
        let out = transform_agent_for_copilot(subagent_src());
        assert!(!out.contains("mode:"));
        assert!(!out.contains("temperature:"));
        assert!(!out.contains("permission:"));
        // task: deny → agent removed; no tools restrictions → edit kept
        assert!(out.contains("\"edit\""), "edit should be present");
        assert!(!out.contains("\"agent\""), "agent should be absent (task deny)");
    }

    #[test]
    fn full_access_omits_tools_key_when_all_tools_allowed() {
        // If after transformation every tool remains, tools key should be omitted.
        // full_src has task:deny → agent removed, so tools key WILL appear.
        // Let's test a genuinely unrestricted agent.
        let src = "---\ndescription: \"All tools.\"\nmode: subagent\n---\nBody.\n";
        let out = transform_agent_for_copilot(src);
        assert!(!out.contains("tools:"), "tools key should be omitted when unrestricted");
        assert!(out.contains("description:"));
        assert!(out.contains("Body."));
    }

    #[test]
    fn description_is_preserved_verbatim() {
        let out = transform_agent_for_copilot(subagent_src());
        assert!(out.contains("\"QA verifier.\""));
    }
}
