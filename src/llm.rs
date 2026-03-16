use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use reqwest::Client;
use serde_json::json;
use tracing::info;
use std::time::Duration;
use std::path::PathBuf;
use directories::ProjectDirs;

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("API Request Failed: {0}")]
    RequestFailed(String),
    #[error("API Response Parse Failed: {0}")]
    ParseFailed(String),
    #[error("Unauthorized: {0}")]
    Unauthorized(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,           // "user", "assistant", "tool"
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRequest {
    pub system_prompt: String,
    pub messages: Vec<Message>,
    pub max_tokens: u32,
    pub temperature: f32,
    pub tools: Vec<ToolDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmResponse {
    pub text: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError>;
}

// -----------------------------------------------------------------------------
// Anthropic Client implementation
// -----------------------------------------------------------------------------

pub struct AnthropicClient {
    client: Client,
    api_key: String,
    model: String,
    base_url: String,
}

impl AnthropicClient {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
            base_url: "https://api.anthropic.com".to_string(),
        }
    }

    /// Create a client that talks through an API proxy (no API key needed — proxy injects it).
    pub fn with_proxy(base_url: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key: String::new(),
            model,
            base_url,
        }
    }

    fn convert_messages(messages: &[Message]) -> Vec<serde_json::Value> {
        let mut result = Vec::new();
        for msg in messages {
            match msg.role.as_str() {
                "user" => {
                    result.push(json!({
                        "role": "user",
                        "content": msg.content
                    }));
                }
                "assistant" => {
                    result.push(json!({
                        "role": "assistant",
                        "content": msg.content
                    }));
                }
                "tool" => {
                    if let Some(ref tool_call_id) = msg.tool_call_id {
                        result.push(json!({
                            "role": "user",
                            "content": [{
                                "type": "tool_result",
                                "tool_use_id": tool_call_id,
                                "content": msg.content
                            }]
                        }));
                    }
                }
                _ => {}
            }
        }
        result
    }

    fn convert_tools(tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
        tools.iter().map(|td| {
            json!({
                "name": td.name,
                "description": td.description,
                "input_schema": td.input_schema
            })
        }).collect()
    }
}

#[async_trait]
impl LlmClient for AnthropicClient {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        info!("Sending request to Anthropic API (model: {})", self.model);

        let mut headers = reqwest::header::HeaderMap::new();
        if !self.api_key.is_empty() {
            headers.insert("x-api-key", reqwest::header::HeaderValue::from_str(&self.api_key).unwrap());
        }
        headers.insert("anthropic-version", "2023-06-01".parse().unwrap());
        headers.insert("content-type", "application/json".parse().unwrap());

        let anthropic_messages = Self::convert_messages(&req.messages);

        let mut payload = json!({
            "model": self.model,
            "max_tokens": req.max_tokens,
            "temperature": req.temperature,
            "system": req.system_prompt,
            "messages": anthropic_messages
        });

        if !req.tools.is_empty() {
            payload.as_object_mut().unwrap().insert(
                "tools".to_string(),
                serde_json::Value::Array(Self::convert_tools(&req.tools)),
            );
        }

        let url = format!("{}/v1/messages", self.base_url);
        let response = self.client.post(&url)
            .headers(headers).json(&payload).send().await
            .map_err(|e| LlmError::RequestFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(LlmError::RequestFailed(response.status().to_string()));
        }

        let body: serde_json::Value = response.json().await.map_err(|e| LlmError::ParseFailed(e.to_string()))?;

        let mut text_parts: Vec<String> = Vec::new();
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        if let Some(content_array) = body.get("content").and_then(|c| c.as_array()) {
            for item in content_array {
                match item.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                            text_parts.push(t.to_string());
                        }
                    }
                    Some("tool_use") => {
                        let id = item.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let name = item.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let arguments = item.get("input")
                            .map(|v| serde_json::to_string(v).unwrap_or_default())
                            .unwrap_or_default();
                        tool_calls.push(ToolCall { id, name, arguments });
                    }
                    _ => {}
                }
            }
        }

        let text = if text_parts.is_empty() { None } else { Some(text_parts.join("")) };
        let tool_calls = if tool_calls.is_empty() { None } else { Some(tool_calls) };

        Ok(LlmResponse { text, tool_calls })
    }
}

