use tauri::{AppHandle, Emitter, State};
use tokio::time::{sleep, Duration};

use crate::app_state::AppState;
use crate::error::AppError;
use crate::models::{
    AccountActionResult, AppSettings, BatchAccountRequest, BootstrapPayload, DatabaseActionResult,
    DatabaseBackupResult, DatabaseInfo, EmailServiceInput, EmailServiceRecord,
    ExportAccountsRequest, ExportPayload, PreviewEmail, PreviewRequest, ServiceActionRequest,
    ServiceTestResult, StartBatchRequest, StartSingleRequest, TaskKind, TaskSnapshot,
    TokenActionResult, UpdateAccountStatusRequest,
};
use crate::openai::engine::EngineFactory;
use crate::services::cpa;
use crate::services::custom_domain::CustomDomainClient;
use crate::services::gptmail::GptMailClient;
use crate::services::token;

#[tauri::command]
pub async fn bootstrap_app(state: State<'_, AppState>) -> Result<BootstrapPayload, String> {
    state.db.bootstrap_payload().map_err(to_string_error)
}

#[tauri::command]
pub async fn save_email_service(
    input: EmailServiceInput,
    state: State<'_, AppState>,
) -> Result<EmailServiceRecord, String> {
    state.db.save_email_service(&input).map_err(to_string_error)
}

#[tauri::command]
pub async fn save_app_settings(
    input: AppSettings,
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    state.db.save_app_settings(&input).map_err(to_string_error)
}

#[tauri::command]
pub async fn get_database_info(state: State<'_, AppState>) -> Result<DatabaseInfo, String> {
    state.db.database_info().map_err(to_string_error)
}

#[tauri::command]
pub async fn backup_database(state: State<'_, AppState>) -> Result<DatabaseBackupResult, String> {
    let backup_root = std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("gptgo-backups");
    state.db.backup_database(&backup_root).map_err(to_string_error)
}

#[tauri::command]
pub async fn clear_registration_tasks(state: State<'_, AppState>) -> Result<DatabaseActionResult, String> {
    state.db.clear_registration_tasks().map_err(to_string_error)?;
    Ok(DatabaseActionResult {
        success: true,
        message: "注册任务记录已清空".to_owned(),
    })
}

#[tauri::command]
pub async fn delete_accounts(
    input: BatchAccountRequest,
    state: State<'_, AppState>,
) -> Result<AccountActionResult, String> {
    let deleted = state.db.delete_accounts(&input.ids).map_err(to_string_error)?;
    Ok(AccountActionResult {
        success: true,
        message: format!("已删除 {} 个账号", deleted),
    })
}

#[tauri::command]
pub async fn refresh_account_token(
    account_id: i64,
    state: State<'_, AppState>,
) -> Result<TokenActionResult, String> {
    let account = state
        .db
        .get_account_by_id(account_id)
        .map_err(to_string_error)?
        .ok_or_else(|| "账号不存在".to_owned())?;
    let result = token::refresh_account(&account, &state.db.load_app_settings().map_err(to_string_error)?)
        .await
        .map_err(to_string_error)?;
    if result.success {
        state
            .db
            .update_account_tokens(
                account_id,
                result.access_token.as_deref(),
                result.refresh_token.as_deref(),
                result.expires_at,
                Some(chrono::Utc::now()),
            )
            .map_err(to_string_error)?;
    }
    Ok(result)
}

#[tauri::command]
pub async fn validate_account_token(
    account_id: i64,
    state: State<'_, AppState>,
) -> Result<TokenActionResult, String> {
    let account = state
        .db
        .get_account_by_id(account_id)
        .map_err(to_string_error)?
        .ok_or_else(|| "账号不存在".to_owned())?;
    let access_token = account
        .access_token
        .as_deref()
        .ok_or_else(|| "账号没有 access_token".to_owned())?;
    token::validate_access_token(access_token, &state.db.load_app_settings().map_err(to_string_error)?)
        .await
        .map_err(to_string_error)
}

