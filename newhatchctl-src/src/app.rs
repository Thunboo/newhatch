use std::{
    collections::VecDeque,
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::Result;
use crossterm::{
    event::{self, Event as TerminalEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::{
    docker::Docker,
    model::{Action, Decision, Service, Snapshot},
    process::{clean_text, Stream},
    ui,
};

const REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const BUFFER_LINES: usize = 5000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Components,
    Actions,
    Bulk,
    Settings,
    Status,
}

impl Focus {
    fn next(self, backwards: bool) -> Self {
        let all = [
            Self::Components,
            Self::Actions,
            Self::Bulk,
            Self::Settings,
            Self::Status,
        ];
        let index = all.iter().position(|item| *item == self).unwrap_or(0);
        all[(index + if backwards { all.len() - 1 } else { 1 }) % all.len()]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Dashboard,
    Logs,
    Output,
}

pub struct Confirmation {
    pub action: Action,
    pub message: &'static str,
    pub yes: bool,
}

#[derive(Default)]
pub struct TextBuffer {
    pub lines: VecDeque<String>,
    pub scroll: usize,
    pub horizontal: u16,
}

impl TextBuffer {
    pub fn push(&mut self, text: String) {
        for line in clean_text(&text).lines() {
            self.lines.push_back(line.to_owned());
            if self.scroll > 0 {
                self.scroll += 1;
            }
            if self.lines.len() > BUFFER_LINES {
                self.lines.pop_front();
            }
        }
        self.scroll = self.scroll.min(self.lines.len().saturating_sub(1));
    }

    pub fn move_up(&mut self, count: usize) {
        self.scroll = self
            .scroll
            .saturating_add(count)
            .min(self.lines.len().saturating_sub(1));
    }

    pub fn move_down(&mut self, count: usize) {
        self.scroll = self.scroll.saturating_sub(count);
    }

    pub fn visible(&self, height: usize) -> String {
        let start = self
            .lines
            .len()
            .saturating_sub(height)
            .saturating_sub(self.scroll);
        self.lines
            .iter()
            .skip(start)
            .take(height)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    }
}

enum WorkerEvent {
    Refreshed(std::result::Result<Snapshot, String>),
    Confirm {
        snapshot: Snapshot,
        action: Action,
        message: &'static str,
    },
    Line(String),
    Completed {
        snapshot: Option<Snapshot>,
        result: std::result::Result<String, String>,
    },
}

struct Task {
    cancelled: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Task {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub struct App {
    pub docker: Docker,
    pub snapshot: Option<Snapshot>,
    pub selected: usize,
    pub action: usize,
    pub bulk: usize,
    pub focus: Focus,
    pub view: View,
    pub confirmation: Option<Confirmation>,
    pub message: String,
    pub output: TextBuffer,
    pub log_buffer: TextBuffer,
    pub log_service: Option<Service>,
    pub settings_scroll: u16,
    pub settings_max_scroll: u16,
    pub status_scroll: (u16, u16),
    task: Option<Task>,
    operating: bool,
    logs: Option<Stream>,
    log_ended: bool,
    events: Receiver<WorkerEvent>,
    sender: SyncSender<WorkerEvent>,
    last_refresh: Instant,
    queued_action: Option<Action>,
}

impl App {
    pub fn new(docker: Docker) -> Self {
        let (sender, events) = mpsc::sync_channel(256);
        Self {
            docker,
            snapshot: None,
            selected: 0,
            action: 0,
            bulk: 0,
            focus: Focus::Components,
            view: View::Dashboard,
            confirmation: None,
            message: "Loading...".into(),
            output: TextBuffer::default(),
            log_buffer: TextBuffer::default(),
            log_service: None,
            settings_scroll: 0,
            settings_max_scroll: 0,
            status_scroll: (0, 0),
            task: None,
            operating: false,
            logs: None,
            log_ended: false,
            events,
            sender,
            last_refresh: Instant::now(),
            queued_action: None,
        }
    }

    pub fn service(&self) -> Service {
        Service::ALL[self.selected]
    }
    pub fn busy(&self) -> bool {
        self.operating
    }
    pub fn running(&self, service: Service) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.running(service))
    }

    pub fn enabled(&self, button: usize) -> bool {
        self.snapshot.as_ref().is_some_and(|snapshot| match button {
            0 => !snapshot.running(self.service()),
            1 | 2 => snapshot.running(self.service()),
            3 => snapshot.exists(self.service()),
            _ => false,
        }) && !self.operating
    }

    pub fn refresh(&mut self) {
        if self.task.is_some() {
            return;
        }
        let docker = self.docker.clone();
        let sender = self.sender.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let thread = thread::spawn(move || {
            let result = docker
                .snapshot(&worker_cancelled)
                .map_err(|error| format!("{error:#}"));
            send_final(&sender, WorkerEvent::Refreshed(result), &worker_cancelled);
        });
        self.task = Some(Task {
            cancelled,
            thread: Some(thread),
        });
        self.last_refresh = Instant::now();
    }

    fn request(&mut self, action: Action, confirmed: bool) {
        if self.operating {
            return;
        }
        if self.task.is_some() {
            self.queued_action = Some(action);
            return;
        }
        self.operating = true;
        self.view = View::Output;
        self.output = TextBuffer::default();
        self.message = "Reading current container state...".into();
        let docker = self.docker.clone();
        let sender = self.sender.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = cancelled.clone();
        let thread = thread::spawn(move || {
            let result = (|| -> Result<Option<Snapshot>> {
                let snapshot = docker.snapshot(&worker_cancelled)?;
                match snapshot.plan(action, confirmed)? {
                    Decision::Confirm(message) => {
                        send_final(
                            &sender,
                            WorkerEvent::Confirm {
                                snapshot,
                                action,
                                message,
                            },
                            &worker_cancelled,
                        );
                        return Ok(None);
                    }
                    Decision::Nothing(message) => {
                        send_final(
                            &sender,
                            WorkerEvent::Completed {
                                snapshot: Some(snapshot),
                                result: Ok(message.into()),
                            },
                            &worker_cancelled,
                        );
                        return Ok(None);
                    }
                    Decision::Execute(steps) => {
                        docker.execute(&steps, &worker_cancelled, |line| {
                            let _ = sender.try_send(WorkerEvent::Line(line));
                        })?
                    }
                }
                Ok(Some(docker.snapshot(&worker_cancelled)?))
            })();
            match result {
                Ok(Some(snapshot)) => send_final(
                    &sender,
                    WorkerEvent::Completed {
                        snapshot: Some(snapshot),
                        result: Ok("Operation completed".into()),
                    },
                    &worker_cancelled,
                ),
                Ok(None) => {}
                Err(error) => {
                    let snapshot = docker.snapshot(&worker_cancelled).ok();
                    send_final(
                        &sender,
                        WorkerEvent::Completed {
                            snapshot,
                            result: Err(format!("{error:#}")),
                        },
                        &worker_cancelled,
                    );
                }
            }
        });
        self.task = Some(Task {
            cancelled,
            thread: Some(thread),
        });
    }

    pub fn tick(&mut self) {
        let events: Vec<_> = self.events.try_iter().collect();
        for event in events {
            match event {
                WorkerEvent::Line(line) => self.output.push(line),
                WorkerEvent::Refreshed(result) => {
                    self.task = None;
                    match result {
                        Ok(snapshot) => {
                            self.snapshot = Some(snapshot);
                            self.message = "Ready".into();
                        }
                        Err(error) => {
                            self.snapshot = None;
                            self.message = clean_text(&error);
                            self.queued_action = None;
                        }
                    }
                    if let Some(action) = self.queued_action.take() {
                        self.request(action, false);
                    }
                }
                WorkerEvent::Confirm {
                    snapshot,
                    action,
                    message,
                } => {
                    self.task = None;
                    self.operating = false;
                    self.snapshot = Some(snapshot);
                    self.view = View::Dashboard;
                    self.confirmation = Some(Confirmation {
                        action,
                        message,
                        yes: false,
                    });
                }
                WorkerEvent::Completed { snapshot, result } => {
                    self.task = None;
                    self.operating = false;
                    self.snapshot = snapshot;
                    self.message = match result {
                        Ok(message) => message,
                        Err(error) => format!("Error: {error}"),
                    };
                    self.output.push(self.message.clone());
                    self.last_refresh = Instant::now();
                }
            }
        }
        if let Some(logs) = &mut self.logs {
            for line in logs.drain() {
                self.log_buffer.push(line);
            }
            if !self.log_ended {
                match logs.poll() {
                    Ok(Some(status)) => {
                        self.log_ended = true;
                        self.log_buffer.push(format!("Log stream ended ({status})"));
                    }
                    Err(error) => {
                        self.log_ended = true;
                        self.log_buffer.push(format!("Log stream error: {error}"));
                    }
                    _ => {}
                }
            }
        }
        if self.view == View::Dashboard
            && self.confirmation.is_none()
            && self.task.is_none()
            && self.last_refresh.elapsed() >= REFRESH_INTERVAL
        {
            self.refresh();
        }
    }

    pub fn key(&mut self, key: KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press {
            return false;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return true;
        }
        if key.code == KeyCode::Char('q') && self.confirmation.is_none() {
            return true;
        }
        if let Some(confirmation) = &mut self.confirmation {
            match key.code {
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right => {
                    confirmation.yes = !confirmation.yes
                }
                KeyCode::Esc | KeyCode::Char('n') => {
                    self.confirmation = None;
                    self.message = "Cancelled".into();
                }
                KeyCode::Enter => {
                    let confirmation = self.confirmation.take().unwrap();
                    if confirmation.yes {
                        self.request(confirmation.action, true);
                    } else {
                        self.message = "Cancelled".into();
                    }
                }
                _ => {}
            }
            return false;
        }
        if self.view != View::Dashboard {
            if key.code == KeyCode::Esc {
                if !self.operating {
                    self.logs = None;
                    self.view = View::Dashboard;
                    self.last_refresh = Instant::now() - REFRESH_INTERVAL;
                }
            } else {
                let buffer = if self.view == View::Logs {
                    &mut self.log_buffer
                } else {
                    &mut self.output
                };
                match key.code {
                    KeyCode::Up => buffer.move_up(1),
                    KeyCode::Down => buffer.move_down(1),
                    KeyCode::PageUp => buffer.move_up(10),
                    KeyCode::PageDown => buffer.move_down(10),
                    KeyCode::Home => buffer.scroll = buffer.lines.len().saturating_sub(1),
                    KeyCode::End => buffer.scroll = 0,
                    KeyCode::Left => buffer.horizontal = buffer.horizontal.saturating_sub(4),
                    KeyCode::Right => buffer.horizontal = buffer.horizontal.saturating_add(4),
                    _ => {}
                }
            }
            return false;
        }
        match key.code {
            KeyCode::Tab => self.focus = self.focus.next(false),
            KeyCode::BackTab => self.focus = self.focus.next(true),
            KeyCode::F(5) => self.refresh(),
            KeyCode::Esc => self.focus = Focus::Components,
            KeyCode::Up | KeyCode::Down if self.focus == Focus::Components => {
                self.selected = (self.selected + if key.code == KeyCode::Up { 2 } else { 1 }) % 3;
                self.action = usize::from(self.running(self.service()));
            }
            KeyCode::Left | KeyCode::Right if self.focus == Focus::Actions => {
                self.action = (self.action + if key.code == KeyCode::Left { 3 } else { 1 }) % 4;
            }
            KeyCode::Up | KeyCode::Down if self.focus == Focus::Bulk => self.bulk = 1 - self.bulk,
            KeyCode::Up if self.focus == Focus::Settings => {
                self.settings_scroll = self.settings_scroll.saturating_sub(1)
            }
            KeyCode::Down if self.focus == Focus::Settings => {
                self.settings_scroll = self
                    .settings_scroll
                    .saturating_add(1)
                    .min(self.settings_max_scroll)
            }
            KeyCode::PageUp if self.focus == Focus::Settings => {
                self.settings_scroll = self.settings_scroll.saturating_sub(10)
            }
            KeyCode::PageDown if self.focus == Focus::Settings => {
                self.settings_scroll = self
                    .settings_scroll
                    .saturating_add(10)
                    .min(self.settings_max_scroll)
            }
            KeyCode::Home if self.focus == Focus::Settings => self.settings_scroll = 0,
            KeyCode::End if self.focus == Focus::Settings => {
                self.settings_scroll = self.settings_max_scroll
            }
            KeyCode::Up if self.focus == Focus::Status => {
                self.status_scroll.0 = self.status_scroll.0.saturating_sub(1)
            }
            KeyCode::Down if self.focus == Focus::Status => {
                self.status_scroll.0 = self.status_scroll.0.saturating_add(1).min(10)
            }
            KeyCode::Left if self.focus == Focus::Status => {
                self.status_scroll.1 = self.status_scroll.1.saturating_sub(4)
            }
            KeyCode::Right if self.focus == Focus::Status => {
                self.status_scroll.1 = self.status_scroll.1.saturating_add(4)
            }
            KeyCode::Enter => match self.focus {
                Focus::Components => {
                    self.focus = Focus::Actions;
                    self.action = usize::from(self.running(self.service()));
                }
                Focus::Actions => self.activate(),
                Focus::Bulk => self.request(
                    if self.bulk == 0 {
                        Action::RestartRunning
                    } else {
                        Action::StopRunning
                    },
                    false,
                ),
                _ => {}
            },
            KeyCode::Char('s') => {
                self.action = 0;
                self.activate();
            }
            KeyCode::Char('r') => {
                self.action = 1;
                self.activate();
            }
            KeyCode::Char('x') => {
                self.action = 2;
                self.activate();
            }
            KeyCode::Char('l') => {
                self.action = 3;
                self.activate();
            }
            _ => {}
        }
        false
    }

    fn activate(&mut self) {
        if !self.enabled(self.action) {
            return;
        }
        let service = self.service();
        match self.action {
            0 => self.request(Action::Start(service), false),
            1 => self.request(Action::Restart(service), false),
            2 => self.request(Action::Stop(service), false),
            3 => match self.docker.logs(service) {
                Ok(logs) => {
                    self.logs = Some(logs);
                    self.log_ended = false;
                    self.log_buffer = TextBuffer::default();
                    self.log_service = Some(service);
                    self.view = View::Logs;
                }
                Err(error) => self.message = format!("Cannot open logs: {error:#}"),
            },
            _ => {}
        }
    }
}

fn send_final(sender: &SyncSender<WorkerEvent>, mut event: WorkerEvent, cancelled: &AtomicBool) {
    while !cancelled.load(Ordering::Relaxed) {
        match sender.try_send(event) {
            Ok(()) | Err(TrySendError::Disconnected(_)) => return,
            Err(TrySendError::Full(returned)) => event = returned,
        }
        thread::sleep(Duration::from_millis(10));
    }
}

struct RestoreTerminal;
impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
    }
}

pub fn run(docker: Docker) -> Result<()> {
    enable_raw_mode()?;
    let _restore = RestoreTerminal;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut app = App::new(docker);
    app.refresh();
    loop {
        app.tick();
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        if event::poll(Duration::from_millis(100))? {
            if let TerminalEvent::Key(key) = event::read()? {
                let size = terminal.size()?;
                if size.width < 80 || size.height < 24 {
                    if key.code == KeyCode::Char('q')
                        || (key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL))
                    {
                        break;
                    }
                    continue;
                }
                if app.key(key) {
                    break;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;

    #[test]
    fn bulk_actions_follow_the_vertical_layout() {
        let mut app = App::new(Docker::with_executable(
            Project {
                directory: "/unused".into(),
            },
            std::ffi::OsStr::new("/unused"),
        ));
        for _ in 0..2 {
            app.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        }
        assert_eq!(app.focus, Focus::Bulk);
        assert_eq!(app.bulk, 0);
        app.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.bulk, 1);
        for key in [KeyCode::Left, KeyCode::Right] {
            app.key(KeyEvent::new(key, KeyModifiers::NONE));
            assert_eq!(app.bulk, 1);
        }
        app.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(app.bulk, 0);
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn repeated_enter_does_not_accept_dependency_prompt() {
        let mut app = App::new(Docker::with_executable(
            Project {
                directory: "/unused".into(),
            },
            std::ffi::OsStr::new("/unused"),
        ));
        app.confirmation = Some(Confirmation {
            action: Action::Start(Service::Frontend),
            message: "Start both?",
            yes: false,
        });
        app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.confirmation.is_none());
        assert!(app.task.is_none());
        assert_eq!(app.message, "Cancelled");
    }

    #[test]
    fn log_scrollback_stays_in_place_while_new_lines_arrive_and_is_bounded() {
        let mut buffer = TextBuffer::default();
        for index in 0..20 {
            buffer.push(index.to_string());
        }
        buffer.move_up(5);
        let visible = buffer.visible(5);
        buffer.push("new".into());
        assert_eq!(buffer.visible(5), visible);
        for _ in 0..6000 {
            buffer.push("line".into());
        }
        assert_eq!(buffer.lines.len(), BUFFER_LINES);
        assert!(buffer.scroll < BUFFER_LINES);
    }
}
