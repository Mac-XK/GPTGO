use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmailServiceType {
    Gptmail,
    CustomDomain,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailServiceRecord {
    pub id: i64,
    pub service_type: EmailServiceType,
    pub name: String,
    pub base_url: String,
    pub has_api_key: bool,
    pub prefix: Option<String>,
    pub enabled: bool,
    pub priority: i64,
    pub last_used: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountRecord {
    pub id: i64,
    pub email: String,
    pub status: String,
    pub password: Option<String>,
    pub workspace_id: Option<String>,
    pub account_id: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub session_token: Option<String>,
    pub last_refresh: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub cpa_uploaded: bool,
    pub cpa_uploaded_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapPayload {
    pub services: Vec<EmailServiceRecord>,
    pub accounts: Vec<AccountRecord>,
    pub settings: AppSettings,
    pub account_stats: AccountStats,
    pub database_info: DatabaseInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailServiceInput {
    pub id: Option<i64>,
    pub service_type: EmailServiceType,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub prefix: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub proxy_enabled: bool,
    pub proxy_http: String,
    pub proxy_https: String,
    pub proxy_all: String,
    pub openai_client_id: String,
    pub openai_auth_url: String,
    pub openai_token_url: String,
    pub openai_redirect_uri: String,
    pub openai_scope: String,
    pub registration_timeout: i64,
    pub registration_max_retries: i64,
    pub batch_interval_seconds: i64,
    pub cpa_enabled: bool,
    pub cpa_api_url: String,
    pub cpa_api_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStats {
    pub total: usize,
    pub active: usize,
    pub failed: usize,
    pub other: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatabaseInfo {
    pub db_path: String,
    pub file_size_bytes: u64,
    pub accounts_count: usize,
    pub services_count: usize,
    pub tasks_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRequest {
    pub service_id: i64,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountActionResult {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenActionResult {
    pub success: bool,
    pub message: String,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub valid: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchAccountRequest {
    pub ids: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAccountStatusRequest {
    pub ids: Vec<i64>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    Json,
    Csv,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportAccountsRequest {
    pub ids: Vec<i64>,
    pub format: ExportFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportPayload {
    pub filename: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseBackupResult {
    pub success: bool,
    pub backup_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceActionRequest {
    pub id: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseActionResult {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreviewEmail {
    pub email: String,
    pub service_id: String,
    pub created_at: DateTime<Utc>,
    pub inbox_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartSingleRequest {
    pub service_id: i64,
    pub preview_email: Option<PreviewEmail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartBatchRequest {
    pub service_id: i64,
    pub count: usize,
    pub preview_emails: Vec<PreviewEmail>,
    pub interval_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    Single,
    Batch,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSnapshot {
    pub id: String,
    pub kind: TaskKind,
    pub status: TaskStatus,
    pub title: String,
    pub progress_total: usize,
    pub progress_completed: usize,
    pub success_count: usize,
    pub failed_count: usize,
    pub current_email: Option<String>,
    pub logs: Vec<String>,
    pub error_message: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistrationSummary {
    pub email: String,
    pub success: bool,
    pub workspace_id: Option<String>,
    pub account_id: Option<String>,
    pub password: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceTestResult {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct EmailServiceConfig {
    pub service_type: EmailServiceType,
    pub base_url: String,
    pub api_key: String,
    pub api_key_header: String,
    pub prefix: Option<String>,
    pub default_domain: Option<String>,
    pub proxy_url: Option<String>,
}
