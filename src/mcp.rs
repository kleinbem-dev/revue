use crate::config::RevueConfig;
use crate::reviewer::review_diff_content;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[derive(Deserialize, Debug)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Serialize, Debug)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Value>,
}

pub async fn run_mcp_server(config_path: Option<&str>) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(stdin).lines();

    while let Ok(Some(line)) = reader.next_line().await {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let err_resp = json!({
                    "jsonrpc": "2.0",
                    "id": null,
                    "error": { "code": -32700, "message": format!("Parse error: {}", e) }
                });
                stdout.write_all(err_resp.to_string().as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
                continue;
            }
        };

        let id = req.id.clone().unwrap_or(Value::Null);

        let resp_result: Option<Value> = match req.method.as_str() {
            "initialize" => Some(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "revue",
                    "version": env!("CARGO_PKG_VERSION")
                }
            })),

            "notifications/initialized" | "initialized" => {
                // Notifications don't get a response
                continue;
            }

            "ping" => Some(json!({})),

            "tools/list" => Some(json!({
                "tools": [
                    {
                        "name": "revue_inspect",
                        "description": "Run Revue two-pass code review (Detective + Judge) on a unified git diff. Returns structured findings, severity, lines, and suggested fixes.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "diff": {
                                    "type": "string",
                                    "description": "The git diff text (e.g. from `git diff` or `git show`)"
                                }
                            },
                            "required": ["diff"]
                        }
                    },
                    {
                        "name": "revue_rules",
                        "description": "List the active code review rules, architectural bounds, and conventions configured in Revue.",
                        "inputSchema": {
                            "type": "object",
                            "properties": {}
                        }
                    }
                ]
            })),

            "tools/call" => {
                let params = req.params.unwrap_or(Value::Null);
                let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(Value::Null);

                match tool_name {
                    "revue_inspect" => {
                        let diff_str = args.get("diff").and_then(|v| v.as_str()).unwrap_or("");
                        if diff_str.trim().is_empty() {
                            Some(json!({
                                "content": [{
                                    "type": "text",
                                    "text": "Error: 'diff' argument cannot be empty."
                                }],
                                "isError": true
                            }))
                        } else {
                            match review_diff_content(diff_str, config_path).await {
                                Ok((output, provider_used)) => {
                                    let result_json = json!({
                                        "provider_used": provider_used,
                                        "summary": output.summary,
                                        "findings_count": output.issues.len(),
                                        "findings": output.issues
                                    });
                                    Some(json!({
                                        "content": [{
                                            "type": "text",
                                            "text": serde_json::to_string_pretty(&result_json).unwrap_or_default()
                                        }]
                                    }))
                                }
                                Err(e) => Some(json!({
                                    "content": [{
                                        "type": "text",
                                        "text": format!("Review failed: {}", e)
                                    }],
                                    "isError": true
                                })),
                            }
                        }
                    }

                    "revue_rules" => {
                        let cfg = RevueConfig::find_and_load(config_path).unwrap_or_default();
                        let rules_json = json!({
                            "language": cfg.review.language,
                            "two_pass": cfg.review.two_pass,
                            "min_severity": cfg.review.min_severity,
                            "tone": cfg.review.tone,
                            "ignore": cfg.review.ignore,
                            "rules": cfg.review.rules,
                        });
                        Some(json!({
                            "content": [{
                                "type": "text",
                                "text": serde_json::to_string_pretty(&rules_json).unwrap_or_default()
                            }]
                        }))
                    }

                    _ => Some(json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Unknown tool: {}", tool_name)
                        }],
                        "isError": true
                    })),
                }
            }

            _ => {
                if !id.is_null() {
                    let err_resp = json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32601, "message": format!("Method not found: {}", req.method) }
                    });
                    stdout.write_all(err_resp.to_string().as_bytes()).await?;
                    stdout.write_all(b"\n").await?;
                    stdout.flush().await?;
                }
                continue;
            }
        };

        if let Some(result) = resp_result {
            let response = JsonRpcResponse {
                jsonrpc: "2.0",
                id,
                result: Some(result),
                error: None,
            };
            stdout
                .write_all(serde_json::to_string(&response)?.as_bytes())
                .await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_initialize_structure() {
        let req_json = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#;
        let req: JsonRpcRequest = serde_json::from_str(req_json).unwrap();
        assert_eq!(req.method, "initialize");
        assert_eq!(req.id, Some(json!(1)));
    }

    #[test]
    fn test_mcp_tools_list_structure() {
        let req_json = r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#;
        let req: JsonRpcRequest = serde_json::from_str(req_json).unwrap();
        assert_eq!(req.method, "tools/list");
    }
}
