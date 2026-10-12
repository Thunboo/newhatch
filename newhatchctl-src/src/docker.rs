use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    sync::atomic::AtomicBool,
};

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    model::{Container, Operation, Service, Snapshot, Step, SETTINGS},
    process::{self, Stream},
    project::Project,
};

#[derive(Clone)]
pub struct Docker {
    pub project: Project,
    executable: OsString,
}

impl Docker {
    pub fn new(project: Project) -> Self {
        Self::with_executable(project, OsStr::new("docker"))
    }

    /// Explicit executable injection lets tests run without a Docker daemon.
    pub fn with_executable(project: Project, executable: &OsStr) -> Self {
        Self {
            project,
            executable: executable.to_owned(),
        }
    }

    fn compose_args(&self, args: &[&str]) -> Vec<String> {
        [
            "compose",
            "--ansi",
            "never",
            "--progress",
            "plain",
            "--project-directory",
        ]
        .into_iter()
        .map(String::from)
        .chain(std::iter::once(
            self.project.directory.to_string_lossy().into_owned(),
        ))
        .chain([
            "--file".into(),
            self.project
                .directory
                .join("compose.yaml")
                .to_string_lossy()
                .into_owned(),
            "--profile".into(),
            "collector".into(),
        ])
        .chain(args.iter().map(|arg| (*arg).to_owned()))
        .collect()
    }

    fn capture(&self, args: &[String], cancelled: &AtomicBool) -> Result<String> {
        process::capture(&self.executable, &self.project.directory, args, cancelled)
    }

    pub fn snapshot(&self, cancelled: &AtomicBool) -> Result<Snapshot> {
        let resolved = self
            .capture(
                &self.compose_args(&["config", "--format", "json"]),
                cancelled,
            )
            .context("Cannot resolve Compose configuration")?;
        let config: ComposeConfig =
            serde_json::from_str(&resolved).context("Invalid Compose configuration JSON")?;
        if config.name.is_empty() {
            bail!("Compose configuration has no project name");
        }
        let mut desired = BTreeMap::new();
        for service in Service::ALL {
            let values = config
                .services
                .get(service.name())
                .with_context(|| format!("Missing {} service", service.name()))?;
            let environment = SETTINGS
                .iter()
                .filter_map(|key| {
                    values.environment.get(*key).map(|value| {
                        (
                            (*key).to_owned(),
                            match value {
                                Value::String(value) => value.clone(),
                                Value::Null => String::new(),
                                other => other.to_string(),
                            },
                        )
                    })
                })
                .collect();
            desired.insert(service, environment);
        }
        let output = self.capture(
            &self.compose_args(&[
                "ps",
                "--all",
                "--format",
                "json",
                "collector",
                "analyzer",
                "frontend",
            ]),
            cancelled,
        )?;
        let rows = parse_ps(&output)?;
        let mut containers = Vec::new();
        let ids: Vec<_> = rows
            .iter()
            .filter(|row| Service::parse(&row.service).is_some())
            .map(|row| row.id.as_str())
            .collect();
        if !ids.is_empty() {
            let args = ["inspect", "--type", "container"]
                .into_iter()
                .chain(ids)
                .map(String::from)
                .collect::<Vec<_>>();
            let output = self
                .capture(&args, cancelled)
                .context("Cannot inspect managed containers")?;
            let inspected: Vec<Inspection> =
                serde_json::from_str(&output).context("Invalid Docker inspect JSON")?;
            for container in inspected {
                if container.config.labels.get("com.docker.compose.project") != Some(&config.name) {
                    continue;
                }
                let Some(service) = container
                    .config
                    .labels
                    .get("com.docker.compose.service")
                    .and_then(|name| Service::parse(name))
                else {
                    continue;
                };
                let environment = container
                    .config
                    .env
                    .iter()
                    .filter_map(|entry| {
                        let (key, value) = entry.split_once('=')?;
                        SETTINGS
                            .contains(&key)
                            .then_some((key.to_owned(), value.to_owned()))
                    })
                    .collect();
                containers.push(Container {
                    id: container.id,
                    service,
                    state: container.state.status,
                    environment,
                });
            }
        }
        let table = self.capture(
            &self.compose_args(&["ps", "--all", "collector", "analyzer", "frontend"]),
            cancelled,
        )?;
        Ok(Snapshot {
            project_name: config.name,
            desired,
            containers,
            status_table: process::clean_text(&table),
        })
    }

