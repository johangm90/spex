use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::sdd::{
    context_gap::list_context_gaps,
    incident::list_incidents,
    interrupt::list_interrupts,
    task::{list_tasks, Task},
    verification_run::list_verification_runs,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalSummary {
    pub open_incidents: usize,
    pub blocking_incidents: usize,
    pub open_context_gaps: usize,
    pub blocking_context_gaps: usize,
    pub active_interrupts: usize,
    pub verification_failures: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpecOperationalState {
    pub summary: OperationalSummary,
    pub next_actionable_tasks: Vec<Task>,
}

pub async fn summarize_spec_operations(
    pool: &SqlitePool,
    spec_id: &str,
) -> Result<SpecOperationalState> {
    let incidents = list_incidents(pool, Some(spec_id), None).await?;
    let context_gaps = list_context_gaps(pool, Some(spec_id), None).await?;
    let interrupts = list_interrupts(pool, Some(spec_id), None).await?;
    let verification_runs = list_verification_runs(pool, Some(spec_id), None, None).await?;
    let tasks = list_tasks(pool, Some(spec_id)).await?;

    let open_incidents: Vec<_> = incidents
        .into_iter()
        .filter(|i| {
            i.status != "resolved" && i.status != "duplicate" && i.status != "not_reproducible"
        })
        .collect();
    let blocking_incidents = open_incidents.iter().filter(|i| i.blocking).count();

    let open_context_gaps: Vec<_> = context_gaps
        .into_iter()
        .filter(|g| g.status != "resolved" && g.status != "wont_fix")
        .collect();
    let blocking_context_gaps = open_context_gaps.iter().filter(|g| g.blocking).count();

    let active_interrupts = interrupts
        .into_iter()
        .filter(|it| it.status == "open" || it.status == "active")
        .count();

    let verification_failures = verification_runs
        .into_iter()
        .filter(|v| v.status == "fail" || v.status == "blocked" || v.status == "flaky")
        .count();

    let mut next_actionable_tasks: Vec<Task> = tasks
        .into_iter()
        .filter(|t| {
            t.status == "ready"
                || t.status == "claimed"
                || t.status == "running"
                || t.status == "awaiting_review"
                || t.status == "verifying"
        })
        .collect();
    next_actionable_tasks.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(SpecOperationalState {
        summary: OperationalSummary {
            open_incidents: open_incidents.len(),
            blocking_incidents,
            open_context_gaps: open_context_gaps.len(),
            blocking_context_gaps,
            active_interrupts,
            verification_failures,
        },
        next_actionable_tasks,
    })
}
