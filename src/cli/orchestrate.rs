use anyhow::{anyhow, Result};
use colored::Colorize;
use sqlx::SqlitePool;

use crate::sdd::{
    event::emit_event,
    plan_version::{create_plan_version, list_plan_versions, supersede_plan_versions},
    replan_request::{create_replan_request, list_replan_requests, update_replan_request},
    scheduler::scheduler_next,
    task::{get_task, task_runtime_metadata, update_task_metadata, TaskLockRequirement},
    task_lease::{
        claim_task_lease, expire_stale_task_leases, heartbeat_task_lease, release_task_lease,
    },
    task_lock::{acquire_task_locks, query_task_locks, release_task_locks},
};

pub async fn cmd_orchestrate_next(
    pool: &SqlitePool,
    project_dir: &str,
    spec: &str,
    agent: &str,
) -> Result<()> {
    let decision = scheduler_next(pool, project_dir, spec, agent).await?;
    if let Some(task) = decision.task {
        println!(
            "{} next task for {}: {} {}",
            "✓".green(),
            agent.cyan(),
            task.id.cyan(),
            task.title
        );
        if let Some(plan) = decision.active_plan_version {
            println!("  plan: {}", plan.dimmed());
        }
        if !decision.skipped.is_empty() {
            println!("  skipped:");
            for reason in decision.skipped.iter().take(8) {
                println!("    - {}", reason);
            }
        }
    } else {
        println!("{} no task available for {}", "ℹ".blue(), agent.cyan());
        println!("  reason: {}", decision.reason);
    }
    Ok(())
}

pub async fn cmd_orchestrate_claim(
    pool: &SqlitePool,
    project_dir: &str,
    spec: Option<&str>,
    task: &str,
    agent: &str,
    auto_lock: bool,
    ttl: i64,
) -> Result<()> {
    let lease = claim_task_lease(pool, project_dir, task, agent, ttl).await?;
    let payload = serde_json::json!({"task": task, "agent": agent, "lease_expires_at": lease.lease_expires_at});
    emit_event(
        pool,
        project_dir,
        "TaskClaimed",
        spec,
        Some(agent),
        &payload.to_string(),
    )
    .await?;
    if auto_lock {
        let task_record = get_task(pool, project_dir, task)
            .await?
            .ok_or_else(|| anyhow!("Task '{}' not found", task))?;
        let meta = task_runtime_metadata(&task_record);
        let locks: Vec<(String, String)> = meta
            .lock_requirements
            .iter()
            .map(|l| (l.lock_type.clone(), l.resource.clone()))
            .collect();
        if !locks.is_empty() {
            let acquired =
                acquire_task_locks(pool, project_dir, task, &task_record.spec, &locks).await?;
            let payload = serde_json::json!({
                "task": task,
                "locks": acquired.iter().map(|l| serde_json::json!({"lock_type": l.lock_type, "resource": l.resource})).collect::<Vec<_>>()
            });
            emit_event(
                pool,
                project_dir,
                "TaskLocksAcquired",
                spec.or(Some(&task_record.spec)),
                Some(agent),
                &payload.to_string(),
            )
            .await?;
            println!("{} auto-acquired {} lock(s)", "✓".green(), acquired.len());
        }
    }
    println!(
        "{} claimed {} for {} until {}",
        "✓".green(),
        lease.task_id.cyan(),
        lease.agent_id.cyan(),
        lease.lease_expires_at
    );
    Ok(())
}

pub async fn cmd_orchestrate_heartbeat(
    pool: &SqlitePool,
    project_dir: &str,
    spec: Option<&str>,
    task: &str,
    ttl: i64,
    progress: Option<&str>,
) -> Result<()> {
    let lease = heartbeat_task_lease(pool, project_dir, task, ttl, Some("running")).await?;
    let payload = serde_json::json!({"task": task, "agent": lease.agent_id, "status": "running", "progress": progress.unwrap_or("heartbeat"), "lease_expires_at": lease.lease_expires_at});
    emit_event(
        pool,
        project_dir,
        "TaskHeartbeat",
        spec,
        Some(&lease.agent_id),
        &payload.to_string(),
    )
    .await?;
    println!(
        "{} heartbeat refreshed for {} until {}",
        "✓".green(),
        lease.task_id.cyan(),
        lease.lease_expires_at
    );
    Ok(())
}