#[tauri::command]
pub async fn batch_validate_tokens(
    input: BatchAccountRequest,
    state: State<'_, AppState>,
) -> Result<Vec<TokenActionResult>, String> {
    let settings = state.db.load_app_settings().map_err(to_string_error)?;
    let mut results = Vec::new();
    for account in state.db.get_accounts_by_ids(&input.ids).map_err(to_string_error)? {
        let result = if let Some(access_token) = account.access_token.as_deref() {
            token::validate_access_token(access_token, &settings)
                .await
                .map_err(to_string_error)?
        } else {
            TokenActionResult {
                success: false,
                message: format!("{} 没有 access_token", account.email),
                access_token: None,
                refresh_token: None,
                expires_at: None,
                valid: Some(false),
            }
        };
        results.push(result);
    }
    Ok(results)
}

#[tauri::command]
pub async fn update_accounts_status(
    input: UpdateAccountStatusRequest,
    state: State<'_, AppState>,
) -> Result<AccountActionResult, String> {
    let updated = state
        .db
        .update_accounts_status(&input.ids, &input.status)
        .map_err(to_string_error)?;
    Ok(AccountActionResult {
        success: true,
        message: format!("已更新 {} 个账号状态", updated),
    })
}

#[tauri::command]
pub async fn export_accounts(
    input: ExportAccountsRequest,
    state: State<'_, AppState>,
) -> Result<ExportPayload, String> {
    state.db.export_accounts(&input.ids, input.format).map_err(to_string_error)
}

#[tauri::command]
pub async fn export_cpa_accounts(
    input: BatchAccountRequest,
    state: State<'_, AppState>,
) -> Result<ExportPayload, String> {
    let accounts = state.db.get_accounts_by_ids(&input.ids).map_err(to_string_error)?;
    cpa::export_cpa_payload(&accounts).map_err(to_string_error)
}

#[tauri::command]
pub async fn upload_cpa_accounts(
    input: BatchAccountRequest,
    state: State<'_, AppState>,
) -> Result<DatabaseActionResult, String> {
    let accounts = state.db.get_accounts_by_ids(&input.ids).map_err(to_string_error)?;
    let settings = state.db.load_app_settings().map_err(to_string_error)?;
    let result = cpa::upload_accounts(&accounts, &settings).await.map_err(to_string_error)?;
    state.db.mark_cpa_uploaded(&input.ids).map_err(to_string_error)?;
    Ok(result)
}

#[tauri::command]
pub async fn delete_email_service(
    input: ServiceActionRequest,
    state: State<'_, AppState>,
) -> Result<DatabaseActionResult, String> {
    state.db.delete_email_service(input.id).map_err(to_string_error)?;
    Ok(DatabaseActionResult {
        success: true,
        message: "服务已删除".to_owned(),
    })
}

