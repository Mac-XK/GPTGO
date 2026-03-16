use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::json;

use crate::error::AppResult;
use crate::models::{
    AccountRecord, AccountStats, AppSettings, BootstrapPayload, DatabaseBackupResult,
    DatabaseInfo, EmailServiceConfig, EmailServiceInput, EmailServiceRecord, EmailServiceType,
    ExportFormat, ExportPayload,
};

#[derive(Clone)]
pub struct Database {
    path: PathBuf,
}

impl Database {
    pub fn new(base_dir: &Path) -> AppResult<Self> {
        fs::create_dir_all(base_dir)?;
        let path = base_dir.join("codex-register-rust.sqlite3");
        let db = Self { path };
        db.init()?;
        Ok(db)
    }

    fn connect(&self) -> AppResult<Connection> {
        Ok(Connection::open(&self.path)?)
    }

    fn init(&self) -> AppResult<()> {
        let conn = self.connect()?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS email_services (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              service_type TEXT NOT NULL,
              name TEXT NOT NULL UNIQUE,
              base_url TEXT NOT NULL,
              api_key TEXT NOT NULL,
              prefix TEXT,
              enabled INTEGER NOT NULL DEFAULT 1,
              priority INTEGER NOT NULL DEFAULT 0,
              last_used TEXT,
              created_at TEXT NOT NULL,
              updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS accounts (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              email TEXT NOT NULL UNIQUE,
              status TEXT NOT NULL,
              password TEXT,
              account_id TEXT,
              workspace_id TEXT,
              access_token TEXT,
              refresh_token TEXT,
              id_token TEXT,
              session_token TEXT,
              error_message TEXT,
              created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS registration_tasks (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              task_uuid TEXT NOT NULL UNIQUE,
              status TEXT NOT NULL,
              payload_json TEXT NOT NULL,
              created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS app_settings (
              key TEXT PRIMARY KEY,
              value TEXT NOT NULL
            );
            "#,
        )?;
        self.migrate(&conn)?;
        Ok(())
    }

    fn migrate(&self, conn: &Connection) -> AppResult<()> {
        ensure_column(conn, "accounts", "last_refresh", "TEXT")?;
        ensure_column(conn, "accounts", "expires_at", "TEXT")?;
        ensure_column(conn, "accounts", "cpa_uploaded", "INTEGER NOT NULL DEFAULT 0")?;
        ensure_column(conn, "accounts", "cpa_uploaded_at", "TEXT")?;
        Ok(())
    }

    pub fn bootstrap_payload(&self) -> AppResult<BootstrapPayload> {
        Ok(BootstrapPayload {
            services: self.list_email_services()?,
            accounts: self.list_accounts()?,
            settings: self.load_app_settings()?,
            account_stats: self.account_stats()?,
            database_info: self.database_info()?,
        })
    }

    pub fn list_email_services(&self) -> AppResult<Vec<EmailServiceRecord>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, service_type, name, base_url, api_key, prefix, enabled, priority, last_used
            FROM email_services
            ORDER BY priority ASC, id ASC
            "#,
        )?;

        let rows = stmt.query_map([], |row| {
            let service_type_text: String = row.get(1)?;
            let service_type = match service_type_text.as_str() {
                "gptmail" => EmailServiceType::Gptmail,
                "custom_domain" => EmailServiceType::CustomDomain,
                _ => EmailServiceType::Gptmail,
            };

            Ok(EmailServiceRecord {
                id: row.get(0)?,
                service_type,
                name: row.get(2)?,
                base_url: row.get(3)?,
                has_api_key: !row.get::<_, String>(4)?.is_empty(),
                prefix: row.get(5)?,
                enabled: row.get::<_, i64>(6)? != 0,
                priority: row.get(7)?,
                last_used: parse_optional_datetime(row.get::<_, Option<String>>(8)?),
            })
        })?;

        let mut services = Vec::new();
        for row in rows {
            services.push(row?);
        }
        Ok(services)
    }

    pub fn list_accounts(&self) -> AppResult<Vec<AccountRecord>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(&(account_select_sql().to_owned() + " ORDER BY created_at DESC LIMIT 20"))?;
        let rows = stmt.query_map([], map_account_row)?;