pub async fn cmd_orchestrate_release(
    pool: &SqlitePool,
    project_dir: &str,
    spec: Option<&str>,
    task: &str,
    final_status: Option<&str>,
) -> Result<()> {
    let lease = release_task_lease(pool, project_dir, task, final_status).await?;
    let _ = release_task_locks(pool, project_dir, task).await?;
    let payload =
        serde_json::json!({"task": task, "final_status": final_status.unwrap_or("released")});
    emit_event(
        pool,
        project_dir,
        "TaskLeaseReleased",
        spec,
        Some(&lease.agent_id),
        &payload.to_string(),
    )
    .await?;
    println!(
        "{} released lease for {} ({})",
        "✓".green(),
        lease.task_id.cyan(),
        lease.status
    );
    Ok(())
}

pub async fn cmd_orchestrate_expire(pool: &SqlitePool, project_dir: &str) -> Result<()> {
    let expired = expire_stale_task_leases(pool, project_dir).await?;
    if expired.is_empty() {
        println!("{} no stale leases", "✓".green());
    } else {
        println!("{} expired {} stale lease(s)", "✓".green(), expired.len());
        for lease in expired {
            println!("  {} {}", lease.task_id.cyan(), lease.agent_id.dimmed());
        }
    }
    Ok(())
}

pub async fn cmd_orchestrate_lock(
    pool: &SqlitePool,
    project_dir: &str,
    task: &str,
    spec: &str,
    modules: &[String],
    semantics: &[String],
    files: &[String],
) -> Result<()> {
    let mut locks = Vec::new();
    locks.extend(modules.iter().cloned().map(|r| ("module".to_string(), r)));
    locks.extend(
        semantics
            .iter()
            .cloned()
            .map(|r| ("semantic".to_string(), r)),
    );
    locks.extend(files.iter().cloned().map(|r| ("file".to_string(), r)));
    if locks.is_empty() {
        return Err(anyhow!("No locks requested"));
    }
    let created = acquire_task_locks(pool, project_dir, task, spec, &locks).await?;
    let payload = serde_json::json!({
        "task": task,
        "locks": created.iter().map(|l| serde_json::json!({"lock_type": l.lock_type, "resource": l.resource})).collect::<Vec<_>>()
    });
    emit_event(
        pool,
        project_dir,
        "TaskLocksAcquired",
        Some(spec),
        None,
        &payload.to_string(),
    )
    .await?;
    println!(
        "{} acquired {} lock(s) for {}",
        "✓".green(),
        created.len(),
        task.cyan()
    );
    Ok(())
}

