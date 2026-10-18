use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::{
    app::{App, Focus, View},
    model::Service,
    process::clean_text,
};

const ACCENT: Color = Color::Cyan;

fn block(title: &str, focused: bool) -> Block<'_> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if focused { ACCENT } else { Color::DarkGray }))
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    if area.width < 80 || area.height < 24 {
        frame.render_widget(
            Paragraph::new(
                "newhatchctl\nResize the terminal to at least 80 × 24.\nq / Ctrl+C: exit",
            ),
            area,
        );
        return;
    }
    let sections = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(area);
    let header = Line::from(vec![
        Span::styled(
            " Нюхач / newhatchctl ",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::raw(clean_text(
            &app.docker.project.directory.display().to_string(),
        )),
    ]);
    frame.render_widget(Paragraph::new(header), sections[0]);
    match app.view {
        View::Dashboard => dashboard(frame, app, sections[1]),
        View::Logs | View::Output => output(frame, app, sections[1]),
    }
    let help = match app.view {
        View::Dashboard if app.focus == Focus::Bulk => "Tab focus · ↑↓ action · Enter execute · Esc components · q quit",
        View::Dashboard if app.focus == Focus::Settings => "Tab focus · ↑↓ / PgUp/PgDn scroll · Home/End · F5 refresh · q quit",
        View::Dashboard => "Tab focus · ↑↓ component/scroll · ←→ action/scroll · Enter select · s/r/x/l · F5 refresh · q quit",
        View::Logs => "↑↓ / PgUp/PgDn scroll · ←→ horizontal · Home oldest · End follow · Esc back · q quit",
        View::Output if app.busy() => "Operation in progress · ↑↓ scroll · Ctrl+C cancels the CLI and exits",
        View::Output => "↑↓ / PgUp/PgDn scroll · ←→ horizontal · Esc back · q quit",
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(clean_text(&app.message)),
            Line::from(help).style(Style::default().fg(Color::DarkGray)),
        ]),
        sections[2],
    );
    if let Some(confirmation) = &app.confirmation {
        let popup = Rect::new(
            area.x + (area.width - 64) / 2,
            area.y + (area.height - 7) / 2,
            64,
            7,
        );
        frame.render_widget(Clear, popup);
        let yes_style = if confirmation.yes {
            Style::default().bg(ACCENT).fg(Color::Black)
        } else {
            Style::default()
        };
        let no_style = if confirmation.yes {
            Style::default()
        } else {
            Style::default().bg(ACCENT).fg(Color::Black)
        };
        let text = vec![
            Line::from(confirmation.message),
            Line::from(""),
            Line::from(vec![
                Span::styled(" [ Yes ] ", yes_style),
                Span::raw("   "),
                Span::styled(" [ No ] ", no_style),
            ]),
            Line::from("Tab / ←→ choose · Enter confirm · Esc cancel")
                .style(Style::default().fg(Color::DarkGray)),
        ];
        frame.render_widget(
            Paragraph::new(text).block(block(" Confirmation ", true)),
            popup,
        );
    }
}

fn dashboard(frame: &mut Frame, app: &mut App, area: Rect) {
    let sections = Layout::vertical([Constraint::Min(13), Constraint::Length(7)]).split(area);
    let columns =
        Layout::horizontal([Constraint::Length(38), Constraint::Min(40)]).split(sections[0]);
    let controls = Layout::vertical([
        Constraint::Length(6),
        Constraint::Length(3),
        Constraint::Min(4),
    ])
    .split(columns[0]);
    let rows = Service::ALL
        .iter()
        .map(|service| {
            let state = app
                .snapshot
                .as_ref()
                .and_then(|snapshot| {
                    snapshot
                        .containers
                        .iter()
                        .find(|container| container.service == *service)
                })
                .map(|container| container.state.as_str())
                .unwrap_or("not created");
            let label = if *service == Service::Analyzer
                && app.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot.desired_value(Service::Analyzer, "ANALYZER") == "remote"
                }) {
                "Receiver / Analyzer"
            } else {
                service.name()
            };
            let color = if state == "running" {
                Color::Green
            } else {
                Color::DarkGray
            };
            ListItem::new(Line::from(vec![
                Span::raw(format!("{label:<20}")),
                Span::styled(state, Style::default().fg(color)),
            ]))
        })
        .collect::<Vec<_>>();
    let list = List::new(rows)
        .block(block(" Components ", app.focus == Focus::Components))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    frame.render_stateful_widget(
        list,
        controls[0],
        &mut ListState::default().with_selected(Some(app.selected)),
    );
    let buttons = ["Start", "Restart", "Stop", "Logs"]
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let mut style = Style::default().fg(if app.enabled(index) {
                Color::White
            } else {
                Color::DarkGray
            });
            if app.focus == Focus::Actions && index == app.action {
                style = style.bg(ACCENT).fg(Color::Black);
            }
            Span::styled(format!("[{label}] "), style)
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(Line::from(buttons)).block(block(" Actions ", app.focus == Focus::Actions)),
        controls[1],
    );
    let bulk = ["Restart running", "Stop running"]
        .iter()
        .enumerate()
        .map(|(index, label)| {
            let selected = app.focus == Focus::Bulk && index == app.bulk;
            Line::from(Span::styled(
                format!("[{label}]"),
                if selected {
                    Style::default().fg(Color::Black).bg(ACCENT)
                } else {
                    Style::default()
                },
            ))
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(bulk).block(block(" All running · ↑↓ ", app.focus == Focus::Bulk)),
        controls[2],
    );

    let settings = if let Some(snapshot) = &app.snapshot {
        let mut lines = Vec::new();
        for setting in snapshot.settings() {
            lines.push(Line::from(vec![
                Span::styled(
                    clean_text(&format!("{} = ", setting.key)),
                    Style::default().fg(ACCENT),
                ),
                Span::raw(if setting.raw {
                    clean_text(&setting.desired).replace('\n', "\\n")
                } else {
                    quoted(&setting.desired)
                }),
            ]));
            for (service, active) in setting.active {
                lines.push(Line::from(vec![
                    Span::raw(format!(
                        "  ACTIVE ({}): {} ",
                        service.name(),
                        quoted(&active)
                    )),
                    Span::styled("[Needs Restart]", Style::default().fg(Color::Yellow)),
                ]));
            }
        }
        lines
    } else {
        vec![Line::from("Waiting for Docker / Compose...")]
    };
    let settings = Paragraph::new(settings)
        .wrap(Wrap { trim: false })
        .block(block(
            " Settings · read only · ↑↓ scroll ",
            app.focus == Focus::Settings,
        ));
    app.settings_max_scroll = settings
        .line_count(columns[1].width)
        .saturating_sub(usize::from(columns[1].height))
        .min(usize::from(u16::MAX)) as u16;
    app.settings_scroll = app.settings_scroll.min(app.settings_max_scroll);
    frame.render_widget(settings.scroll((app.settings_scroll, 0)), columns[1]);
    let table = app
        .snapshot
        .as_ref()
        .map(|snapshot| snapshot.status_table.clone())
        .unwrap_or_else(|| clean_text(&app.message));
    frame.render_widget(
        Paragraph::new(table)
            .block(block(
                " Compose ps · ↑↓ / ←→ scroll ",
                app.focus == Focus::Status,
            ))
            .scroll(app.status_scroll),
        sections[1],
    );
}

