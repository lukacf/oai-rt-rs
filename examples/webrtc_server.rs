#![allow(
    clippy::missing_errors_doc,
    clippy::needless_pass_by_value,
    clippy::print_stdout,
    clippy::print_stderr,
    clippy::result_large_err
)]

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Html;
use axum::routing::{get, post};
use axum::{Json, Router};
use oai_rt_rs::protocol::models::Nullable;
use oai_rt_rs::transport::rest::RealtimeRestAdapter;
use oai_rt_rs::{
    AudioConfig, GPT_REALTIME_2, GPT_REALTIME_TRANSLATE, GPT_REALTIME_WHISPER, InputAudioConfig,
    InputAudioTranscription, OutputAudioConfig, OutputModalities, ReasoningConfig, ReasoningEffort,
    Result, SessionConfig, SessionKind, Tool, ToolChoice, ToolChoiceMode, Voice,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::services::ServeDir;

const INDEX_HTML: &str = include_str!("browser/index.html");
const CHAT_HTML: &str = include_str!("browser/webrtc_chat_tools.html");
const TRANSLATE_HTML: &str = include_str!("browser/webrtc_translate.html");

#[derive(Clone)]
struct AppState {
    rest: RealtimeRestAdapter,
    api_key: String,
    http: reqwest::Client,
    safety_identifier: Option<String>,
    web_search_model: String,
}

#[derive(Debug, Deserialize)]
struct TranslationQuery {
    language: Option<String>,
}

#[derive(Debug, Serialize)]
struct BrowserSecret {
    value: String,
    expires_at: u64,
}

#[derive(Debug, Deserialize)]
struct WebSearchRequest {
    query: String,
}

#[derive(Debug, Serialize)]
struct WebSearchResponse {
    response_text: String,
    citations: Vec<WebSearchCitation>,
}

#[derive(Debug, Serialize)]
struct WebSearchCitation {
    title: String,
    url: String,
}

type AppResult<T> = std::result::Result<T, (StatusCode, String)>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let api_key = std::env::var("OPENAI_API_KEY").map_err(|_| {
        oai_rt_rs::Error::InvalidClientEvent("OPENAI_API_KEY must be set".to_string())
    })?;
    let safety_identifier = std::env::var("OPENAI_SAFETY_IDENTIFIER")
        .ok()
        .filter(|value| !value.trim().is_empty());
    let web_search_model =
        std::env::var("OPENAI_WEB_SEARCH_MODEL").unwrap_or_else(|_| "gpt-5.5".to_string());
    let state = Arc::new(AppState {
        rest: RealtimeRestAdapter::new(&api_key)?,
        api_key,
        http: reqwest::Client::new(),
        safety_identifier,
        web_search_model,
    });

    let app = Router::new()
        .route("/", get(index))
        .route("/webrtc_chat_tools.html", get(chat_page))
        .route("/webrtc_translate.html", get(translate_page))
        .nest_service("/browser", ServeDir::new("examples/browser"))
        .nest_service("/assets", ServeDir::new("examples/browser/assets"))
        .route("/session/voice", get(voice_session))
        .route("/session/translation", get(translation_session))
        .route("/tools/web_search", post(web_search))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("serving WebRTC examples at http://{addr}/");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

async fn chat_page() -> Html<&'static str> {
    Html(CHAT_HTML)
}

async fn translate_page() -> Html<&'static str> {
    Html(TRANSLATE_HTML)
}

async fn voice_session(State(state): State<Arc<AppState>>) -> AppResult<Json<BrowserSecret>> {
    let response = state
        .rest
        .create_client_secret_with_expiry_and_safety_identifier(
            voice_session_config(),
            None,
            state.safety_identifier.as_deref(),
        )
        .await
        .map_err(internal_error)?;
    Ok(Json(BrowserSecret {
        value: response.value,
        expires_at: response.expires_at,
    }))
}

async fn translation_session(
    State(state): State<Arc<AppState>>,
    Query(query): Query<TranslationQuery>,
) -> AppResult<Json<BrowserSecret>> {
    let language = query.language.unwrap_or_else(|| "es".to_string());
    let language = language.trim();
    if language.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "language query parameter must not be empty".to_string(),
        ));
    }
    if language.eq_ignore_ascii_case("se") {
        return Err((
            StatusCode::BAD_REQUEST,
            "use language code 'sv' for Swedish; 'se' is not accepted by the translation endpoint"
                .to_string(),
        ));
    }
    let response = state
        .rest
        .create_translation_client_secret_with_expiry_and_safety_identifier(
            translation_session_config(language),
            None,
            state.safety_identifier.as_deref(),
        )
        .await
        .map_err(internal_error)?;
    Ok(Json(BrowserSecret {
        value: response.value,
        expires_at: response.expires_at,
    }))
}

