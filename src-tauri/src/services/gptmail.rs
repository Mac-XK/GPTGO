use chrono::Utc;
use reqwest::Proxy;
use serde::Deserialize;
use tokio::time::{sleep, Duration};

use crate::error::{AppError, AppResult};
use crate::models::{EmailServiceConfig, PreviewEmail, ServiceTestResult};

#[derive(Debug, Deserialize)]
struct GenerateEmailEnvelope {
    success: bool,
    data: Option<GenerateEmailData>,
    error: Option<String>,
    auth: Option<AuthData>,
}

#[derive(Debug, Deserialize)]
struct GenerateEmailData {
    email: String,
}

#[derive(Debug, Deserialize)]
struct AuthData {
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InboxEnvelope {
    success: bool,
    data: Option<InboxData>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct InboxData {
    emails: Vec<InboxEmail>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InboxEmail {
    pub from_address: String,
    pub subject: Option<String>,
    pub content: Option<String>,
    pub html_content: Option<String>,
    pub timestamp: Option<i64>,
}

pub struct GptMailClient {
    config: EmailServiceConfig,
}

impl GptMailClient {
    pub fn new(config: EmailServiceConfig) -> Self {
        Self { config }
    }

    pub fn proxy_url(&self) -> Option<String> {
        self.config.proxy_url.clone()
    }

    pub async fn preview_emails(&self, count: usize) -> AppResult<Vec<PreviewEmail>> {
        let client = build_client(self.config.proxy_url.clone())?;
        let mut emails = Vec::with_capacity(count);

        for _ in 0..count {
            let request = if let Some(prefix) = self.config.prefix.as_deref().filter(|value| !value.is_empty()) {
                client
                    .post(format!("{}/generate-email", self.config.base_url.trim_end_matches('/')))
                    .header("X-API-Key", &self.config.api_key)
                    .json(&serde_json::json!({ "prefix": prefix }))
            } else {
                client
                    .get(format!("{}/generate-email", self.config.base_url.trim_end_matches('/')))
                    .header("X-API-Key", &self.config.api_key)
            };

            let response = request.send().await?;
            let payload: GenerateEmailEnvelope = response.json().await?;
            if !payload.success {
                return Err(AppError::from(
                    payload.error.unwrap_or_else(|| "GPTMail 生成邮箱失败".to_owned()),
                ));
            }

            let data = payload
                .data
                .ok_or_else(|| AppError::from("GPTMail 返回缺少邮箱数据"))?;

            emails.push(PreviewEmail {
                email: data.email.clone(),
                service_id: data.email,
                created_at: Utc::now(),
                inbox_token: payload.auth.and_then(|auth| auth.token),
            });
        }

        Ok(emails)
    }

    pub async fn test(&self) -> AppResult<ServiceTestResult> {
        let preview = self.preview_emails(1).await?;
        let first = preview.first().ok_or_else(|| AppError::from("测试未生成邮箱"))?;
        Ok(ServiceTestResult {
            success: true,
            message: format!("GPTMail 连接正常，测试邮箱: {}", first.email),
        })
    }

    pub async fn wait_for_verification_code(
        &self,
        email: &str,
        sent_at: i64,
    ) -> AppResult<String> {
        let code_pattern = regex::Regex::new(r"(?<!\d)(\d{6})(?!\d)")?;

        for _ in 0..40 {
            let inbox = self.fetch_inbox(email).await?;
            for message in inbox {
                if message.timestamp.unwrap_or_default() + 2 < sent_at {
                    continue;
                }

                let combined = format!(
                    "{}\n{}\n{}\n{}",
                    message.from_address,
                    message.subject.clone().unwrap_or_default(),
                    message.content.clone().unwrap_or_default(),
                    message.html_content.clone().unwrap_or_default()
                );

                if !combined.to_lowercase().contains("openai") {
                    continue;
                }

                if let Some(captures) = code_pattern.captures(&combined) {
                    if let Some(matched) = captures.get(1) {
                        return Ok(matched.as_str().to_owned());
                    }
                }
            }

            sleep(Duration::from_secs(3)).await;
        }

        Err(AppError::from("等待 GPTMail 验证码超时"))
    }

    async fn fetch_inbox(&self, email: &str) -> AppResult<Vec<InboxEmail>> {
        let client = build_client(self.config.proxy_url.clone())?;
        let response = client
            .get(format!(
                "{}/emails",
                self.config.base_url.trim_end_matches('/')
            ))
            .query(&[("email", email)])
            .header("X-API-Key", &self.config.api_key)
            .send()
            .await?;
        let payload: InboxEnvelope = response.json().await?;
        if !payload.success {
            return Err(AppError::from(
                payload.error.unwrap_or_else(|| "GPTMail 拉取收件箱失败".to_owned()),
            ));
        }
        Ok(payload.data.map(|value| value.emails).unwrap_or_default())
    }
}

fn build_client(proxy_override: Option<String>) -> AppResult<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .cookie_store(true)
        .user_agent("codex-register-rust/0.1");

    if let Some(proxy_url) = self_or_env_proxy(proxy_override) {
        builder = builder.proxy(Proxy::all(proxy_url)?);
    }

    Ok(builder.build()?)
}

fn self_or_env_proxy(explicit: Option<String>) -> Option<String> {
    explicit
        .filter(|value| !value.trim().is_empty())
        .or_else(|| std::env::var("HTTPS_PROXY").ok())
        .or_else(|| std::env::var("https_proxy").ok())
        .or_else(|| std::env::var("ALL_PROXY").ok())
        .or_else(|| std::env::var("all_proxy").ok())
        .filter(|value| !value.trim().is_empty())
}
