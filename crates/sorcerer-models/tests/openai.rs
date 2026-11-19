use std::time::Duration;

use mockito::Matcher;
use serde_json::json;
use sorcerer_core::Cancel;
use sorcerer_core::adapters::env::MapEnv;
use sorcerer_core::domain::config::ProviderConfig;
use sorcerer_models::adapters::openai::OpenAiCompatible;
use sorcerer_models::domain::model::{
    Completion, Content, Message, ModelId, Role, StopReason, ToolCall, ToolSpec,
};
use sorcerer_models::ports::provider::Provider;

fn provider(url: &str, key: bool) -> OpenAiCompatible {
    let cfg = ProviderConfig {
        url: Some(url.into()),
        api_key_env: key.then(|| "LOCAL_KEY".into()),
        aliases: [("sonnet".to_string(), "qwen3:32b".to_string())].into(),
        ..Default::default()
    };
    OpenAiCompatible::from_config("local", &cfg, &MapEnv::default().var_set("LOCAL_KEY", "k"))
        .unwrap()
        .with_backoff(Duration::ZERO)
}

fn req(tools: Vec<ToolSpec>, messages: Vec<Message>) -> Completion {
    Completion {
        model: ModelId("qwen3:32b".into()),
        system: "be brief".into(),
        messages,
        tools,
        max_tokens: 100,
        timeout: Duration::from_secs(5),
    }
}

const REPLY: &str = r#"{"choices":[{"message":{"content":null,"tool_calls":[{"id":"c1","type":"function","function":{"name":"read","arguments":"{\"path\":\"a.py\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":7,"completion_tokens":3}}"#;

#[test]
fn sends_chat_completions_with_tools_in_the_function_shape() {
    let mut s = mockito::Server::new();
    let m = s
        .mock("POST", "/chat/completions")
        .match_header("authorization", "Bearer k")
        .match_body(Matcher::Json(json!({
            "model": "qwen3:32b", "max_tokens": 100,
            "messages": [{"role": "system", "content": "be brief"}, {"role": "user", "content": "q"}],
            "tools": [{"type": "function", "function": {"name": "read", "description": "read a file", "parameters": {"type": "object"}}}]
        })))
        .with_body(REPLY)
        .create();
    let tool = ToolSpec {
        name: "read".into(),
        description: "read a file".into(),
        schema: json!({"type": "object"}),
    };
    provider(&s.url(), true)
        .complete(
            req(vec![tool], vec![Message::user("q")]),
            &Cancel::default(),
        )
        .unwrap();
    m.assert();
}

#[test]
fn reads_tool_calls_with_string_arguments() {
    let mut s = mockito::Server::new();
    s.mock("POST", "/chat/completions")
        .with_body(REPLY)
        .create();
    let r = provider(&s.url(), true)
        .complete(req(vec![], vec![Message::user("q")]), &Cancel::default())
        .unwrap();
    assert_eq!(
        r.tool_calls,
        vec![ToolCall {
            id: "c1".into(),
            name: "read".into(),
            input: json!({"path": "a.py"})
        }]
    );
    assert_eq!(r.stop, StopReason::ToolUse);
    assert_eq!((r.usage.input_tokens, r.usage.output_tokens), (7, 3));
    assert_eq!(r.text, "");
}

#[test]
fn tool_calls_and_results_are_sent_back_in_the_openai_shape() {
    let mut s = mockito::Server::new();
    let m = s
        .mock("POST", "/chat/completions")
        .match_body(Matcher::PartialJson(json!({"messages": [
            {"role": "system", "content": "be brief"},
            {"role": "user", "content": "q"},
            {"role": "assistant", "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "read", "arguments": "{\"path\":\"a.py\"}"}}]},
            {"role": "tool", "tool_call_id": "c1", "content": "1\tx"}
        ]})))
        .with_body(REPLY)
        .create();
    let call = ToolCall {
        id: "c1".into(),
        name: "read".into(),
        input: json!({"path": "a.py"}),
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
                id: "c1".into(),
                text: "1\tx".into(),
            }],
        },
    ];
    provider(&s.url(), false)
        .complete(req(vec![], msgs), &Cancel::default())
        .unwrap();
    m.assert();
}

#[test]
fn an_unmapped_alias_names_the_key_to_set() {
    let p = provider("http://unused", false);
    assert_eq!(p.resolve("sonnet").unwrap().0, "qwen3:32b");
    assert_eq!(p.resolve("llama3.1:8b").unwrap().0, "llama3.1:8b");
    let e = p.resolve("haiku").unwrap_err().to_string();
    assert!(e.contains("[model.providers.local] aliases.haiku"), "{e}");
}

#[test]
fn no_key_sends_no_authorization_header() {
    let mut s = mockito::Server::new();
    let m = s
        .mock("POST", "/chat/completions")
        .match_header("authorization", Matcher::Missing)
        .with_body(REPLY)
        .create();
    provider(&s.url(), false)
        .complete(req(vec![], vec![Message::user("q")]), &Cancel::default())
        .unwrap();
    m.assert();
}

#[test]
fn no_url_is_an_error() {
    let Err(e) =
        OpenAiCompatible::from_config("local", &ProviderConfig::default(), &MapEnv::default())
    else {
        panic!()
    };
    assert!(e.to_string().contains("url"), "{e}");
}