// -----------------------------------------------------------------------------
// OpenAI Client implementation (With Device OAuth support)
// -----------------------------------------------------------------------------

/// Default OAuth client ID for the `OpenAI` Codex CLI flow.
pub const OPENAI_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

const OPENAI_DEVICE_CODE_URL: &str =
    "https://auth.openai.com/api/accounts/deviceauth/usercode";
const OPENAI_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const OPENAI_OAUTH_SCOPES: &str = "openid profile email offline_access";

#[derive(Deserialize)]
struct DeviceAuthResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_interval() -> u64 {
    5
}

/// Token response from the device-code polling step.
#[derive(Deserialize)]
#[allow(dead_code)] // access_token is deserialized but only id_token/refresh_token are used
struct DeviceTokenResponse {
    access_token: Option<String>,
    id_token: Option<String>,
    refresh_token: Option<String>,
    error: Option<String>,
}

/// Token response from the token-exchange step (returns an API key).
#[derive(Deserialize)]
struct TokenExchangeResponse {
    access_token: Option<String>,
    error: Option<String>,
}

/// Persisted OAuth tokens so the device flow is only needed once.
#[derive(Serialize, Deserialize, Default)]
struct OAuthTokenCache {
    refresh_token: Option<String>,
}

impl OAuthTokenCache {
    fn path() -> PathBuf {
        if let Some(dirs) = ProjectDirs::from("", "", "ABA") {
            dirs.config_dir().join("oauth-tokens.json")
        } else {
            PathBuf::from(".aba_oauth_tokens.json")
        }
    }

    fn load() -> Self {
        let path = Self::path();
        if path.exists()
            && let Ok(data) = std::fs::read_to_string(&path)
            && let Ok(cache) = serde_json::from_str(&data)
        {
            return cache;
        }
        Self::default()
    }

    fn save(&self) {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, json);
        }
    }
}

pub struct OpenAiOAuthClient {
    client: Client,
    model: String,
    client_id: String,
    api_key: Option<String>,
    base_url: String,
}

impl OpenAiOAuthClient {
    pub fn new(client_id_or_key: String, model: String, is_oauth: bool) -> Self {
        Self {
            client: Client::new(),
            model,
            client_id: if is_oauth {
                client_id_or_key.clone()
            } else {
                String::new()
            },
            api_key: if is_oauth {
                None
            } else {
                Some(client_id_or_key)
            },
            base_url: "https://api.openai.com".to_string(),
        }
    }

    /// Create a client that talks through an API proxy (no API key needed -- proxy injects it).
    pub fn with_proxy(base_url: String, model: String) -> Self {
        Self {
            client: Client::new(),
            model,
            client_id: String::new(),
            api_key: Some(String::new()), // Skip OAuth flow; proxy handles auth
            base_url,
        }
    }

