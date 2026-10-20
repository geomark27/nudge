//! Núcleo de Nudge: independiente de la interfaz para poder probarlo y reutilizarlo
//! desde cualquier "cáscara" (Tauri, CLI, etc.).

pub mod model;
pub mod schedule;
pub mod service;
pub mod sound;
pub mod store;

pub use model::{
    Action, Alert, Anchor, Backup, Days, ImportMode, Occurrence, Reminder, ReminderSpec, ReminderStats, Schedule,
    Theme, TimeOfDay, Window,
};
pub use service::Service;
pub use store::Store;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Validation(String),
    #[error("no existe el recordatorio {0}")]
    NotFound(i64),
    #[error("error de base de datos: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("datos corruptos: {0}")]
    Json(#[from] serde_json::Error),
}
