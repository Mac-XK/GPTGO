use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::anyhow;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{Datelike, Utc};
use rand::Rng;
use reqwest::{header, redirect::Policy, Client, Proxy, Response};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::error::AppResult;
use crate::models::{EmailServiceConfig, EmailServiceType, PreviewEmail, RegistrationSummary};
use crate::services::custom_domain::CustomDomainClient;
use crate::services::gptmail::GptMailClient;

use super::constants::*;

type LogFn = Box<dyn Fn(String) + Send + Sync>;

pub struct OpenAiRegistrationEngine {
    client: Client,
    cookies: HashMap<String, String>,
    mail_provider: MailProvider,
    email: String,
    preview_email: Option<PreviewEmail>,
    log: LogFn,
    password: Option<String>,
    oauth_state: String,
    code_verifier: String,
    auth_url: String,
    is_existing_account: bool,
}

#[derive(Debug, Deserialize)]
struct SignupResponse {
    page: Option<SignupPage>,
}

#[derive(Debug, Deserialize)]
struct SignupPage {
    #[serde(rename = "type")]
    page_type: Option<String>,
}

pub struct EngineFactory;

impl EngineFactory {
    pub fn build(
        config: EmailServiceConfig,
        preview_email: Option<PreviewEmail>,
        log: impl Fn(String) + Send + Sync + 'static,
    ) -> AppResult<OpenAiRegistrationEngine> {
        let client = build_client(config.proxy_url.clone())?;
        let mail_provider = match config.service_type {
            EmailServiceType::Gptmail => MailProvider::GptMail(GptMailClient::new(config)),
            EmailServiceType::CustomDomain => MailProvider::CustomDomain(CustomDomainClient::new(config)),
        };
        let email = preview_email
            .as_ref()
            .map(|item| item.email.clone())
            .unwrap_or_default();
        let (oauth_state, code_verifier, auth_url) = generate_oauth_url();

        Ok(OpenAiRegistrationEngine {
            client,
            cookies: HashMap::new(),
            mail_provider,
            email,
            preview_email,
            log: Box::new(log),
            password: None,
            oauth_state,
            code_verifier,
            auth_url,
            is_existing_account: false,
        })
    }
}

impl OpenAiRegistrationEngine {
    pub async fn run(mut self) -> AppResult<RegistrationSummary> {
        self.push_log("开始单次注册流程");

        let location = self.check_ip_location().await?;
        self.push_log(format!("IP 位置: {location}"));

        if self.email.is_empty() {
            let preview = self
                .mail_provider
                .preview_emails(1)
                .await?
                .into_iter()
                .next()
                .ok_or_else(|| anyhow!("预生成邮箱为空"))?;
            self.email = preview.email.clone();
            self.preview_email = Some(preview);
        } else {
            self.push_log(format!("使用预生成邮箱: {}", self.email));
        }

        let did = self.fetch_device_id().await?;
        self.push_log(format!("Device ID: {did}"));

        let sentinel_token = self.check_sentinel(&did).await?;
        if sentinel_token.is_some() {
            self.push_log("Sentinel 检查通过");
        } else {
            self.push_log("Sentinel 未返回 token，继续尝试注册");
        }

        self.submit_signup_form(&did, sentinel_token.as_deref()).await?;

        let otp_sent_at = if self.is_existing_account {
            self.push_log("检测到已注册账号，跳过密码设置和验证码发送");
            current_ts()
        } else {
            self.register_password().await?;
            self.send_verification_code().await?;
            current_ts()
        };

        let code = self
            .mail_provider
            .wait_for_verification_code(
                self.preview_email
                    .as_ref()
                    .ok_or_else(|| anyhow!("缺少预生成邮箱信息"))?,
                otp_sent_at,
            )
            .await?;
        self.push_log(format!("成功获取验证码: {code}"));

        self.validate_verification_code(&code).await?;

        if !self.is_existing_account {
            self.create_user_account().await?;
        }

        let workspace_id = self.get_workspace_id()?;
        self.push_log(format!("Workspace ID: {workspace_id}"));

        let continue_url = self.select_workspace(&workspace_id).await?;
        let callback_url = self.follow_redirects(&continue_url).await?;
        let token_info = self.handle_oauth_callback(&callback_url).await?;

        if let Some(session_token) = self.cookies.get("__Secure-next-auth.session-token") {
            self.push_log("获取到 Session Token");
            self.cookies
                .insert("__Secure-next-auth.session-token".to_owned(), session_token.clone());
        }

        self.push_log("注册流程完成");
        Ok(RegistrationSummary {
            email: self.email.clone(),
            success: true,
            workspace_id: Some(workspace_id),
            account_id: token_info.account_id.clone(),
            password: self.password.clone(),
            access_token: Some(token_info.access_token),
            refresh_token: Some(token_info.refresh_token),
            id_token: Some(token_info.id_token),
            error_message: None,
        })
    }

