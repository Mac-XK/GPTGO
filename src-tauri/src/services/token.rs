use chrono::{Duration, Utc};
use reqwest::{cookie::Jar, header, Client, Proxy, Url};
use serde::Deserialize;
use std::sync::Arc;

use crate::error::AppResult;
use crate::models::{AccountRecord, AppSettings, TokenActionResult};

const SESSION_URL: &str = "https://chatgpt.com/api/auth/session";
const TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const VALIDATE_URL: &str = "https://chatgpt.com/backend-api/me";

#[derive(Debug, Deserialize)]
struct SessionPayload {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    expires: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthPayload {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

pub async fn refresh_account(
    account: &AccountRecord,
    settings: &AppSettings,
) -> AppResult<TokenActionResult> {
    if let Some(session_token) = account.session_token.as_deref().filter(|v| !v.is_empty()) {
        let result = refresh_by_session_token(session_token, settings).await?;
        if result.success {
            return Ok(result);
        }
    }

    if let Some(refresh_token) = account.refresh_token.as_deref().filter(|v| !v.is_empty()) {
        return refresh_by_oauth_token(refresh_token, settings).await;
    }

    Ok(TokenActionResult {
        success: false,
        message: "账号没有可用的刷新方式（缺少 session_token 和 refresh_token）".to_owned(),
        access_token: None,
        refresh_token: None,
        expires_at: None,
        valid: None,
    })
}

pub async fn validate_access_token(access_token: &str, settings: &AppSettings) -> AppResult<TokenActionResult> {
    let client = build_client(settings)?;
    let response = client
        .get(VALIDATE_URL)
        .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
        .header(header::ACCEPT, "application/json")
        .send()
        .await?;

    let (success, message) = match response.status().as_u16() {
        200 => (true, "Token 有效".to_owned()),
        401 => (false, "Token 无效或已过期".to_owned()),
        403 => (false, "账号可能被封禁".to_owned()),
        code => (false, format!("验证失败: HTTP {code}")),
    };

    Ok(TokenActionResult {
        success,
        message,
        access_token: None,
        refresh_token: None,
        expires_at: None,
        valid: Some(success),
    })
}

async fn refresh_by_session_token(session_token: &str, settings: &AppSettings) -> AppResult<TokenActionResult> {
    let jar = Jar::default();
    let cookie_url = Url::parse("https://chatgpt.com")?;
    jar.add_cookie_str(
        &format!("__Secure-next-auth.session-token={session_token}; Domain=.chatgpt.com; Path=/"),
        &cookie_url,
    );

    let client = build_client_with_jar(settings, Arc::new(jar))?;
    let response = client
        .get(SESSION_URL)
        .header(header::ACCEPT, "application/json")
        .send()
        .await?;

    if !response.status().is_success() {
        return Ok(TokenActionResult {
            success: false,
            message: format!("Session Token 刷新失败: HTTP {}", response.status().as_u16()),
            access_token: None,
            refresh_token: None,
            expires_at: None,
            valid: None,
        });
    }

    let payload: SessionPayload = response.json().await?;
    let access_token = payload.access_token.clone();
    Ok(TokenActionResult {
        success: access_token.is_some(),
        message: if access_token.is_some() {
            "Session Token 刷新成功".to_owned()
        } else {
            "Session Token 刷新失败: 未找到 accessToken".to_owned()
        },
        access_token,
        refresh_token: None,
        expires_at: payload
            .expires
            .and_then(|value| chrono::DateTime::parse_from_rfc3339(&value).ok())
            .map(|value| value.with_timezone(&Utc)),
        valid: None,
    })
}

async fn refresh_by_oauth_token(refresh_token: &str, settings: &AppSettings) -> AppResult<TokenActionResult> {
    let client = build_client(settings)?;
    let response = client
        .post(TOKEN_URL)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .form(&[
            ("client_id", settings.openai_client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("redirect_uri", settings.openai_redirect_uri.as_str()),
        ])
        .send()
        .await?;

    if !response.status().is_success() {
        return Ok(TokenActionResult {
            success: false,
            message: format!("OAuth Token 刷新失败: HTTP {}", response.status().as_u16()),
            access_token: None,
            refresh_token: None,
            expires_at: None,
            valid: None,
        });
    }

    let payload: OAuthPayload = response.json().await?;
    let expires_at = payload
        .expires_in
        .map(|seconds| Utc::now() + Duration::seconds(seconds));
    Ok(TokenActionResult {
        success: payload.access_token.is_some(),
        message: if payload.access_token.is_some() {
            "OAuth Token 刷新成功".to_owned()
        } else {
            "OAuth Token 刷新失败: 未找到 access_token".to_owned()
        },
        access_token: payload.access_token,
        refresh_token: payload.refresh_token,
        expires_at,
        valid: None,
    })
}

fn build_client(settings: &AppSettings) -> AppResult<Client> {
    build_client_with_jar(settings, Arc::new(Jar::default()))
}

fn build_client_with_jar(settings: &AppSettings, jar: Arc<Jar>) -> AppResult<Client> {
    let mut builder = Client::builder()
        .cookie_provider(jar)
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36");

    if settings.proxy_enabled {
        if let Some(proxy_url) = [&settings.proxy_https, &settings.proxy_http, &settings.proxy_all]
            .iter()
            .find_map(|value| {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_owned())
                }
            })
        {
            builder = builder.proxy(Proxy::all(proxy_url)?);
        }
    }

    Ok(builder.build()?)
}
