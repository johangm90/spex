use anyhow::{anyhow, Result};
use colored::Colorize;
use sqlx::SqlitePool;

use crate::sdd::{
    context_gap::{create_context_gap, get_context_gap, list_context_gaps, update_context_gap},
    handoff_snapshot::{create_handoff_snapshot, get_handoff_snapshot, list_handoff_snapshots},
    incident::{create_incident, get_incident, list_incidents, update_incident},
    interrupt::{create_interrupt, get_interrupt, list_interrupts, update_interrupt},
    verification_run::{create_verification_run, get_verification_run, list_verification_runs},
};

use super::util::colorize_status;

#[allow(clippy::too_many_arguments)]
pub async fn cmd_incident_add(
    pool: &SqlitePool,
    id: &str,
    spec: &str,
    task: Option<&str>,
    title: &str,
    severity: &str,
    source: &str,
    blocking: bool,
    repro_steps: Option<&str>,
) -> Result<()> {
    let incident = create_incident(
        pool,
        id,
        spec,
        task,
        title,
        severity,
        source,
        blocking,
        repro_steps,
    )
    .await?;
    println!(
        "{} Incident {} created for {}",
        "✓".green(),
        incident.id.cyan(),
        incident.spec_id.cyan()
    );
    Ok(())
}

pub async fn cmd_incident_list(
    pool: &SqlitePool,
    spec: Option<&str>,
    status: Option<&str>,
    json: bool,
) -> Result<()> {
    let incidents = list_incidents(pool, spec, status).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&incidents)?);
        return Ok(());
    }
    if incidents.is_empty() {
        println!("{}", "No incidents found.".dimmed());
        return Ok(());
    }
    println!(
        "{:<14} {:<12} {:<10} {:<18} Title",
        "ID".bold(),
        "Spec".bold(),
        "Severity".bold(),
        "Status".bold()
    );
    println!("{}", "─".repeat(90).dimmed());
    for item in incidents {
        let sev = if item.severity == "critical" || item.severity == "high" {
            item.severity.red()
        } else {
            item.severity.yellow()
        };
        let title = if item.blocking {
            format!("[blocking] {}", item.title)
        } else {
            item.title
        };
        println!(
            "{:<14} {:<12} {:<10} {:<18} {}",
            item.id.cyan(),
            item.spec_id.dimmed(),
            sev,
            colorize_status(&item.status),
            title
        );
    }
    Ok(())
}

pub async fn cmd_incident_show(pool: &SqlitePool, id: &str) -> Result<()> {
    let item = get_incident(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Incident '{}' not found", id))?;
    println!(
        "{}",
        format!("═══ {} ══════════════════════════════════", item.id).cyan()
    );
    println!("  {}", item.title.bold());
    println!("  Spec:     {}", item.spec_id.cyan());
    if let Some(task) = &item.task_id {
        println!("  Task:     {}", task.cyan());
    }
    println!(
        "  Severity: {} | Status: {}",
        item.severity,
        colorize_status(&item.status)
    );
    println!(
        "  Source:   {} | Blocking: {}",
        item.source,
        if item.blocking {
            "yes".red()
        } else {
            "no".dimmed()
        }
    );
    if let Some(repro) = &item.repro_steps {
        println!("  Repro:    {}", repro);
    }
    if let Some(root) = &item.root_cause {
        println!("  Root:     {}", root);
    }
    if let Some(fix) = &item.fix_strategy {
        println!("  Fix:      {}", fix);
    }
    Ok(())
}

pub async fn cmd_incident_update(
    pool: &SqlitePool,
    id: &str,
    status: Option<&str>,
    blocking: Option<bool>,
    root_cause: Option<&str>,
    fix_strategy: Option<&str>,
) -> Result<()> {
    let item = update_incident(pool, id, status, blocking, root_cause, fix_strategy).await?;
    println!(
        "{} Incident {} updated to {}",
        "✓".green(),
        item.id.cyan(),
        colorize_status(&item.status)
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn cmd_gap_add(
    pool: &SqlitePool,
    id: &str,
    spec: &str,
    task: Option<&str>,
    kind: &str,
    criticality: &str,
    blocking: bool,
    question: &str,
    assumption: Option<&str>,
) -> Result<()> {
    let gap = create_context_gap(
        pool,
        id,
        spec,
        task,
        kind,
        criticality,
        blocking,
        question,
        assumption,
    )
    .await?;
    println!(
        "{} Context gap {} created for {}",
        "✓".green(),
        gap.id.cyan(),
        gap.spec_id.cyan()
    );
    Ok(())
}

pub async fn cmd_gap_list(
    pool: &SqlitePool,
    spec: Option<&str>,
    status: Option<&str>,
    json: bool,
) -> Result<()> {
    let gaps = list_context_gaps(pool, spec, status).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&gaps)?);
        return Ok(());
    }
    if gaps.is_empty() {
        println!("{}", "No context gaps found.".dimmed());
        return Ok(());
    }
    println!(
        "{:<14} {:<12} {:<18} {:<10} Question",
        "ID".bold(),
        "Spec".bold(),
        "Status".bold(),
        "Crit".bold()
    );
    println!("{}", "─".repeat(90).dimmed());
    for item in gaps {
        let crit = if item.criticality == "high" {
            item.criticality.red()
        } else {
            item.criticality.yellow()
        };
        let question = if item.blocking {
            format!("[blocking] {}", item.question)
        } else {
            item.question
        };
        println!(
            "{:<14} {:<12} {:<18} {:<10} {}",
            item.id.cyan(),
            item.spec_id.dimmed(),
            colorize_status(&item.status),
            crit,
            question
        );
    }
    Ok(())
}