async fn web_search(
    State(state): State<Arc<AppState>>,
    Json(request): Json<WebSearchRequest>,
) -> AppResult<Json<WebSearchResponse>> {
    let query = request.query.trim();
    if query.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "query must not be empty".to_string(),
        ));
    }

    let mut req = state
        .http
        .post("https://api.openai.com/v1/responses")
        .bearer_auth(&state.api_key)
        .json(&json!({
            "model": state.web_search_model,
            "reasoning": { "effort": "low" },
            "tools": [{ "type": "web_search" }],
            "tool_choice": "auto",
            "include": ["web_search_call.action.sources"],
            "input": format!(
                "Search the web for this voice assistant query and answer concisely with citations: {query}"
            )
        }));
    if let Some(identifier) = &state.safety_identifier {
        req = req.header("OpenAI-Safety-Identifier", identifier);
    }

    let response = req.send().await.map_err(example_http_error)?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_else(|_| String::new());
        return Err((status, body));
    }

    let value = response.json::<Value>().await.map_err(example_http_error)?;
    let response_text = extract_response_text(&value)
        .map(|text| speech_friendly_text(&text))
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| "I found results, but could not extract a concise answer.".to_string());
    Ok(Json(WebSearchResponse {
        citations: extract_citations(&value),
        response_text,
    }))
}

fn internal_error(error: oai_rt_rs::Error) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn example_http_error(error: reqwest::Error) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn voice_session_config() -> SessionConfig {
    let mut config = SessionConfig::new(
        SessionKind::Realtime,
        GPT_REALTIME_2,
        OutputModalities::Audio,
    );
    config.instructions = Some(
        r#"You are a concise voice assistant.

## Preambles
Use short preambles only when they help the user understand that work is happening.
Before web_search, say one brief natural sentence like "I'll look that up now." Then call the tool immediately.
Do not use preambles for simple direct answers or unclear audio.

## Tools
- Use sum for arithmetic with two integers.
- Use web_search when the user asks for current, recent, time-sensitive, factual, or web-dependent information.
- Do not use web_search for stable general knowledge or local app questions.
- If a search request is too vague, ask one short clarifying question instead of searching.
- After web_search returns, answer conversationally in two or three sentences and mention source names naturally."#
            .to_string(),
    );
    config.audio = Some(AudioConfig {
        input: Some(InputAudioConfig {
            transcription: Some(Nullable::Value(InputAudioTranscription {
                model: Some(GPT_REALTIME_WHISPER.to_string()),
                language: None,
                prompt: None,
            })),
            ..InputAudioConfig::default()
        }),
        output: Some(OutputAudioConfig {
            voice: Some(Voice::from("marin")),
            ..OutputAudioConfig::default()
        }),
    });
    config.reasoning = Some(ReasoningConfig {
        effort: Some(ReasoningEffort::Low),
    });
    config.tools = Some(vec![sum_tool(), web_search_tool()]);
    config.tool_choice = Some(ToolChoice::Mode(ToolChoiceMode::Auto));
    config
}

fn translation_session_config(language: &str) -> SessionConfig {
    let mut config = SessionConfig::new(
        SessionKind::Translation,
        GPT_REALTIME_TRANSLATE,
        OutputModalities::Audio,
    );
    config.audio = Some(AudioConfig {
        input: None,
        output: Some(OutputAudioConfig {
            language: Some(language.to_string()),
            ..OutputAudioConfig::default()
        }),
    });
    config
}

fn sum_tool() -> Tool {
    Tool::Function {
        name: "sum".to_string(),
        description: Some("Add two signed integers.".to_string()),
        parameters: json!({
            "type": "object",
            "properties": {
                "a": { "type": "integer" },
                "b": { "type": "integer" }
            },
            "required": ["a", "b"],
            "additionalProperties": false
        }),
    }
}

fn web_search_tool() -> Tool {
    Tool::Function {
        name: "web_search".to_string(),
        description: Some(
            "Search the web for current, recent, time-sensitive, factual, or web-dependent information. Say a short preamble before calling this tool."
                .to_string(),
        ),
        parameters: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "A concise web search query derived from the user's request."
                }
            },
            "required": ["query"],
            "additionalProperties": false
        }),
    }
}

fn extract_response_text(value: &Value) -> Option<String> {
    if let Some(text) = value.get("output_text").and_then(Value::as_str) {
        return Some(text.to_string());
    }

    let output = value.get("output")?.as_array()?;
    let mut text = String::new();
    for item in output {
        let Some(content) = item.get("content").and_then(Value::as_array) else {
            continue;
        };
        for part in content {
            if let Some(part_text) = part.get("text").and_then(Value::as_str) {
                text.push_str(part_text);
            }
        }
    }
    Some(text)
}

