use crate::error::{Error, Result};
use crate::types::{SearchEvent, SearchResponse, SearchWebResult};
use serde::Deserialize;
use serde_json::{Map, Value};

/// A step in the Perplexity response "text" array.
#[derive(Deserialize)]
struct TextStep {
    step_type: String,
    #[serde(default)]
    content: StepContent,
}

/// Content of a single response step.
#[derive(Deserialize, Default)]
struct StepContent {
    /// For FINAL steps, a JSON-encoded string containing the answer and web_results.
    answer: Option<String>,
}

/// The decoded payload of a FINAL step's "answer" JSON string.
#[derive(Deserialize)]
struct FinalAnswerData {
    answer: Option<String>,
    #[serde(default)]
    web_results: Vec<SearchWebResult>,
}

/// Parses an SSE event JSON string into a SearchEvent.
pub(crate) fn parse_sse_event(json_str: &str) -> Result<SearchEvent> {
    let mut content: Map<String, Value> =
        serde_json::from_str(json_str).map_err(Error::Json)?;

    // If the "text" field is a JSON string, expand it in-place so the full
    // parsed structure is available in `raw`.
    expand_text_field(&mut content);

    let (answer, web_results) = extract_answer_and_web_results(&content);
    let backend_uuid = extract_string(&content, "backend_uuid");
    let attachments = extract_string_array(&content, "attachments");
    let raw = Value::Object(content);

    Ok(SearchEvent { answer, web_results, backend_uuid, attachments, raw })
}

/// If the "text" field is a JSON string, replace it with the parsed value.
fn expand_text_field(content: &mut Map<String, Value>) {
    let parsed = match content.get("text").and_then(|v| v.as_str()) {
        Some(s) => serde_json::from_str::<Value>(s).ok(),
        None => None,
    };
    if let Some(v) = parsed {
        content.insert("text".to_string(), v);
    }
}

/// Extracts answer and web_results from the event content.
///
/// Tries the FINAL step inside the "text" steps array first (the older API
/// shape), then the "blocks" array (the shape Perplexity switched to circa
/// 2026-07), then falls back to the top-level "answer" field (no
/// web_results).
fn extract_answer_and_web_results(
    content: &Map<String, Value>,
) -> (Option<String>, Vec<SearchWebResult>) {
    if let Some(result) = extract_from_final_step(content) {
        return result;
    }
    if let Some(result) = extract_from_blocks(content) {
        return result;
    }
    (extract_string(content, "answer"), Vec::new())
}

/// Extracts answer and web_results from the "blocks" array. Each block is
/// either a `markdown_block` (the answer text, appended to across SSE
/// events) or a `web_result_block` (the search results); other block kinds
/// are ignored.
fn extract_from_blocks(
    content: &Map<String, Value>,
) -> Option<(Option<String>, Vec<SearchWebResult>)> {
    let blocks = content.get("blocks")?.as_array()?;
    if blocks.is_empty() {
        return None;
    }

    let mut answer = None;
    let mut web_results = Vec::new();

    for block in blocks {
        if let Some(text) = block
            .get("markdown_block")
            .and_then(|md| md.get("answer"))
            .and_then(|v| v.as_str())
        {
            answer = Some(text.to_string());
        }
        if let Some(results) =
            block.get("web_result_block").and_then(|b| b.get("web_results"))
        {
            if let Ok(parsed) = serde_json::from_value::<Vec<SearchWebResult>>(results.clone()) {
                web_results = parsed;
            }
        }
    }

    if answer.is_none() && web_results.is_empty() {
        return None;
    }
    Some((answer, web_results))
}

/// Deserializes the "text" steps array and pulls answer + web_results from the
/// FINAL step. Returns `None` when no FINAL step exists or parsing fails.
fn extract_from_final_step(
    content: &Map<String, Value>,
) -> Option<(Option<String>, Vec<SearchWebResult>)> {
    let text_value = content.get("text")?;

    let steps: Vec<TextStep> = serde_json::from_value(text_value.clone()).ok()?;

    let final_step = steps.into_iter().find(|s| s.step_type == "FINAL")?;
    let answer_json = final_step.content.answer?;

    let data: FinalAnswerData = serde_json::from_str(&answer_json).ok()?;
    Some((data.answer, data.web_results))
}

/// Extracts a string value from the content map.
fn extract_string(content: &Map<String, Value>, key: &str) -> Option<String> {
    content.get(key).and_then(|v| v.as_str()).map(str::to_owned)
}

