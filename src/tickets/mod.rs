//! Pluggable ticket backends: project tasks can be projected to GitHub Issues
//! or Markdown files in addition to always living in spex state.
//!
//! `SpexState` is always available; `GitHub` and `Markdown` are opt-in via the
//! `[tickets]` table in `.spex/config.toml` or the `--to` flag on
//! `spex task export`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};

use crate::config::TicketsConfig;
use crate::sdd::{spec::Spec, task::Task};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketBackend {
    SpexState,
    GitHub,
    Markdown,
}

impl TicketBackend {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "spex" | "spex-state" | "state" => Some(Self::SpexState),
            "github" | "gh" | "issues" => Some(Self::GitHub),
            "markdown" | "md" | "file" | "files" => Some(Self::Markdown),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::SpexState => "spex-state",
            Self::GitHub => "github",
            Self::Markdown => "markdown",
        }
    }
}

/// Where a task ended up after an export.
#[derive(Debug, Clone)]
pub struct TicketRef {
    pub backend: &'static str,
    pub external_id: String,
    pub url: Option<String>,
}

pub trait TicketSink {
    /// Fail fast if the backend's prerequisites are missing (e.g. `gh` not on PATH).
    fn preflight(&self) -> Result<()>;

    /// Project a single task. `dry_run` must not perform side effects.
    fn export_task(&self, task: &Task, spec: &Spec, dry_run: bool) -> Result<TicketRef>;
}

/// Resolve the effective backend: `--to` flag wins, then config, then default.
pub fn resolve_backend(flag: Option<&str>, cfg: Option<&TicketsConfig>) -> Result<TicketBackend> {
    if let Some(f) = flag {
        return TicketBackend::from_str(f).ok_or_else(|| {
            anyhow!("unknown ticket backend '{f}' (spex-state | github | markdown)")
        });
    }
    if let Some(c) = cfg {
        return TicketBackend::from_str(&c.backend).ok_or_else(|| {
            anyhow!(
                "unknown ticket backend '{}' in .spex/config.toml",
                c.backend
            )
        });
    }
    Ok(TicketBackend::SpexState)
}

pub fn sink_for(
    backend: TicketBackend,
    cfg: Option<&TicketsConfig>,
    project_root: &Path,
) -> Box<dyn TicketSink> {
    match backend {
        TicketBackend::SpexState => Box::new(SpexStateSink),
        TicketBackend::GitHub => Box::new(GitHubSink {
            repo: cfg.and_then(|c| c.github_repo.clone()),
        }),
        TicketBackend::Markdown => {
            let rel = cfg
                .and_then(|c| c.markdown_dir.clone())
                .unwrap_or_else(|| ".spex/tasks".to_string());
            Box::new(MarkdownSink {
                dir: project_root.join(rel),
            })
        }
    }
}

// ─── spex-state ─────────────────────────────────────────────────────────────

struct SpexStateSink;

impl TicketSink for SpexStateSink {
    fn preflight(&self) -> Result<()> {
        Ok(())
    }
    fn export_task(&self, task: &Task, _spec: &Spec, _dry_run: bool) -> Result<TicketRef> {
        Ok(TicketRef {
            backend: "spex-state",
            external_id: task.id.clone(),
            url: None,
        })
    }
}

// ─── markdown ──────────────────────────────────────────────────────────────

struct MarkdownSink {
    dir: PathBuf,
}

impl TicketSink for MarkdownSink {
    fn preflight(&self) -> Result<()> {
        if let Some(parent) = self.dir.parent() {
            if !parent.exists() {
                bail!("parent directory {} does not exist", parent.display());
            }
        }
        Ok(())
    }

    fn export_task(&self, task: &Task, spec: &Spec, dry_run: bool) -> Result<TicketRef> {
        let path = self.dir.join(format!("{}.md", task.id));
        let contents = render_markdown(task, spec);

        if !dry_run {
            std::fs::create_dir_all(&self.dir)
                .with_context(|| format!("creating {}", self.dir.display()))?;
            let mut f = std::fs::File::create(&path)
                .with_context(|| format!("writing {}", path.display()))?;
            f.write_all(contents.as_bytes())?;
        }

        Ok(TicketRef {
            backend: "markdown",
            external_id: path.display().to_string(),
            url: None,
        })
    }
}

fn render_markdown(task: &Task, spec: &Spec) -> String {
    let inputs: Vec<String> = serde_json::from_str(&task.inputs).unwrap_or_default();
    let inputs_yaml = if inputs.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", inputs.join(", "))
    };
    format!(
        "---\n\
         id: {id}\n\
         spec: {spec_id}\n\
         title: {title:?}\n\
         agent: {agent}\n\
         status: {status}\n\
         inputs: {inputs_yaml}\n\
         output_artifact: {artifact}\n\
         created_at: {created}\n\
         ---\n\n\
         # {id} — {title}\n\n\
         **Spec:** {spec_id} — {spec_title}\n\
         **Agent:** {agent}\n\
         **Status:** {status}\n",
        id = task.id,
        spec_id = task.spec,
        title = task.title,
        agent = task.agent,
        status = task.status,
        artifact = task.output_artifact.as_deref().unwrap_or("null"),
        created = task.created_at,
        spec_title = spec.title,
    )
}

