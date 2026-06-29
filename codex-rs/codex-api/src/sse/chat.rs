use crate::common::ResponseEvent;
use crate::common::ResponseStream;
use crate::error::ApiError;
use crate::rate_limits::parse_all_rate_limits;
use crate::telemetry::SseTelemetry;
use codex_client::ByteStream;
use codex_client::StreamResponse;
use codex_protocol::models::ContentItem;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio::time::timeout;
use tracing::debug;
use tracing::trace;

const OPENAI_MODEL_HEADER: &str = "openai-model";
const REQUEST_ID_HEADER: &str = "x-request-id";

/// Spawns a task that drives an OpenAI Chat Completions SSE byte stream and emits
/// the canonical [`ResponseEvent`] sequence on an mpsc channel, returning a
/// [`ResponseStream`] that mirrors the Responses API path exactly so that
/// downstream consumers are unchanged.
pub fn spawn_chat_stream(
    stream_response: StreamResponse,
    idle_timeout: Duration,
    telemetry: Option<Arc<dyn SseTelemetry>>,
) -> ResponseStream {
    let rate_limit_snapshots = parse_all_rate_limits(&stream_response.headers);
    let server_model = stream_response
        .headers
        .get(OPENAI_MODEL_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(ToString::to_string);
    let upstream_request_id = stream_response
        .headers
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    let (tx_event, rx_event) = mpsc::channel::<Result<ResponseEvent, ApiError>>(1600);
    tokio::spawn(async move {
        if let Some(model) = server_model {
            let _ = tx_event.send(Ok(ResponseEvent::ServerModel(model))).await;
        }
        for snapshot in rate_limit_snapshots {
            let _ = tx_event.send(Ok(ResponseEvent::RateLimits(snapshot))).await;
        }
        process_chat_sse(stream_response.bytes, tx_event, idle_timeout, telemetry).await;
    });

    ResponseStream {
        rx_event,
        upstream_request_id,
    }
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChunk {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    #[serde(default)]
    delta: ChatDelta,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ChatDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ChatToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct ChatToolCallDelta {
    #[serde(default)]
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<ChatFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct ChatFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatUsage {
    #[serde(default)]
    prompt_tokens: i64,
    #[serde(default)]
    completion_tokens: i64,
    #[serde(default)]
    total_tokens: i64,
    #[serde(default)]
    prompt_tokens_details: Option<ChatPromptTokensDetails>,
    #[serde(default)]
    completion_tokens_details: Option<ChatCompletionTokensDetails>,
}

#[derive(Debug, Default, Deserialize)]
struct ChatPromptTokensDetails {
    #[serde(default)]
    cached_tokens: i64,
}

#[derive(Debug, Default, Deserialize)]
struct ChatCompletionTokensDetails {
    #[serde(default)]
    reasoning_tokens: i64,
}

impl From<ChatUsage> for TokenUsage {
    fn from(val: ChatUsage) -> Self {
        TokenUsage {
            input_tokens: val.prompt_tokens,
            cached_input_tokens: val
                .prompt_tokens_details
                .map(|d| d.cached_tokens)
                .unwrap_or(0),
            output_tokens: val.completion_tokens,
            reasoning_output_tokens: val
                .completion_tokens_details
                .map(|d| d.reasoning_tokens)
                .unwrap_or(0),
            total_tokens: val.total_tokens,
        }
    }
}

/// Accumulates streamed tool-call fragments keyed by `choices[].delta.tool_calls[].index`.
#[derive(Debug, Default)]
struct ToolCallAccumulator {
    id: Option<String>,
    name: String,
    arguments: String,
}

#[derive(Default)]
struct ChatStreamState {
    response_id: Option<String>,
    assistant_text: String,
    tool_calls: BTreeMap<usize, ToolCallAccumulator>,
    token_usage: Option<TokenUsage>,
    saw_created: bool,
    completed: bool,
}

async fn process_chat_sse(
    stream: ByteStream,
    tx_event: mpsc::Sender<Result<ResponseEvent, ApiError>>,
    idle_timeout: Duration,
    telemetry: Option<Arc<dyn SseTelemetry>>,
) {
    let mut stream = stream.eventsource();
    let mut state = ChatStreamState::default();

    loop {
        let start = Instant::now();
        let response = timeout(idle_timeout, stream.next()).await;
        if let Some(t) = telemetry.as_ref() {
            t.on_sse_poll(&response, start.elapsed());
        }
        let sse = match response {
            Ok(Some(Ok(sse))) => sse,
            Ok(Some(Err(e))) => {
                debug!("Chat SSE Error: {e:#}");
                let _ = tx_event.send(Err(ApiError::Stream(e.to_string()))).await;
                return;
            }
            Ok(None) => {
                // Stream closed. Flush whatever we have and synthesize Completed.
                if !state.completed {
                    let _ = flush_completion(&tx_event, &mut state).await;
                }
                return;
            }
            Err(_) => {
                let _ = tx_event
                    .send(Err(ApiError::Stream(
                        "idle timeout waiting for SSE".into(),
                    )))
                    .await;
                return;
            }
        };

        let data = sse.data.trim();
        trace!("Chat SSE event: {data}");

        if data == "[DONE]" {
            if !state.completed {
                if flush_completion(&tx_event, &mut state).await.is_err() {
                    return;
                }
            }
            return;
        }

        let chunk: ChatCompletionChunk = match serde_json::from_str(data) {
            Ok(chunk) => chunk,
            Err(e) => {
                // Surface upstream error payloads (e.g. `{"error": {...}}`).
                if let Some(err) = parse_error_payload(data) {
                    let _ = tx_event.send(Err(err)).await;
                    return;
                }
                debug!("Failed to parse Chat SSE event: {e}, data: {data}");
                continue;
            }
        };

        if !state.saw_created {
            state.saw_created = true;
            if tx_event.send(Ok(ResponseEvent::Created {})).await.is_err() {
                return;
            }
        }

        if let Some(id) = chunk.id {
            if state.response_id.is_none() {
                state.response_id = Some(id);
            }
        }

        if let Some(usage) = chunk.usage {
            state.token_usage = Some(usage.into());
        }

        for choice in chunk.choices {
            if let Some(content) = choice.delta.content
                && !content.is_empty()
            {
                state.assistant_text.push_str(&content);
                if tx_event
                    .send(Ok(ResponseEvent::OutputTextDelta(content)))
                    .await
                    .is_err()
                {
                    return;
                }
            }

            if let Some(tool_calls) = choice.delta.tool_calls {
                for tc in tool_calls {
                    let entry = state.tool_calls.entry(tc.index).or_default();
                    if let Some(id) = tc.id {
                        entry.id = Some(id);
                    }
                    if let Some(function) = tc.function {
                        if let Some(name) = function.name {
                            entry.name = name;
                        }
                        if let Some(arguments) = function.arguments {
                            let item_id = entry
                                .id
                                .clone()
                                .unwrap_or_else(|| format!("tool_call_{}", tc.index));
                            entry.arguments.push_str(&arguments);
                            if tx_event
                                .send(Ok(ResponseEvent::ToolCallInputDelta {
                                    item_id: item_id.clone(),
                                    call_id: Some(item_id),
                                    delta: arguments,
                                }))
                                .await
                                .is_err()
                            {
                                return;
                            }
                        }
                    }
                }
            }

            match choice.finish_reason.as_deref() {
                Some("tool_calls") => {
                    if flush_tool_calls(&tx_event, &mut state).await.is_err() {
                        return;
                    }
                }
                Some("stop") | Some("length") => {
                    if flush_assistant_message(&tx_event, &mut state)
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                _ => {}
            }
        }
    }
}

async fn flush_tool_calls(
    tx_event: &mpsc::Sender<Result<ResponseEvent, ApiError>>,
    state: &mut ChatStreamState,
) -> Result<(), ()> {
    let tool_calls = std::mem::take(&mut state.tool_calls);
    for (index, acc) in tool_calls {
        let call_id = acc.id.unwrap_or_else(|| format!("tool_call_{index}"));
        let item = ResponseItem::FunctionCall {
            id: None,
            name: acc.name,
            namespace: None,
            arguments: acc.arguments,
            call_id,
            internal_chat_message_metadata_passthrough: None,
        };
        tx_event
            .send(Ok(ResponseEvent::OutputItemDone(item)))
            .await
            .map_err(|_| ())?;
    }
    Ok(())
}

async fn flush_assistant_message(
    tx_event: &mpsc::Sender<Result<ResponseEvent, ApiError>>,
    state: &mut ChatStreamState,
) -> Result<(), ()> {
    if state.assistant_text.is_empty() {
        return Ok(());
    }
    let text = std::mem::take(&mut state.assistant_text);
    let item = ResponseItem::Message {
        id: None,
        role: "assistant".to_string(),
        content: vec![ContentItem::OutputText { text }],
        phase: None,
        internal_chat_message_metadata_passthrough: None,
    };
    tx_event
        .send(Ok(ResponseEvent::OutputItemDone(item)))
        .await
        .map_err(|_| ())
}

/// Emits any buffered tool calls / assistant message, then the terminal
/// [`ResponseEvent::Completed`].
async fn flush_completion(
    tx_event: &mpsc::Sender<Result<ResponseEvent, ApiError>>,
    state: &mut ChatStreamState,
) -> Result<(), ()> {
    flush_tool_calls(tx_event, state).await?;
    flush_assistant_message(tx_event, state).await?;
    let response_id = state
        .response_id
        .clone()
        .unwrap_or_else(|| "chatcmpl".to_string());
    state.completed = true;
    tx_event
        .send(Ok(ResponseEvent::Completed {
            response_id,
            token_usage: state.token_usage.take(),
            end_turn: Some(true),
        }))
        .await
        .map_err(|_| ())
}

fn parse_error_payload(data: &str) -> Option<ApiError> {
    #[derive(Deserialize)]
    struct ErrorEnvelope {
        error: ErrorBody,
    }
    #[derive(Deserialize)]
    struct ErrorBody {
        #[serde(default)]
        code: Option<String>,
        #[serde(default)]
        message: Option<String>,
    }

    let envelope: ErrorEnvelope = serde_json::from_str(data).ok()?;
    let code = envelope.error.code.as_deref();
    let message = envelope.error.message.unwrap_or_default();
    let error = match code {
        Some("context_length_exceeded") => ApiError::ContextWindowExceeded,
        Some("insufficient_quota") => ApiError::QuotaExceeded,
        Some("rate_limit_exceeded") => ApiError::Retryable {
            message,
            delay: None,
        },
        _ => ApiError::Stream(message),
    };
    Some(error)
}