pub async fn cmd_orchestrate_locks(
    pool: &SqlitePool,
    project_dir: &str,
    spec: Option<&str>,
    task: Option<&str>,
) -> Result<()> {
    let locks = query_task_locks(pool, project_dir, spec, task, true).await?;
    if locks.is_empty() {
        println!("{}", "No active locks.".dimmed());
        return Ok(());
    }
    for lock in locks {
        println!(
            "{} {} {} {}",
            lock.task_id.cyan(),
            lock.lock_type.yellow(),
            lock.resource,
            lock.status.dimmed()
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn cmd_orchestrate_task_metadata(
    pool: &SqlitePool,
    project_dir: &str,
    task: &str,
    depends_on: &[String],
    conflicts_with: &[String],
    lock_set: &[String],
    priority: Option<i64>,
    risk_level: Option<&str>,
    execution_bucket: Option<&str>,
    estimate_points: Option<i64>,
    unblock_value: Option<i64>,
    plan_version: Option<&str>,
) -> Result<()> {
    let lock_requirements: Vec<TaskLockRequirement> = lock_set
        .iter()
        .filter_map(|entry| {
            let (lock_type, resource) = entry.split_once(':')?;
            Some(TaskLockRequirement {
                lock_type: lock_type.to_string(),
                resource: resource.to_string(),
            })
        })
        .collect();
    let updated = update_task_metadata(
        pool,
        project_dir,
        task,
        Some(depends_on),
        Some(conflicts_with),
        Some(lock_set),
        Some(&lock_requirements),
        priority,
        risk_level,
        execution_bucket,
        estimate_points,
        unblock_value,
        Some(plan_version),
    )
    .await?;
    println!(
        "{} updated task metadata for {}",
        "✓".green(),
        updated.id.cyan()
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn cmd_orchestrate_replan(
    pool: &SqlitePool,
    project_dir: &str,
    id: &str,
    spec: &str,
    task: Option<&str>,
    agent: &str,
    reason: &str,
    impact: &[String],
    proposed_action: Option<&str>,
) -> Result<()> {
    let req = create_replan_request(
        pool,
        project_dir,
        id,
        spec,
        task,
        agent,
        reason,
        impact,
        proposed_action,
    )
    .await?;
    let payload = serde_json::json!({"task": task, "reason": reason, "impact": impact, "proposed_action": proposed_action});
    emit_event(
        pool,
        project_dir,
        "ReplanRequested",
        Some(spec),
        Some(agent),
        &payload.to_string(),
    )
    .await?;
    println!(
        "{} replan request {} created for {}",
        "✓".green(),
        req.id.cyan(),
        req.spec_id.cyan()
    );
    Ok(())
}

pub async fn cmd_orchestrate_replans(
    pool: &SqlitePool,
    project_dir: &str,
    spec: Option<&str>,
    status: Option<&str>,
) -> Result<()> {
    let items = list_replan_requests(pool, project_dir, spec, status).await?;
    if items.is_empty() {
        println!("{}", "No replan requests.".dimmed());
        return Ok(());
    }
    for item in items {
        println!(
            "{} {} {}",
            item.id.cyan(),
            item.status.yellow(),
            item.reason
        );
    }
    Ok(())
}

pub async fn cmd_orchestrate_replan_update(
    pool: &SqlitePool,
    project_dir: &str,
    id: &str,
    status: &str,
) -> Result<()> {
    let item = update_replan_request(pool, project_dir, id, status).await?;
    println!(
        "{} replan {} -> {}",
        "✓".green(),
        item.id.cyan(),
        item.status
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn cmd_orchestrate_plan_version(
    pool: &SqlitePool,
    project_dir: &str,
    id: &str,
    spec: &str,
    version: i64,
    reason: Option<&str>,
    plan_json: &str,
    supersede: bool,
) -> Result<()> {
    if supersede {
        supersede_plan_versions(pool, project_dir, spec).await?;
    }
    let plan = create_plan_version(pool, project_dir, id, spec, version, reason, plan_json).await?;
    println!(
        "{} plan {} v{} active for {}",
        "✓".green(),
        plan.id.cyan(),
        plan.version,
        plan.spec_id.cyan()
    );
    Ok(())
}

pub async fn cmd_orchestrate_plan_versions(
    pool: &SqlitePool,
    project_dir: &str,
    spec: Option<&str>,
) -> Result<()> {
    let plans = list_plan_versions(pool, project_dir, spec).await?;
    if plans.is_empty() {
        println!("{}", "No plan versions.".dimmed());
        return Ok(());
    }
    for plan in plans {
        println!(
            "{} {} v{} {}",
            plan.spec_id.cyan(),
            plan.id.cyan(),
            plan.version,
            plan.status
        );
    }
    Ok(())
}