#[tauri::command]
pub async fn toggle_email_service(
    id: i64,
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<DatabaseActionResult, String> {
    state.db.toggle_email_service(id, enabled).map_err(to_string_error)?;
    Ok(DatabaseActionResult {
        success: true,
        message: if enabled { "服务已启用" } else { "服务已禁用" }.to_owned(),
    })
}

#[tauri::command]
pub async fn test_email_service(
    service_id: i64,
    state: State<'_, AppState>,
) -> Result<ServiceTestResult, String> {
    let config = state.db.load_email_service_config(service_id).map_err(to_string_error)?;
    let result = match config.service_type {
        crate::models::EmailServiceType::Gptmail => GptMailClient::new(config).test().await,
        crate::models::EmailServiceType::CustomDomain => CustomDomainClient::new(config).test().await,
    }
    .map_err(to_string_error)?;
    state.db.touch_service_last_used(service_id).map_err(to_string_error)?;
    Ok(result)
}

#[tauri::command]
pub async fn preview_emails(
    request: PreviewRequest,
    state: State<'_, AppState>,
) -> Result<Vec<PreviewEmail>, String> {
    if request.count == 0 || request.count > 20 {
        return Err("预生成数量必须在 1 到 20 之间".to_owned());
    }

    let config = state.db.load_email_service_config(request.service_id).map_err(to_string_error)?;
    let emails = match config.service_type {
        crate::models::EmailServiceType::Gptmail => GptMailClient::new(config).preview_emails(request.count).await,
        crate::models::EmailServiceType::CustomDomain => CustomDomainClient::new(config).preview_emails(request.count).await,
    }
    .map_err(to_string_error)?;
    state.db.touch_service_last_used(request.service_id).map_err(to_string_error)?;
    Ok(emails)
}

#[tauri::command]
pub async fn confirm_preview_plan(
    service_id: i64,
    emails: Vec<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    if emails.is_empty() {
        return Err("没有可确认的预生成邮箱".to_owned());
    }

    state
        .db
        .queue_registration_plan(service_id, &emails)
        .map_err(to_string_error)?;

    Ok("已记录确认操作，正式注册执行器将在下一阶段继续迁移到 Rust".to_owned())
}

#[tauri::command]
pub async fn start_single_registration(
    request: StartSingleRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TaskSnapshot, String> {
    let config = state.db.load_email_service_config(request.service_id).map_err(to_string_error)?;
    let task = state
        .tasks
        .create_task(TaskKind::Single, "单次注册".to_owned(), 1)
        .await;
    let task_for_spawn = task.clone();
    let tasks = state.tasks.clone();
    let db = state.db.clone();
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        tasks.append_log(&task_for_spawn.id, "任务已创建").await;
        let task_id = task_for_spawn.id.clone();
        emit_task(&app_handle, &tasks, &task_id).await;
        let email = request
            .preview_email
            .as_ref()
            .map(|item| item.email.clone());
        tasks.mark_running(&task_id, email).await;
        emit_task(&app_handle, &tasks, &task_id).await;

        let engine = match EngineFactory::build(config.clone(), request.preview_email.clone(), {
            let tasks = tasks.clone();
            let task_id = task_id.clone();
            move |message| {
                let tasks = tasks.clone();
                let task_id = task_id.clone();
                tauri::async_runtime::spawn(async move {
                    tasks.append_log(&task_id, message).await;
                });
            }
        }) {
            Ok(engine) => engine,
            Err(error) => {
                tasks.mark_failed(&task_id, error.to_string()).await;
                return;
            }
        };

        match engine.run().await {
            Ok(summary) => {
                let _ = db.save_account_result(
                    &summary.email,
                    "active",
                    summary.password.as_deref(),
                    summary.account_id.as_deref(),
                    summary.workspace_id.as_deref(),
                    summary.access_token.as_deref(),
                    summary.refresh_token.as_deref(),
                    summary.id_token.as_deref(),
                    None,
                    None,
                );
                tasks.mark_progress(&task_id, 1, 1, 0, Some(summary.email)).await;
                tasks.mark_completed(&task_id).await;
                emit_task(&app_handle, &tasks, &task_id).await;
            }
            Err(error) => {
                let _ = db.save_account_result(
                    request
                        .preview_email
                        .as_ref()
                        .map(|item| item.email.as_str())
                        .unwrap_or(""),
                    "failed",
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                    Some(&error.to_string()),
                );
                tasks.append_log(&task_id, format!("任务失败: {error}")).await;
                tasks.mark_progress(&task_id, 1, 0, 1, None).await;
                tasks.mark_failed(&task_id, error.to_string()).await;
                emit_task(&app_handle, &tasks, &task_id).await;
            }
        }
    });

    Ok(task)
}

