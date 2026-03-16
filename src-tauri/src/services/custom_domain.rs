use chrono::Utc;
use regex::Regex;
use reqwest::Proxy;
use serde::Deserialize;
use tokio::time::{sleep, Duration};

use crate::error::{AppError, AppResult};
use crate::models::{EmailServiceConfig, PreviewEmail, ServiceTestResult};

#[derive(Debug, Deserialize)]
struct ConfigEnvelope {
    #[serde(rename = "emailDomains")]
    email_domains: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GenerateEnvelope {
    email: Option<String>,
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MessagesEnvelope {
    messages: Option<Vec<MessageItem>>,
}

#[derive(Debug, Deserialize)]
struct MessageItem {
    id: Option<String>,
    #[serde(default)]
    from_address: String,
    #[serde(default)]
    subject: String,
}

#[derive(Debug, Deserialize)]
struct MessageEnvelope {
    message: Option<MessageBody>,
}

#[derive(Debug, Deserialize)]
struct MessageBody {
    #[serde(default)]
    content: String,
    #[serde(default)]
    html: String,
}

pub struct CustomDomainClient {
    config: EmailServiceConfig,
}

impl CustomDomainClient {
    pub fn new(config: EmailServiceConfig) -> Self {
        Self { config }
    }

    pub fn proxy_url(&self) -> Option<String> {
        self.config.proxy_url.clone()
    }

    pub async fn preview_emails(&self, count: usize) -> AppResult<Vec<PreviewEmail>> {
        let client = build_client(self.config.proxy_url.clone())?;
        let mut result = Vec::with_capacity(count);
        let default_domain = self.resolve_default_domain(&client).await?;

        for _ in 0..count {
            let response = client
                .post(format!("{}/api/emails/generate", self.config.base_url.trim_end_matches('/')))
                .header(self.config.api_key_header.as_str(), self.config.api_key.as_str())
                .json(&serde_json::json!({
                    "name": self.config.prefix.clone().unwrap_or_default(),
                    "expiryTime": 3_600_000,
                    "domain": default_domain,
                }))
                .send()
                .await?;
            let payload: GenerateEnvelope = response.json().await?;
            let email = payload
                .email
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| AppError::from("自定义邮箱 API 未返回 email"))?;
            let service_id = payload
                .id
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| AppError::from("自定义邮箱 API 未返回 id"))?;

            result.push(PreviewEmail {
                email,
                service_id,
                created_at: Utc::now(),
                inbox_token: None,
            });
        }

        Ok(result)
    }

    pub async fn test(&self) -> AppResult<ServiceTestResult> {
        let preview = self.preview_emails(1).await?;
        let first = preview.first().ok_or_else(|| AppError::from("测试未生成邮箱"))?;
        Ok(ServiceTestResult {
            success: true,
            message: format!("自定义邮箱 API 可用，测试邮箱: {}", first.email),
        })
    }

    pub async fn wait_for_verification_code(
        &self,
        preview_email: &PreviewEmail,
        _sent_at: i64,
    ) -> AppResult<String> {
        let client = build_client(self.config.proxy_url.clone())?;
        let matcher = Regex::new(r"(?<!\d)(\d{6})(?!\d)")?;

        for _ in 0..40 {
            let response = client
                .get(format!(
                    "{}/api/emails/{}",
                    self.config.base_url.trim_end_matches('/'),
                    preview_email.service_id
                ))
                .header(self.config.api_key_header.as_str(), self.config.api_key.as_str())
                .send()
                .await?;
            let envelope: MessagesEnvelope = response.json().await?;

            for message in envelope.messages.unwrap_or_default() {
                let message_id = match message.id {
                    Some(value) if !value.trim().is_empty() => value,
                    _ => continue,
                };

                let detail = client
                    .get(format!(
                        "{}/api/emails/{}/{}",
                        self.config.base_url.trim_end_matches('/'),
                        preview_email.service_id,
                        message_id
                    ))
                    .header(self.config.api_key_header.as_str(), self.config.api_key.as_str())
                    .send()
                    .await?;
                let body: MessageEnvelope = detail.json().await?;
                let message_body = body.message.unwrap_or(MessageBody {
                    content: String::new(),
                    html: String::new(),
                });
                let merged = format!(
                    "{}\n{}\n{}\n{}",
                    message.from_address,
                    message.subject,
                    message_body.content,
                    message_body.html
                );

                if !merged.to_lowercase().contains("openai") {
                    continue;
                }

                if let Some(captures) = matcher.captures(&merged) {
                    if let Some(matched) = captures.get(1) {
                        return Ok(matched.as_str().to_owned());
                    }
                }
            }

            sleep(Duration::from_secs(3)).await;
        }

        Err(AppError::from("等待自定义邮箱验证码超时"))
    }

    async fn resolve_default_domain(&self, client: &reqwest::Client) -> AppResult<String> {
        if let Some(domain) = self
            .config
            .default_domain
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        {
            return Ok(domain.trim().to_owned());
        }

        let response = client
            .get(format!("{}/api/config", self.config.base_url.trim_end_matches('/')))
            .header(self.config.api_key_header.as_str(), self.config.api_key.as_str())
            .send()
            .await?;
        let payload: ConfigEnvelope = response.json().await?;
        let domain = payload
            .email_domains
            .unwrap_or_default()
            .split(',')
            .find_map(|item| {
                let trimmed = item.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_owned())
                }
            })
            .ok_or_else(|| AppError::from("无法从自定义邮箱 API 配置中解析默认域名"))?;
        Ok(domain)
    }
}

fn build_client(proxy_override: Option<String>) -> AppResult<reqwest::Client> {
    let mut builder = reqwest::Client::builder()
        .cookie_store(true)
        .user_agent("GPTGO/0.1");

    if let Some(proxy_url) = proxy_override
        .filter(|value| !value.trim().is_empty())
        .or_else(|| std::env::var("HTTPS_PROXY").ok())
        .or_else(|| std::env::var("https_proxy").ok())
        .or_else(|| std::env::var("ALL_PROXY").ok())
        .or_else(|| std::env::var("all_proxy").ok())
        .filter(|value| !value.trim().is_empty())
    {
        builder = builder.proxy(Proxy::all(proxy_url)?);
    }

    Ok(builder.build()?)
}