    /// Exchange an `id_token` for an `OpenAI` API key via token-exchange grant.
    async fn exchange_for_api_key(&self, id_token: &str) -> Result<String, LlmError> {
        let res = self
            .client
            .post(OPENAI_TOKEN_URL)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:token-exchange"),
                ("client_id", &self.client_id),
                ("requested_token", "openai-api-key"),
                ("subject_token", id_token),
                (
                    "subject_token_type",
                    "urn:ietf:params:oauth:token-type:id_token",
                ),
            ])
            .send()
            .await
            .map_err(|e| LlmError::RequestFailed(e.to_string()))?;

        let body: TokenExchangeResponse = res
            .json()
            .await
            .map_err(|e| LlmError::ParseFailed(e.to_string()))?;

        if let Some(api_key) = body.access_token {
            Ok(api_key)
        } else {
            let err_msg = body
                .error
                .unwrap_or_else(|| "unknown error".to_string());
            Err(LlmError::Unauthorized(format!(
                "Token exchange failed: {err_msg}"
            )))
        }
    }

    /// Try to refresh an existing token, returning a fresh API key on success.
    async fn try_refresh(&self, refresh_token: &str) -> Result<String, LlmError> {
        info!("Attempting to refresh OAuth token...");
        let res = self
            .client
            .post(OPENAI_TOKEN_URL)
            .form(&[
                ("grant_type", "refresh_token"),
                ("client_id", &self.client_id),
                ("refresh_token", refresh_token),
            ])
            .send()
            .await
            .map_err(|e| LlmError::RequestFailed(e.to_string()))?;

        let body: DeviceTokenResponse = res
            .json()
            .await
            .map_err(|e| LlmError::ParseFailed(e.to_string()))?;

        if let Some(ref err) = body.error {
            return Err(LlmError::Unauthorized(format!(
                "Refresh failed: {err}"
            )));
        }

        let id_token = body.id_token.ok_or_else(|| {
            LlmError::ParseFailed("No id_token in refresh response".to_string())
        })?;

        // Persist the (possibly rotated) refresh token.
        if let Some(new_rt) = &body.refresh_token {
            let mut cache = OAuthTokenCache::load();
            cache.refresh_token = Some(new_rt.clone());
            cache.save();
        }

        let api_key = self.exchange_for_api_key(&id_token).await?;
        info!("Successfully refreshed OAuth token.");
        Ok(api_key)
    }

    /// Obtain an API key via OAuth -- tries cached refresh first, falls back to device flow.
    async fn get_access_token(&self) -> Result<String, LlmError> {
        // Direct API key (no OAuth needed).
        if let Some(key) = &self.api_key {
            return Ok(key.clone());
        }

        // Try cached refresh token first.
        let cache = OAuthTokenCache::load();
        if let Some(ref rt) = cache.refresh_token {
            match self.try_refresh(rt).await {
                Ok(api_key) => return Ok(api_key),
                Err(e) => {
                    info!("Cached refresh token expired or invalid: {e}. Starting device flow.");
                }
            }
        }

        // --- Full device-code flow ---
        info!("Starting OAuth Device Flow for OpenAI Codex...");
        let res = self
            .client
            .post(OPENAI_DEVICE_CODE_URL)
            .form(&[
                ("client_id", self.client_id.as_str()),
                ("scope", OPENAI_OAUTH_SCOPES),
            ])
            .send()
            .await
            .map_err(|e| LlmError::RequestFailed(e.to_string()))?;

        let device_auth: DeviceAuthResponse = res
            .json()
            .await
            .map_err(|e| LlmError::ParseFailed(e.to_string()))?;

        info!("===============================================");
        info!("ACTION REQUIRED: OAuth Login");
        info!("Please visit: {}", device_auth.verification_uri);
        info!("And enter the code: {}", device_auth.user_code);
        info!("Waiting for authorization...");
        info!("===============================================");

        let interval = std::cmp::max(device_auth.interval, 5);

        // Poll for device authorization.
        let device_tokens = loop {
            tokio::time::sleep(Duration::from_secs(interval)).await;

            let res = self
                .client
                .post(OPENAI_TOKEN_URL)
                .form(&[
                    ("client_id", self.client_id.as_str()),
                    (
                        "grant_type",
                        "urn:ietf:params:oauth:grant-type:device_code",
                    ),
                    ("device_code", device_auth.device_code.as_str()),
                ])
                .send()
                .await
                .map_err(|e| LlmError::RequestFailed(e.to_string()))?;

            let token_res: DeviceTokenResponse = res.json().await.unwrap_or(
                DeviceTokenResponse {
                    access_token: None,
                    id_token: None,
                    refresh_token: None,
                    error: None,
                },
            );

            if token_res.id_token.is_some() {
                break token_res;
            }

            if let Some(ref err) = token_res.error
                && err != "authorization_pending"
                && err != "slow_down"
            {
                return Err(LlmError::Unauthorized(format!(
                    "OAuth Error: {err}"
                )));
            }
        };

        // Persist the refresh token for future sessions.
        if let Some(ref rt) = device_tokens.refresh_token {
            let mut new_cache = OAuthTokenCache::load();
            new_cache.refresh_token = Some(rt.clone());
            new_cache.save();
            info!("Saved refresh token for future sessions.");
        }

        let id_token = device_tokens.id_token.ok_or_else(|| {
            LlmError::ParseFailed("No id_token in device flow response".to_string())
        })?;

        // Exchange the id_token for an API key.
        let api_key = self.exchange_for_api_key(&id_token).await?;
        info!("Successfully obtained OAuth API key!");
        Ok(api_key)
    }

    fn convert_messages(messages: &[Message]) -> Vec<serde_json::Value> {
        let mut result = Vec::new();
        for msg in messages {
            match msg.role.as_str() {
                "user" => {
                    result.push(json!({
                        "role": "user",
                        "content": msg.content
                    }));
                }
                "assistant" => {
                    let mut m = json!({
                        "role": "assistant",
                        "content": msg.content
                    });
                    if let Some(ref tcs) = msg.tool_calls {
                        let openai_tool_calls: Vec<serde_json::Value> = tcs.iter().map(|tc| {
                            json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": tc.arguments
                                }
                            })
                        }).collect();
                        m.as_object_mut().unwrap().insert(
                            "tool_calls".to_string(),
                            serde_json::Value::Array(openai_tool_calls),
                        );
                    }
                    result.push(m);
                }
                "tool" => {
                    if let Some(ref tool_call_id) = msg.tool_call_id {
                        result.push(json!({
                            "role": "tool",
                            "tool_call_id": tool_call_id,
                            "content": msg.content
                        }));
                    }
                }
                _ => {}
            }
        }
        result
    }

    fn convert_tools(tools: &[ToolDefinition]) -> Vec<serde_json::Value> {
        tools.iter().map(|td| {
            json!({
                "type": "function",
                "function": {
                    "name": td.name,
                    "description": td.description,
                    "parameters": td.input_schema
                }
            })
        }).collect()
    }
}

