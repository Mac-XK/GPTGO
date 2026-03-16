use crate::db::Database;
use crate::task_manager::TaskManager;

#[derive(Clone)]
pub struct AppState {
    pub db: Database,
    pub tasks: TaskManager,
}