    async fn check_ip_location(&mut self) -> AppResult<String> {
        let response = self.client.get("https://cloudflare.com/cdn-cgi/trace").send().await?;
        let body = response.text().await?;
        let loc = body
            .lines()
            .find_map(|line| line.strip_prefix("loc="))
            .unwrap_or("")
            .to_owned();
        if matches!(loc.as_str(), "CN" | "HK" | "MO" | "TW") {
            return Err(anyhow!("当前 IP 地理位置不支持: {loc}").into());
        }
        Ok(loc)
    }

    async fn fetch_device_id(&mut self) -> AppResult<String> {
        let response = self.client.get(&self.auth_url).send().await?;
        self.capture_cookies(&response);
        self.cookies
            .get("oai-did")
            .cloned()
            .ok_or_else(|| anyhow!("未能获取到 oai-did Cookie").into())
    }

    async fn check_sentinel(&mut self, did: &str) -> AppResult<Option<String>> {
        let response = self
            .client
            .post(SENTINEL_URL)
            .header("origin", "https://sentinel.openai.com")
            .header(
                "referer",
                "https://sentinel.openai.com/backend-api/sentinel/frame.html?sv=20260219f9f6",
            )
            .header("content-type", "text/plain;charset=UTF-8")
            .body(format!(r#"{{"p":"","id":"{did}","flow":"authorize_continue"}}"#))
            .send()
            .await?;
        self.capture_cookies(&response);
        if !response.status().is_success() {
            return Ok(None);
        }
        let value = response.json::<serde_json::Value>().await?;
        Ok(value.get("token").and_then(|value| value.as_str()).map(ToOwned::to_owned))
    }

    async fn submit_signup_form(&mut self, did: &str, sentinel_token: Option<&str>) -> AppResult<()> {
        let mut request = self
            .client
            .post(SIGNUP_URL)
            .header("referer", "https://auth.openai.com/create-account")
            .header("accept", "application/json")
            .header("content-type", "application/json");

        if let Some(token) = sentinel_token {
            request = request.header(
                "openai-sentinel-token",
                format!(r#"{{"p": "", "t": "", "c": "{token}", "id": "{did}", "flow": "authorize_continue"}}"#),
            );
        }

        let response = request
            .body(format!(
                r#"{{"username":{{"value":"{}","kind":"email"}},"screen_hint":"signup"}}"#,
                self.email
            ))
            .send()
            .await?;
        self.capture_cookies(&response);
        let status = response.status();
        let body = response.text().await?;
        self.push_log(format!("提交注册表单状态: {}", status.as_u16()));
        if !status.is_success() {
            return Err(anyhow!("提交注册表单失败: {}", body).into());
        }
        let parsed: SignupResponse = serde_json::from_str(&body)?;
        let page_type = parsed
            .page
            .and_then(|page| page.page_type)
            .unwrap_or_default();
        self.push_log(format!("响应页面类型: {page_type}"));
        self.is_existing_account = page_type == "email_otp_verification";
        Ok(())
    }

    async fn register_password(&mut self) -> AppResult<()> {
        let password = generate_password();
        let response = self
            .client
            .post(REGISTER_URL)
            .header("referer", "https://auth.openai.com/create-account/password")
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .body(json!({ "password": password, "username": self.email }).to_string())
            .send()
            .await?;
        self.capture_cookies(&response);
        self.push_log(format!("提交密码状态: {}", response.status().as_u16()));
        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("注册密码失败: {body}").into());
        }
        self.password = Some(password);
        Ok(())
    }

    async fn send_verification_code(&mut self) -> AppResult<()> {
        let response = self
            .client
            .get(SEND_OTP_URL)
            .header("referer", "https://auth.openai.com/create-account/password")
            .header("accept", "application/json")
            .send()
            .await?;
        self.capture_cookies(&response);
        self.push_log(format!("验证码发送状态: {}", response.status().as_u16()));
        if !response.status().is_success() {
            return Err(anyhow!("发送验证码失败").into());
        }
        Ok(())
    }

    async fn validate_verification_code(&mut self, code: &str) -> AppResult<()> {
        let response = self
            .client
            .post(VALIDATE_OTP_URL)
            .header("referer", "https://auth.openai.com/email-verification")
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .body(json!({ "code": code }).to_string())
            .send()
            .await?;
        self.capture_cookies(&response);
        self.push_log(format!("验证码校验状态: {}", response.status().as_u16()));
        if !response.status().is_success() {
            return Err(anyhow!("验证验证码失败").into());
        }
        Ok(())
    }

    async fn create_user_account(&mut self) -> AppResult<()> {
        let profile = generate_random_user_info();
        self.push_log(format!(
            "生成用户信息: {}, 生日: {}",
            profile.name, profile.birthdate
        ));
        let response = self
            .client
            .post(CREATE_ACCOUNT_URL)
            .header("referer", "https://auth.openai.com/about-you")
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .body(json!({ "name": profile.name, "birthdate": profile.birthdate }).to_string())
            .send()
            .await?;
        self.capture_cookies(&response);
        self.push_log(format!("账户创建状态: {}", response.status().as_u16()));
        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("创建账户失败: {body}").into());
        }
        Ok(())
    }

    fn get_workspace_id(&self) -> AppResult<String> {
        let auth_cookie = self
            .cookies
            .get("oai-client-auth-session")
            .cloned()
            .ok_or_else(|| anyhow!("缺少 oai-client-auth-session cookie"))?;

        let first_segment = auth_cookie
            .split('.')
            .next()
            .ok_or_else(|| anyhow!("授权 cookie 格式无效"))?;
        let decoded = decode_b64_json(first_segment)?;
        let workspaces = decoded
            .get("workspaces")
            .and_then(|value| value.as_array())
            .ok_or_else(|| anyhow!("授权 cookie 不包含 workspace"))?;
        let workspace_id = workspaces
            .first()
            .and_then(|item| item.get("id"))
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_owned();
        if workspace_id.is_empty() {
            return Err(anyhow!("无法解析 workspace_id").into());
        }
        Ok(workspace_id)
    }

    async fn select_workspace(&mut self, workspace_id: &str) -> AppResult<String> {
        let response = self
            .client
            .post(SELECT_WORKSPACE_URL)
            .header("referer", "https://auth.openai.com/sign-in-with-chatgpt/codex/consent")
            .header("content-type", "application/json")
            .body(json!({ "workspace_id": workspace_id }).to_string())
            .send()
            .await?;
        self.capture_cookies(&response);
        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("选择 workspace 失败: {body}").into());
        }
        let body = response.json::<serde_json::Value>().await?;
        body.get("continue_url")
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned)
            .ok_or_else(|| anyhow!("continue_url 缺失").into())
    }

    async fn follow_redirects(&mut self, start_url: &str) -> AppResult<String> {
        let mut current_url = start_url.to_owned();
        for index in 0..6 {
            self.push_log(format!("重定向 {}/6", index + 1));
            let response = self.client.get(&current_url).send().await?;
            self.capture_cookies(&response);
            if !response.status().is_redirection() {
                break;
            }
            let location = response
                .headers()
                .get(header::LOCATION)
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| anyhow!("重定向缺少 Location"))?;
            let next_url = reqwest::Url::parse(&current_url)?.join(location)?.to_string();
            if next_url.contains("code=") && next_url.contains("state=") {
                return Ok(next_url);
            }
            current_url = next_url;
        }
        Err(anyhow!("未能在重定向链中找到 OAuth callback").into())
    }

    async fn handle_oauth_callback(&self, callback_url: &str) -> AppResult<TokenInfo> {
        let url = reqwest::Url::parse(callback_url)?;
        let query = url.query_pairs().collect::<HashMap<_, _>>();
        let code = query
            .get("code")
            .ok_or_else(|| anyhow!("callback 缺少 code"))?
            .to_string();
        let state = query
            .get("state")
            .ok_or_else(|| anyhow!("callback 缺少 state"))?
            .to_string();
        if state != self.oauth_state {
            return Err(anyhow!("OAuth state 不匹配").into());
        }

        let response = build_client(self.mail_provider.proxy_url())?
            .post(OAUTH_TOKEN_URL)
            .header("content-type", "application/x-www-form-urlencoded")
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", OAUTH_CLIENT_ID),
                ("code", &code),
                ("redirect_uri", OAUTH_REDIRECT_URI),
                ("code_verifier", &self.code_verifier),
            ])
            .send()
            .await?;
        let payload: serde_json::Value = response.json().await?;
        let id_token = payload
            .get("id_token")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_owned();
        let claims = decode_jwt_claims(&id_token)?;
        let account_id = claims
            .get("https://api.openai.com/auth")
            .and_then(|value| value.get("chatgpt_account_id"))
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned);
        Ok(TokenInfo {
            access_token: payload
                .get("access_token")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_owned(),
            refresh_token: payload
                .get("refresh_token")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_owned(),
            id_token,
            account_id,
        })
    }

    fn capture_cookies(&mut self, response: &Response) {
        for value in response.headers().get_all(header::SET_COOKIE) {
            if let Ok(raw) = value.to_str() {
                if let Some((name, rest)) = raw.split_once('=') {
                    let cookie_value = rest.split(';').next().unwrap_or("").to_owned();
                    self.cookies.insert(name.to_owned(), cookie_value);
                }
            }
        }
    }

    fn push_log(&self, message: impl Into<String>) {
        (self.log)(message.into());
    }
}

