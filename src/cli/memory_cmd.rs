use anyhow::Result;
use colored::Colorize;
use sqlx::SqlitePool;

use crate::sdd::memory::{memory_gc, memory_get_full, memory_list, memory_search};

pub async fn cmd_memory_list(
    pool: &SqlitePool,
    agent: &str,
    spec: Option<&str>,
    mem_type: Option<&str>,
    limit: Option<i64>,
) -> Result<()> {
    let entries = memory_list(pool, agent, spec, mem_type, limit, None).await?;

    if entries.is_empty() {
        println!("No entries found.");
        return Ok(());
    }

    for m in &entries {
        let type_str = m.type_.as_deref().unwrap_or("-");
        let spec_str = if m.spec.is_empty() { "-" } else { &m.spec };
        println!(
            "{} {} [{}] spec={}  rev={} access={}",
            "•".dimmed(),
            m.key.cyan(),
            type_str.yellow(),
            spec_str,
            m.revision_count,
            m.access_count,
        );
    }
    println!("\n{} entries", entries.len());
    Ok(())
}

pub async fn cmd_memory_show(
    pool: &SqlitePool,
    agent: &str,
    key: &str,
    spec: Option<&str>,
) -> Result<()> {
    let entry = memory_get_full(pool, agent, key, spec).await?;

    match entry {
        Some(m) => {
            println!("{}: {}", "agent".dimmed(), m.agent);
            println!("{}: {}", "key".dimmed(), m.key.cyan());
            println!("{}: {}", "spec".dimmed(), if m.spec.is_empty() { "-" } else { &m.spec });
            println!("{}: {}", "type".dimmed(), m.type_.as_deref().unwrap_or("-"));
            println!("{}: {}", "updated_at".dimmed(), m.updated_at);
            println!("{}: {}", "revision".dimmed(), m.revision_count);
            println!("{}: {}", "access_count".dimmed(), m.access_count);
            if let Some(ref exp) = m.expires_at {
                println!("{}: {}", "expires_at".dimmed(), exp);
            }
            println!("\n{}", m.value);
        }
        None => {
            println!("Not found.");
        }
    }
    Ok(())
}

pub async fn cmd_memory_search(
    pool: &SqlitePool,
    query: &str,
    agent: &str,
    spec: Option<&str>,
    mem_type: Option<&str>,
) -> Result<()> {
    let results = memory_search(pool, agent, query, spec, mem_type, None).await?;

    if results.is_empty() {
        println!("No results.");
        return Ok(());
    }

    for m in &results {
        let type_str = m.type_.as_deref().unwrap_or("-");
        println!(
            "{} {} [{}]",
            "•".dimmed(),
            m.key.cyan(),
            type_str.yellow(),
        );
    }
    println!("\n{} results", results.len());
    Ok(())
}

pub async fn cmd_memory_gc(pool: &SqlitePool, dry_run: bool) -> Result<()> {
    let result = memory_gc(pool, dry_run).await?;

    if dry_run {
        println!("{}", "Dry run — no rows removed.".yellow());
    }

    println!(
        "Soft-deleted: {}  Expired: {}",
        result.deleted_count, result.expired_count
    );

    if !result.sample_keys.is_empty() {
        println!("Sample keys: {}", result.sample_keys.join(", "));
    }

    if result.deleted_count == 0 && result.expired_count == 0 {
        println!("{}", "Nothing to collect.".dimmed());
    } else if !dry_run {
        println!("{} GC complete.", "✓".green());
    }

    Ok(())
}
