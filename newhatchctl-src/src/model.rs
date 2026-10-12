use std::collections::BTreeMap;

use anyhow::{bail, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Service {
    Collector,
    Analyzer,
    Frontend,
}

impl Service {
    pub const ALL: [Self; 3] = [Self::Collector, Self::Analyzer, Self::Frontend];

    pub fn name(self) -> &'static str {
        match self {
            Self::Collector => "collector",
            Self::Analyzer => "analyzer",
            Self::Frontend => "frontend",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|service| service.name() == name)
    }
}

pub const SETTINGS: [&str; 5] = [
    "CAPTURE_INTERFACE",
    "FLAG_REGEX",
    "FRONTEND_PORT",
    "ANALYZER",
    "AUTH_ALLOWED_SUBNETS",
];

#[derive(Clone, Debug)]
pub struct Container {
    pub id: String,
    pub service: Service,
    pub state: String,
    pub environment: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub project_name: String,
    pub desired: BTreeMap<Service, BTreeMap<String, String>>,
    pub containers: Vec<Container>,
    pub status_table: String,
}

impl Snapshot {
    pub fn running(&self, service: Service) -> bool {
        self.containers
            .iter()
            .any(|container| container.service == service && container.state == "running")
    }

    pub fn exists(&self, service: Service) -> bool {
        self.containers
            .iter()
            .any(|container| container.service == service)
    }

    pub fn desired_value(&self, service: Service, key: &str) -> &str {
        self.desired
            .get(&service)
            .and_then(|env| env.get(key))
            .map(String::as_str)
            .unwrap_or("")
    }

    pub fn settings(&self) -> Vec<Setting> {
        SETTINGS
            .iter()
            .map(|key| {
                let owner = match *key {
                    "FRONTEND_PORT" | "AUTH_ALLOWED_SUBNETS" => Service::Frontend,
                    _ => Service::Analyzer,
                };
                let desired = self.desired_value(owner, key).to_owned();
                let active = self
                    .containers
                    .iter()
                    .filter(|container| {
                        if *key == "CAPTURE_INTERFACE" {
                            container.service == Service::Collector
                                || (container.service == Service::Analyzer
                                    && container.environment.get("ANALYZER").map(String::as_str)
                                        == Some("local"))
                        } else {
                            container.service == owner
                        }
                    })
                    .filter_map(|container| {
                        let applied = container.environment.get(*key).cloned().unwrap_or_default();
                        (applied != desired).then_some((container.service, applied))
                    })
                    .collect();
                Setting {
                    key,
                    desired,
                    active,
                }
            })
            .collect()
    }

    pub fn plan(&self, action: Action, confirmed: bool) -> Result<Decision> {
        let mut steps = Vec::new();
        match action {
            Action::Start(service) | Action::Restart(service) => {
                if matches!(action, Action::Start(_)) && self.running(service) {
                    return Ok(Decision::Nothing("Component is already running"));
                }
                if matches!(action, Action::Restart(_)) && !self.running(service) {
                    return Ok(Decision::Nothing("Component is not running"));
                }
                if service == Service::Frontend && !self.running(Service::Analyzer) {
                    if !confirmed {
                        return Ok(Decision::Confirm(
                            "Frontend requires Analyzer. Start both containers?",
                        ));
                    }
                    steps.push(Step {
                        service: Service::Analyzer,
                        operation: Operation::Start,
                    });
                }
                steps.push(Step {
                    service,
                    operation: if matches!(action, Action::Restart(_)) {
                        Operation::Recreate
                    } else {
                        Operation::Start
                    },
                });
            }
            Action::Stop(service) => {
                if !self.running(service) {
                    return Ok(Decision::Nothing("Component is not running"));
                }
                if service == Service::Analyzer && self.running(Service::Frontend) {
                    if !confirmed {
                        return Ok(Decision::Confirm(
                            "Frontend requires Analyzer. Stop both containers?",
                        ));
                    }
                    steps.push(Step {
                        service: Service::Frontend,
                        operation: Operation::Stop,
                    });
                }
                steps.push(Step {
                    service,
                    operation: Operation::Stop,
                });
            }
            Action::RestartRunning | Action::StopRunning => {
                let restart = action == Action::RestartRunning;
                if restart && self.running(Service::Frontend) && !self.running(Service::Analyzer) {
                    bail!("Start Analyzer first: restarting Frontend would require starting a stopped component");
                }
                let order = if restart {
                    [Service::Analyzer, Service::Collector, Service::Frontend]
                } else {
                    [Service::Frontend, Service::Analyzer, Service::Collector]
                };
                for service in order.into_iter().filter(|service| self.running(*service)) {
                    steps.push(Step {
                        service,
                        operation: if restart {
                            Operation::Recreate
                        } else {
                            Operation::Stop
                        },
                    });
                }
            }
        }
        if steps.is_empty() {
            return Ok(Decision::Nothing("No running containers"));
        }
        for step in &steps {
            if self
                .containers
                .iter()
                .filter(|container| container.service == step.service)
                .count()
                > 1
            {
                bail!(
                    "Multiple containers for {}: scaled deployments are not supported",
                    step.service.name()
                );
            }
        }
        Ok(Decision::Execute(steps))
    }
}

