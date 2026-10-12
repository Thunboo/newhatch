#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::PermissionsExt,
    process::Command,
    sync::atomic::AtomicBool,
    thread,
    time::{Duration, Instant},
};

use newhatchctl::{
    docker::Docker,
    model::{Action, Decision, Service, Snapshot},
    project::Project,
};
use serde_json::{json, Value};

struct Fixture {
    directory: tempfile::TempDir,
    docker: Docker,
}

impl Fixture {
    fn new(services: &[(&str, &str)]) -> Self {
        let directory = tempfile::tempdir().unwrap();
        fs::write(
            directory.path().join("compose.yaml"),
            include_str!("../../compose.yaml"),
        )
        .unwrap();
        let executable = directory.path().join("fake-docker");
        fs::write(&executable, include_str!("fake_docker.py")).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        let env = json!({"CAPTURE_INTERFACE":"eth0", "ANALYZER":"local", "FLAG_REGEX":"FLAG\\{[^}\\r\\n]+\\}", "FRONTEND_PORT":"8080", "AUTH_ALLOWED_SUBNETS":"10.0.0.0/8"});
        let mut containers = serde_json::Map::new();
        for (service, state) in services {
            containers.insert(
                (*service).into(),
                json!({"id":format!("id-{service}"), "state":state, "env":env}),
            );
        }
        let state = json!({"name":"newhatch", "desired": {
            "collector": env, "analyzer":env, "frontend":env, "suricata":{}, "test-flag":{}
        }, "containers":containers, "images":services.iter().map(|(service, _)| *service).collect::<Vec<_>>()});
        fs::write(directory.path().join("fake-state.json"), state.to_string()).unwrap();
        let docker = Docker::with_executable(
            Project::resolve(directory.path(), None).unwrap(),
            executable.as_os_str(),
        );
        Self { directory, docker }
    }