    pub fn execute(
        &self,
        steps: &[Step],
        cancelled: &AtomicBool,
        mut observer: impl FnMut(String),
    ) -> Result<()> {
        for step in steps {
            if step.service == Service::Frontend && step.operation != Operation::Stop {
                let output = self.capture(
                    &self.compose_args(&["ps", "--all", "--format", "json", "analyzer"]),
                    cancelled,
                )?;
                if !parse_ps(&output)?
                    .iter()
                    .any(|row| row.service == "analyzer" && row.state == "running")
                {
                    bail!("Analyzer is not running. Frontend was not started; check Analyzer logs");
                }
            }
            let mut args = match step.operation {
                Operation::Start => vec!["up", "--detach", "--no-deps", "--pull", "never"],
                Operation::Recreate => vec![
                    "up",
                    "--detach",
                    "--no-deps",
                    "--pull",
                    "never",
                    "--force-recreate",
                ],
                Operation::Stop => vec!["stop"],
            };
            args.push(step.service.name());
            observer(format!(
                "{} {}",
                match step.operation {
                    Operation::Start => "Starting",
                    Operation::Recreate => "Recreating",
                    Operation::Stop => "Stopping",
                },
                step.service.name()
            ));
            // Compose builds an absent build-service image; without --build it reuses existing images.
            Stream::spawn(
                &self.executable,
                &self.project.directory,
                &self.compose_args(&args),
            )?
            .execute(cancelled, &mut observer)?;
        }
        Ok(())
    }

    pub fn logs(&self, service: Service) -> Result<Stream> {
        Stream::spawn(
            &self.executable,
            &self.project.directory,
            &self.compose_args(&[
                "logs",
                "--no-color",
                "--tail",
                "200",
                "--follow",
                service.name(),
            ]),
        )
    }
}

#[derive(Deserialize)]
struct ComposeConfig {
    name: String,
    services: BTreeMap<String, ComposeService>,
}

#[derive(Deserialize)]
struct ComposeService {
    #[serde(default)]
    environment: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
struct PsRow {
    #[serde(rename = "ID")]
    id: String,
    #[serde(rename = "Service")]
    service: String,
    #[serde(rename = "State", default)]
    state: String,
}

fn parse_ps(output: &str) -> Result<Vec<PsRow>> {
    if output.trim().is_empty() {
        return Ok(Vec::new());
    }
    if output.trim_start().starts_with('[') {
        return serde_json::from_str(output).context("Invalid Compose ps JSON");
    }
    serde_json::Deserializer::from_str(output)
        .into_iter::<PsRow>()
        .collect::<std::result::Result<Vec<_>, _>>()
        .context("Invalid Compose ps JSON")
}

#[derive(Deserialize)]
struct Inspection {
    #[serde(rename = "Id")]
    id: String,
    #[serde(rename = "Config")]
    config: InspectedConfig,
    #[serde(rename = "State")]
    state: InspectedState,
}

#[derive(Deserialize)]
struct InspectedConfig {
    #[serde(rename = "Env", default)]
    env: Vec<String>,
    #[serde(rename = "Labels", default)]
    labels: BTreeMap<String, String>,
}

#[derive(Deserialize)]
struct InspectedState {
    #[serde(rename = "Status")]
    status: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ps_accepts_compose_array_and_ndjson_and_empty_output() {
        let row = r#"{"ID":"abc","Service":"collector","State":"running"}"#;
        assert_eq!(parse_ps(&format!("[{row}]")).unwrap().len(), 1);
        assert_eq!(parse_ps(&format!("{row}\n{row}\n")).unwrap().len(), 2);
        assert!(parse_ps("").unwrap().is_empty());
        assert!(parse_ps("not json").is_err());
    }
}
