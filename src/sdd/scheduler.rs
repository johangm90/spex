use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::sdd::{
    context_gap::list_context_gaps,
    incident::list_incidents,
    interrupt::list_interrupts,
    plan_version::get_active_plan_version,
    task::{list_tasks, task_runtime_metadata, Task},
    task_lease::get_task_lease,
    task_lock::query_task_locks,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerDecision {
    pub spec_id: String,
    pub agent: String,
    pub task: Option<Task>,
    pub active_plan_version: Option<String>,
    pub reason: String,
    pub skipped: Vec<String>,
}

pub async fn scheduler_next(
    pool: &SqlitePool,
    project_dir: &str,
    spec_id: &str,
    agent: &str,
) -> Result<SchedulerDecision> {
    let incidents = list_incidents(pool, project_dir, Some(spec_id), None).await?;
    let gaps = list_context_gaps(pool, project_dir, Some(spec_id), None).await?;
    let interrupts = list_interrupts(pool, project_dir, Some(spec_id), None).await?;
    let blocking_incidents = incidents
        .iter()
        .filter(|i| {
            i.blocking
                && i.status != "resolved"
                && i.status != "duplicate"
                && i.status != "not_reproducible"
        })
        .count();
    let blocking_gaps = gaps
        .iter()
        .filter(|g| g.blocking && g.status != "resolved" && g.status != "wont_fix")
        .count();
    let active_interrupts = interrupts
        .iter()
        .filter(|it| it.status == "open" || it.status == "active")
        .count();
    let active_plan = get_active_plan_version(pool, project_dir, spec_id).await?;

    if blocking_incidents > 0 || blocking_gaps > 0 {
        return Ok(SchedulerDecision {
            spec_id: spec_id.to_string(),
            agent: agent.to_string(),
            task: None,
            active_plan_version: active_plan.as_ref().map(|p| p.id.clone()),
            reason: format!(
                "spec has blockers: {} incidents, {} context gaps",
                blocking_incidents, blocking_gaps
            ),
            skipped: vec![],
        });
    }
    if active_interrupts > 0 {
        return Ok(SchedulerDecision {
            spec_id: spec_id.to_string(),
            agent: agent.to_string(),
            task: None,
            active_plan_version: active_plan.as_ref().map(|p| p.id.clone()),
            reason: format!("spec has {} active interrupts", active_interrupts),
            skipped: vec![],
        });
    }

    let all_tasks = list_tasks(pool, project_dir, Some(spec_id)).await?;
    let active_locks = query_task_locks(pool, project_dir, Some(spec_id), None, true).await?;
    let mut candidates: Vec<Task> = all_tasks
        .iter()
        .filter(|t| t.agent == agent && t.status == "ready")
        .cloned()
        .collect();
    candidates.sort_by(|a, b| {
        let a_meta = task_runtime_metadata(a);
        let b_meta = task_runtime_metadata(b);
        a_meta
            .priority
            .cmp(&b_meta.priority)
            .then_with(|| {
                bucket_rank(&a_meta.execution_bucket).cmp(&bucket_rank(&b_meta.execution_bucket))
            })
            .then_with(|| b_meta.unblock_value.cmp(&a_meta.unblock_value))
            .then_with(|| a_meta.estimate_points.cmp(&b_meta.estimate_points))
            .then_with(|| risk_rank(&a_meta.risk_level).cmp(&risk_rank(&b_meta.risk_level)))
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut skipped = Vec::new();
    for task in candidates {
        if get_task_lease(pool, project_dir, &task.id).await?.is_some() {
            skipped.push(format!("{} skipped: already leased", task.id));
            continue;
        }
        let meta = task_runtime_metadata(&task);
        let unmet: Vec<String> = meta
            .depends_on
            .iter()
            .filter(|dep| {
                !all_tasks
                    .iter()
                    .find(|t| &t.id == *dep)
                    .map(|t| t.status == "done")
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        if !unmet.is_empty() {
            skipped.push(format!(
                "{} skipped: waiting on deps {}",
                task.id,
                unmet.join(",")
            ));
            continue;
        }
        let active_conflicts: Vec<String> = meta
            .conflicts_with
            .iter()
            .filter(|conflict| {
                all_tasks
                    .iter()
                    .find(|t| &t.id == *conflict)
                    .map(|t| {
                        matches!(
                            t.status.as_str(),
                            "claimed" | "running" | "awaiting_review" | "verifying"
                        )
                    })
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        if !active_conflicts.is_empty() {
            skipped.push(format!(
                "{} skipped: conflicts with active {}",
                task.id,
                active_conflicts.join(",")
            ));
            continue;
        }
        if meta.execution_bucket == "serialized_only"
            && all_tasks.iter().any(|t| {
                matches!(
                    t.status.as_str(),
                    "claimed" | "running" | "awaiting_review" | "verifying"
                ) && t.id != task.id
            })
        {
            skipped.push(format!(
                "{} skipped: serialized_only while other tasks active",
                task.id
            ));
            continue;
        }
        let lock_conflicts: Vec<String> = meta
            .lock_requirements
            .iter()
            .filter_map(|required| {
                active_locks
                    .iter()
                    .find(|active| {
                        active.task_id != task.id
                            && active.lock_type == required.lock_type
                            && active.resource == required.resource
                    })
                    .map(|active| {
                        format!(
                            "{}:{} by {}",
                            active.lock_type, active.resource, active.task_id
                        )
                    })
            })
            .collect();
        if !lock_conflicts.is_empty() {
            skipped.push(format!(
                "{} skipped: lock conflict {}",
                task.id,
                lock_conflicts.join(",")
            ));
            continue;
        }
        if let Some(plan) = &active_plan {
            if let Some(task_plan_version) = &meta.plan_version {
                if task_plan_version != &plan.id {
                    skipped.push(format!(
                        "{} skipped: plan mismatch {} != {}",
                        task.id, task_plan_version, plan.id
                    ));
                    continue;
                }
            }
        }
        return Ok(SchedulerDecision {
            spec_id: spec_id.to_string(),
            agent: agent.to_string(),
            task: Some(task),
            active_plan_version: active_plan.as_ref().map(|p| p.id.clone()),
            reason: format!(
                "task ready; priority={}, bucket={}, risk={}, estimate={}, unblock={}",
                meta.priority,
                meta.execution_bucket,
                meta.risk_level,
                meta.estimate_points,
                meta.unblock_value
            ),
            skipped,
        });
    }

    Ok(SchedulerDecision {
        spec_id: spec_id.to_string(),
        agent: agent.to_string(),
        task: None,
        active_plan_version: active_plan.as_ref().map(|p| p.id.clone()),
        reason: "no ready task available after dependency/conflict/lock checks".to_string(),
        skipped,
    })
}

fn bucket_rank(bucket: &str) -> i32 {
    match bucket {
        "safe_parallel" => 0,
        "coordinated_parallel" => 1,
        "serialized_only" => 2,
        _ => 3,
    }
}

fn risk_rank(risk: &str) -> i32 {
    match risk {
        "low" => 0,
        "medium" => 1,
        "high" => 2,
        "critical" => 3,
        _ => 4,
    }
}