pub async fn cmd_gap_show(pool: &SqlitePool, id: &str) -> Result<()> {
    let item = get_context_gap(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Context gap '{}' not found", id))?;
    println!(
        "{}",
        format!("═══ {} ══════════════════════════════════", item.id).cyan()
    );
    println!("  Spec:       {}", item.spec_id.cyan());
    if let Some(task) = &item.task_id {
        println!("  Task:       {}", task.cyan());
    }
    println!("  Kind:       {}", item.kind);
    println!(
        "  Criticality:{} | Status: {}",
        item.criticality,
        colorize_status(&item.status)
    );
    println!(
        "  Blocking:   {}",
        if item.blocking {
            "yes".red()
        } else {
            "no".dimmed()
        }
    );
    println!("  Question:   {}", item.question);
    if let Some(assumption) = &item.assumption {
        println!("  Assumption: {}", assumption);
    }
    if let Some(resolution) = &item.resolution {
        println!("  Resolution: {}", resolution);
    }
    Ok(())
}

pub async fn cmd_gap_update(
    pool: &SqlitePool,
    id: &str,
    status: Option<&str>,
    blocking: Option<bool>,
    assumption: Option<&str>,
    resolution: Option<&str>,
) -> Result<()> {
    let item = update_context_gap(pool, id, status, blocking, assumption, resolution).await?;
    println!(
        "{} Context gap {} updated to {}",
        "✓".green(),
        item.id.cyan(),
        colorize_status(&item.status)
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn cmd_verify_add(
    pool: &SqlitePool,
    id: &str,
    spec: &str,
    task: Option<&str>,
    slice: Option<&str>,
    kind: &str,
    status: &str,
    command: Option<&str>,
    summary: &str,
    evidence: Option<&str>,
) -> Result<()> {
    let run = create_verification_run(
        pool, id, spec, task, slice, kind, status, command, summary, evidence,
    )
    .await?;
    println!(
        "{} Verification run {} recorded for {}",
        "✓".green(),
        run.id.cyan(),
        run.spec_id.cyan()
    );
    Ok(())
}

pub async fn cmd_verify_list(
    pool: &SqlitePool,
    spec: Option<&str>,
    task: Option<&str>,
    status: Option<&str>,
    json: bool,
) -> Result<()> {
    let runs = list_verification_runs(pool, spec, task, status).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&runs)?);
        return Ok(());
    }
    if runs.is_empty() {
        println!("{}", "No verification runs found.".dimmed());
        return Ok(());
    }
    println!(
        "{:<14} {:<12} {:<14} {:<16} Summary",
        "ID".bold(),
        "Spec".bold(),
        "Kind".bold(),
        "Status".bold()
    );
    println!("{}", "─".repeat(90).dimmed());
    for item in runs {
        println!(
            "{:<14} {:<12} {:<14} {:<16} {}",
            item.id.cyan(),
            item.spec_id.dimmed(),
            item.kind,
            colorize_status(&item.status),
            item.summary
        );
    }
    Ok(())
}

