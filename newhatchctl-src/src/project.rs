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
    fn image_build_and_mixed_deployments_are_recognized_by_service_names() {
        let directory = tempfile::tempdir().unwrap();
        for compose in [
            r#"services:
  collector: {image: 'ghcr.io/thunboo/newhatch-collector:latest'}
  analyzer: {image: 'ghcr.io/thunboo/newhatch-analyzer:latest'}
  frontend: {image: 'ghcr.io/thunboo/newhatch-frontend:latest'}
"#,
            r#"services:
  collector: {build: {context: '.', dockerfile: 'docker/collector.Dockerfile'}}
  analyzer: {build: {context: '.', dockerfile: 'docker/analyzer.Dockerfile'}}
  frontend: {build: './web'}
"#,
            r#"services:
  collector: {image: '${NEWHATCH_IMAGE_PREFIX}-collector:${NEWHATCH_IMAGE_TAG}'}
  analyzer: {build: '.'}
  frontend: {image: 'nginx:alpine'}
"#,
        ] {
            fs::write(directory.path().join("compose.yaml"), compose).unwrap();
            assert_eq!(
                Project::resolve(directory.path(), None).unwrap().directory,
                fs::canonicalize(directory.path()).unwrap()
            );
        }
    }

    #[test]
    fn each_managed_service_name_is_required() {
        let directory = tempfile::tempdir().unwrap();
        for missing in ["collector", "analyzer", "frontend"] {
            let mut services = serde_yaml::Mapping::new();
            for name in ["collector", "analyzer", "frontend"] {
                if name != missing {
                    services.insert(name.into(), Value::Mapping(serde_yaml::Mapping::new()));
                }
            }
            let mut compose = serde_yaml::Mapping::new();
            compose.insert("services".into(), Value::Mapping(services));
            fs::write(
                directory.path().join("compose.yaml"),
                serde_yaml::to_string(&compose).unwrap(),
            )
            .unwrap();
            assert_eq!(
                Project::resolve(directory.path(), None)
                    .unwrap_err()
                    .to_string(),
                "Unknown Newhatch location"
            );
        }
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
