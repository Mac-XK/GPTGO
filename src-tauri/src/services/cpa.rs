use chrono::Utc;
use reqwest::{multipart, Client, Proxy};
use serde_json::json;

use crate::error::{AppError, AppResult};
use crate::models::{AccountRecord, AppSettings, DatabaseActionResult, ExportPayload};

pub fn generate_token_json(account: &AccountRecord) -> serde_json::Value {
    json!({
        "type": "codex",
        "email": account.email,
        "expired": account.expires_at.map(|v| v.to_rfc3339()).unwrap_or_default(),
        "id_token": account.id_token.clone().unwrap_or_default(),
        "account_id": account.account_id.clone().unwrap_or_default(),
        "access_token": account.access_token.clone().unwrap_or_default(),
        "last_refresh": account.last_refresh.map(|v| v.to_rfc3339()).unwrap_or_default(),
        "refresh_token": account.refresh_token.clone().unwrap_or_default(),
    })
}

pub fn export_cpa_payload(accounts: &[AccountRecord]) -> AppResult<ExportPayload> {
    let payloads = accounts.iter().map(generate_token_json).collect::<Vec<_>>();
    Ok(ExportPayload {
        filename: format!("cpa-tokens-{}.json", Utc::now().format("%Y%m%d-%H%M%S")),
        content: serde_json::to_string_pretty(&payloads)?,
    })
}

pub async fn upload_accounts(accounts: &[AccountRecord], settings: &AppSettings) -> AppResult<DatabaseActionResult> {
    if !settings.cpa_enabled {
      return Err(AppError::from("CPA 上传未启用"));
    }
    if settings.cpa_api_url.trim().is_empty() || settings.cpa_api_token.trim().is_empty() {
      return Err(AppError::from("CPA API URL 或 Token 未配置"));
    }

    let client = build_client()?;
    let mut success = 0usize;
    for account in accounts {
      let token_data = generate_token_json(account);
      let filename = format!("{}.json", account.email);
      let part = multipart::Part::bytes(serde_json::to_vec_pretty(&token_data)?)
        .file_name(filename)
        .mime_str("application/json")
        .map_err(|e| AppError::from(e.to_string()))?;
      let form = multipart::Form::new().part("file", part);
      let response = client
        .post(format!("{}/v0/management/auth-files", settings.cpa_api_url.trim_end_matches('/')))
        .bearer_auth(&settings.cpa_api_token)
        .multipart(form)
        .send()
        .await?;
      if response.status().is_success() {
        success += 1;
      }
    }

    Ok(DatabaseActionResult {
      success: success == accounts.len(),
      message: format!("已上传 {} / {} 个账号到 CPA", success, accounts.len()),
    })
}

fn build_client() -> AppResult<Client> {
    let mut builder = Client::builder().user_agent("GPTGO/0.1");
    if let Some(proxy_url) = std::env::var("HTTPS_PROXY")
        .ok()
        .or_else(|| std::env::var("https_proxy").ok())
        .or_else(|| std::env::var("ALL_PROXY").ok())
        .or_else(|| std::env::var("all_proxy").ok())
        .filter(|value| !value.trim().is_empty())
    {
        builder = builder.proxy(Proxy::all(proxy_url)?);
    }
    Ok(builder.build()?)
}