pub async fn cmd_verify_show(pool: &SqlitePool, id: &str) -> Result<()> {
    let item = get_verification_run(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Verification run '{}' not found", id))?;
    println!(
        "{}",
        format!("═══ {} ══════════════════════════════════", item.id).cyan()
    );
    println!("  Spec:    {}", item.spec_id.cyan());
    if let Some(task) = &item.task_id {
        println!("  Task:    {}", task.cyan());
    }
    if let Some(slice) = &item.slice_id {
        println!("  Slice:   {}", slice.cyan());
    }
    println!(
        "  Kind:    {} | Status: {}",
        item.kind,
        colorize_status(&item.status)
    );
    if let Some(command) = &item.command {
        println!("  Command: {}", command);
    }
    println!("  Summary: {}", item.summary);
    if let Some(evidence) = &item.evidence {
        println!("  Evidence:{}", evidence);
    }
    Ok(())
}

pub async fn cmd_interrupt_add(
    pool: &SqlitePool,
    id: &str,
    spec: &str,
    reason_type: &str,
    preempted_tasks: &[String],
    resume_hint: Option<&str>,
) -> Result<()> {
    let item = create_interrupt(pool, id, spec, reason_type, preempted_tasks, resume_hint).await?;
    println!(
        "{} Interrupt {} created for {}",
        "✓".green(),
        item.id.cyan(),
        item.spec_id.cyan()
    );
    Ok(())
}

pub async fn cmd_interrupt_list(
    pool: &SqlitePool,
    spec: Option<&str>,
    status: Option<&str>,
    json: bool,
) -> Result<()> {
    let items = list_interrupts(pool, spec, status).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }
    if items.is_empty() {
        println!("{}", "No interrupts found.".dimmed());
        return Ok(());
    }
    println!(
        "{:<14} {:<12} {:<18} Resume Hint",
        "ID".bold(),
        "Spec".bold(),
        "Status".bold()
    );
    println!("{}", "─".repeat(90).dimmed());
    for item in items {
        println!(
            "{:<14} {:<12} {:<18} {}",
            item.id.cyan(),
            item.spec_id.dimmed(),
            colorize_status(&item.status),
            item.resume_hint.unwrap_or_else(|| "—".to_string())
        );
    }
    Ok(())
}

pub async fn cmd_interrupt_show(pool: &SqlitePool, id: &str) -> Result<()> {
    let item = get_interrupt(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Interrupt '{}' not found", id))?;
    println!(
        "{}",
        format!("═══ {} ══════════════════════════════════", item.id).cyan()
    );
    println!("  Spec:       {}", item.spec_id.cyan());
    println!("  Reason:     {}", item.reason_type);
    println!("  Status:     {}", colorize_status(&item.status));
    println!("  Preempted:  {}", item.preempted_tasks);
    if let Some(resume_hint) = &item.resume_hint {
        println!("  Resume:     {}", resume_hint);
    }
    Ok(())
}

pub async fn cmd_interrupt_update(
    pool: &SqlitePool,
    id: &str,
    status: Option<&str>,
    resume_hint: Option<&str>,
) -> Result<()> {
    let item = update_interrupt(pool, id, status, resume_hint).await?;
    println!(
        "{} Interrupt {} updated to {}",
        "✓".green(),
        item.id.cyan(),
        colorize_status(&item.status)
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn cmd_handoff_add(
    pool: &SqlitePool,
    id: &str,
    spec: &str,
    interrupt: Option<&str>,
    last_wave: Option<i64>,
    last_task: Option<&str>,
    files_touched: &[String],
    decisions: &[String],
    open_risks: &[String],
    next_steps: &[String],
) -> Result<()> {
    let item = create_handoff_snapshot(
        pool,
        id,
        spec,
        interrupt,
        last_wave,
        last_task,
        files_touched,
        decisions,
        open_risks,
        next_steps,
    )
    .await?;
    println!(
        "{} Handoff snapshot {} created for {}",
        "✓".green(),
        item.id.cyan(),
        item.spec_id.cyan()
    );
    Ok(())
}

pub async fn cmd_handoff_list(pool: &SqlitePool, spec: Option<&str>, json: bool) -> Result<()> {
    let items = list_handoff_snapshots(pool, spec).await?;
    if json {
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }
    if items.is_empty() {
        println!("{}", "No handoff snapshots found.".dimmed());
        return Ok(());
    }
    println!(
        "{:<14} {:<12} {:<10} {:<14} Next Steps",
        "ID".bold(),
        "Spec".bold(),
        "Wave".bold(),
        "Last Task".bold()
    );
    println!("{}", "─".repeat(90).dimmed());
    for item in items {
        println!(
            "{:<14} {:<12} {:<10} {:<14} {}",
            item.id.cyan(),
            item.spec_id.dimmed(),
            item.last_wave
                .map(|v| v.to_string())
                .unwrap_or_else(|| "—".to_string()),
            item.last_task.unwrap_or_else(|| "—".to_string()),
            item.next_steps
        );
    }
    Ok(())
}

pub async fn cmd_handoff_show(pool: &SqlitePool, id: &str) -> Result<()> {
    let item = get_handoff_snapshot(pool, id)
        .await?
        .ok_or_else(|| anyhow!("Handoff snapshot '{}' not found", id))?;
    println!(
        "{}",
        format!("═══ {} ══════════════════════════════════", item.id).cyan()
    );
    println!("  Spec:        {}", item.spec_id.cyan());
    if let Some(interrupt) = &item.interrupt_id {
        println!("  Interrupt:   {}", interrupt.cyan());
    }
    if let Some(wave) = item.last_wave {
        println!("  Last Wave:   {}", wave);
    }
    if let Some(task) = &item.last_task {
        println!("  Last Task:   {}", task.cyan());
    }
    println!("  Files:       {}", item.files_touched);
    println!("  Decisions:   {}", item.decisions);
    println!("  Open Risks:  {}", item.open_risks);
    println!("  Next Steps:  {}", item.next_steps);
    Ok(())
}
