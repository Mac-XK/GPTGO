use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::RwLock;

use crate::models::{TaskKind, TaskSnapshot, TaskStatus};

#[derive(Clone, Default)]
pub struct TaskManager {
    tasks: Arc<RwLock<HashMap<String, TaskSnapshot>>>,
}

impl TaskManager {
    pub async fn create_task(&self, kind: TaskKind, title: String, total: usize) -> TaskSnapshot {
        let snapshot = TaskSnapshot {
            id: uuid::Uuid::new_v4().to_string(),
            kind,
            status: TaskStatus::Pending,
            title,
            progress_total: total.max(1),
            progress_completed: 0,
            success_count: 0,
            failed_count: 0,
            current_email: None,
            logs: vec![],
            error_message: None,
            updated_at: Utc::now(),
        };

        self.tasks
            .write()
            .await
            .insert(snapshot.id.clone(), snapshot.clone());
        snapshot
    }

    pub async fn get_task(&self, task_id: &str) -> Option<TaskSnapshot> {
        self.tasks.read().await.get(task_id).cloned()
    }

    pub async fn list_tasks(&self) -> Vec<TaskSnapshot> {
        let mut items = self.tasks.read().await.values().cloned().collect::<Vec<_>>();
        items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        items
    }

    pub async fn append_log(&self, task_id: &str, message: impl Into<String>) {
        if let Some(task) = self.tasks.write().await.get_mut(task_id) {
            let timestamp = Utc::now().format("%H:%M:%S").to_string();
            task.logs.push(format!("[{timestamp}] {}", message.into()));
            if task.logs.len() > 300 {
                let drop_count = task.logs.len() - 300;
                task.logs.drain(0..drop_count);
            }
            task.updated_at = Utc::now();
        }
    }

    pub async fn mark_running(&self, task_id: &str, email: Option<String>) {
        if let Some(task) = self.tasks.write().await.get_mut(task_id) {
            task.status = TaskStatus::Running;
            task.current_email = email;
            task.updated_at = Utc::now();
        }
    }

    pub async fn mark_progress(
        &self,
        task_id: &str,
        completed: usize,
        success_count: usize,
        failed_count: usize,
        current_email: Option<String>,
    ) {
        if let Some(task) = self.tasks.write().await.get_mut(task_id) {
            task.progress_completed = completed;
            task.success_count = success_count;
            task.failed_count = failed_count;
            task.current_email = current_email;
            task.updated_at = Utc::now();
        }
    }

    pub async fn mark_completed(&self, task_id: &str) {
        if let Some(task) = self.tasks.write().await.get_mut(task_id) {
            task.status = TaskStatus::Completed;
            task.progress_completed = task.progress_total;
            task.updated_at = Utc::now();
        }
    }

    pub async fn mark_failed(&self, task_id: &str, error_message: impl Into<String>) {
        if let Some(task) = self.tasks.write().await.get_mut(task_id) {
            task.status = TaskStatus::Failed;
            task.error_message = Some(error_message.into());
            task.updated_at = Utc::now();
        }
    }
}
