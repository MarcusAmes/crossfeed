#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeRuleRow {
    pub id: i64,
    pub rule_type: String,
    pub pattern_type: String,
    pub target: String,
    pub pattern: String,
    pub enabled: bool,
    pub created_at: String,
}