    fn state(&self) -> Value {
        serde_json::from_str(
            &fs::read_to_string(self.directory.path().join("fake-state.json")).unwrap(),
        )
        .unwrap()
    }
    fn change(&self, edit: impl FnOnce(&mut Value)) {
        let mut state = self.state();
        edit(&mut state);
        fs::write(
            self.directory.path().join("fake-state.json"),
            state.to_string(),
        )
        .unwrap();
    }
    fn snapshot(&self) -> Snapshot {
        self.docker.snapshot(&AtomicBool::new(false)).unwrap()
    }
    fn calls(&self) -> Vec<Vec<String>> {
        fs::read_to_string(self.directory.path().join("calls.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
    fn execute(&self, action: Action, confirmed: bool) -> anyhow::Result<()> {
        let Decision::Execute(steps) = self.snapshot().plan(action, confirmed)? else {
            panic!("Expected executable action")
        };
        self.docker.execute(&steps, &AtomicBool::new(false), |_| {})
    }
    fn mutations(&self) -> Vec<Vec<String>> {
        self.calls()
            .into_iter()
            .filter(|args| args.iter().any(|arg| arg == "up" || arg == "stop"))
            .collect()
    }
}

#[test]
fn snapshot_inspects_only_managed_containers_once_and_keeps_standard_headers() {
    let fixture = Fixture::new(&[
        ("collector", "running"),
        ("analyzer", "exited"),
        ("suricata", "running"),
        ("test-flag", "running"),
    ]);
    let snapshot = fixture.snapshot();
    assert_eq!(snapshot.containers.len(), 2);
    assert!(snapshot
        .status_table
        .starts_with("NAME IMAGE COMMAND SERVICE CREATED STATUS PORTS"));
    assert!(!snapshot.status_table.contains("suricata"));
    assert!(!format!("{snapshot:?}").contains("fixture-secret"));
    let inspect: Vec<_> = fixture
        .calls()
        .into_iter()
        .filter(|args| args[0] == "inspect")
        .collect();
    assert_eq!(inspect.len(), 1);
    let mut ids = inspect[0][3..].to_vec();
    ids.sort();
    assert_eq!(ids, ["id-analyzer", "id-collector"]);
}

#[test]
fn bulk_recreation_applies_config_and_preserves_stopped_and_excluded_containers() {
    let fixture = Fixture::new(&[
        ("collector", "running"),
        ("analyzer", "exited"),
        ("suricata", "running"),
    ]);
    fixture.change(|state| state["desired"]["collector"]["CAPTURE_INTERFACE"] = "eth1".into());
    let before = fixture.state();
    fixture.execute(Action::RestartRunning, false).unwrap();
    let after = fixture.state();
    assert_eq!(
        after["containers"]["analyzer"],
        before["containers"]["analyzer"]
    );
    assert_eq!(
        after["containers"]["suricata"],
        before["containers"]["suricata"]
    );
    assert_eq!(
        after["containers"]["collector"]["env"]["CAPTURE_INTERFACE"],
        "eth1"
    );
    assert_ne!(
        after["containers"]["collector"]["id"],
        before["containers"]["collector"]["id"]
    );
    let mutations = fixture.mutations();
    assert_eq!(mutations.len(), 1);
    assert!(mutations[0].contains(&"--force-recreate".into()));
    assert!(!mutations[0].contains(&"--build".into()));
    assert_eq!(mutations[0].last().unwrap(), "collector");
}

#[test]
fn frontend_confirmation_starts_backend_first_and_stop_confirmation_reverses_order() {
    let fixture = Fixture::new(&[]);
    assert!(matches!(
        fixture
            .snapshot()
            .plan(Action::Start(Service::Frontend), false)
            .unwrap(),
        Decision::Confirm(_)
    ));
    fixture
        .execute(Action::Start(Service::Frontend), true)
        .unwrap();
    assert!(fixture.snapshot().running(Service::Frontend));
    assert_eq!(fixture.state()["images"], json!(["analyzer", "frontend"]));
    assert!(matches!(
        fixture
            .snapshot()
            .plan(Action::Stop(Service::Analyzer), false)
            .unwrap(),
        Decision::Confirm(_)
    ));
    fixture
        .execute(Action::Stop(Service::Analyzer), true)
        .unwrap();
    let order: Vec<_> = fixture
        .mutations()
        .iter()
        .map(|args| args.last().unwrap().clone())
        .collect();
    assert_eq!(order, ["analyzer", "frontend", "frontend", "analyzer"]);
}

#[test]
fn backend_failure_or_immediate_exit_prevents_frontend_start() {
    for flag in ["fail_up", "early_exit"] {
        let fixture = Fixture::new(&[]);
        fixture.change(|state| state[flag] = "analyzer".into());
        assert!(fixture
            .execute(Action::Start(Service::Frontend), true)
            .is_err());
        assert!(fixture.state()["containers"].get("frontend").is_none());
        assert_eq!(fixture.mutations().len(), 1);
    }
}

#[test]
fn bulk_stop_uses_frontend_first_and_never_touches_optional_services() {
    let fixture = Fixture::new(&[
        ("collector", "running"),
        ("analyzer", "running"),
        ("frontend", "running"),
        ("suricata", "running"),
    ]);
    fixture.execute(Action::StopRunning, false).unwrap();
    let order: Vec<_> = fixture
        .mutations()
        .iter()
        .map(|args| args.last().unwrap().clone())
        .collect();
    assert_eq!(order, ["frontend", "analyzer", "collector"]);
    assert_eq!(
        fixture.state()["containers"]["suricata"]["state"],
        "running"
    );
    assert!(Service::ALL
        .into_iter()
        .all(|service| !fixture.snapshot().running(service)));
}

#[test]
fn ndjson_status_and_log_process_cleanup_work_without_docker() {
    let fixture = Fixture::new(&[("collector", "running")]);
    fixture.change(|state| state["ndjson"] = true.into());
    assert!(fixture.snapshot().running(Service::Collector));
    let stream = fixture.docker.logs(Service::Collector).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut lines = Vec::new();
    while lines.len() < 201 && Instant::now() < deadline {
        lines.extend(stream.drain());
        thread::sleep(Duration::from_millis(10));
    }
    assert!(lines.iter().any(|line| line == "fixture log 199"));
    assert!(lines.iter().any(|line| line == "fixture live log"));
    let pid: i32 = fs::read_to_string(fixture.directory.path().join("log-pid"))
        .unwrap()
        .parse()
        .unwrap();
    drop(stream);
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
}

#[test]
fn executable_help_unknown_location_and_noninteractive_error_do_not_invoke_docker() {
    let binary = env!("CARGO_BIN_EXE_newhatchctl");
    let empty = tempfile::tempdir().unwrap();
    let help = Command::new(binary)
        .arg("--help")
        .current_dir(empty.path())
        .output()
        .unwrap();
    assert!(help.status.success());
    assert!(String::from_utf8(help.stdout)
        .unwrap()
        .contains("NEWHATCH_DIR"));
    let unknown = Command::new(binary)
        .current_dir(empty.path())
        .env_remove("NEWHATCH_DIR")
        .output()
        .unwrap();
    assert!(!unknown.status.success());
    assert_eq!(
        String::from_utf8(unknown.stderr).unwrap(),
        "Unknown Newhatch location\n"
    );
    let fixture = Fixture::new(&[]);
    let noninteractive = Command::new(binary)
        .current_dir(fixture.directory.path())
        .output()
        .unwrap();
    assert!(!noninteractive.status.success());
    assert!(String::from_utf8(noninteractive.stderr)
        .unwrap()
        .contains("interactive terminal"));
    assert!(!fixture.directory.path().join("calls.jsonl").exists());
}
