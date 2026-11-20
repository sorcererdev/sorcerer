#![allow(clippy::too_many_lines)]
use std::time::Duration;

use mockito::Matcher;
use serde_json::json;
use sorcerer_core::Cancel;
use sorcerer_core::adapters::env::MapEnv;
use sorcerer_core::domain::config::ProviderConfig;
use sorcerer_models::Error;
use sorcerer_models::adapters::anthropic::Anthropic;
use sorcerer_models::domain::model::{
    Completion, Content, Message, ModelId, Role, StopReason, ToolCall, ToolSpec,
};
use sorcerer_models::ports::provider::Provider;

fn provider(url: &str) -> Anthropic {
    let cfg = ProviderConfig {
        url: Some(url.into()),
        ..Default::default()
    };
    Anthropic::from_config(
        &cfg,
        &MapEnv::default().var_set("ANTHROPIC_API_KEY", "sk-test"),
    )
    .unwrap()
    .with_backoff(Duration::ZERO)
}

fn req(tools: Vec<ToolSpec>, messages: Vec<Message>) -> Completion {
    Completion {
        model: ModelId("claude-sonnet-5-5".into()),
        system: "be brief".into(),
        messages,
        tools,
        max_tokens: 100,
        timeout: Duration::from_secs(5),
    }
}

const REPLY: &str = r#"{"content":[{"type":"text","text":"hello"},{"type":"tool_use","id":"t1","name":"read","input":{"path":"a.py"}}],"stop_reason":"tool_use","usage":{"input_tokens":1000,"output_tokens":100}}"#;

#[test]
fn sends_one_messages_request_with_headers() {
    let mut s = mockito::Server::new();
    let m = s
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "sk-test")
        .match_header("anthropic-version", "2023-06-01")
        .match_header("content-type", "application/json")
        .match_body(Matcher::Json(json!({"model":"claude-sonnet-5-5","system":"be brief","max_tokens":100,"messages":[{"role":"user","content":[{"type":"text","text":"q"}]}]})))
        .with_body(REPLY)
        .create();
    provider(&s.url())
        .complete(req(vec![], vec![Message::user("q")]), &Cancel::default())
        .unwrap();
    m.assert();
}

#[test]
fn tools_are_sent_with_input_schema() {
    let mut s = mockito::Server::new();
    let m = s
        .mock("POST", "/v1/messages")
        .match_body(Matcher::PartialJson(json!({"tools":[{"name":"read","description":"read a file","input_schema":{"type":"object"}}]})))
        .with_body(REPLY)
        .create();
    let tool = ToolSpec {
        name: "read".into(),
        description: "read a file".into(),
        schema: json!({"type":"object"}),
    };
    provider(&s.url())
        .complete(
            req(vec![tool], vec![Message::user("q")]),
            &Cancel::default(),
        )
        .unwrap();
    m.assert();
}

#[test]
fn reads_text_tool_calls_usage_and_prices_it() {
    let mut s = mockito::Server::new();
    s.mock("POST", "/v1/messages").with_body(REPLY).create();
    let r = provider(&s.url())
        .complete(req(vec![], vec![Message::user("q")]), &Cancel::default())
        .unwrap();
    assert_eq!(r.text, "hello");
    assert_eq!(
        r.tool_calls,
        vec![ToolCall {
            id: "t1".into(),
            name: "read".into(),
            input: json!({"path":"a.py"})
        }]
    );
    assert_eq!(r.stop, StopReason::ToolUse);
    assert_eq!((r.usage.input_tokens, r.usage.output_tokens), (1000, 100));
    // sonnet 5.5: $2 in, $10 out per million tokens
    assert!(
        (r.cost_usd.unwrap() - 0.003).abs() < 1e-12,
        "{:?}",
        r.cost_usd
    );
}

#[test]
fn a_tool_result_is_sent_as_a_user_tool_result_block() {
    let mut s = mockito::Server::new();
    let m = s
        .mock("POST", "/v1/messages")
        .match_body(Matcher::PartialJson(json!({"messages":[
            {"role":"user","content":[{"type":"text","text":"q"}]},
            {"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"read","input":{"path":"a.py"}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"1\tx = 1"}]}
        ]})))
        .with_body(REPLY)
        .create();
    let call = ToolCall {
        id: "t1".into(),
        name: "read".into(),
        input: json!({"path":"a.py"}),
    };
    let msgs = vec![
        Message::user("q"),
        Message {
            role: Role::Assistant,
            content: vec![Content::ToolCall(call)],
        },
        Message {
            role: Role::User,
            content: vec![Content::ToolResult {
                id: "t1".into(),
                text: "1\tx = 1".into(),
            }],
        },
    ];
    provider(&s.url())
        .complete(req(vec![], msgs), &Cancel::default())
        .unwrap();
    m.assert();
}

#[test]
fn a_429_is_retried_twice_then_an_error() {
    let mut s = mockito::Server::new();
    let m = s
        .mock("POST", "/v1/messages")
        .with_status(429)
        .with_body("slow down")
        .expect(3)
        .create();
    let e = provider(&s.url())
        .complete(req(vec![], vec![Message::user("q")]), &Cancel::default())
        .unwrap_err();
    assert!(matches!(e, Error::Http { status: 429, .. }), "{e}");
    m.assert();
}

#[test]
fn a_529_then_success_succeeds() {
    let mut s = mockito::Server::new();
    s.mock("POST", "/v1/messages")
        .with_status(529)
        .expect(1)
        .create();
    s.mock("POST", "/v1/messages").with_body(REPLY).create();
    assert!(
        provider(&s.url())
            .complete(req(vec![], vec![Message::user("q")]), &Cancel::default())
            .is_ok()
    );
}

#[test]
fn no_key_is_an_error_naming_the_variable() {
    let Err(e) = Anthropic::from_config(&ProviderConfig::default(), &MapEnv::default()) else {
        panic!()
    };
    assert!(e.to_string().contains("ANTHROPIC_API_KEY"), "{e}");
    let cfg = ProviderConfig {
        api_key_env: Some("MY_KEY".into()),
        ..Default::default()
    };
    let Err(e) = Anthropic::from_config(&cfg, &MapEnv::default()) else {
        panic!()
    };
    assert!(e.to_string().contains("MY_KEY"), "{e}");
}

#[test]
fn aliases_resolve_and_an_unknown_alias_names_the_config_key() {
    let p = provider("http://unused");
    assert_eq!(p.resolve("sonnet").unwrap().0, "claude-sonnet-5-5");
    assert_eq!(p.resolve("cheap").unwrap().0, "claude-haiku-5-5");
    assert_eq!(p.resolve("claude-opus-5").unwrap().0, "claude-opus-5");
    let e = p.resolve("fast").unwrap_err().to_string();
    assert!(
        e.contains("[model.providers.anthropic] aliases.fast"),
        "{e}"
    );
}

#[test]
fn cancel_returns_promptly() {
    let mut s = mockito::Server::new();
    s.mock("POST", "/v1/messages")
        .with_chunked_body(|w| {
            std::thread::sleep(Duration::from_secs(3));
            w.write_all(REPLY.as_bytes())
        })
        .create();
    let c = Cancel::default();
    let c2 = c.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        c2.cancel();
    });
    let t = std::time::Instant::now();
    let e = provider(&s.url())
        .complete(req(vec![], vec![Message::user("q")]), &c)
        .unwrap_err();
    assert!(matches!(e, Error::Cancelled), "{e}");
    assert!(t.elapsed() < Duration::from_secs(2));
}