enum MailProvider {
    GptMail(GptMailClient),
    CustomDomain(CustomDomainClient),
}

impl MailProvider {
    async fn preview_emails(&self, count: usize) -> AppResult<Vec<PreviewEmail>> {
        match self {
            Self::GptMail(client) => client.preview_emails(count).await,
            Self::CustomDomain(client) => client.preview_emails(count).await,
        }
    }

    async fn wait_for_verification_code(
        &self,
        preview_email: &PreviewEmail,
        sent_at: i64,
    ) -> AppResult<String> {
        match self {
            Self::GptMail(client) => client.wait_for_verification_code(&preview_email.email, sent_at).await,
            Self::CustomDomain(client) => client.wait_for_verification_code(preview_email, sent_at).await,
        }
    }

    fn proxy_url(&self) -> Option<String> {
        match self {
            Self::GptMail(client) => client.proxy_url(),
            Self::CustomDomain(client) => client.proxy_url(),
        }
    }
}

struct TokenInfo {
    access_token: String,
    refresh_token: String,
    id_token: String,
    account_id: Option<String>,
}

struct RandomUserProfile {
    name: String,
    birthdate: String,
}

fn build_client(proxy_override: Option<String>) -> AppResult<Client> {
    let mut builder = Client::builder()
        .cookie_store(true)
        .redirect(Policy::none())
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36");

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


fn generate_oauth_url() -> (String, String, String) {
    let state = uuid::Uuid::new_v4().to_string();
    let code_verifier = uuid::Uuid::new_v4().to_string().replace('-', "") + &uuid::Uuid::new_v4().to_string().replace('-', "");
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(code_verifier.as_bytes()));
    let auth_url = format!(
        "{OAUTH_AUTH_URL}?client_id={}&response_type=code&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256&prompt=login&id_token_add_organizations=true&codex_cli_simplified_flow=true",
        urlencoding::encode(OAUTH_CLIENT_ID),
        urlencoding::encode(OAUTH_REDIRECT_URI),
        urlencoding::encode(OAUTH_SCOPE),
        urlencoding::encode(&state),
        urlencoding::encode(&challenge),
    );
    (state, code_verifier, auth_url)
}

