//! FerraSSH core: russh + alacritty_terminal + encrypted vault + SFTP sync.

pub mod ai;
pub mod algs;
pub mod crypto;
pub mod error;
pub mod monitor;
pub mod oplog;
pub mod session;
pub mod sftp;
pub mod ssh;
pub mod store;
pub mod term;
pub mod zk;

pub use algs::{AlgProfileKind, AlgSpec};
pub use crypto::VaultKey;
pub use error::{Error, Result};
pub use session::{LiveSession, SessionManager};
pub use store::{AppSettings, SavedSession, Store};
