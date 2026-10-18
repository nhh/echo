//! Alle Dateien eines Servers liegen in einem Datenordner: Standard ist das
//! Profil des Nutzers (`%AppData%\echo-server` bzw. `~/.config/echo-server`),
//! mit `--data` jeder andere Ordner (Container: ein Volume). Aufbau wie beim
//! C#-Vorgaenger, damit ein umgestellter Server seine Daten findet.

use std::path::{Path, PathBuf};

const DEFAULT_FOLDER_NAME: &str = "echo-server";
const ARCHIVE_EXTENSION: &str = "sqlar";

#[derive(Debug, Clone)]
pub struct ServerPaths {
    root: PathBuf,
}

impl ServerPaths {
    /// Legt den Ordner an, wenn es ihn noch nicht gibt.
    pub fn new(root: Option<PathBuf>) -> std::io::Result<Self> {
        let root = match root {
            Some(root) => root,
            None => default_root()?,
        };
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Die Archive (echo-182): eine SQLite-Datei je Kanal und je privatem
    /// Gespraech, dazu eine fuer den Serverchat und die Konfiguration.
    #[must_use]
    pub fn archives(&self) -> PathBuf {
        self.root.join("archives")
    }

    #[must_use]
    pub fn server_archive(&self) -> PathBuf {
        self.archives().join(format!("server.{ARCHIVE_EXTENSION}"))
    }

    #[must_use]
    pub fn channel_archives(&self) -> PathBuf {
        self.archives().join("channels")
    }

    #[must_use]
    pub fn dm_archives(&self) -> PathBuf {
        self.archives().join("dm")
    }

    /// Archive geloeschter Kanaele; weg sind sie erst, wenn der Betreiber sie loescht.
    #[must_use]
    pub fn archive_trash(&self) -> PathBuf {
        self.archives().join("trash")
    }

    /// Laufende Uploads, bis sie ins Archiv ihres Kanals uebernommen sind.
    #[must_use]
    pub fn incoming(&self) -> PathBuf {
        self.archives().join("incoming")
    }

    #[must_use]
    pub fn host_certificate(&self) -> PathBuf {
        self.root.join("host_cert.pem")
    }

    #[must_use]
    pub fn host_key(&self) -> PathBuf {
        self.root.join("host_key.pem")
    }
}

pub(crate) fn archive_extension() -> &'static str {
    ARCHIVE_EXTENSION
}

fn default_root() -> std::io::Result<PathBuf> {
    dirs::config_dir()
        .map(|config| config.join(DEFAULT_FOLDER_NAME))
        .ok_or_else(|| std::io::Error::other("kein Profilordner fuer die Serverdaten gefunden"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_matches_the_csharp_server() {
        let dir = tempfile::TempDir::new().unwrap();
        let paths = ServerPaths::new(Some(dir.path().join("daten"))).unwrap();
        assert!(paths.root().is_dir());
        assert!(paths.server_archive().ends_with("archives/server.sqlar"));
        assert!(paths.channel_archives().ends_with("archives/channels"));
        assert!(paths.dm_archives().ends_with("archives/dm"));
        assert!(paths.archive_trash().ends_with("archives/trash"));
        assert!(paths.incoming().ends_with("archives/incoming"));
        assert!(paths.host_certificate().ends_with("host_cert.pem"));
        assert!(paths.host_key().ends_with("host_key.pem"));
        assert!(default_root().unwrap().ends_with("echo-server"));
    }
}
