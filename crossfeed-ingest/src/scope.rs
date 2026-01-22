use std::path::Path;

use crossfeed_proxy::{ScopePatternType, ScopeRule, ScopeRuleType, ScopeTarget, is_in_scope};
use crossfeed_storage::{ScopeRuleRow, SqliteStore};

#[derive(Debug, Clone)]
pub struct ScopeEvaluation {
    pub scope_status_at_capture: String,
    pub scope_rules_version: i64,
    pub capture_filtered: bool,
    pub timeline_filtered: bool,
}

pub fn evaluate_scope(store_path: &Path, host: &str, path: &str) -> Result<ScopeEvaluation, String> {
    let store = SqliteStore::open(store_path)?;
    let rules = store.list_scope_rules()?;
    let scope_rules: Vec<ScopeRule> = rules
        .into_iter()
        .filter_map(map_scope_rule)
        .collect();
    let in_scope = is_in_scope(&scope_rules, host, path);
    let scope_status_at_capture = if in_scope {
        "in_scope"
    } else {
        "out_of_scope"
    };
    Ok(ScopeEvaluation {
        scope_status_at_capture: scope_status_at_capture.to_string(),
        scope_rules_version: scope_rules.len() as i64,
        capture_filtered: true,
        timeline_filtered: true,
    })
}

fn map_scope_rule(row: ScopeRuleRow) -> Option<ScopeRule> {
    let rule_type = match row.rule_type.to_lowercase().as_str() {
        "include" => ScopeRuleType::Include,
        "exclude" => ScopeRuleType::Exclude,
        _ => return None,
    };
    let pattern_type = match row.pattern_type.to_lowercase().as_str() {
        "wildcard" => ScopePatternType::Wildcard,
        "regex" => ScopePatternType::Regex,
        _ => return None,
    };
    let target = match row.target.to_lowercase().as_str() {
        "host" => ScopeTarget::Host,
        "path" => ScopeTarget::Path,
        _ => return None,
    };
    Some(ScopeRule {
        rule_type,
        pattern_type,
        target,
        pattern: row.pattern,
        enabled: row.enabled,
    })
}
