//! AttaOS API client wrapper
//!
//! 轻量 HTTP client，用于与 attaos 服务通信。

use atta_types::{ChatEvent, ChatRequest};
use futures::StreamExt;
use reqwest::Client;

/// AttaOS API 客户端
pub struct AttaClient {
    base_url: String,
    client: Client,
}

#[allow(dead_code)]
impl AttaClient {
    /// 创建新的客户端
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            client: Client::new(),
        }
    }

    // ── helpers ──

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    async fn get_json(&self, path: &str) -> Result<serde_json::Value, reqwest::Error> {
        self.client.get(self.url(path)).send().await?.json().await
    }

    async fn post_json(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, reqwest::Error> {
        self.client
            .post(self.url(path))
            .json(body)
            .send()
            .await?
            .json()
            .await
    }

    pub async fn put_json(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, reqwest::Error> {
        self.client
            .put(self.url(path))
            .json(body)
            .send()
            .await?
            .json()
            .await
    }

    pub async fn delete_resource(&self, path: &str) -> Result<serde_json::Value, reqwest::Error> {
        self.client
            .delete(self.url(path))
            .send()
            .await?
            .json()
            .await
    }

    // ── system ──

    /// 健康检查
    pub async fn health(&self) -> Result<bool, reqwest::Error> {
        let resp = self
            .client
            .get(self.url("/api/v1/health"))
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await?;
        Ok(resp.status().is_success())
    }

    /// 系统配置
    pub async fn system_config(&self) -> Result<serde_json::Value, reqwest::Error> {
        self.get_json("/api/v1/system/config").await
    }

    /// 系统指标
    pub async fn metrics(&self) -> Result<serde_json::Value, reqwest::Error> {
        self.get_json("/api/v1/system/metrics").await
    }

    // ── tasks ──

    /// 列出任务（支持过滤）
    pub async fn list_tasks_filtered(
        &self,
        status: Option<&str>,
        flow_id: Option<&str>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<serde_json::Value, reqwest::Error> {
        let mut params = Vec::new();
        if let Some(s) = status {
            params.push(format!("status={s}"));
        }
        if let Some(f) = flow_id {
            params.push(format!("flow_id={f}"));
        }
        if let Some(l) = limit {
            params.push(format!("limit={l}"));
        }
        if let Some(o) = offset {
            params.push(format!("offset={o}"));
        }
        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        self.get_json(&format!("/api/v1/tasks{query}")).await
    }

    /// 列出任务
    pub async fn list_tasks(&self) -> Result<serde_json::Value, reqwest::Error> {
        self.get_json("/api/v1/tasks").await
    }

    /// 获取单个任务
    pub async fn get_task(&self, id: &str) -> Result<serde_json::Value, reqwest::Error> {
        self.get_json(&format!("/api/v1/tasks/{id}")).await
    }

    /// 创建任务
    pub async fn create_task(
        &self,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, reqwest::Error> {
        self.post_json("/api/v1/tasks", body).await
    }

    /// 删除任务
    pub async fn delete_task(&self, id: &str) -> Result<serde_json::Value, reqwest::Error> {
        self.delete_resource(&format!("/api/v1/tasks/{id}")).await
    }

    /// 取消任务
    pub async fn cancel_task(&self, id: &str) -> Result<serde_json::Value, reqwest::Error> {
        self.post_json(
            &format!("/api/v1/tasks/{id}/cancel"),
            &serde_json::json!({}),
        )
        .await
    }

    // ── flows ──

    /// 列出 flows
    pub async fn list_flows(&self) -> Result<serde_json::Value, reqwest::Error> {
        self.get_json("/api/v1/flows").await
    }

    /// 获取单个 flow
    pub async fn get_flow(&self, id: &str) -> Result<serde_json::Value, reqwest::Error> {
        self.get_json(&format!("/api/v1/flows/{id}")).await
    }

    /// 创建 flow
    pub async fn create_flow(
        &self,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, reqwest::Error> {
        self.post_json("/api/v1/flows", body).await
    }

    /// 删除 flow
    pub async fn delete_flow(&self, id: &str) -> Result<serde_json::Value, reqwest::Error> {
        self.delete_resource(&format!("/api/v1/flows/{id}")).await
    }

    // ── skills ──

    /// 列出 skills
    pub async fn list_skills(&self) -> Result<serde_json::Value, reqwest::Error> {
        self.get_json("/api/v1/skills").await
    }

    /// 获取单个 skill
    pub async fn get_skill(&self, id: &str) -> Result<serde_json::Value, reqwest::Error> {
        self.get_json(&format!("/api/v1/skills/{id}")).await
    }

    /// 创建 skill
    pub async fn create_skill(
        &self,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, reqwest::Error> {
        self.post_json("/api/v1/skills", body).await
    }

    // ── tools ──

    /// 列出 tools
    pub async fn list_tools(&self) -> Result<serde_json::Value, reqwest::Error> {
        self.get_json("/api/v1/tools").await
    }

    /// 获取单个 tool
    pub async fn get_tool(&self, name: &str) -> Result<serde_json::Value, reqwest::Error> {
        self.get_json(&format!("/api/v1/tools/{name}")).await
    }

    /// 测试 tool
    pub async fn test_tool(
        &self,
        name: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value, reqwest::Error> {
        self.post_json(&format!("/api/v1/tools/{name}/test"), args)
            .await
    }

    // ── MCP ──

    /// 列出 MCP servers
    pub async fn list_mcp_servers(&self) -> Result<serde_json::Value, reqwest::Error> {
        self.get_json("/api/v1/mcp/servers").await
    }

    /// 获取单个 MCP server
    pub async fn get_mcp_server(&self, name: &str) -> Result<serde_json::Value, reqwest::Error> {
        self.get_json(&format!("/api/v1/mcp/servers/{name}")).await
    }

    /// 注册 MCP server
    pub async fn register_mcp_server(
        &self,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, reqwest::Error> {
        self.post_json("/api/v1/mcp/servers", body).await
    }

    /// 注销 MCP server
    pub async fn unregister_mcp_server(
        &self,
        name: &str,
    ) -> Result<serde_json::Value, reqwest::Error> {
        self.delete_resource(&format!("/api/v1/mcp/servers/{name}"))
            .await
    }

    /// 连接 MCP server
    pub async fn connect_mcp_server(
        &self,
        name: &str,
    ) -> Result<serde_json::Value, reqwest::Error> {
        self.post_json(
            &format!("/api/v1/mcp/servers/{name}/connect"),
            &serde_json::json!({}),
        )
        .await
    }

    /// 断开 MCP server
    pub async fn disconnect_mcp_server(
        &self,
        name: &str,
    ) -> Result<serde_json::Value, reqwest::Error> {
        self.post_json(
            &format!("/api/v1/mcp/servers/{name}/disconnect"),
            &serde_json::json!({}),
        )
        .await
    }

    // ── approvals ──

    /// 列出审批
    pub async fn list_approvals(
        &self,
        status: Option<&str>,
    ) -> Result<serde_json::Value, reqwest::Error> {
        let query = match status {
            Some(s) => format!("?status={s}"),
            None => String::new(),
        };
        self.get_json(&format!("/api/v1/approvals{query}")).await
    }

    /// 获取单个审批
    pub async fn get_approval(&self, id: &str) -> Result<serde_json::Value, reqwest::Error> {
        self.get_json(&format!("/api/v1/approvals/{id}")).await
    }

    /// 批准
    pub async fn approve(
        &self,
        id: &str,
        comment: Option<&str>,
    ) -> Result<serde_json::Value, reqwest::Error> {
        let body = match comment {
            Some(c) => serde_json::json!({"comment": c}),
            None => serde_json::json!({}),
        };
        self.post_json(&format!("/api/v1/approvals/{id}/approve"), &body)
            .await
    }

    /// 拒绝
    pub async fn deny(
        &self,
        id: &str,
        reason: Option<&str>,
    ) -> Result<serde_json::Value, reqwest::Error> {
        let body = match reason {
            Some(r) => serde_json::json!({"reason": r}),
            None => serde_json::json!({}),
        };
        self.post_json(&format!("/api/v1/approvals/{id}/deny"), &body)
            .await
    }

    // ── nodes ──

    /// 列出节点
    pub async fn list_nodes(&self) -> Result<serde_json::Value, reqwest::Error> {
        self.get_json("/api/v1/nodes").await
    }

    /// 获取单个节点
    pub async fn get_node(&self, id: &str) -> Result<serde_json::Value, reqwest::Error> {
        self.get_json(&format!("/api/v1/nodes/{id}")).await
    }

    /// 排空节点
    pub async fn drain_node(&self, id: &str) -> Result<serde_json::Value, reqwest::Error> {
        self.post_json(&format!("/api/v1/nodes/{id}/drain"), &serde_json::json!({}))
            .await
    }

    /// 恢复节点
    pub async fn resume_node(&self, id: &str) -> Result<serde_json::Value, reqwest::Error> {
        self.post_json(
            &format!("/api/v1/nodes/{id}/resume"),
            &serde_json::json!({}),
        )
        .await
    }

    // ── channels ──

    /// 列出 channels
    pub async fn list_channels(&self) -> Result<serde_json::Value, reqwest::Error> {
        self.get_json("/api/v1/channels").await
    }

    /// Channel 健康检查
    pub async fn channel_health(&self, name: &str) -> Result<serde_json::Value, reqwest::Error> {
        self.get_json(&format!("/api/v1/channels/{name}/health"))
            .await
    }

    // ── security ──

    /// 获取安全策略
    pub async fn get_security_policy(&self) -> Result<serde_json::Value, reqwest::Error> {
        self.get_json("/api/v1/security/policy").await
    }

    /// 更新安全策略
    pub async fn update_security_policy(
        &self,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, reqwest::Error> {
        self.put_json("/api/v1/security/policy", body).await
    }

    // ── audit ──

    /// 查询审计
    pub async fn query_audit(
        &self,
        actor: Option<&str>,
        action: Option<&str>,
        from: Option<&str>,
        to: Option<&str>,
        limit: Option<u32>,
    ) -> Result<serde_json::Value, reqwest::Error> {
        let mut params = Vec::new();
        if let Some(a) = actor {
            params.push(format!("actor_id={a}"));
        }
        if let Some(a) = action {
            params.push(format!("action={a}"));
        }
        if let Some(f) = from {
            params.push(format!("from={f}"));
        }
        if let Some(t) = to {
            params.push(format!("to={t}"));
        }
        if let Some(l) = limit {
            params.push(format!("limit={l}"));
        }
        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        self.get_json(&format!("/api/v1/audit{query}")).await
    }

    /// 导出审计
    pub async fn export_audit(
        &self,
        format: Option<&str>,
        actor: Option<&str>,
        action: Option<&str>,
        from: Option<&str>,
        to: Option<&str>,
        limit: Option<u32>,
    ) -> Result<String, reqwest::Error> {
        let mut params = Vec::new();
        if let Some(f) = format {
            params.push(format!("format={f}"));
        }
        if let Some(a) = actor {
            params.push(format!("actor_id={a}"));
        }
        if let Some(a) = action {
            params.push(format!("action={a}"));
        }
        if let Some(f) = from {
            params.push(format!("from={f}"));
        }
        if let Some(t) = to {
            params.push(format!("to={t}"));
        }
        if let Some(l) = limit {
            params.push(format!("limit={l}"));
        }
        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        self.client
            .get(self.url(&format!("/api/v1/audit/export{query}")))
            .send()
            .await?
            .text()
            .await
    }

    // ── chat ──

    /// 获取 base_url（仅用于测试）
    #[cfg(test)]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// 流式聊天 — 返回 ChatEvent 迭代器
    pub async fn chat_stream(
        &self,
        request: &ChatRequest,
    ) -> Result<impl futures::Stream<Item = Result<ChatEvent, anyhow::Error>>, anyhow::Error> {
        let resp = self
            .client
            .post(self.url("/api/v1/chat"))
            .json(request)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("chat API returned {status}: {body}");
        }

        // Parse SSE stream
        let stream = resp.bytes_stream().map(move |chunk_result| {
            let chunk = chunk_result?;
            let text = String::from_utf8_lossy(&chunk);
            // SSE format: "data: {...}\n\n"
            // May contain multiple events in one chunk
            let mut last_event = None;
            for line in text.lines() {
                if let Some(data) = line.strip_prefix("data: ") {
                    if let Ok(event) = serde_json::from_str::<ChatEvent>(data) {
                        last_event = Some(event);
                    }
                }
            }
            match last_event {
                Some(event) => Ok(event),
                None => Ok(ChatEvent::TextDelta {
                    delta: String::new(),
                }),
            }
        });

        Ok(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_stores_base_url() {
        let client = AttaClient::new("http://localhost:3000");
        assert_eq!(client.base_url(), "http://localhost:3000");
    }

    #[test]
    fn new_trims_trailing_slash() {
        let client = AttaClient::new("http://localhost:3000/");
        assert_eq!(client.base_url(), "http://localhost:3000");
    }

    #[test]
    fn new_trims_multiple_trailing_slashes() {
        let client = AttaClient::new("http://localhost:3000///");
        assert_eq!(client.base_url(), "http://localhost:3000");
    }

    #[test]
    fn url_construction_health() {
        let client = AttaClient::new("http://myhost:4000");
        let url = format!("{}/api/v1/health", client.base_url());
        assert_eq!(url, "http://myhost:4000/api/v1/health");
    }

    #[test]
    fn url_construction_list_tasks() {
        let client = AttaClient::new("http://localhost:3000");
        let url = format!("{}/api/v1/tasks", client.base_url());
        assert_eq!(url, "http://localhost:3000/api/v1/tasks");
    }

    #[test]
    fn url_construction_get_task() {
        let client = AttaClient::new("http://localhost:3000");
        let id = "task-123";
        let url = format!("{}/api/v1/tasks/{id}", client.base_url());
        assert_eq!(url, "http://localhost:3000/api/v1/tasks/task-123");
    }

    #[test]
    fn url_construction_list_skills() {
        let client = AttaClient::new("http://localhost:3000");
        let url = format!("{}/api/v1/skills", client.base_url());
        assert_eq!(url, "http://localhost:3000/api/v1/skills");
    }

    #[test]
    fn url_construction_get_skill() {
        let client = AttaClient::new("http://localhost:5000");
        let id = "my-skill";
        let url = format!("{}/api/v1/skills/{id}", client.base_url());
        assert_eq!(url, "http://localhost:5000/api/v1/skills/my-skill");
    }

    #[test]
    fn url_construction_list_channels() {
        let client = AttaClient::new("http://localhost:3000");
        let url = format!("{}/api/v1/channels", client.base_url());
        assert_eq!(url, "http://localhost:3000/api/v1/channels");
    }

    #[test]
    fn url_construction_chat() {
        let client = AttaClient::new("http://localhost:3000");
        let url = format!("{}/api/v1/chat", client.base_url());
        assert_eq!(url, "http://localhost:3000/api/v1/chat");
    }

    #[tokio::test]
    async fn health_constructs_correct_request() {
        let client = AttaClient::new("http://127.0.0.1:1");
        let result = client.health().await;
        assert!(result.is_err());
    }
}
