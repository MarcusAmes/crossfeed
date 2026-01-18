use crate::config::{ScopePatternType, ScopeRule, ScopeRuleType, ScopeTarget};

pub fn is_in_scope(rules: &[ScopeRule], host: &str, path: &str) -> bool {
    let mut include_match = false;
    let mut exclude_match = false;

    for rule in rules.iter().filter(|rule| rule.enabled) {
        if matches_rule(rule, host, path) {
            match rule.rule_type {
                ScopeRuleType::Include => include_match = true,
                ScopeRuleType::Exclude => exclude_match = true,
            }
        }
    }

    if exclude_match {
        return false;
    }

    if include_match {
        return true;
    }

    false
}

fn matches_rule(rule: &ScopeRule, host: &str, path: &str) -> bool {
    let target_value = match rule.target {
        ScopeTarget::Host => host,
        ScopeTarget::Path => path,
    };

    match rule.pattern_type {
        ScopePatternType::Wildcard => wildcard_match(&rule.pattern, target_value),
        ScopePatternType::Regex => {
            regex::Regex::new(&rule.pattern)
                .map(|re| re.is_match(target_value))
                .unwrap_or(false)
        }
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let mut pat_iter = pattern.split('*');
    let mut pos = 0;

    if let Some(prefix) = pat_iter.next() {
        if !value.starts_with(prefix) {
            return false;
        }
        pos += prefix.len();
    }

    for part in pat_iter {
        if part.is_empty() {
            continue;
        }
        match value[pos..].find(part) {
            Some(index) => {
                pos += index + part.len();
            }
            None => return false,
        }
    }

    if !pattern.ends_with('*') {
        if let Some(last) = pattern.split('*').last() {
            return value.ends_with(last);
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use crate::config::{ScopePatternType, ScopeRule, ScopeRuleType, ScopeTarget};

    use super::is_in_scope;

    #[test]
    fn exclude_overrides_include() {
        let rules = vec![
            ScopeRule {
                rule_type: ScopeRuleType::Include,
                pattern_type: ScopePatternType::Wildcard,
                target: ScopeTarget::Host,
                pattern: "*.example.com".to_string(),
                enabled: true,
            },
            ScopeRule {
                rule_type: ScopeRuleType::Exclude,
                pattern_type: ScopePatternType::Wildcard,
                target: ScopeTarget::Host,
                pattern: "api.example.com".to_string(),
                enabled: true,
            },
        ];

        assert!(!is_in_scope(&rules, "api.example.com", "/"));
    }

    #[test]
    fn include_matches_when_present() {
        let rules = vec![ScopeRule {
            rule_type: ScopeRuleType::Include,
            pattern_type: ScopePatternType::Wildcard,
            target: ScopeTarget::Host,
            pattern: "*.example.com".to_string(),
            enabled: true,
        }];

        assert!(is_in_scope(&rules, "api.example.com", "/"));
    }

    #[test]
    fn no_rules_means_out_of_scope() {
        let rules = Vec::new();
        assert!(!is_in_scope(&rules, "example.com", "/"));
    }
}
