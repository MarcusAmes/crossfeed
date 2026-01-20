use regex::Regex;

use crate::{AnalysisConfig, AnalysisResult, FuzzError};

pub fn analyze_response(body: &[u8], config: &AnalysisConfig) -> Result<AnalysisResult, FuzzError> {
    let text = String::from_utf8_lossy(body);
    let mut grep_matches = Vec::new();
    for needle in &config.grep {
        if text.contains(needle) {
            grep_matches.push(needle.clone());
        }
    }

    let mut extracts = Vec::new();
    for pattern in &config.extract {
        let regex = Regex::new(pattern).map_err(|err| FuzzError::Analysis(err.to_string()))?;
        let mut matches = Vec::new();
        for capture in regex.captures_iter(&text) {
            for idx in 1..capture.len() {
                if let Some(value) = capture.get(idx) {
                    matches.push(value.as_str().to_string());
                }
            }
        }
        extracts.push(matches);
    }

    Ok(AnalysisResult {
        grep_matches,
        extracts,
    })
}