#[async_trait]
impl LlmClient for OpenAiOAuthClient {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let token = self.get_access_token().await?;
        info!("Sending request to OpenAI API (model: {})", self.model);

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Authorization", reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")).unwrap());
        headers.insert("content-type", "application/json".parse().unwrap());

        let openai_messages = {
            let mut msgs = vec![json!({"role": "system", "content": req.system_prompt})];
            msgs.extend(Self::convert_messages(&req.messages));
            msgs
        };

        let mut payload = json!({
            "model": self.model,
            "max_tokens": req.max_tokens,
            "temperature": req.temperature,
            "messages": openai_messages
        });

        if !req.tools.is_empty() {
            payload.as_object_mut().unwrap().insert(
                "tools".to_string(),
                serde_json::Value::Array(Self::convert_tools(&req.tools)),
            );
        }

        let url = format!("{}/v1/chat/completions", self.base_url);
        let response = self.client.post(&url)
            .headers(headers).json(&payload).send().await
            .map_err(|e| LlmError::RequestFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(LlmError::RequestFailed(response.status().to_string()));
        }

        let body: serde_json::Value = response.json().await.map_err(|e| LlmError::ParseFailed(e.to_string()))?;

        let mut text = None;
        let mut tool_calls: Vec<ToolCall> = Vec::new();

        if let Some(choices) = body.get("choices").and_then(|c| c.as_array())
            && let Some(first_choice) = choices.first()
            && let Some(msg) = first_choice.get("message")
        {
            if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
                text = Some(content.to_string());
            }
            if let Some(tcs) = msg.get("tool_calls").and_then(|t| t.as_array()) {
                for tc in tcs {
                    let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let name = tc.get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                        .to_string();
                    let arguments = tc.get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|a| a.as_str())
                        .unwrap_or("")
                        .to_string();
                    tool_calls.push(ToolCall { id, name, arguments });
                }
            }
        }

        let tool_calls = if tool_calls.is_empty() { None } else { Some(tool_calls) };

        Ok(LlmResponse { text, tool_calls })
    }
}
