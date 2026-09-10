//! One error type for every command, serialized to the frontend as a string.

use std::fmt;

#[derive(Debug)]
pub enum Error {
    Db(rusqlite::Error),
    Io(std::io::Error),
    Json(serde_json::Error),
    Tauri(tauri::Error),
    /// User-facing rule violation, e.g. deleting the Main Board.
    Rejected(String),
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, Error>;

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Db(e) => write!(f, "database error: {e}"),
            Error::Io(e) => write!(f, "file error: {e}"),
            Error::Json(e) => write!(f, "data format error: {e}"),
            Error::Tauri(e) => write!(f, "app error: {e}"),
            Error::Rejected(msg) => write!(f, "{msg}"),
            Error::NotFound(what) => write!(f, "{what} not found"),
        }
    }
}

impl std::error::Error for Error {}

impl serde::Serialize for Error {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

macro_rules! from_error {
    ($src:ty, $variant:ident) => {
        impl From<$src> for Error {
            fn from(e: $src) -> Self {
                Error::$variant(e)
            }
        }
    };
}

from_error!(rusqlite::Error, Db);
from_error!(std::io::Error, Io);
from_error!(serde_json::Error, Json);
from_error!(tauri::Error, Tauri);

pub fn rejected(msg: impl Into<String>) -> Error {
    Error::Rejected(msg.into())
}

pub fn not_found(what: impl Into<String>) -> Error {
    Error::NotFound(what.into())
}
