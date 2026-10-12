use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Result};
use serde_yaml::Value;

#[derive(Clone, Debug)]
pub struct Project {
    pub directory: PathBuf,
}

impl Project {
    pub fn resolve(current: &Path, fallback: Option<&OsStr>) -> Result<Self> {
        for directory in std::iter::once(current.to_path_buf())
            .chain(fallback.filter(|path| !path.is_empty()).map(PathBuf::from))
        {
            if is_newhatch(&directory) {
                return Ok(Self {
                    directory: fs::canonicalize(directory)?,
                });
            }
        }
        bail!("Unknown Newhatch location")
    }
}

fn is_newhatch(directory: &Path) -> bool {
    let Ok(bytes) = fs::read(directory.join("compose.yaml")) else {
        return false;
    };
    let Ok(value) = serde_yaml::from_slice::<Value>(&bytes) else {
        return false;
    };
    let Some(services) = value.get("services") else {
        return false;
    };
    ["collector", "analyzer", "frontend"]
        .iter()
        .all(|name| services.get(*name).is_some())
        && services["analyzer"]["build"]["dockerfile"].as_str()
            == Some("crates/analyzer/Dockerfile")
        && services["collector"]["build"]["dockerfile"].as_str()
            == Some("crates/collector/Dockerfile")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(directory: &Path) {
        fs::write(
            directory.join("compose.yaml"),
            include_str!("../../compose.yaml"),
        )
        .unwrap();
    }

    #[test]
    fn local_installation_wins_over_host_variable() {
        let local = tempfile::tempdir().unwrap();
        let fallback = tempfile::tempdir().unwrap();
        fixture(local.path());
        fixture(fallback.path());
        assert_eq!(
            Project::resolve(local.path(), Some(fallback.path().as_os_str()))
                .unwrap()
                .directory,
            fs::canonicalize(local.path()).unwrap()
        );
    }

    #[test]
    fn unrelated_compose_falls_back_and_invalid_locations_fail() {
        let local = tempfile::tempdir().unwrap();
        let fallback = tempfile::tempdir().unwrap();
        fs::write(
            local.path().join("compose.yaml"),
            "services:\n  frontend: {}\n",
        )
        .unwrap();
        fixture(fallback.path());
        assert_eq!(
            Project::resolve(local.path(), Some(fallback.path().as_os_str()))
                .unwrap()
                .directory,
            fs::canonicalize(fallback.path()).unwrap()
        );
        assert_eq!(
            Project::resolve(local.path(), None)
                .unwrap_err()
                .to_string(),
            "Unknown Newhatch location"
        );
        assert!(Project::resolve(local.path(), Some(local.path().as_os_str())).is_err());
    }
}