        let mut accounts = Vec::new();
        for row in rows {
            accounts.push(row?);
        }
        Ok(accounts)
    }

    pub fn get_account_by_id(&self, account_id: i64) -> AppResult<Option<AccountRecord>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(&(account_select_sql().to_owned() + " WHERE id = ? LIMIT 1"))?;
        Ok(stmt.query_row([account_id], map_account_row).optional()?)
    }

    pub fn get_accounts_by_ids(&self, ids: &[i64]) -> AppResult<Vec<AccountRecord>> {
        let mut result = Vec::new();
        for id in ids {
            if let Some(account) = self.get_account_by_id(*id)? {
                result.push(account);
            }
        }
        Ok(result)
    }

    pub fn save_email_service(&self, input: &EmailServiceInput) -> AppResult<EmailServiceRecord> {
        let conn = self.connect()?;
        let now = Utc::now().to_rfc3339();
        let service_type = match input.service_type {
            EmailServiceType::Gptmail => "gptmail",
            EmailServiceType::CustomDomain => "custom_domain",
        };

        let existing_id = if let Some(id) = input.id {
            Some(id)
        } else {
            conn.query_row(
                "SELECT id FROM email_services WHERE service_type = ? AND name = ? LIMIT 1",
                params![service_type, input.name],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
        };

        if let Some(id) = existing_id {
            conn.execute(
                r#"
                UPDATE email_services
                SET name = ?, base_url = ?, api_key = ?, prefix = ?, enabled = ?, priority = 0, updated_at = ?
                WHERE id = ?
                "#,
                params![
                    input.name,
                    input.base_url,
                    input.api_key,
                    normalize_optional(input.prefix.clone()),
                    input.enabled as i64,
                    now,
                    id
                ],
            )?;
        } else {
            conn.execute(
                r#"
                INSERT INTO email_services (service_type, name, base_url, api_key, prefix, enabled, priority, created_at, updated_at)
                VALUES (?, ?, ?, ?, ?, ?, 0, ?, ?)
                "#,
                params![
                    service_type,
                    input.name,
                    input.base_url,
                    input.api_key,
                    normalize_optional(input.prefix.clone()),
                    input.enabled as i64,
                    now,
                    now
                ],
            )?;
        }

        let record = self
            .list_email_services()?
            .into_iter()
            .find(|service| {
                service.name == input.name
                    && std::mem::discriminant(&service.service_type)
                        == std::mem::discriminant(&input.service_type)
            })
            .expect("service must exist after save");
        Ok(record)
    }

    pub fn delete_email_service(&self, service_id: i64) -> AppResult<()> {
        let conn = self.connect()?;
        conn.execute("DELETE FROM email_services WHERE id = ?", [service_id])?;
        Ok(())
    }

    pub fn toggle_email_service(&self, service_id: i64, enabled: bool) -> AppResult<()> {
        let conn = self.connect()?;
        conn.execute(
            "UPDATE email_services SET enabled = ?, updated_at = ? WHERE id = ?",
            params![enabled as i64, Utc::now().to_rfc3339(), service_id],
        )?;
        Ok(())
    }

    pub fn load_email_service_config(&self, service_id: i64) -> AppResult<EmailServiceConfig> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT service_type, base_url, api_key, prefix
            FROM email_services
            WHERE id = ? AND enabled = 1
            LIMIT 1
            "#,
        )?;

        let config = stmt.query_row([service_id], |row| {
            let service_type_text: String = row.get(0)?;
            let service_type = match service_type_text.as_str() {
                "gptmail" => EmailServiceType::Gptmail,
                "custom_domain" => EmailServiceType::CustomDomain,
                _ => EmailServiceType::Gptmail,
            };
            Ok(EmailServiceConfig {
                service_type,
                base_url: row.get(1)?,
                api_key: row.get(2)?,
                api_key_header: "X-API-Key".to_owned(),
                prefix: row.get(3)?,
                default_domain: None,
                proxy_url: None,
            })
        })?;
        let settings = self.load_app_settings()?;
        let proxy_url = if settings.proxy_enabled {
            first_non_empty(&[&settings.proxy_https, &settings.proxy_http, &settings.proxy_all])
        } else {
            None
        };
        let enriched = match config.service_type {
            EmailServiceType::CustomDomain => EmailServiceConfig {
                default_domain: config.prefix.clone(),
                prefix: None,
                proxy_url,
                ..config
            },
            _ => EmailServiceConfig { proxy_url, ..config },
        };
        Ok(enriched)
    }

    pub fn touch_service_last_used(&self, service_id: i64) -> AppResult<()> {
        let conn = self.connect()?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE email_services SET last_used = ?, updated_at = ? WHERE id = ?",
            params![now, now, service_id],
        )?;
        Ok(())
    }

    pub fn queue_registration_plan(&self, service_id: i64, emails: &[String]) -> AppResult<()> {
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO registration_tasks (task_uuid, status, payload_json, created_at) VALUES (?, 'preview_confirmed', ?, ?)",
            params![
                uuid::Uuid::new_v4().to_string(),
                json!({ "service_id": service_id, "emails": emails, "phase": "preview_confirmed" }).to_string(),
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    pub fn load_app_settings(&self) -> AppResult<AppSettings> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare("SELECT key, value FROM app_settings")?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;

        let mut map = std::collections::HashMap::new();
        for row in rows {
            let (key, value) = row?;
            map.insert(key, value);
        }

        Ok(AppSettings {
            proxy_enabled: map.get("proxy_enabled").map(|v| v == "true").unwrap_or(false),
            proxy_http: map.get("proxy_http").cloned().unwrap_or_default(),
            proxy_https: map.get("proxy_https").cloned().unwrap_or_default(),
            proxy_all: map.get("proxy_all").cloned().unwrap_or_default(),
            openai_client_id: map
                .get("openai_client_id")
                .cloned()
                .unwrap_or_else(|| "app_EMoamEEZ73f0CkXaXp7hrann".to_owned()),
            openai_auth_url: map
                .get("openai_auth_url")
                .cloned()
                .unwrap_or_else(|| "https://auth.openai.com/oauth/authorize".to_owned()),
            openai_token_url: map
                .get("openai_token_url")
                .cloned()
                .unwrap_or_else(|| "https://auth.openai.com/oauth/token".to_owned()),
            openai_redirect_uri: map
                .get("openai_redirect_uri")
                .cloned()
                .unwrap_or_else(|| "http://localhost:1455/auth/callback".to_owned()),
            openai_scope: map
                .get("openai_scope")
                .cloned()
                .unwrap_or_else(|| "openid email profile offline_access".to_owned()),
            registration_timeout: map
                .get("registration_timeout")
                .and_then(|v| v.parse().ok())
                .unwrap_or(120),
            registration_max_retries: map
                .get("registration_max_retries")
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
            batch_interval_seconds: map
                .get("batch_interval_seconds")
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
            cpa_enabled: map.get("cpa_enabled").map(|v| v == "true").unwrap_or(false),
            cpa_api_url: map.get("cpa_api_url").cloned().unwrap_or_default(),
            cpa_api_token: map.get("cpa_api_token").cloned().unwrap_or_default(),
        })
    }

    pub fn save_app_settings(&self, settings: &AppSettings) -> AppResult<AppSettings> {
        let conn = self.connect()?;
        let values = [
            ("proxy_enabled", settings.proxy_enabled.to_string()),
            ("proxy_http", settings.proxy_http.clone()),
            ("proxy_https", settings.proxy_https.clone()),
            ("proxy_all", settings.proxy_all.clone()),
            ("openai_client_id", settings.openai_client_id.clone()),
            ("openai_auth_url", settings.openai_auth_url.clone()),
            ("openai_token_url", settings.openai_token_url.clone()),
            ("openai_redirect_uri", settings.openai_redirect_uri.clone()),
            ("openai_scope", settings.openai_scope.clone()),
            ("registration_timeout", settings.registration_timeout.to_string()),
            ("registration_max_retries", settings.registration_max_retries.to_string()),
            ("batch_interval_seconds", settings.batch_interval_seconds.to_string()),
            ("cpa_enabled", settings.cpa_enabled.to_string()),
            ("cpa_api_url", settings.cpa_api_url.clone()),
            ("cpa_api_token", settings.cpa_api_token.clone()),
        ];

        for (key, value) in values {
            conn.execute(
                "INSERT INTO app_settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )?;
        }

        self.load_app_settings()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn save_account_result(
        &self,
        email: &str,
        status: &str,
        password: Option<&str>,
        account_id: Option<&str>,
        workspace_id: Option<&str>,
        access_token: Option<&str>,
        refresh_token: Option<&str>,
        id_token: Option<&str>,
        session_token: Option<&str>,
        error_message: Option<&str>,
    ) -> AppResult<()> {
        let conn = self.connect()?;
        conn.execute(
            r#"
            INSERT INTO accounts (
              email, status, password, account_id, workspace_id,
              access_token, refresh_token, id_token, session_token, error_message, created_at
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(email) DO UPDATE SET
              status = excluded.status,
              password = excluded.password,
              account_id = excluded.account_id,
                workspace_id = excluded.workspace_id,
                access_token = excluded.access_token,
                refresh_token = excluded.refresh_token,
                id_token = excluded.id_token,
                session_token = excluded.session_token,
                error_message = excluded.error_message
            "#,
            params![
                email,
                status,
                password,
                account_id,
                workspace_id,
                access_token,
                refresh_token,
                id_token,
                session_token,
                error_message,
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn update_account_tokens(
        &self,
        account_id: i64,
        access_token: Option<&str>,
        refresh_token: Option<&str>,
        expires_at: Option<DateTime<Utc>>,
        last_refresh: Option<DateTime<Utc>>,
    ) -> AppResult<()> {
        let conn = self.connect()?;
        conn.execute(
            "UPDATE accounts SET access_token = ?, refresh_token = ?, expires_at = ?, last_refresh = ? WHERE id = ?",
            params![
                access_token,
                refresh_token,
                expires_at.map(|v| v.to_rfc3339()),
                last_refresh.map(|v| v.to_rfc3339()),
                account_id
            ],
        )?;
        Ok(())
    }

    pub fn mark_cpa_uploaded(&self, ids: &[i64]) -> AppResult<usize> {
        let conn = self.connect()?;
        let mut updated = 0usize;
        let now = Utc::now().to_rfc3339();
        for id in ids {
            updated += conn.execute(
                "UPDATE accounts SET cpa_uploaded = 1, cpa_uploaded_at = ? WHERE id = ?",
                params![now, id],
            )?;
        }
        Ok(updated)
    }

    pub fn account_stats(&self) -> AppResult<AccountStats> {
        let conn = self.connect()?;
        let total = count_table(&conn, "accounts")?;
        let active = count_where(&conn, "accounts", "status = 'active'")?;
        let failed = count_where(&conn, "accounts", "status = 'failed'")?;
        Ok(AccountStats {
            total,
            active,
            failed,
            other: total.saturating_sub(active + failed),
        })
    }

    pub fn database_info(&self) -> AppResult<DatabaseInfo> {
        let metadata = fs::metadata(&self.path).ok();
        let conn = self.connect()?;
        Ok(DatabaseInfo {
            db_path: self.path.display().to_string(),
            file_size_bytes: metadata.map(|m| m.len()).unwrap_or(0),
            accounts_count: count_table(&conn, "accounts")?,
            services_count: count_table(&conn, "email_services")?,
            tasks_count: count_table(&conn, "registration_tasks")?,
        })
    }

    pub fn backup_database(&self, backup_dir: &Path) -> AppResult<DatabaseBackupResult> {
        fs::create_dir_all(backup_dir)?;
        let filename = format!("gptgo-backup-{}.sqlite3", Utc::now().format("%Y%m%d-%H%M%S"));
        let backup_path = backup_dir.join(filename);
        fs::copy(&self.path, &backup_path)?;
        Ok(DatabaseBackupResult {
            success: true,
            backup_path: backup_path.display().to_string(),
        })
    }

    pub fn clear_registration_tasks(&self) -> AppResult<()> {
        let conn = self.connect()?;
        conn.execute("DELETE FROM registration_tasks", [])?;
        Ok(())
    }

    pub fn delete_accounts(&self, ids: &[i64]) -> AppResult<usize> {
        let conn = self.connect()?;
        let mut deleted = 0usize;
        for id in ids {
            deleted += conn.execute("DELETE FROM accounts WHERE id = ?", [id])?;
        }
        Ok(deleted)
    }

    pub fn update_accounts_status(&self, ids: &[i64], status: &str) -> AppResult<usize> {
        let conn = self.connect()?;
        let mut updated = 0usize;
        for id in ids {
            updated += conn.execute("UPDATE accounts SET status = ? WHERE id = ?", params![status, id])?;
        }
        Ok(updated)
    }

    pub fn export_accounts(&self, ids: &[i64], format: ExportFormat) -> AppResult<ExportPayload> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT id, email, status, password, workspace_id, created_at FROM accounts WHERE id = ?",
        )?;
        let mut rows_data = Vec::new();
        for id in ids {
            if let Some(record) = stmt
                .query_row([id], |row| {
                    Ok(AccountRecord {
                        id: row.get(0)?,
                        email: row.get(1)?,
                        status: row.get(2)?,
                        password: row.get(3)?,
                        workspace_id: row.get(4)?,
                        account_id: None,
                        access_token: None,
                        refresh_token: None,
                        id_token: None,
                        session_token: None,
                        last_refresh: None,
                        expires_at: None,
                        cpa_uploaded: false,
                        cpa_uploaded_at: None,
                        created_at: parse_datetime(&row.get::<_, String>(5)?),
                    })
                })
                .optional()?
            {
                rows_data.push(record);
            }
        }

        match format {
            ExportFormat::Json => Ok(ExportPayload {
                filename: format!("accounts-{}.json", Utc::now().format("%Y%m%d-%H%M%S")),
                content: serde_json::to_string_pretty(&rows_data)?,
            }),
            ExportFormat::Csv => {
                let mut lines = vec!["id,email,status,password,workspace_id,created_at".to_owned()];
                for account in rows_data {
                    lines.push(format!(
                        "{},{},{},{},{},{}",
                        account.id,
                        csv_escape(&account.email),
                        csv_escape(&account.status),
                        csv_escape(account.password.as_deref().unwrap_or("")),
                        csv_escape(account.workspace_id.as_deref().unwrap_or("")),
                        csv_escape(&account.created_at.to_rfc3339()),
                    ));
                }
                Ok(ExportPayload {
                    filename: format!("accounts-{}.csv", Utc::now().format("%Y%m%d-%H%M%S")),
                    content: lines.join("\n"),
                })
            }
        }
    }
}

fn account_select_sql() -> &'static str {
    r#"
    SELECT id, email, status, password, workspace_id, account_id,
           access_token, refresh_token, id_token, session_token,
           last_refresh, expires_at, cpa_uploaded, cpa_uploaded_at, created_at
    FROM accounts
    "#
}

fn map_account_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AccountRecord> {
    let created_at: String = row.get(14)?;
    Ok(AccountRecord {
        id: row.get(0)?,
        email: row.get(1)?,
        status: row.get(2)?,
        password: row.get(3)?,
        workspace_id: row.get(4)?,
        account_id: row.get(5)?,
        access_token: row.get(6)?,
        refresh_token: row.get(7)?,
        id_token: row.get(8)?,
        session_token: row.get(9)?,
        last_refresh: parse_optional_datetime(row.get::<_, Option<String>>(10)?),
        expires_at: parse_optional_datetime(row.get::<_, Option<String>>(11)?),
        cpa_uploaded: row.get::<_, Option<i64>>(12)?.unwrap_or(0) != 0,
        cpa_uploaded_at: parse_optional_datetime(row.get::<_, Option<String>>(13)?),
        created_at: parse_datetime(&created_at),
    })
}

fn normalize_optional(value: String) -> Option<String> {
    let trimmed = value.trim().to_owned();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn parse_datetime(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw)
        .map(|value| value.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn parse_optional_datetime(raw: Option<String>) -> Option<DateTime<Utc>> {
    raw.as_deref().map(parse_datetime)
}

fn first_non_empty(values: &[&str]) -> Option<String> {
    values
        .iter()
        .find_map(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_owned())
            }
        })
}

fn ensure_column(conn: &Connection, table: &str, column: &str, column_type: &str) -> AppResult<()> {
    let pragma = format!("PRAGMA table_info({table})");
    let mut stmt = conn.prepare(&pragma)?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for row in rows {
        if row? == column {
            return Ok(());
        }
    }
    let alter = format!("ALTER TABLE {table} ADD COLUMN {column} {column_type}");
    conn.execute(&alter, [])?;
    Ok(())
}

fn count_table(conn: &Connection, table: &str) -> AppResult<usize> {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    Ok(conn.query_row(&sql, [], |row| row.get::<_, i64>(0))? as usize)
}

fn count_where(conn: &Connection, table: &str, clause: &str) -> AppResult<usize> {
    let sql = format!("SELECT COUNT(*) FROM {table} WHERE {clause}");
    Ok(conn.query_row(&sql, [], |row| row.get::<_, i64>(0))? as usize)
}

fn csv_escape(value: &str) -> String {
    let needs_quotes = value.contains(',') || value.contains('"') || value.contains('\n');
    if needs_quotes {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}
