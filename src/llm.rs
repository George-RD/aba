use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use reqwest::Client;
use serde_json::json;
use tracing::info;
use std::time::Duration;

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
pub struct LlmRequest {
    pub system_prompt: String,
    pub user_prompt: String,
    pub max_tokens: u32,
    pub temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

impl AnthropicClient {
    pub fn new(api_key: String, model: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            model,
        }
    }
}

#[async_trait]
impl LlmClient for AnthropicClient {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        info!("Sending request to Anthropic API (model: {})", self.model);
        
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("x-api-key", reqwest::header::HeaderValue::from_str(&self.api_key).unwrap());
        headers.insert("anthropic-version", "2023-06-01".parse().unwrap());
        headers.insert("content-type", "application/json".parse().unwrap());

        let payload = json!({
            "model": self.model,
            "max_tokens": req.max_tokens,
            "temperature": req.temperature,
            "system": req.system_prompt,
            "messages": [
                {
                    "role": "user",
                    "content": req.user_prompt
                }
            ]
        });

        let response = self.client.post("https://api.anthropic.com/v1/messages")
            .headers(headers).json(&payload).send().await
            .map_err(|e| LlmError::RequestFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(LlmError::RequestFailed(response.status().to_string()));
        }

        let body: serde_json::Value = response.json().await.map_err(|e| LlmError::ParseFailed(e.to_string()))?;
        
        let mut text = None;
        if let Some(content_array) = body.get("content").and_then(|c| c.as_array()) {
            for item in content_array {
                if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                        text = Some(t.to_string());
                    }
                }
            }
        }

        Ok(LlmResponse { text, tool_calls: None })
    }
}

// -----------------------------------------------------------------------------
// OpenAI Client implementation (With Device OAuth support)
// -----------------------------------------------------------------------------

#[derive(Deserialize)]
struct DeviceAuthResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    interval: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    error: Option<String>,
}

pub struct OpenAiOAuthClient {
    client: Client,
    model: String,
    client_id: String,
    api_key: Option<String>,
}

impl OpenAiOAuthClient {
    pub fn new(client_id_or_key: String, model: String, is_oauth: bool) -> Self {
        Self {
            client: Client::new(),
            model,
            client_id: if is_oauth { client_id_or_key.clone() } else { "".to_string() },
            api_key: if !is_oauth { Some(client_id_or_key) } else { None },
        }
    }

    async fn get_access_token(&self) -> Result<String, LlmError> {
        if let Some(key) = &self.api_key {
            return Ok(key.clone());
        }

        let auth_url = "https://auth0.openai.com/oauth/device/code";
        let token_url = "https://auth0.openai.com/oauth/token";

        info!("Starting OAuth Device Flow for Codex Subscription...");
        let res = self.client.post(auth_url)
            .form(&[("client_id", &self.client_id), ("scope", &"offline_access openid profile".to_string())])
            .send().await.map_err(|e| LlmError::RequestFailed(e.to_string()))?;

        let device_auth: DeviceAuthResponse = res.json().await.map_err(|e| LlmError::ParseFailed(e.to_string()))?;

        info!("===============================================");
        info!("ACTION REQUIRED: OAuth Login");
        info!("Please visit: {}", device_auth.verification_uri);
        info!("And enter the code: {}", device_auth.user_code);
        info!("Waiting for authorization...");
        info!("===============================================");

        let interval = std::cmp::max(device_auth.interval, 5);
        
        loop {
            tokio::time::sleep(Duration::from_secs(interval)).await;
            
            let res = self.client.post(token_url)
                .form(&[
                    ("client_id", &self.client_id),
                    ("grant_type", &"urn:ietf:params:oauth:grant-type:device_code".to_string()),
                    ("device_code", &device_auth.device_code),
                ]).send().await.map_err(|e| LlmError::RequestFailed(e.to_string()))?;

            let token_res: TokenResponse = res.json().await.unwrap_or(TokenResponse { access_token: None, error: None });

            if let Some(token) = token_res.access_token {
                info!("Successfully obtained OAuth access token!");
                return Ok(token);
            } else if let Some(err) = token_res.error {
                if err != "authorization_pending" {
                    return Err(LlmError::Unauthorized(format!("OAuth Error: {}", err)));
                }
            }
        }
    }
}

#[async_trait]
impl LlmClient for OpenAiOAuthClient {
    async fn complete(&self, req: LlmRequest) -> Result<LlmResponse, LlmError> {
        let token = self.get_access_token().await?;
        info!("Sending request to OpenAI API (model: {})", self.model);
        
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("Authorization", reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token)).unwrap());
        headers.insert("content-type", "application/json".parse().unwrap());

        let payload = json!({
            "model": self.model,
            "max_tokens": req.max_tokens,
            "temperature": req.temperature,
            "messages": [
                { "role": "system", "content": req.system_prompt },
                { "role": "user", "content": req.user_prompt }
            ]
        });

        let response = self.client.post("https://api.openai.com/v1/chat/completions")
            .headers(headers).json(&payload).send().await
            .map_err(|e| LlmError::RequestFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(LlmError::RequestFailed(response.status().to_string()));
        }

        let body: serde_json::Value = response.json().await.map_err(|e| LlmError::ParseFailed(e.to_string()))?;
        
        let mut text = None;
        if let Some(choices) = body.get("choices").and_then(|c| c.as_array()) {
            if let Some(first_choice) = choices.first() {
                if let Some(msg) = first_choice.get("message") {
                    if let Some(content) = msg.get("content").and_then(|c| c.as_str()) {
                        text = Some(content.to_string());
                    }
                }
            }
        }

        Ok(LlmResponse { text, tool_calls: None })
    }
}
