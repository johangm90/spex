//! Heuristic complexity classifier — the router step the orchestrator uses to
//! pick a workflow tier (and, indirectly, a model tier) for an incoming task.
//!
//! Pure function, no I/O, no LLM call. Explicit signal flags dominate; keyword
//! sniffing of the description is only a fallback when a flag is absent.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    Trivial,
    Standard,
    Complex,
}

#[derive(Debug, Default)]
pub struct ClassifyInput<'a> {
    pub description: &'a str,
    pub files_touched: Option<i64>,
    pub crosses_subsystems: Option<bool>,
    pub public_contract: Option<bool>,
    pub new_user_visible_feature: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Classification {
    pub tier: Tier,
    pub score: i64,
    pub rationale: Vec<String>,
}

const CONTRACT_KW: &[&str] = &[
    "schema",
    "migration",
    "public api",
    "api endpoint",
    "mcp tool",
    "cli command",
    "breaking change",
    "wire format",
    "protocol",
];
const FEATURE_KW: &[&str] = &[
    "add command",
    "new command",
    "new subcommand",
    "new workflow",
    "new integration",
    "new feature",
    "implement ",
];
const SUBSYSTEM_KW: &[&str] = &[
    "end-to-end",
    "across the",
    "cli and mcp",
    "mcp and cli",
    "full stack",
];
const TRIVIAL_KW: &[&str] = &[
    "rename",
    "typo",
    "comment",
    "docstring",
    "readme",
    "formatting",
    "reformat",
    "lint fix",
    "clippy fix",
    "bump version",
];

/// Classify a task into a workflow tier.
pub fn classify(input: &ClassifyInput) -> Classification {
    let desc = input.description.to_lowercase();
    let mut score = 0i64;
    let mut rationale: Vec<String> = Vec::new();

    let contract = input
        .public_contract
        .unwrap_or_else(|| CONTRACT_KW.iter().any(|k| desc.contains(k)));
    if contract {
        score += 3;
        rationale.push("touches a public contract (API / schema / CLI / MCP)".into());
    }

    let feature = input
        .new_user_visible_feature
        .unwrap_or_else(|| FEATURE_KW.iter().any(|k| desc.contains(k)));
    if feature {
        score += 3;
        rationale.push("new user-visible feature".into());
    }

    let crosses = input
        .crosses_subsystems
        .unwrap_or_else(|| SUBSYSTEM_KW.iter().any(|k| desc.contains(k)));
    if crosses {
        score += 2;
        rationale.push("crosses multiple subsystems".into());
    }

    if let Some(n) = input.files_touched {
        if n > 3 {
            score += 1;
            rationale.push(format!("{n} files touched"));
        }
    }

    // De-escalation: an explicitly trivial-sounding change with no positive
    // signal stays trivial.
    if score == 0 && TRIVIAL_KW.iter().any(|k| desc.contains(k)) {
        rationale.push("trivial-scope keywords, no complexity signal".into());
    }

    let tier = if score >= 3 {
        Tier::Complex
    } else if score >= 1 {
        Tier::Standard
    } else {
        Tier::Trivial
    };

    if rationale.is_empty() {
        rationale.push("no complexity signal detected".into());
    }

    Classification {
        tier,
        score,
        rationale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(desc: &str) -> ClassifyInput<'_> {
        ClassifyInput {
            description: desc,
            ..Default::default()
        }
    }

    #[test]
    fn explicit_public_contract_is_complex() {
        let c = classify(&ClassifyInput {
            description: "adjust internal helper",
            public_contract: Some(true),
            ..Default::default()
        });
        assert_eq!(c.tier, Tier::Complex);
        assert!(c.score >= 3);
    }

    #[test]
    fn rename_is_trivial() {
        let c = classify(&input("rename the function get_spec to fetch_spec"));
        assert_eq!(c.tier, Tier::Trivial);
        assert_eq!(c.score, 0);
    }

    #[test]
    fn keyword_new_command_is_complex() {
        let c = classify(&input("add command `spex eval export` to dump scorecards"));
        assert_eq!(c.tier, Tier::Complex);
    }

    #[test]
    fn crosses_only_is_standard() {
        let c = classify(&ClassifyInput {
            description: "refactor the pulse output",
            crosses_subsystems: Some(true),
            ..Default::default()
        });
        assert_eq!(c.tier, Tier::Standard);
        assert_eq!(c.score, 2);
    }

    #[test]
    fn many_files_alone_is_standard() {
        let c = classify(&ClassifyInput {
            description: "tidy imports repo-wide",
            files_touched: Some(9),
            ..Default::default()
        });
        assert_eq!(c.tier, Tier::Standard);
        assert_eq!(c.score, 1);
    }

    #[test]
    fn schema_keyword_bumps_to_complex() {
        let c = classify(&input("refactor the sessions schema"));
        assert_eq!(c.tier, Tier::Complex);
        assert!(c.rationale.iter().any(|r| r.contains("public contract")));
    }
}
