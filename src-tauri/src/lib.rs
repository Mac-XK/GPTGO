mod app_state;
mod commands;
mod db;
mod error;
mod models;
mod openai;
mod services;
mod task_manager;

use app_state::AppState;
use db::Database;
use tauri::Manager;
use task_manager::TaskManager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| anyhow::anyhow!("获取应用数据目录失败: {error}"))?;

            let db = Database::new(&app_data_dir)?;
            app.manage(AppState {
                db,
                tasks: TaskManager::default(),
            });

            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::bootstrap_app,
            commands::save_email_service,
            commands::save_app_settings,
            commands::get_database_info,
            commands::backup_database,
            commands::clear_registration_tasks,
            commands::delete_accounts,
            commands::update_accounts_status,
            commands::refresh_account_token,
            commands::validate_account_token,
            commands::batch_validate_tokens,
            commands::export_accounts,
            commands::export_cpa_accounts,
            commands::upload_cpa_accounts,
            commands::delete_email_service,
            commands::toggle_email_service,
            commands::test_email_service,
            commands::preview_emails,
            commands::confirm_preview_plan,
            commands::start_single_registration,
            commands::start_batch_registration,
            commands::get_task,
            commands::list_tasks,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