#[tauri::command]
pub async fn start_batch_registration(
    request: StartBatchRequest,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<TaskSnapshot, String> {
    if request.count == 0 || request.count > 20 {
        return Err("批量数量必须在 1 到 20 之间".to_owned());
    }

    let config = state.db.load_email_service_config(request.service_id).map_err(to_string_error)?;
    let task = state
        .tasks
        .create_task(TaskKind::Batch, "批量注册".to_owned(), request.count)
        .await;
    let task_for_spawn = task.clone();
    let tasks = state.tasks.clone();
    let db = state.db.clone();
    let app_handle = app.clone();

    tauri::async_runtime::spawn(async move {
        let task_id = task_for_spawn.id.clone();
        tasks.mark_running(&task_id, None).await;
        tasks.append_log(&task_id, format!("批量任务启动，共 {} 个邮箱", request.count)).await;
        emit_task(&app_handle, &tasks, &task_id).await;

        let preview_emails = request.preview_emails;
        let mut success_count = 0usize;
        let mut failed_count = 0usize;

        for index in 0..request.count {
            let preview_email = preview_emails.get(index).cloned();
            let current_email = preview_email.as_ref().map(|item| item.email.clone());
            tasks
                .append_log(
                    &task_id,
                    format!("开始处理第 {} 个邮箱{}", index + 1, current_email.as_deref().map(|e| format!(": {e}")).unwrap_or_default()),
                )
                .await;
            tasks
                .mark_progress(&task_id, index, success_count, failed_count, current_email.clone())
                .await;
            emit_task(&app_handle, &tasks, &task_id).await;

            let engine = match EngineFactory::build(config.clone(), preview_email.clone(), {
                let tasks = tasks.clone();
                let task_id = task_id.clone();
                move |message| {
                    let tasks = tasks.clone();
                    let task_id = task_id.clone();
                    tauri::async_runtime::spawn(async move {
                        tasks.append_log(&task_id, message).await;
                    });
                }
            }) {
                Ok(engine) => engine,
                Err(error) => {
                    failed_count += 1;
                    tasks.append_log(&task_id, format!("初始化失败: {error}")).await;
                    emit_task(&app_handle, &tasks, &task_id).await;
                    continue;
                }
            };

            match engine.run().await {
                Ok(summary) => {
                    success_count += 1;
                    let _ = db.save_account_result(
                        &summary.email,
                        "active",
                        summary.password.as_deref(),
                        summary.account_id.as_deref(),
                        summary.workspace_id.as_deref(),
                        summary.access_token.as_deref(),
                        summary.refresh_token.as_deref(),
                        summary.id_token.as_deref(),
                        None,
                        None,
                    );
                    tasks.append_log(&task_id, format!("注册成功: {}", summary.email)).await;
                    emit_task(&app_handle, &tasks, &task_id).await;
                }
                Err(error) => {
                    failed_count += 1;
                    tasks.append_log(&task_id, format!("注册失败: {error}")).await;
                    if let Some(email) = current_email.as_deref() {
                        let _ = db.save_account_result(
                            email,
                            "failed",
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            None,
                            Some(&error.to_string()),
                        );
                    }
                    emit_task(&app_handle, &tasks, &task_id).await;
                }
            }

            tasks
                .mark_progress(&task_id, index + 1, success_count, failed_count, None)
                .await;
            emit_task(&app_handle, &tasks, &task_id).await;
            if index + 1 < request.count && request.interval_seconds > 0 {
                sleep(Duration::from_secs(request.interval_seconds)).await;
            }
        }

        if failed_count > 0 && success_count == 0 {
            tasks.mark_failed(&task_id, "批量任务全部失败").await;
        } else {
            tasks.mark_completed(&task_id).await;
        }
        emit_task(&app_handle, &tasks, &task_id).await;
    });

    Ok(task)
}

#[tauri::command]
pub async fn get_task(task_id: String, state: State<'_, AppState>) -> Result<TaskSnapshot, String> {
    state
        .tasks
        .get_task(&task_id)
        .await
        .ok_or_else(|| "任务不存在".to_owned())
}

#[tauri::command]
pub async fn list_tasks(state: State<'_, AppState>) -> Result<Vec<TaskSnapshot>, String> {
    Ok(state.tasks.list_tasks().await)
}

fn to_string_error(error: AppError) -> String {
    error.to_string()
}

async fn emit_task(app: &AppHandle, tasks: &crate::task_manager::TaskManager, task_id: &str) {
    if let Some(task) = tasks.get_task(task_id).await {
        let _ = app.emit("task://updated", task);
    }
}