fn extract_citations(value: &Value) -> Vec<WebSearchCitation> {
    let Some(output) = value.get("output").and_then(Value::as_array) else {
        return Vec::new();
    };

    let mut citations = Vec::new();
    for item in output {
        let Some(content) = item.get("content").and_then(Value::as_array) else {
            continue;
        };
        for part in content {
            let Some(annotations) = part.get("annotations").and_then(Value::as_array) else {
                continue;
            };
            for annotation in annotations {
                if annotation.get("type").and_then(Value::as_str) != Some("url_citation") {
                    continue;
                }
                let Some(url) = annotation.get("url").and_then(Value::as_str) else {
                    continue;
                };
                let title = annotation
                    .get("title")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty())
                    .unwrap_or(url);
                citations.push(WebSearchCitation {
                    title: title.to_string(),
                    url: url.to_string(),
                });
            }
        }
    }
    citations
}

fn speech_friendly_text(text: &str) -> String {
    strip_markdown_links(text).replace("**", "")
}

fn strip_markdown_links(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut stripped = String::with_capacity(text.len());
    let mut i = 0;

    while i < chars.len() {
        if chars[i] != '[' {
            stripped.push(chars[i]);
            i += 1;
            continue;
        }

        let Some(label_end) = chars[i + 1..]
            .iter()
            .position(|ch| *ch == ']')
            .map(|offset| i + 1 + offset)
        else {
            stripped.push(chars[i]);
            i += 1;
            continue;
        };
        let link_start = label_end + 1;
        if chars.get(link_start) != Some(&'(') {
            stripped.push(chars[i]);
            i += 1;
            continue;
        }
        let Some(link_end) = chars[link_start + 1..]
            .iter()
            .position(|ch| *ch == ')')
            .map(|offset| link_start + 1 + offset)
        else {
            stripped.push(chars[i]);
            i += 1;
            continue;
        };

        stripped.extend(chars[i + 1..label_end].iter());
        i = link_end + 1;
    }

    stripped
}

#[cfg(test)]
mod tests {
    use super::{
        extract_citations, extract_response_text, speech_friendly_text, translation_session_config,
        voice_session_config,
    };
    use oai_rt_rs::{GPT_REALTIME_2, GPT_REALTIME_TRANSLATE, SessionKind, Tool};
    use serde_json::json;

    #[test]
    fn voice_config_uses_realtime_tools_and_transcription() {
        let config = voice_session_config();
        assert_eq!(config.kind, SessionKind::Realtime);
        assert_eq!(config.model, GPT_REALTIME_2);
        assert!(config.tools.as_ref().is_some_and(|tools| !tools.is_empty()));
        assert!(
            config
                .audio
                .and_then(|audio| audio.input)
                .and_then(|input| input.transcription)
                .is_some()
        );
        let tool_names: Vec<&str> = config
            .tools
            .as_ref()
            .into_iter()
            .flatten()
            .filter_map(|tool| match tool {
                Tool::Function { name, .. } => Some(name.as_str()),
                Tool::Mcp(_) => None,
            })
            .collect();
        assert!(tool_names.contains(&"sum"));
        assert!(tool_names.contains(&"web_search"));
    }

    #[test]
    fn translation_config_uses_language_and_translation_kind() {
        let config = translation_session_config("fr");
        assert_eq!(config.kind, SessionKind::Translation);
        assert_eq!(config.model, GPT_REALTIME_TRANSLATE);
        let language = config
            .audio
            .and_then(|audio| audio.output)
            .and_then(|output| output.language);
        assert_eq!(language.as_deref(), Some("fr"));
    }

    #[test]
    fn extracts_web_search_text_and_citations() {
        let value = json!({
            "output": [{
                "type": "message",
                "content": [{
                    "type": "output_text",
                    "text": "The answer is current.",
                    "annotations": [{
                        "type": "url_citation",
                        "title": "Example",
                        "url": "https://example.com"
                    }]
                }]
            }]
        });

        assert_eq!(
            extract_response_text(&value).as_deref(),
            Some("The answer is current.")
        );
        let citations = extract_citations(&value);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0].title, "Example");
        assert_eq!(citations[0].url, "https://example.com");
    }

    #[test]
    fn speech_text_removes_markdown_link_urls() {
        assert_eq!(
            speech_friendly_text("See **OpenAI** ([docs](https://example.com))."),
            "See OpenAI (docs)."
        );
    }
}
