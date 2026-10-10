//! Local CLI composition. Imported bundles carry data, never writer credentials.
pub mod args;
pub mod bundle;
pub mod commands;
pub mod demo;
pub mod display;
pub mod records;
pub mod workspace;
pub use records::{BranchRecord, RunRecord};
pub use workspace::Workspace;

#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error("{0}")]
    Input(String),
    #[error("{0}")]
    Infrastructure(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Unknown(String),
}
impl CliError {
    pub fn input(message: impl Into<String>) -> Self {
        Self::Input(message.into())
    }
    /// Detailed backend errors may contain host paths or external data: the public
    /// diagnostic intentionally retains only a stable category.
    pub fn infrastructure(_error: impl std::fmt::Display) -> Self {
        Self::Infrastructure("local infrastructure operation failed".into())
    }
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Conflict(_) => 2,
            Self::Unknown(_) => 3,
            Self::Input(_) => 4,
            Self::Infrastructure(_) => 5,
        }
    }
}
macro_rules! infrastructure_from { ($($ty:ty),* $(,)?) => {$ (impl From<$ty> for CliError { fn from(error:$ty)->Self { Self::infrastructure(error) } })*}; }
infrastructure_from!(
    std::io::Error,
    rusqlite::Error,
    serde_json::Error,
    chronos_codec::CodecError,
    chronos_store::StoreError,
    chronos_state::StateError,
    chronos_ledger::LedgerError,
    chronos_scheduler::ScheduleError,
    chronos_scheduler::ReplayError,
    chronos_state::execution::ExecutionError,
    chronos_adapters::AdapterError,
    chronos_transport::TransportError,
    chronos_sandbox::SandboxError
);
