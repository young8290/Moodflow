//! Real loopback HTTP checks: no external credentials, image files or network services.
use axum::{
    Json, Router,
    http::{StatusCode, header::LOCATION},
    response::{IntoResponse, Response},
    routing::post,
};
use chrono::Local;
use moodflow_hub_core::infra::{
    clock::ManualClock,
    gateway::{CompleteRequest, Message, Scenario},
};
use moodflow_hub_gateway::{ApiKey, GatewayConfig, HttpGateway, InlineImage, LlmConfig};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::mpsc;

fn request() -> CompleteRequest {
    CompleteRequest {
        scenario: Scenario::Chat,
        prompt_ver: "synthetic-tools/1".into(),
        messages: vec![Message {
            role: "user".into(),
            content: "synthetic private input".into(),
        }],
    }
}
async fn serve(
    handler: impl Fn(Value) -> Response + Send + Sync + 'static,
) -> (String, tokio::task::JoinHandle<()>) {
    let handler = Arc::new(handler);
    let router = Router::new().route(
        "/chat/completions",
        post(move |Json(value): Json<Value>| {
            let handler = handler.clone();
            async move { handler(value) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (format!("http://{address}"), handle)
}
fn gateway(
    base: String,
) -> (
    HttpGateway,
    mpsc::UnboundedReceiver<moodflow_hub_core::infra::gateway::NetLogEntry>,
) {
    let mut llm = LlmConfig::new(base);
    llm.models = vec!["selected".into(), "backup".into()];
    llm.api_key = Some(ApiKey::new("synthetic-key-not-real"));
    let (tx, rx) = mpsc::unbounded_channel();
    (
        HttpGateway::new(
            GatewayConfig {
                llm: Some(llm),
                jev: None,
                caps: vec![],
            },
            Arc::new(ManualClock::new(Local::now())),
        )
        .unwrap()
        .with_net_log(tx),
        rx,
    )
}
fn response(model: &str, reason: &str, text: &str) -> Response {
    Json(json!({"model":model,"choices":[{"message":{"content":text},"finish_reason":reason}]}))
        .into_response()
}
#[tokio::test]
async fn images_are_inline_only_selected_model_no_content_in_logs() {
    let captured = Arc::new(Mutex::new(Value::Null));
    let capture = captured.clone();
    let (base, server) = serve(move |value| {
        *capture.lock().unwrap() = value;
        response("selected", "stop", "x^2")
    })
    .await;
    let (gateway, mut logs) = gateway(base);
    let image = InlineImage::from_bytes(b"\x89PNG\r\n\x1a\nsynthetic-transport-fixture").unwrap();
    let result = gateway
        .complete_tool(request(), "selected", &[image])
        .await
        .unwrap();
    assert_eq!(result.text, "x^2");
    let body = captured.lock().unwrap().clone();
    assert_eq!(body["model"], "selected");
    assert_eq!(body["max_tokens"], 2048);
    assert_eq!(body["stream"], false);
    assert_eq!(
        body["messages"][0]["content"][0]["text"],
        "synthetic private input"
    );
    assert!(
        body["messages"][0]["content"][1]["image_url"]["url"]
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,")
    );
    let mut saved = String::new();
    while let Ok(entry) = logs.try_recv() {
        saved.push_str(&serde_json::to_string(&entry).unwrap());
    }
    for private in [
        "synthetic private input",
        "synthetic-key-not-real",
        "data:image/png",
        "x^2",
    ] {
        assert!(!saved.contains(private));
    }
    server.abort();
}
#[tokio::test]
async fn incomplete_or_wrong_model_outputs_are_rejected_without_fallback() {
    for (model, reason, text) in [
        ("other", "stop", "answer"),
        ("selected", "length", "partial"),
        ("selected", "stop", ""),
    ] {
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        let (base, server) = serve(move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
            response(model, reason, text)
        })
        .await;
        let (gateway, _) = gateway(base);
        assert!(
            gateway
                .complete_tool(request(), "selected", &[])
                .await
                .is_err()
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        server.abort();
    }
}
#[tokio::test]
async fn oversized_responses_and_redirects_never_deliver_or_forward_credentials() {
    let hits = Arc::new(AtomicUsize::new(0));
    let seen = hits.clone();
    let (destination, target) = serve(move |_| {
        seen.fetch_add(1, Ordering::SeqCst);
        response("selected", "stop", "unexpected")
    })
    .await;
    let (base, server) = serve(move |_| {
        (
            StatusCode::TEMPORARY_REDIRECT,
            [(LOCATION, format!("{destination}/chat/completions"))],
            "",
        )
            .into_response()
    })
    .await;
    let (gw, _) = gateway(base);
    assert!(gw.complete_tool(request(), "selected", &[]).await.is_err());
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    server.abort();
    target.abort();
    let (base, server) =
        serve(move |_| response("selected", "stop", &"x".repeat(1024 * 1024 + 1))).await;
    let (gw, _) = gateway(base);
    assert!(gw.complete_tool(request(), "selected", &[]).await.is_err());
    server.abort();
    assert!(InlineImage::from_bytes(&[0; 4 * 1024 * 1024 + 1]).is_err());
    assert!(InlineImage::from_bytes(b"https://example.invalid/image.png").is_err());
}
