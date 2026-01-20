use async_stream::try_stream;

use crate::{
    AnalysisConfig, FuzzError, FuzzResult, FuzzRunConfig, FuzzTemplate, PlaceholderSpec,
    analyze_response, apply_transform_pipeline, payload_to_bytes,
};
use crossfeed_storage::{TimelineRequest, TimelineResponse};

pub fn expand_fuzz_requests(
    template: &FuzzTemplate,
    specs: &[PlaceholderSpec],
) -> Result<Vec<Vec<u8>>, FuzzError> {
    let mut requests = Vec::new();
    let mut by_index = specs.to_vec();
    by_index.sort_by_key(|spec| spec.index);

    let mut expanded = vec![template.request_bytes.clone()];
    for spec in &by_index {
        let mut next = Vec::new();
        for request in &expanded {
            for payload in &spec.payloads {
                let mut value = payload_to_bytes(payload);
                if let Some(prefix) = &spec.prefix {
                    let mut prefixed = prefix.clone();
                    prefixed.extend_from_slice(&value);
                    value = prefixed;
                }
                if let Some(suffix) = &spec.suffix {
                    let mut suffixed = value;
                    suffixed.extend_from_slice(suffix);
                    value = suffixed;
                }
                let value = apply_transform_pipeline(&value, &spec.transforms)?;
                let replaced = replace_placeholder(request, &template, spec.index, &value)?;
                next.push(replaced);
            }
        }
        expanded = next;
    }

    requests.extend(expanded);
    Ok(requests)
}

fn replace_placeholder(
    request: &[u8],
    template: &FuzzTemplate,
    index: usize,
    value: &[u8],
) -> Result<Vec<u8>, FuzzError> {
    let placeholder = template
        .placeholders
        .iter()
        .find(|item| item.index == index)
        .ok_or_else(|| FuzzError::Template(format!("missing placeholder {index}")))?;

    let mut output = Vec::new();
    let mut cursor = 0;
    let mut ranges = placeholder.ranges.clone();
    ranges.sort_by_key(|range| range.start);
    for range in ranges {
        output.extend_from_slice(&request[cursor..range.start]);
        output.extend_from_slice(value);
        cursor = range.end;
    }
    output.extend_from_slice(&request[cursor..]);
    Ok(output)
}

pub fn run_fuzz<'a, I>(
    template: &'a FuzzTemplate,
    specs: &'a [PlaceholderSpec],
    analysis: &'a AnalysisConfig,
    config: &'a FuzzRunConfig,
    mut sender: impl FnMut(TimelineRequest, TimelineResponse) -> Result<i64, FuzzError> + 'a,
    responses: I,
) -> impl futures_core::Stream<Item = Result<FuzzResult, FuzzError>> + 'a
where
    I: IntoIterator<Item = (TimelineRequest, TimelineResponse)> + 'a,
{
    try_stream! {
        let _ = config;
        let _ = template;
        let _ = specs;
        for (request, response) in responses {
            let body = response.response_body.clone();
            let analysis_result = analyze_response(&body, analysis)?;
            let timeline_request_id = sender(request, response)?;
            yield FuzzResult { timeline_request_id, analysis: analysis_result };
        }
    }
}
