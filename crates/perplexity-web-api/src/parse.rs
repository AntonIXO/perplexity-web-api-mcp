use crate::error::{Error, Result};
use crate::types::{SearchEvent, SearchWebResult};
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
/// Tries the FINAL step inside the "text" steps array first, then falls back
/// to the top-level "answer" field (which carries no web_results).
fn extract_answer_and_web_results(
    content: &Map<String, Value>,
) -> (Option<String>, Vec<SearchWebResult>) {
    if let Some(result) = extract_from_final_step(content) {
        return result;
    }
    (extract_string(content, "answer"), Vec::new())
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

/// Extracts a thread UUID or slug from a Perplexity search/chat URL or ID string.
///
/// Handles URLs such as:
/// - `https://www.perplexity.ai/search/3b060972-a099-4264-b392-096fb2a3bdf9`
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

    let segment = if clean.starts_with("http://")
        || clean.starts_with("https://")
        || clean.contains("perplexity.ai")
    {
        clean.rsplit('/').next().unwrap_or(clean)
    } else {
        clean
    };

    if let Some(uuid) = extract_uuid_from_str(segment) { uuid } else { segment.to_string() }
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
                "https://www.perplexity.ai/page/some-custom-page-slug"
            ),
            "some-custom-page-slug"
        );
    }
}
