//! Bounded MCP write-only surface; independent of HTTP and any local data store.
use super::WriteRequest;
use serde_json::{Value, json};
pub const MODERN: &str = "2026-07-28";
pub const LEGACY: &str = "2025-11-25";
pub const VERSIONS: [&str; 4] = [MODERN, LEGACY, "2025-06-18", "2025-03-26"];
pub enum Action {
    Discover,
    Initialize(String),
    List,
    Ping,
    Notification,
    Write(WriteRequest),
}
pub struct Call {
    pub id: Value,
    pub modern: bool,
    pub action: Action,
}
pub struct Failure {
    pub id: Value,
    pub code: i32,
    pub modern: bool,
}
impl Failure {
    pub fn reply(&self) -> Value {
        let message = match self.code {
            -32020 => "Header mismatch",
            -32022 => "Unsupported protocol version",
            -32601 => "Method not found",
            _ => "Invalid request",
        };
        let mut error = json!({"code":self.code,"message":message});
        if self.code == -32022 {
            error["data"] = json!({"supported":VERSIONS});
        }
        json!({"jsonrpc":"2.0","id":self.id,"error":error})
    }
}
/// Transport decodes a sentinel-encoded Mcp-Name before passing it here.
pub fn parse(
    value: Value,
    protocol: Option<&str>,
    method_header: Option<&str>,
    name_header: Option<&str>,
) -> Result<Call, Failure> {
    let id = value.get("id").cloned().unwrap_or(Value::Null);
    let meta = &value["params"]["_meta"];
    let declared = meta["io.modelcontextprotocol/protocolVersion"].as_str();
    let modern = declared.is_some() || protocol == Some(MODERN);
    let fail = |code| Failure {
        id: id.clone(),
        code,
        modern,
    };
    if value["jsonrpc"] != "2.0"
        || !value.is_object()
        || value.get("result").is_some()
        || value.get("error").is_some()
        || !(id.is_null()
            || id.is_i64()
            || id.is_u64()
            || id.as_str().is_some_and(|s| s.len() <= 128))
    {
        return Err(fail(-32600));
    }
    let method = value["method"].as_str().ok_or_else(|| fail(-32600))?;
    if modern {
        let declared = declared.ok_or_else(|| fail(-32020))?;
        if declared != MODERN {
            return Err(fail(-32022));
        }
        if protocol != Some(declared)
            || method_header != Some(method)
            || !meta["io.modelcontextprotocol/clientInfo"].is_object()
            || !meta["io.modelcontextprotocol/clientCapabilities"].is_object()
        {
            return Err(fail(-32020));
        }
        if method == "tools/call" && name_header != value["params"]["name"].as_str() {
            return Err(fail(-32020));
        }
    } else if let Some(version) = protocol
        && !VERSIONS.contains(&version)
    {
        return Err(fail(-32022));
    }
    let action = match method {
        "server/discover" => Action::Discover,
        "initialize" if !modern => {
            let proposed = value["params"]["protocolVersion"]
                .as_str()
                .unwrap_or(LEGACY);
            Action::Initialize(
                if VERSIONS[1..].contains(&proposed) {
                    proposed
                } else {
                    LEGACY
                }
                .into(),
            )
        }
        "notifications/initialized" if !modern && value.get("id").is_none() => Action::Notification,
        "ping" => Action::Ping,
        "tools/list" => Action::List,
        "tools/call" => {
            let name = value["params"]["name"]
                .as_str()
                .ok_or_else(|| fail(-32600))?;
            let operation = match name {
                "workspace_push" => "push",
                "workspace_stream_begin" => "begin",
                "workspace_stream_chunk" => "chunk",
                "workspace_stream_finish" => "finish",
                "workspace_stream_cancel" => "cancel",
                _ => return Err(fail(-32601)),
            };
            let mut args = value["params"]["arguments"]
                .as_object()
                .cloned()
                .ok_or_else(|| fail(-32600))?;
            if args.contains_key("operation") {
                return Err(fail(-32600));
            }
            args.insert("operation".into(), json!(operation));
            Action::Write(serde_json::from_value(Value::Object(args)).map_err(|_| fail(-32600))?)
        }
        _ => return Err(fail(-32601)),
    };
    if value.get("id").is_none() && !matches!(action, Action::Notification) {
        return Err(fail(-32600));
    }
    Ok(Call { id, modern, action })
}
pub fn identity() -> Value {
    json!({"name":"Moodflow workspace","version":"1.0.0"})
}
pub fn tools() -> Value {
    let id = json!({"type":"string","format":"uuid"});
    let text = json!({"type":"string","maxLength":16384});
    let specs = [
        (
            "workspace_push",
            json!({"session_id":id,"message_id":id,"text":text}),
            vec!["session_id", "message_id", "text"],
        ),
        (
            "workspace_stream_begin",
            json!({"session_id":id,"stream_id":id}),
            vec!["session_id", "stream_id"],
        ),
        (
            "workspace_stream_chunk",
            json!({"stream_id":id,"sequence":{"type":"integer","minimum":0,"maximum":16383},"text":text}),
            vec!["stream_id", "sequence", "text"],
        ),
        (
            "workspace_stream_finish",
            json!({"session_id":id,"stream_id":id}),
            vec!["session_id", "stream_id"],
        ),
        (
            "workspace_stream_cancel",
            json!({"stream_id":id}),
            vec!["stream_id"],
        ),
    ];
    json!({"resultType":"complete","tools":specs.into_iter().map(|(name,properties,required)| json!({"name":name,"description":"Write inert content to an explicitly chosen local inbox. No read, execution or input delivery.","inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false},"annotations":{"readOnlyHint":false,"destructiveHint":false,"openWorldHint":false}})).collect::<Vec<_>>()})
}
#[cfg(test)]
mod tests {
    use super::*;
    fn request(method: &str) -> Value {
        json!({"jsonrpc":"2.0","id":1,"method":method,"params":{}})
    }
    #[test]
    fn only_write_tools_are_discoverable_and_reads_are_rejected_in_both_eras() {
        assert_eq!(tools()["tools"].as_array().unwrap().len(), 5);
        for method in [
            "resources/read",
            "resources/list",
            "workspace_read",
            "prompts/get",
        ] {
            assert!(matches!(
                parse(request(method), None, None, None),
                Err(Failure { code: -32601, .. })
            ));
        }
        let mut modern = request("tools/list");
        modern["params"]["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":MODERN,"io.modelcontextprotocol/clientInfo":{"name":"fixture","version":"1"},"io.modelcontextprotocol/clientCapabilities":{}});
        assert!(parse(modern.clone(), Some(MODERN), Some("tools/list"), None).is_ok());
        assert!(matches!(
            parse(modern.clone(), Some(MODERN), Some("resources/read"), None),
            Err(Failure { code: -32020, .. })
        ));
        modern["params"]["_meta"]["io.modelcontextprotocol/protocolVersion"] = json!("1900-01-01");
        assert!(matches!(
            parse(modern, Some("1900-01-01"), Some("tools/list"), None),
            Err(Failure { code: -32022, .. })
        ));
    }
}
