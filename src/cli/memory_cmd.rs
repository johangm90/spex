use anyhow::Result;
use colored::Colorize;
use sqlx::SqlitePool;

use crate::sdd::memory::{memory_gc, memory_get_full, memory_list, memory_search, memory_set};

pub async fn cmd_memory_list(
    pool: &SqlitePool,
    agent: &str,
    spec: Option<&str>,
    mem_type: Option<&str>,
    limit: Option<i64>,
    json: bool,
) -> Result<()> {
    let entries = memory_list(pool, agent, spec, mem_type, limit, None).await?;

    if json {
        // Emit a JSON array; parse each value field so objects aren't double-encoded.
        let items: Vec<serde_json::Value> = entries
            .iter()
            .map(|m| {
                let parsed_value: serde_json::Value = serde_json::from_str(&m.value)
                    .unwrap_or(serde_json::Value::String(m.value.clone()));
                serde_json::json!({
                    "agent": m.agent,
                    "key": m.key,
                    "spec": if m.spec.is_empty() { None::<&str> } else { Some(m.spec.as_str()) },
                    "type": m.type_,
                    "value": parsed_value,
                    "updated_at": m.updated_at,
                    "revision_count": m.revision_count,
                    "access_count": m.access_count,
                    "expires_at": m.expires_at,
                    "related_to": m.related_to,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }

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
    json: bool,
) -> Result<()> {
    let entry = memory_get_full(pool, agent, key, spec).await?;

    match entry {
        Some(m) => {
            if json {
                let parsed_value: serde_json::Value = serde_json::from_str(&m.value)
                    .unwrap_or(serde_json::Value::String(m.value.clone()));
                let out = serde_json::json!({
                    "agent": m.agent,
                    "key": m.key,
                    "spec": if m.spec.is_empty() { None::<&str> } else { Some(m.spec.as_str()) },
                    "type": m.type_,
                    "value": parsed_value,
                    "updated_at": m.updated_at,
                    "revision_count": m.revision_count,
                    "access_count": m.access_count,
                    "expires_at": m.expires_at,
                    "related_to": m.related_to,
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
                return Ok(());
            }
            println!("{}: {}", "agent".dimmed(), m.agent);
            println!("{}: {}", "key".dimmed(), m.key.cyan());
            println!(
                "{}: {}",
                "spec".dimmed(),
                if m.spec.is_empty() { "-" } else { &m.spec }
            );
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
            if json {
                println!("null");
            } else {
                println!("Not found.");
            }
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
    json: bool,
) -> Result<()> {
    let results = memory_search(pool, agent, query, spec, mem_type, None).await?;

    if json {
        let items: Vec<serde_json::Value> = results
            .iter()
            .map(|m| {
                let parsed_value: serde_json::Value = serde_json::from_str(&m.value)
                    .unwrap_or(serde_json::Value::String(m.value.clone()));
                serde_json::json!({
                    "agent": m.agent,
                    "key": m.key,
                    "spec": if m.spec.is_empty() { None::<&str> } else { Some(m.spec.as_str()) },
                    "type": m.type_,
                    "value": parsed_value,
                    "updated_at": m.updated_at,
                    "revision_count": m.revision_count,
                    "access_count": m.access_count,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
        return Ok(());
    }

    if results.is_empty() {
        println!("No results.");
        return Ok(());
    }

    for m in &results {
        let type_str = m.type_.as_deref().unwrap_or("-");
        println!("{} {} [{}]", "•".dimmed(), m.key.cyan(), type_str.yellow(),);
    }
    println!("\n{} results", results.len());
    Ok(())
}

pub struct MemorySetOpts<'a> {
    pub spec: Option<&'a str>,
    pub mem_type: Option<&'a str>,
    pub ttl: Option<i64>,
    pub related_to: Option<&'a str>,
    pub json: bool,
}

pub async fn cmd_memory_set(
    pool: &SqlitePool,
    agent: &str,
    key: &str,
    value: &str,
    opts: MemorySetOpts<'_>,
) -> Result<()> {
    // Accept value as raw JSON or as a plain string — wrap plain strings automatically.
    let value_json = if serde_json::from_str::<serde_json::Value>(value).is_ok() {
        value.to_string()
    } else {
        serde_json::to_string(value)?
    };

    memory_set(
        pool,
        agent,
        key,
        &value_json,
        opts.spec,
        opts.mem_type,
        opts.ttl,
        opts.related_to,
    )
    .await?;

    if opts.json {
        println!(
            "{}",
            serde_json::json!({"ok": true, "agent": agent, "key": key})
        );
    } else {
        println!(
            "{} memory set: {} / {}",
            "✓".green(),
            agent.cyan(),
            key.yellow()
        );
    }
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