// ─── github ────────────────────────────────────────────────────────────────

struct GitHubSink {
    repo: Option<String>,
}

impl GitHubSink {
    fn create_args(&self, task: &Task, spec: &Spec) -> Vec<String> {
        let mut args = vec![
            "issue".into(),
            "create".into(),
            "--title".into(),
            format!("{}: {}", task.id, task.title),
            "--body".into(),
            format!(
                "spex-task: {}\nspec: {} — {}\nagent: {}\nstatus: {}\n",
                task.id, task.spec, spec.title, task.agent, task.status
            ),
        ];
        if let Some(repo) = &self.repo {
            args.push("--repo".into());
            args.push(repo.clone());
        }
        args
    }
}

impl TicketSink for GitHubSink {
    fn preflight(&self) -> Result<()> {
        let ok = Command::new("gh")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !ok {
            bail!(
                "`gh` CLI not found or not runnable — install GitHub CLI and run `gh auth login`"
            );
        }
        Ok(())
    }

    fn export_task(&self, task: &Task, spec: &Spec, dry_run: bool) -> Result<TicketRef> {
        let args = self.create_args(task, spec);

        if dry_run {
            return Ok(TicketRef {
                backend: "github",
                external_id: format!("(dry-run) gh {}", args.join(" ")),
                url: None,
            });
        }

        let out = Command::new("gh")
            .args(&args)
            .output()
            .context("running `gh issue create`")?;
        if !out.status.success() {
            bail!(
                "gh issue create failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }

        let stdout = String::from_utf8_lossy(&out.stdout);
        let url = stdout
            .lines()
            .rev()
            .find(|l| l.contains("github.com"))
            .map(|l| l.trim().to_string());
        let number = url
            .as_deref()
            .and_then(|u| u.rsplit('/').next())
            .map(|n| format!("#{n}"))
            .unwrap_or_else(|| "(created)".to_string());

        Ok(TicketRef {
            backend: "github",
            external_id: number,
            url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TicketsConfig;

    fn task() -> Task {
        Task {
            id: "TASK-1".into(),
            spec: "SPEC-1".into(),
            title: "do the thing (AC-1)".into(),
            agent: "sdd-builder".into(),
            status: "pending".into(),
            inputs: "[]".into(),
            output_artifact: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    fn spec() -> Spec {
        Spec {
            id: "SPEC-1".into(),
            title: "The feature".into(),
            status: "approved".into(),
            priority: "P1".into(),
            depends_on: "[]".into(),
            agents: "[]".into(),
            ac_total: 1,
            ac_passed: 0,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            updated_by: None,
        }
    }

    #[test]
    fn backend_from_str_accepts_aliases() {
        assert_eq!(TicketBackend::from_str("gh"), Some(TicketBackend::GitHub));
        assert_eq!(TicketBackend::from_str("MD"), Some(TicketBackend::Markdown));
        assert_eq!(
            TicketBackend::from_str("spex-state"),
            Some(TicketBackend::SpexState)
        );
        assert_eq!(TicketBackend::from_str("jira"), None);
    }

    #[test]
    fn resolve_backend_precedence() {
        let cfg = TicketsConfig {
            backend: "github".into(),
            github_repo: None,
            markdown_dir: None,
        };
        assert_eq!(
            resolve_backend(Some("markdown"), Some(&cfg)).unwrap(),
            TicketBackend::Markdown
        );
        assert_eq!(
            resolve_backend(None, Some(&cfg)).unwrap(),
            TicketBackend::GitHub
        );
        assert_eq!(
            resolve_backend(None, None).unwrap(),
            TicketBackend::SpexState
        );
        assert!(resolve_backend(Some("bogus"), None).is_err());
    }

    #[test]
    fn markdown_sink_writes_frontmatter_file() {
        let dir = tempfile::tempdir().unwrap();
        let sink = MarkdownSink {
            dir: dir.path().join("tasks"),
        };
        let r = sink.export_task(&task(), &spec(), false).unwrap();
        let body = std::fs::read_to_string(&r.external_id).unwrap();
        assert!(body.starts_with("---\n"));
        assert!(body.contains("id: TASK-1"));
        assert!(body.contains("spec: SPEC-1"));
        assert!(body.contains("The feature"));
    }

    #[test]
    fn markdown_sink_dry_run_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let sink = MarkdownSink {
            dir: dir.path().join("tasks"),
        };
        let r = sink.export_task(&task(), &spec(), true).unwrap();
        assert!(!Path::new(&r.external_id).exists());
    }

    #[test]
    fn github_create_args_include_repo_and_title() {
        let sink = GitHubSink {
            repo: Some("acme/widgets".into()),
        };
        let args = sink.create_args(&task(), &spec());
        assert_eq!(args[0], "issue");
        assert_eq!(args[1], "create");
        assert!(args.contains(&"--repo".to_string()));
        assert!(args.contains(&"acme/widgets".to_string()));
        assert!(args.iter().any(|a| a.starts_with("TASK-1: do the thing")));
    }
}
