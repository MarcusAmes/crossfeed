use std::ops::Range;

use crate::{FuzzError, FuzzTemplate, Placeholder};

pub fn parse_template(request_bytes: &[u8], prefix: &str) -> Result<FuzzTemplate, FuzzError> {
    let text = String::from_utf8_lossy(request_bytes);
    let mut placeholders = Vec::new();
    let mut search_index = 0;

    while let Some(start) = text[search_index..].find(prefix) {
        let start_index = search_index + start;
        let remainder = &text[start_index..];
        let end_index = remainder
            .find(">>")
            .ok_or_else(|| FuzzError::Template("unterminated placeholder".to_string()))?;
        let token = &remainder[..end_index + 2];
        let index = parse_index(token, prefix)?;
        let byte_start = text[..start_index].as_bytes().len();
        let byte_end = text[..start_index + end_index + 2].as_bytes().len();
        upsert_placeholder(
            &mut placeholders,
            index,
            token.to_string(),
            byte_start..byte_end,
        );
        search_index = start_index + end_index + 2;
    }

    Ok(FuzzTemplate {
        request_bytes: request_bytes.to_vec(),
        placeholders,
    })
}

fn parse_index(token: &str, prefix: &str) -> Result<usize, FuzzError> {
    if !token.starts_with(prefix) {
        return Err(FuzzError::Template(
            "placeholder prefix mismatch".to_string(),
        ));
    }
    let trimmed = token
        .trim_start_matches(prefix)
        .trim_start_matches(':')
        .trim_end_matches(">>");
    let index = trimmed
        .parse::<usize>()
        .map_err(|_| FuzzError::Template("placeholder index must be a number".to_string()))?;
    Ok(index)
}

fn upsert_placeholder(
    placeholders: &mut Vec<Placeholder>,
    index: usize,
    token: String,
    range: Range<usize>,
) {
    if let Some(existing) = placeholders.iter_mut().find(|item| item.index == index) {
        existing.ranges.push(range);
    } else {
        placeholders.push(Placeholder {
            index,
            token,
            ranges: vec![range],
        });
    }
}