/// Extracts an array of strings from the content map.
fn extract_string_array(content: &Map<String, Value>, key: &str) -> Vec<String> {
    content
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

/// Parses a direct thread REST API response JSON into a SearchResponse.
pub fn parse_thread_response(json_val: &Value) -> Result<SearchResponse> {
    let obj = json_val.as_object().ok_or_else(|| Error::Custom("Invalid JSON response".to_string()))?;

    let mut answer_parts = Vec::new();
    let mut web_results = Vec::new();

    // Check for messages / entries array
    if let Some(entries) = obj.get("entries").or_else(|| obj.get("messages")).and_then(|v| v.as_array()) {
        for (idx, entry) in entries.iter().enumerate() {
            if let Some(query) = entry.get("query_str").or_else(|| entry.get("question")).and_then(|v| v.as_str()) {
                answer_parts.push(format!("### Turn {}\n**User:** {}\n", idx + 1, query));
            }
            if let Some(ans) = entry.get("answer").and_then(|v| v.as_str()) {
                answer_parts.push(format!("**Answer:** {}\n", ans));
            }
        }
    } else {
        // Fall back to top-level answer/text extraction
        let (ans, wr) = extract_answer_and_web_results(obj);
        if let Some(a) = ans {
            answer_parts.push(a);
        }
        web_results = wr;
    }

    let backend_uuid = extract_string(obj, "backend_uuid")
        .or_else(|| extract_string(obj, "uuid"))
        .or_else(|| extract_string(obj, "id"));

    let attachments = extract_string_array(obj, "attachments");

    let full_answer = if answer_parts.is_empty() {
        None
    } else {
        Some(answer_parts.join("\n"))
    };

    Ok(SearchResponse {
        answer: full_answer,
        web_results,
        follow_up: crate::types::FollowUpContext {
            backend_uuid,
            attachments,
        },
        raw: json_val.clone(),
    })
}

/// Extracts a thread UUID or slug from a Perplexity search/chat/computer task URL or ID string.
///
/// Handles URLs such as:
/// - `https://www.perplexity.ai/search/3b060972-a099-4264-b392-096fb2a3bdf9`
/// - `https://www.perplexity.ai/computer/tasks/69199f18-ff90-4db6-ba24-adccd51abc74`
/// - `https://www.perplexity.ai/search/some-topic-3b060972-a099-4264-b392-096fb2a3bdf9?s=u`
/// - `https://www.perplexity.ai/page/some-page-slug`
/// - Raw UUID: `3b060972-a099-4264-b392-096fb2a3bdf9`
pub fn extract_thread_uuid_or_slug(url_or_id: &str) -> String {
    let trimmed = url_or_id.trim();
    let clean = trimmed
        .split('#')
        .next()
        .unwrap_or(trimmed)
        .split('?')
        .next()
        .unwrap_or(trimmed)
        .trim_end_matches('/');

    if let Some(uuid) = extract_uuid_from_str(clean) {
        return uuid;
    }

    let segment = if clean.starts_with("http://")
        || clean.starts_with("https://")
        || clean.contains("perplexity.ai")
    {
        clean.rsplit('/').next().unwrap_or(clean)
    } else {
        clean
    };

    segment.to_string()
}

fn extract_uuid_from_str(s: &str) -> Option<String> {
    static UUID_RE: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();
    let re = UUID_RE.get_or_init(|| {
        regex_lite::Regex::new(
            r"(?i)[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}",
        )
        .expect("valid regex")
    });

    re.find(s).map(|m| m.as_str().to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_event() {
        let json = r#"{"answer": "Hello world"}"#;
        let event = parse_sse_event(json).unwrap();

        assert_eq!(event.answer, Some("Hello world".to_string()));
        assert!(event.web_results.is_empty());
        assert!(event.backend_uuid.is_none());
        assert!(event.attachments.is_empty());
    }

    #[test]
    fn test_parse_event_with_backend_uuid() {
        let json = r#"{"answer": "Test", "backend_uuid": "abc-123"}"#;
        let event = parse_sse_event(json).unwrap();

        assert_eq!(event.answer, Some("Test".to_string()));
        assert_eq!(event.backend_uuid, Some("abc-123".to_string()));
    }

    #[test]
    fn test_parse_event_with_attachments() {
        let json = r#"{"answer": "Test", "attachments": ["url1", "url2"]}"#;
        let event = parse_sse_event(json).unwrap();

        assert_eq!(event.attachments, vec!["url1", "url2"]);
    }

    #[test]
    fn test_parse_event_with_nested_text_json() {
        // Simulates the "text" field containing JSON string with steps
        let inner_answer = r#"{"answer": "Nested answer", "web_results": [{"name": "Source", "url": "https://example.com", "snippet": "Example"}]}"#;
        let text_content = serde_json::json!([
            {
                "step_type": "SEARCH",
                "content": {}
            },
            {
                "step_type": "FINAL",
                "content": {
                    "answer": inner_answer
                }
            }
        ]);
        let text_str = serde_json::to_string(&text_content).unwrap();

        let json = serde_json::json!({
            "text": text_str,
            "some_field": "value"
        });

        let event = parse_sse_event(&json.to_string()).unwrap();

        assert_eq!(event.answer, Some("Nested answer".to_string()));
        assert_eq!(event.web_results.len(), 1);
        assert_eq!(event.web_results[0].name, "Source");
        assert_eq!(event.web_results[0].url, "https://example.com");
        assert_eq!(event.web_results[0].snippet, "Example");
        // The "text" field should be parsed and stored in raw
        assert!(event.raw.get("text").is_some());
        assert!(event.raw.get("some_field").is_some());
    }

    #[test]
    fn test_parse_event_with_blocks_shape() {
        // Perplexity's current response shape (observed 2026-07): no "text"
        // field at all, just a "blocks" array with markdown_block and
        // web_result_block entries.
        let json = serde_json::json!({
            "blocks": [
                {
                    "intended_usage": "ask_text",
                    "markdown_block": {
                        "progress": "DONE",
                        "chunks": [],
                        "chunk_starting_offset": 0,
                        "answer": "Paris."
                    }
                },
                {
                    "intended_usage": "web_results",
                    "web_result_block": {
                        "progress": "DONE",
                        "web_results": [
                            {
                                "name": "Capital of France",
                                "url": "https://example.com/paris",
                                "snippet": "Paris is the capital of France.",
                                "timestamp": "2026-01-01T00:00:00",
                                "trust": true
                            }
                        ]
                    }
                }
            ],
            "final": true
        });

        let event = parse_sse_event(&json.to_string()).unwrap();

        assert_eq!(event.answer, Some("Paris.".to_string()));
        assert_eq!(event.web_results.len(), 1);
        assert_eq!(event.web_results[0].name, "Capital of France");
        assert_eq!(event.web_results[0].url, "https://example.com/paris");
    }

    #[test]
    fn test_parse_event_blocks_shape_no_web_results() {
        // A blocks-shape event with only a markdown_block (no web results
        // surfaced yet, e.g. an early progress event in the stream).
        let json = serde_json::json!({
            "blocks": [
                {
                    "intended_usage": "ask_text",
                    "markdown_block": {
                        "progress": "INPROGRESS",
                        "chunks": [],
                        "chunk_starting_offset": 0,
                        "answer": "The boiling point of water is 100"
                    }
                }
            ]
        });

        let event = parse_sse_event(&json.to_string()).unwrap();

        assert_eq!(
            event.answer,
            Some("The boiling point of water is 100".to_string())
        );
        assert!(event.web_results.is_empty());
    }

    #[test]
    fn test_parse_event_fallback_to_top_level() {
        // When text doesn't contain FINAL step, fall back to top-level
        let text_content = serde_json::json!([
            {
                "step_type": "SEARCH",
                "content": {}
            }
        ]);
        let text_str = serde_json::to_string(&text_content).unwrap();

        let json = serde_json::json!({
            "text": text_str,
            "answer": "Top level answer"
        });

        let event = parse_sse_event(&json.to_string()).unwrap();

        assert_eq!(event.answer, Some("Top level answer".to_string()));
        assert!(event.web_results.is_empty());
    }

    #[test]
    fn test_parse_event_raw_contains_all_keys() {
        let json = r#"{
            "answer": "Test",
            "backend_uuid": "uuid",
            "attachments": [],
            "extra_field": "should be in raw",
            "another": 123
        }"#;
        let event = parse_sse_event(json).unwrap();

        // All keys, including extracted ones, are present in raw
        assert!(event.raw.get("answer").is_some());
        assert!(event.raw.get("backend_uuid").is_some());
        assert!(event.raw.get("attachments").is_some());
        assert!(event.raw.get("extra_field").is_some());
        assert!(event.raw.get("another").is_some());
    }

    #[test]
    fn test_parse_event_empty_fields() {
        let json = r#"{}"#;
        let event = parse_sse_event(json).unwrap();

        assert!(event.answer.is_none());
        assert!(event.web_results.is_empty());
        assert!(event.backend_uuid.is_none());
        assert!(event.attachments.is_empty());
    }

    #[test]
    fn test_parse_invalid_json() {
        let result = parse_sse_event("not json");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_thread_uuid_or_slug() {
        assert_eq!(
            extract_thread_uuid_or_slug(
                "https://www.perplexity.ai/search/3b060972-a099-4264-b392-096fb2a3bdf9"
            ),
            "3b060972-a099-4264-b392-096fb2a3bdf9"
        );
        assert_eq!(
            extract_thread_uuid_or_slug(
                "https://www.perplexity.ai/search/some-topic-3b060972-a099-4264-b392-096fb2a3bdf9?s=u#top"
            ),
            "3b060972-a099-4264-b392-096fb2a3bdf9"
        );
        assert_eq!(
            extract_thread_uuid_or_slug("3b060972-a099-4264-b392-096fb2a3bdf9"),
            "3b060972-a099-4264-b392-096fb2a3bdf9"
        );
        assert_eq!(
            extract_thread_uuid_or_slug(
                "https://www.perplexity.ai/computer/tasks/69199f18-ff90-4db6-ba24-adccd51abc74"
            ),
            "69199f18-ff90-4db6-ba24-adccd51abc74"
        );
        assert_eq!(
            extract_thread_uuid_or_slug(
                "https://www.perplexity.ai/page/some-custom-page-slug"
            ),
            "some-custom-page-slug"
        );
    }
}