fn output(frame: &mut Frame, app: &App, area: Rect) {
    let (buffer, title) = if app.view == View::Logs {
        (
            &app.log_buffer,
            format!(
                " {} logs · {} ",
                app.log_service.map(Service::name).unwrap_or("component"),
                if app.log_buffer.scroll == 0 {
                    "following"
                } else {
                    "scrollback"
                }
            ),
        )
    } else {
        (&app.output, " Operation output ".into())
    };
    frame.render_widget(
        Paragraph::new(buffer.visible(usize::from(area.height.saturating_sub(2))))
            .scroll((0, buffer.horizontal))
            .block(block(&title, true)),
        area,
    );
}

fn quoted(value: &str) -> String {
    format!(
        "\"{}\"",
        clean_text(value).replace('\n', "\\n").replace('"', "\\\"")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        docker::Docker,
        model::{Container, Snapshot},
        project::Project,
    };
    use ratatui::{backend::TestBackend, Terminal};
    use std::{collections::BTreeMap, ffi::OsStr};

    #[test]
    fn settings_scroll_to_the_last_file_value_and_clamp_after_refresh() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = App::new(Docker::with_executable(
            Project {
                directory: "/unused".into(),
            },
            OsStr::new("/unused"),
        ));
        let mut file_settings: Vec<_> = (0..60)
            .map(|index| (format!("CUSTOM_{index}"), format!("value_{index}")))
            .collect();
        file_settings.push((
            "LONG_VALUE".into(),
            format!("{}last-value-marker", "word ".repeat(200)),
        ));
        app.snapshot = Some(Snapshot {
            file_settings,
            ..Snapshot::default()
        });
        app.focus = Focus::Settings;
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        assert!(app.settings_max_scroll > 20);
        app.key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE));
        assert_eq!(app.settings_scroll, 10);
        app.key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("last-value-marker"));
        app.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.settings_scroll, app.settings_max_scroll);
        app.snapshot.as_mut().unwrap().file_settings.clear();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        assert_eq!(app.settings_scroll, 0);
    }

    #[test]
    fn dashboard_renders_status_headers_and_restart_annotations() {
        let mut app = App::new(Docker::with_executable(
            Project {
                directory: "/newhatch".into(),
            },
            OsStr::new("/unused"),
        ));
        app.snapshot = Some(Snapshot {
            desired: BTreeMap::from([(
                Service::Analyzer,
                BTreeMap::from([("ANALYZER".into(), "remote".into())]),
            )]),
            containers: vec![Container {
                id: "id".into(),
                service: Service::Analyzer,
                state: "running".into(),
                environment: BTreeMap::from([("ANALYZER".into(), "local".into())]),
            }],
            status_table: "NAME IMAGE COMMAND SERVICE CREATED STATUS PORTS\nnewhatch-analyzer"
                .into(),
            ..Snapshot::default()
        });
        let mut terminal = Terminal::new(TestBackend::new(140, 35)).unwrap();
        terminal.draw(|frame| draw(frame, &mut app)).unwrap();
        let buffer = terminal.backend().buffer();
        let text = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains("Receiver / Analyzer"));
        assert!(text.contains("[Needs Restart]"));
        assert!(text.contains("NAME IMAGE COMMAND SERVICE CREATED STATUS PORTS"));
    }
}
