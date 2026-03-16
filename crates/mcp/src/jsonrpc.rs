//! JSON-RPC 2.0 消息类型
//!
//! MCP 协议基于 JSON-RPC 2.0 进行通信，本模块定义请求、响应、错误的序列化结构。

use serde::{Deserialize, Serialize};

/// JSON-RPC 2.0 请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    /// 固定为 "2.0"
    pub jsonrpc: String,
    /// 请求 ID（用于匹配响应）
    pub id: serde_json::Value,
    /// 方法名
    pub method: String,
    /// 参数（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    /// 创建新的 JSON-RPC 请求
    pub fn new(id: u64, method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: serde_json::Value::Number(id.into()),
            method: method.into(),
            params,
        }
    }
}

/// JSON-RPC 2.0 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    /// 固定为 "2.0"
    pub jsonrpc: String,
    /// 请求 ID（与请求中的 id 对应）
    pub id: serde_json::Value,
    /// 成功时的返回值
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// 失败时的错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 错误对象
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    /// 错误码
    pub code: i64,
    /// 错误消息
    pub message: String,
    /// 附加数据（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 通知（无 id 字段，不期望响应）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    /// 固定为 "2.0"
    pub jsonrpc: String,
    /// 方法名
    pub method: String,
    /// 参数（可选）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcNotification {
    /// 创建新的 JSON-RPC 通知
    pub fn new(method: impl Into<String>, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params,
        }
    }
}

// ── 标准 JSON-RPC 错误码 ──

/// 解析错误
pub const PARSE_ERROR: i64 = -32700;
/// 无效请求
pub const INVALID_REQUEST: i64 = -32600;
/// 方法不存在
pub const METHOD_NOT_FOUND: i64 = -32601;
/// 无效参数
pub const INVALID_PARAMS: i64 = -32602;
/// 内部错误
pub const INTERNAL_ERROR: i64 = -32603;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let req = JsonRpcRequest::new(1, "tools/list", Some(serde_json::json!({})));

        let json = serde_json::to_string(&req).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["id"], 1);
        assert_eq!(parsed["method"], "tools/list");
        assert_eq!(parsed["params"], serde_json::json!({}));
    }

    #[test]
    fn test_request_without_params() {
        let req = JsonRpcRequest::new(2, "ping", None);
        let json = serde_json::to_string(&req).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["id"], 2);
        assert_eq!(parsed["method"], "ping");
        // params should be absent (skip_serializing_if = None)
        assert!(parsed.get("params").is_none());
    }

    #[test]
    fn test_response_success_serialization() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: serde_json::Value::Number(1.into()),
            result: Some(serde_json::json!({"tools": []})),
            error: None,
        };

        let json = serde_json::to_string(&resp).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["id"], 1);
        assert!(parsed.get("result").is_some());
        assert!(parsed.get("error").is_none());
    }

    #[test]
    fn test_response_error_serialization() {
        let resp = JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id: serde_json::Value::Number(1.into()),
            result: None,
            error: Some(JsonRpcError {
                code: METHOD_NOT_FOUND,
                message: "Method not found".to_string(),
                data: None,
            }),
        };

        let json = serde_json::to_string(&resp).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["jsonrpc"], "2.0");
        assert!(parsed.get("result").is_none());
        assert_eq!(parsed["error"]["code"], METHOD_NOT_FOUND);
        assert_eq!(parsed["error"]["message"], "Method not found");
    }

    #[test]
    fn test_response_deserialization_success() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"echo","description":"Echo tool","inputSchema":{"type":"object"}}]}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();

        assert_eq!(resp.jsonrpc, "2.0");
        assert_eq!(resp.id, serde_json::Value::Number(1.into()));
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());

        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "echo");
    }

    #[test]
    fn test_response_deserialization_error() {
        let json = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found","data":{"detail":"unknown method"}}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();

        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, METHOD_NOT_FOUND);
        assert_eq!(err.message, "Method not found");
        assert!(err.data.is_some());
        assert_eq!(err.data.unwrap()["detail"], "unknown method");
    }

    #[test]
    fn test_notification_serialization() {
        let notif =
            JsonRpcNotification::new("notifications/initialized", Some(serde_json::json!({})));

        let json = serde_json::to_string(&notif).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["method"], "notifications/initialized");
        // Notifications should NOT have an "id" field
        assert!(parsed.get("id").is_none());
    }

    #[test]
    fn test_request_roundtrip() {
        let req = JsonRpcRequest::new(
            42,
            "tools/call",
            Some(serde_json::json!({
                "name": "read_file",
                "arguments": {"path": "/tmp/test.txt"}
            })),
        );

        let json = serde_json::to_string(&req).unwrap();
        let deserialized: JsonRpcRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.jsonrpc, "2.0");
        assert_eq!(deserialized.id, serde_json::Value::Number(42.into()));
        assert_eq!(deserialized.method, "tools/call");
        assert!(deserialized.params.is_some());

        let params = deserialized.params.unwrap();
        assert_eq!(params["name"], "read_file");
        assert_eq!(params["arguments"]["path"], "/tmp/test.txt");
    }

    #[test]
    fn test_error_with_data() {
        let err = JsonRpcError {
            code: INTERNAL_ERROR,
            message: "Internal error".to_string(),
            data: Some(serde_json::json!({"stack": "trace here"})),
        };

        let json = serde_json::to_string(&err).unwrap();
        let deserialized: JsonRpcError = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.code, INTERNAL_ERROR);
        assert_eq!(deserialized.data.unwrap()["stack"], "trace here");
    }

    #[test]
    fn test_error_constants() {
        assert_eq!(PARSE_ERROR, -32700);
        assert_eq!(INVALID_REQUEST, -32600);
        assert_eq!(METHOD_NOT_FOUND, -32601);
        assert_eq!(INVALID_PARAMS, -32602);
        assert_eq!(INTERNAL_ERROR, -32603);
    }
}