#[derive(Clone, Debug)]
pub struct Setting {
    pub key: &'static str,
    pub desired: String,
    pub active: Vec<(Service, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Start(Service),
    Restart(Service),
    Stop(Service),
    RestartRunning,
    StopRunning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    Start,
    Recreate,
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub service: Service,
    pub operation: Operation,
}

#[derive(Debug)]
pub enum Decision {
    Execute(Vec<Step>),
    Confirm(&'static str),
    Nothing(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(services: &[Service]) -> Snapshot {
        Snapshot {
            containers: services
                .iter()
                .map(|service| Container {
                    id: service.name().into(),
                    service: *service,
                    state: "running".into(),
                    environment: BTreeMap::new(),
                })
                .collect(),
            ..Snapshot::default()
        }
    }

    #[test]
    fn bulk_restart_touches_only_running_services() {
        let mut state = snapshot(&[Service::Collector]);
        state.containers.push(Container {
            id: "stopped".into(),
            service: Service::Analyzer,
            state: "exited".into(),
            environment: BTreeMap::new(),
        });
        let Decision::Execute(steps) = state.plan(Action::RestartRunning, false).unwrap() else {
            panic!()
        };
        assert_eq!(
            steps,
            vec![Step {
                service: Service::Collector,
                operation: Operation::Recreate
            }]
        );
        assert!(matches!(
            Snapshot::default()
                .plan(Action::RestartRunning, false)
                .unwrap(),
            Decision::Nothing(_)
        ));
    }

    #[test]
    fn dependency_prompts_and_ordering() {
        assert!(matches!(
            Snapshot::default()
                .plan(Action::Start(Service::Frontend), false)
                .unwrap(),
            Decision::Confirm(_)
        ));
        let Decision::Execute(steps) = Snapshot::default()
            .plan(Action::Start(Service::Frontend), true)
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            steps.iter().map(|step| step.service).collect::<Vec<_>>(),
            vec![Service::Analyzer, Service::Frontend]
        );
        let state = snapshot(&[Service::Analyzer, Service::Frontend]);
        assert!(matches!(
            state.plan(Action::Stop(Service::Analyzer), false).unwrap(),
            Decision::Confirm(_)
        ));
        let Decision::Execute(steps) = state.plan(Action::Stop(Service::Analyzer), true).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            steps.iter().map(|step| step.service).collect::<Vec<_>>(),
            vec![Service::Frontend, Service::Analyzer]
        );
        assert!(snapshot(&[Service::Frontend])
            .plan(Action::RestartRunning, false)
            .is_err());
    }

    #[test]
    fn remote_analyzer_interface_is_ignored_but_collector_is_compared() {
        let mut state = snapshot(&[Service::Analyzer, Service::Collector]);
        state.desired.insert(
            Service::Analyzer,
            BTreeMap::from([("CAPTURE_INTERFACE".into(), "eth1".into())]),
        );
        state.containers[0].environment = BTreeMap::from([
            ("ANALYZER".into(), "remote".into()),
            ("CAPTURE_INTERFACE".into(), "eth0".into()),
        ]);
        state.containers[1]
            .environment
            .insert("CAPTURE_INTERFACE".into(), "eth0".into());
        let setting = state.settings().remove(0);
        assert_eq!(setting.active, vec![(Service::Collector, "eth0".into())]);
        state.containers[0]
            .environment
            .insert("ANALYZER".into(), "local".into());
        assert_eq!(state.settings()[0].active.len(), 2);
    }
}
