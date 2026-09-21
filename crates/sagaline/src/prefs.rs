//! User preference store for the desktop binary.
//!
//! Persists non-secret, non-job preferences (currently just
//! [`ProjectLocation`]) as a small TOML file under the data
//! directory. Distinct from `SagalineStore` (which holds encrypted
//! keys + job rows in redb); preferences are plaintext, machine-
//! local, and frequently updated.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use sagaline_core::ProjectLocation;

const PREFS_FILENAME: &str = "prefs.toml";

#[derive(Debug, Error)]
pub enum PrefsError {
    #[error("failed to read prefs file {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write prefs file {path}: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse prefs file {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("failed to serialize prefs file {path}: {source}")]
    Serialize {
        path: PathBuf,
        #[source]
        source: toml::ser::Error,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PrefsFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project_location: Option<String>,
}

/// In-memory view of the persisted preferences. Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct Prefs {
    pub project_location: Option<ProjectLocation>,
}

impl Prefs {
    /// Read the prefs file from `data_dir`. Missing file yields
    /// `Prefs::default()` — the user simply hasn't set a
    /// project location yet.
    pub fn load(data_dir: &Path) -> Result<Self, PrefsError> {
        let path = data_dir.join(PREFS_FILENAME);
        let text = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => {
                return Err(PrefsError::Read {
                    path,
                    source: e,
                });
            }
        };
        let parsed: PrefsFile =
            toml::from_str(&text).map_err(|source| PrefsError::Parse { path, source })?;
        Ok(Self {
            project_location: parsed
                .project_location
                .map(|s| ProjectLocation::new(PathBuf::from(s))),
        })
    }

    /// Persist the prefs to `data_dir`. Creates the directory if
    /// missing; overwrites the existing file.
    pub fn save(&self, data_dir: &Path) -> Result<(), PrefsError> {
        std::fs::create_dir_all(data_dir).map_err(|source| PrefsError::Write {
            path: data_dir.to_path_buf(),
            source,
        })?;
        let path = data_dir.join(PREFS_FILENAME);
        let body = PrefsFile {
            project_location: self.project_location.as_ref().map(|p| p.display()),
        };
        let text = toml::to_string_pretty(&body).map_err(|source| PrefsError::Serialize {
            path: path.clone(),
            source,
        })?;
        std::fs::write(&path, text).map_err(|source| PrefsError::Write {
            path,
            source,
        })?;
        Ok(())
    }
}
