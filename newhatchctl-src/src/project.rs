use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};
use serde_yaml::Value;

#[derive(Clone, Debug)]
pub struct Project {
    pub directory: PathBuf,
}

impl Project {
    pub fn env_settings(&self) -> Result<Vec<(String, String)>> {
        let text = match fs::read_to_string(self.directory.join(".env")) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error).context("Cannot read .env"),
        };
        let mut settings = Vec::new();
        let mut lines = text.trim_start_matches('\u{feff}').lines();
        while let Some(line) = lines.next() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let line = line
                .strip_prefix("export")
                .filter(|rest| rest.starts_with(char::is_whitespace))
                .unwrap_or(line)
                .trim_start();
            let (key, value) = line.split_once(['=', ':']).unwrap_or((line, ""));
            let mut value = value.trim().to_owned();
            while !quoted_value_ends(&value) {
                let Some(next) = lines.next() else { break };
                value.push('\n');
                value.push_str(next);
            }
            let key = key.trim();
            if key == "NEWHATCH_PASSWORD" {
                value = "*****".to_owned();
            }
            settings.push((key.to_owned(), value));
        }
        Ok(settings)
    }

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

fn quoted_value_ends(value: &str) -> bool {
    let Some(quote @ ('\'' | '"')) = value.chars().next() else {
        return true;
    };
    let mut escaped = false;
    for character in value.chars().skip(1) {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == quote {
            return true;
        }
    }
    false
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
    fn env_values_preserve_file_syntax_without_expansion() {
        let directory = tempfile::tempdir().unwrap();
        let project = Project {
            directory: directory.path().to_owned(),
        };
        assert!(project.env_settings().unwrap().is_empty());
        let text = concat!(
            "\u{feff}# Settings\r\n\r\n",
            "FLAG_REGEX=FLAG\\{[^}\\r\\n]+\\}\r\n",
            "export\tCUSTOM = \"value=a # literal\" # comment\r\n",
            "MULTILINE='first\r\n# second\r\nFAKE=value\r\nlast'\r\n",
            "EMPTY=\r\nUNSET\r\nCOLON: value\r\n",
            "FUTURE=${UNKNOWN:-fallback}\r\n",
            "COMMAND=$(touch should-not-exist)\r\n",
        );
        let path = directory.path().join(".env");
        fs::write(&path, text).unwrap();
        assert_eq!(
            project.env_settings().unwrap(),
            vec![
                ("FLAG_REGEX".into(), r"FLAG\{[^}\r\n]+\}".into()),
                ("CUSTOM".into(), r#""value=a # literal" # comment"#.into()),
                (
                    "MULTILINE".into(),
                    "'first\n# second\nFAKE=value\nlast'".into()
                ),
                ("EMPTY".into(), "".into()),
                ("UNSET".into(), "".into()),
                ("COLON".into(), "value".into()),
                ("FUTURE".into(), "${UNKNOWN:-fallback}".into()),
                ("COMMAND".into(), "$(touch should-not-exist)".into()),
            ]
        );
        assert_eq!(fs::read_to_string(path).unwrap(), text);
        assert!(!directory.path().join("should-not-exist").exists());
    }

    #[test]
    fn env_password_is_masked_without_modifying_file() {
        let directory = tempfile::tempdir().unwrap();
        let project = Project {
            directory: directory.path().to_owned(),
        };
        let path = directory.path().join(".env");
        for value in [
            "fixture-secret",
            "'fixture-secret'",
            "\"fixture-secret\"",
            "'fixture-secret\nFAKE=hidden\nlast'",
            "",
        ] {
            let text = format!("export NEWHATCH_PASSWORD = {value}\nNEWHATCH_LOGIN=admin\n");
            fs::write(&path, &text).unwrap();
            assert_eq!(
                project.env_settings().unwrap(),
                vec![
                    ("NEWHATCH_PASSWORD".into(), "*****".into()),
                    ("NEWHATCH_LOGIN".into(), "admin".into()),
                ]
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
        }
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