fn decode_b64_json(raw: &str) -> AppResult<serde_json::Value> {
    let bytes = URL_SAFE_NO_PAD.decode(raw)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn decode_jwt_claims(id_token: &str) -> AppResult<serde_json::Value> {
    let payload = id_token
        .split('.')
        .nth(1)
        .ok_or_else(|| anyhow!("无效的 id_token"))?;
    decode_b64_json(payload)
}

fn generate_password() -> String {
    let mut rng = rand::thread_rng();
    (0..12)
        .map(|_| {
            let idx = rng.gen_range(0..PASSWORD_CHARSET.len());
            PASSWORD_CHARSET[idx] as char
        })
        .collect()
}

fn generate_random_user_info() -> RandomUserProfile {
    const FIRST_NAMES: &[&str] = &[
        "James", "John", "Robert", "Michael", "William", "David", "Richard", "Joseph", "Thomas",
        "Charles", "Emma", "Olivia", "Ava", "Isabella", "Sophia", "Mia", "Charlotte", "Amelia",
        "Harper", "Evelyn", "Alex", "Jordan", "Taylor", "Morgan", "Casey", "Riley", "Jamie",
        "Avery", "Quinn", "Skyler",
    ];
    let mut rng = rand::thread_rng();
    let name = FIRST_NAMES[rng.gen_range(0..FIRST_NAMES.len())].to_owned();
    let current_year = Utc::now().year();
    let year = rng.gen_range((current_year - 45)..=(current_year - 18));
    let month = rng.gen_range(1..=12);
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ => 28,
    };
    let day = rng.gen_range(1..=max_day);
    RandomUserProfile {
        name,
        birthdate: format!("{year:04}-{month:02}-{day:02}"),
    }
}

fn current_ts() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_secs() as i64
}
