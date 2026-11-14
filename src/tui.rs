use crate::collector::{self, endpoint_parts, TcpConnection};
use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Cell, Paragraph, Row, Table, TableState, Wrap},
    Frame,
};
use std::{
    cmp::Ordering,
    net::SocketAddr,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SortColumn {
    Local,
    Remote,
    State,
    Pid,
    Process,
    LocalPort,
    RemotePort,
}

impl SortColumn {
    const ALL: [Self; 7] = [
        Self::Local,
        Self::Remote,
        Self::State,
        Self::Pid,
        Self::Process,
        Self::LocalPort,
        Self::RemotePort,
    ];
    const DISPLAY: [Self; 7] = [
        Self::Local,
        Self::LocalPort,
        Self::Remote,
        Self::RemotePort,
        Self::State,
        Self::Pid,
        Self::Process,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Local => "LOCAL",
            Self::Remote => "REMOTE",
            Self::State => "STATE",
            Self::Pid => "PID",
            Self::Process => "PROCESS",
            Self::LocalPort => "LOCAL PORT",
            Self::RemotePort => "REMOTE PORT",
        }
    }

    fn compare(self, a: &TcpConnection, b: &TcpConnection) -> Ordering {
        match self {
            Self::Local => compare_endpoint_by(&a.local, &b.local, |a, b| a.ip().cmp(&b.ip())),
            Self::Remote => compare_endpoint_by(&a.remote, &b.remote, |a, b| a.ip().cmp(&b.ip())),
            Self::LocalPort => {
                compare_endpoint_by(&a.local, &b.local, |a, b| a.port().cmp(&b.port()))
            }
            Self::RemotePort => {
                compare_endpoint_by(&a.remote, &b.remote, |a, b| a.port().cmp(&b.port()))
            }
            Self::State => a.state.to_lowercase().cmp(&b.state.to_lowercase()),
            Self::Pid => a.pid.cmp(&b.pid),
            Self::Process => a.process.to_lowercase().cmp(&b.process.to_lowercase()),
        }
    }
}

fn compare_endpoints(a: &str, b: &str) -> Ordering {
    compare_endpoint_by(a, b, |a, b| {
        a.ip().cmp(&b.ip()).then_with(|| a.port().cmp(&b.port()))
    })
}

fn compare_endpoint_by(
    a: &str,
    b: &str,
    compare: impl FnOnce(SocketAddr, SocketAddr) -> Ordering,
) -> Ordering {
    match (a.parse::<SocketAddr>(), b.parse::<SocketAddr>()) {
        (Ok(a), Ok(b)) => compare(a, b),
        // Keep malformed or non-IP endpoints in a separate group for a total ordering.
        (Ok(_), Err(_)) => Ordering::Less,
        (Err(_), Ok(_)) => Ordering::Greater,
        (Err(_), Err(_)) => a.cmp(b),
    }
}

pub(crate) struct App {
    entries: Vec<TcpConnection>,
    pub(crate) visible: Vec<TcpConnection>,
    table: TableState,
    sort_column: SortColumn,
    sort_descending: bool,
    listening: bool,
    process_filter: String,
    query: String,
    editing: bool,
    previous_query: String,
    paused: bool,
    error: Option<String>,
    updated: Option<Instant>,
}

impl App {
    pub(crate) fn new(listening: bool, process_filter: String) -> Self {
        Self {
            entries: Vec::new(),
            visible: Vec::new(),
            table: TableState::default(),
            sort_column: SortColumn::Pid,
            sort_descending: false,
            listening,
            process_filter,
            query: String::new(),
            editing: false,
            previous_query: String::new(),
            paused: false,
            error: None,
            updated: None,
        }
    }

    pub(crate) fn set_entries(&mut self, entries: Vec<TcpConnection>) {
        self.entries = entries;
        self.refilter();
    }

    fn refilter(&mut self) {
        let selected = self
            .table
            .selected()
            .and_then(|i| self.visible.get(i))
            .cloned();
        let query = self.query.to_lowercase();
        let process_filter = self.process_filter.to_lowercase();
        self.visible = self
            .entries
            .iter()
            .filter(|entry| !self.listening || entry.state == "LISTEN")
            .filter(|entry| entry.process.to_lowercase().contains(&process_filter))
            .filter(|entry| {
                [
                    entry.local.as_str(),
                    entry.remote.as_str(),
                    entry.state.as_str(),
                    entry.process.as_str(),
                ]
                .iter()
                .any(|value| value.to_lowercase().contains(&query))
                    || entry.pid.to_string().contains(&query)
            })
            .cloned()
            .collect();
        self.visible.sort_by(|a, b| {
            let order = self
                .sort_column
                .compare(a, b)
                .then_with(|| a.pid.cmp(&b.pid))
                .then_with(|| compare_endpoints(&a.local, &b.local))
                .then_with(|| compare_endpoints(&a.remote, &b.remote))
                .then_with(|| a.process.cmp(&b.process))
                .then_with(|| a.state.cmp(&b.state));
            if self.sort_descending {
                order.reverse()
            } else {
                order
            }
        });
        let index = selected.and_then(|old| {
            self.visible.iter().position(|entry| {
                entry.pid == old.pid && entry.local == old.local && entry.remote == old.remote
            })
        });
        self.table.select(if self.visible.is_empty() {
            None
        } else {
            Some(index.unwrap_or_else(|| {
                self.table
                    .selected()
                    .unwrap_or(0)
                    .min(self.visible.len() - 1)
            }))
        });
    }

    fn sort_by(&mut self, column: SortColumn) {
        if self.sort_column == column {
            self.sort_descending = !self.sort_descending;
        } else {
            self.sort_column = column;
            self.sort_descending = false;
        }
        self.refilter();
    }

    fn refresh(&mut self) {
        match collector::tcp_connections() {
            Ok(entries) => {
                self.set_entries(entries);
                self.error = None;
                self.updated = Some(Instant::now());
            }
            Err(error) => self.error = Some(format!("{error:#}")),
        }
    }

    fn move_selection(&mut self, delta: isize) {
        if !self.visible.is_empty() {
            let next = self
                .table
                .selected()
                .unwrap_or(0)
                .saturating_add_signed(delta);
            self.table.select(Some(next.min(self.visible.len() - 1)));
        }
    }

    /// Returns true when the user requests exit.
    fn key(&mut self, key: KeyEvent) -> bool {
        if key.kind == KeyEventKind::Release {
            return false;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return true;
        }
        if self.editing {
            match key.code {
                KeyCode::Enter => self.editing = false,
                KeyCode::Esc => {
                    self.query.clone_from(&self.previous_query);
                    self.editing = false;
                }
                KeyCode::Backspace => {
                    self.query.pop();
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.query.push(c);
                }
                _ => {}
            }
            self.refilter();
            return false;
        }
        match key.code {
            KeyCode::Char(c @ '1'..='7') => {
                self.sort_by(SortColumn::ALL[(c as u8 - b'1') as usize]);
            }
            KeyCode::Char('q') | KeyCode::Esc => return true,
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::Home => self.move_selection(isize::MIN),
            KeyCode::End => self.move_selection(isize::MAX),
            KeyCode::Char('/') => {
                self.previous_query.clone_from(&self.query);
                self.editing = true;
            }
            KeyCode::Char('l') => {
                self.listening = !self.listening;
                self.refilter();
            }
            KeyCode::Char('c') => {
                self.query.clear();
                self.process_filter.clear();
                self.refilter();
            }
            KeyCode::Char(' ') => self.paused = !self.paused,
            KeyCode::Char('r') => self.refresh(),
            _ => {}
        }
        false
    }

    fn draw(&mut self, frame: &mut Frame, interval: Duration) {
        let areas = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(5),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(frame.area());
        let listeners = self.entries.iter().filter(|e| e.state == "LISTEN").count();
        let established = self
            .entries
            .iter()
            .filter(|e| e.state == "ESTABLISHED")
            .count();
        let status = if self.paused { "PAUSED" } else { "LIVE" };
        let age = self.updated.map_or_else(
            || "not yet refreshed".to_owned(),
            |time| format!("updated {}s ago", time.elapsed().as_secs()),
        );
        let summary = format!(
            " {status} | {} shown / {} total | {listeners} listening | {established} established | refresh {}s | {age}",
            self.visible.len(), self.entries.len(), interval.as_secs()
        );
        frame.render_widget(
            Paragraph::new(summary)
                .block(Block::bordered().title(" NetView · IPv4 TCP "))
                .style(Style::default().fg(Color::Cyan)),
            areas[0],
        );

        let rows = self.visible.iter().map(|entry| {
            let (local, local_port) = endpoint_parts(&entry.local);
            let (remote, remote_port) = endpoint_parts(&entry.remote);
            let color = match entry.state.as_str() {
                "ESTABLISHED" => Color::Green,
                "LISTEN" => Color::Cyan,
                _ => Color::Yellow,
            };
            Row::new(vec![
                Cell::from(local),
                Cell::from(local_port),
                Cell::from(remote),
                Cell::from(remote_port),
                Cell::from(entry.state.clone()).style(Style::default().fg(color)),
                Cell::from(entry.pid.to_string()),
                Cell::from(entry.process.clone()),
            ])
        });
        let title = if self.visible.is_empty() {
            " Connections — no matching connections "
        } else {
            " Connections "
        };
        let table = Table::new(
            rows,
            [
                Constraint::Length(15),
                Constraint::Length(12),
                Constraint::Length(15),
                Constraint::Length(13),
                Constraint::Length(13),
                Constraint::Length(7),
                Constraint::Min(12),
            ],
        )
        .header(
            Row::new(SortColumn::DISPLAY.map(|column| {
                if column == self.sort_column {
                    format!(
                        "{} {}",
                        column.label(),
                        if self.sort_descending { "↓" } else { "↑" }
                    )
                } else {
                    column.label().to_owned()
                }
            }))
            .style(Style::default().add_modifier(Modifier::BOLD)),
        )
        .block(Block::bordered().title(title))
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
        frame.render_stateful_widget(table, areas[1], &mut self.table);

        let details = self
            .table
            .selected()
            .and_then(|i| self.visible.get(i))
            .map(|entry| {
                format!(
                    "{} → {}  [{}]  PID {}\nProcess: {}",
                    entry.local, entry.remote, entry.state, entry.pid, entry.process
                )
            })
            .unwrap_or_else(|| "No connection selected.".to_owned());
        frame.render_widget(
            Paragraph::new(details)
                .wrap(Wrap { trim: false })
                .block(Block::bordered().title(" Selected connection ")),
            areas[2],
        );
        let mode = if self.listening {
            "listeners only"
        } else {
            "all connections"
        };
        let process_filter = if self.process_filter.is_empty() {
            String::new()
        } else {
            format!(" | process: {}", self.process_filter)
        };
        let filter = format!(
            "All columns: {}{} | {mode}{process_filter}",
            self.query,
            if self.editing {
                "▏ (Enter: apply · Esc: cancel)"
            } else {
                ""
            }
        );
        frame.render_widget(
            Paragraph::new(filter)
                .style(Style::default().fg(if self.editing {
                    Color::Yellow
                } else {
                    Color::White
                }))
                .block(Block::bordered().title(" Filter · / to edit · c to clear ")),
            areas[3],
        );
        let footer = match &self.error {
            Some(error) => vec![
                Line::styled(
                    format!("Refresh failed: {error}"),
                    Style::default().fg(Color::Red),
                ),
                Line::from("r retry · q quit | Last successful results retained"),
            ],
            None => vec![
                Line::from(
                    "Sort: 1 local · 6 local port · 2 remote · 7 remote port · 3 state · 4 PID · 5 process (repeat: reverse)",
                ),
                Line::from("↑/↓ j/k select · PgUp/PgDn scroll · Home/End · / search · l listeners"),
                Line::from("Space pause · r refresh · c clear · q/Esc quit · Ctrl+C quit"),
            ],
        };
        frame.render_widget(Paragraph::new(footer), areas[4]);
    }
}

struct RestoreTerminal;

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

pub(crate) fn run(listening: bool, query: String, interval: Duration) -> Result<()> {
    // Ratatui installs a panic hook that restores the terminal before reporting a panic.
    let _restore = RestoreTerminal;
    let mut terminal = ratatui::try_init()?;
    let mut app = App::new(listening, query);
    app.refresh();
    let mut last_refresh = Instant::now();
    loop {
        if !app.paused && last_refresh.elapsed() >= interval {
            app.refresh();
            last_refresh = Instant::now();
        }
        terminal.draw(|frame| app.draw(frame, interval))?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
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
    use ratatui::{backend::TestBackend, Terminal};

    fn entry(pid: u32, process: &str, state: &str) -> TcpConnection {
        TcpConnection {
            local: format!("127.0.0.1:{}", 8000 + pid),
            remote: "0.0.0.0:0".into(),
            state: state.into(),
            pid,
            process: process.into(),
        }
    }

    fn populated() -> App {
        let mut app = App::new(false, String::new());
        app.set_entries(vec![
            entry(3, "Chrome", "ESTABLISHED"),
            entry(1, "sshd", "LISTEN"),
            entry(2, "chrome-helper", "LISTEN"),
        ]);
        app
    }

    fn press(app: &mut App, code: KeyCode) -> bool {
        app.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn port_sort_keys_remain_text_while_searching() {
        let mut app = populated();
        press(&mut app, KeyCode::Char('6'));
        assert_eq!(app.sort_column, SortColumn::LocalPort);
        assert!(!app.sort_descending);
        press(&mut app, KeyCode::Char('7'));
        assert_eq!(app.sort_column, SortColumn::RemotePort);
        press(&mut app, KeyCode::Char('/'));
        press(&mut app, KeyCode::Char('6'));
        press(&mut app, KeyCode::Char('7'));
        assert_eq!(app.query, "67");
        assert_eq!(app.sort_column, SortColumn::RemotePort);
        assert!(!app.sort_descending);
    }

    #[test]
    fn address_and_port_comparisons_are_independent() {
        let mut a = entry(1, "a", "ESTABLISHED");
        let mut b = entry(2, "b", "ESTABLISHED");
        a.local = "192.0.2.10:9".into();
        b.local = "192.0.2.2:100".into();
        a.remote = "[::10]:65535".into();
        b.remote = "[::2]:0".into();
        assert_eq!(SortColumn::Local.compare(&a, &b), Ordering::Greater);
        assert_eq!(SortColumn::LocalPort.compare(&a, &b), Ordering::Less);
        assert_eq!(SortColumn::Remote.compare(&a, &b), Ordering::Greater);
        assert_eq!(SortColumn::RemotePort.compare(&a, &b), Ordering::Greater);
        b.local = "192.0.2.10:100".into();
        assert_eq!(SortColumn::Local.compare(&a, &b), Ordering::Equal);
        b.local = "192.0.2.2:9".into();
        assert_eq!(SortColumn::LocalPort.compare(&a, &b), Ordering::Equal);
        b.remote = "invalid".into();
        assert_eq!(SortColumn::RemotePort.compare(&a, &b), Ordering::Less);
    }

    #[test]
    fn every_column_sorts_in_both_directions() {
        let a = TcpConnection {
            local: format!("{}:100", std::net::Ipv4Addr::LOCALHOST),
            remote: format!("{}:80", std::net::Ipv4Addr::new(192, 0, 2, 10)),
            state: "LISTEN".into(),
            pid: 10,
            process: "Alpha".into(),
        };
        let b = TcpConnection {
            local: format!("{}:20", std::net::Ipv4Addr::LOCALHOST),
            remote: format!("{}:443", std::net::Ipv4Addr::new(192, 0, 2, 2)),
            state: "ESTABLISHED".into(),
            pid: 2,
            process: "zebra".into(),
        };
        for (i, column) in SortColumn::ALL.into_iter().enumerate() {
            let mut app = App::new(false, String::new());
            app.set_entries(vec![a.clone(), b.clone()]);
            // Switch away first so selecting any column starts ascending.
            app.sort_column = if column == SortColumn::Pid {
                SortColumn::Local
            } else {
                SortColumn::Pid
            };
            let key = KeyCode::Char(char::from(b'1' + i as u8));
            press(&mut app, key);
            let expected = if matches!(column, SortColumn::Process | SortColumn::RemotePort) {
                vec![10, 2]
            } else {
                vec![2, 10]
            };
            assert_eq!(
                app.visible.iter().map(|e| e.pid).collect::<Vec<_>>(),
                expected,
                "{column:?}"
            );
            assert!(!app.sort_descending);
            press(&mut app, key);
            assert!(app.sort_descending);
            assert_eq!(
                app.visible.iter().map(|e| e.pid).collect::<Vec<_>>(),
                expected.into_iter().rev().collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn sorting_preserves_selection_and_survives_refresh_and_filtering() {
        let mut app = populated();
        press(&mut app, KeyCode::Down);
        let selected_pid = app.visible[app.table.selected().unwrap()].pid;
        press(&mut app, KeyCode::Char('5'));
        assert_eq!(app.visible[app.table.selected().unwrap()].pid, selected_pid);
        press(&mut app, KeyCode::Char('5'));
        let mut entries = app.entries.clone();
        entries.reverse();
        app.set_entries(entries);
        assert_eq!(app.sort_column, SortColumn::Process);
        assert!(app.sort_descending);
        assert_eq!(app.visible[app.table.selected().unwrap()].pid, selected_pid);
        assert_eq!(
            app.visible.iter().map(|e| e.pid).collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        app.query = "chrome".into();
        app.refilter();
        assert_eq!(
            app.visible.iter().map(|e| e.pid).collect::<Vec<_>>(),
            vec![2, 3]
        );
    }

    #[test]
    fn equal_values_have_deterministic_ties_across_refreshes() {
        let mut app = App::new(false, String::new());
        app.set_entries(vec![
            entry(10, "same", "LISTEN"),
            entry(2, "SAME", "LISTEN"),
        ]);
        press(&mut app, KeyCode::Char('5'));
        assert_eq!(
            app.visible.iter().map(|e| e.pid).collect::<Vec<_>>(),
            vec![2, 10]
        );
        let mut entries = app.entries.clone();
        entries.reverse();
        app.set_entries(entries);
        assert_eq!(
            app.visible.iter().map(|e| e.pid).collect::<Vec<_>>(),
            vec![2, 10]
        );
    }

    #[test]
    fn numeric_search_input_does_not_change_sort_and_empty_sort_is_safe() {
        let mut app = populated();
        press(&mut app, KeyCode::Char('/'));
        press(&mut app, KeyCode::Char('5'));
        assert_eq!(app.query, "5");
        assert_eq!(app.sort_column, SortColumn::Pid);
        assert!(!app.sort_descending);
        press(&mut app, KeyCode::Enter);
        for c in '1'..='7' {
            press(&mut app, KeyCode::Char(c));
            assert_eq!(app.table.selected(), None);
        }
    }

    #[test]
    fn endpoint_comparison_handles_ipv6_and_invalid_values() {
        assert_eq!(compare_endpoints("[::1]:9", "[::1]:100"), Ordering::Less);
        assert_eq!(compare_endpoints("[::2]:80", "[::10]:80"), Ordering::Less);
        assert_eq!(compare_endpoints("bad", "[::1]:80"), Ordering::Greater);
        assert_eq!(compare_endpoints("[::1]:80", "bad"), Ordering::Less);
        assert_eq!(compare_endpoints("bad-a", "bad-b"), Ordering::Less);
    }

    #[test]
    fn headers_show_active_sort_direction() {
        let mut app = populated();
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        for (key, label) in [
            (None, "PID ↑"),
            (Some('4'), "PID ↓"),
            (Some('5'), "PROCESS ↑"),
            (Some('6'), "LOCAL PORT ↑"),
            (Some('7'), "REMOTE PORT ↑"),
        ] {
            if let Some(key) = key {
                press(&mut app, KeyCode::Char(key));
            }
            terminal
                .draw(|frame| app.draw(frame, Duration::from_secs(2)))
                .unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(text.contains(label), "missing {label}");
        }
    }

    #[test]
    fn search_matches_each_column_case_insensitively() {
        let connection = TcpConnection {
            local: "local.example:8123".into(),
            remote: "remote.example:9443".into(),
            state: "ESTABLISHED".into(),
            pid: 45678,
            process: "/opt/MyService".into(),
        };
        for query in [
            "LOCAL.EXAMPLE",
            "8123",
            "REMOTE.EXAMPLE",
            "9443",
            ":9443",
            "eStAbLiShEd",
            "567",
            "MYSERVICE",
        ] {
            let mut app = App::new(false, String::new());
            app.set_entries(vec![connection.clone()]);
            press(&mut app, KeyCode::Char('/'));
            for c in query.chars() {
                press(&mut app, KeyCode::Char(c));
            }
            press(&mut app, KeyCode::Enter);
            assert_eq!(app.visible.len(), 1, "query: {query}");
        }
    }

    #[test]
    fn empty_search_matches_all_and_unmatched_search_clears_selection() {
        let mut app = populated();
        app.query = "not-in-any-column".into();
        app.refilter();
        assert!(app.visible.is_empty());
        assert_eq!(app.table.selected(), None);
        press(&mut app, KeyCode::Char('c'));
        assert_eq!(app.visible.len(), 3);
        assert_eq!(app.table.selected(), Some(0));
    }

    #[test]
    fn all_column_search_combines_with_process_and_listener_filters() {
        let mut app = App::new(true, "CHROME".into());
        app.set_entries(populated().entries);
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.visible[0].pid, 2);
        app.query = "established".into();
        app.refilter();
        assert!(app.visible.is_empty());
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.visible[0].pid, 3);
        press(&mut app, KeyCode::Char('c'));
        assert_eq!(app.visible.len(), 3);
        assert!(app.process_filter.is_empty());
    }

    #[test]
    fn startup_process_filter_does_not_match_other_columns() {
        let mut app = App::new(false, "LISTEN".into());
        app.set_entries(populated().entries);
        assert!(app.visible.is_empty());
    }

    #[test]
    fn filters_are_case_insensitive_and_composable() {
        let mut app = populated();
        app.query = "CHROME".into();
        app.refilter();
        assert_eq!(app.visible.len(), 2);
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.visible.len(), 1);
        assert_eq!(app.visible[0].pid, 2);
        press(&mut app, KeyCode::Char('c'));
        assert_eq!(app.visible.len(), 2);
    }

    #[test]
    fn navigation_clamps_and_empty_lists_have_no_selection() {
        let mut app = populated();
        assert_eq!(app.visible[0].pid, 1);
        press(&mut app, KeyCode::End);
        assert_eq!(app.table.selected(), Some(2));
        press(&mut app, KeyCode::Down);
        assert_eq!(app.table.selected(), Some(2));
        press(&mut app, KeyCode::Home);
        assert_eq!(app.table.selected(), Some(0));
        press(&mut app, KeyCode::Up);
        assert_eq!(app.table.selected(), Some(0));
        app.set_entries(vec![]);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.table.selected(), None);
    }

    #[test]
    fn selection_survives_refresh_and_removed_connections() {
        let mut app = populated();
        press(&mut app, KeyCode::Down);
        assert_eq!(app.visible[app.table.selected().unwrap()].pid, 2);
        app.set_entries(vec![entry(2, "chrome-helper", "LISTEN")]);
        assert_eq!(app.table.selected(), Some(0));
        app.set_entries(vec![entry(4, "new", "LISTEN")]);
        assert_eq!(app.table.selected(), Some(0));
    }

    #[test]
    fn selection_tracks_connection_when_new_rows_are_inserted() {
        let mut app = populated();
        press(&mut app, KeyCode::Down);
        let mut entries = app.entries.clone();
        entries.push(entry(0, "new", "LISTEN"));
        app.set_entries(entries);
        assert_eq!(app.table.selected(), Some(2));
        assert_eq!(app.visible[2].pid, 2);
    }

    #[test]
    fn search_supports_unicode_cancel_and_commit() {
        let mut app = populated();
        press(&mut app, KeyCode::Char('/'));
        assert!(!press(&mut app, KeyCode::Char('q')));
        press(&mut app, KeyCode::Char('é'));
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.query, "q");
        assert!(!press(&mut app, KeyCode::Esc));
        assert_eq!(app.query, "");
        assert_eq!(app.visible.len(), 3);
        press(&mut app, KeyCode::Char('/'));
        for c in "ssh".chars() {
            press(&mut app, KeyCode::Char(c));
        }
        press(&mut app, KeyCode::Enter);
        assert!(!app.editing);
        assert_eq!(app.visible.len(), 1);
    }

    #[test]
    fn pause_exit_and_key_release() {
        let mut app = populated();
        press(&mut app, KeyCode::Char(' '));
        assert!(app.paused);
        press(&mut app, KeyCode::Char(' '));
        assert!(!app.paused);
        assert!(!app.key(KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            KeyEventKind::Release,
        )));
        assert!(press(&mut app, KeyCode::Char('q')));
        app.editing = true;
        assert!(app.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)));
    }

    #[test]
    fn renders_table_details_and_error() {
        let mut app = populated();
        app.error = Some("test failure".into());
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal
            .draw(|frame| app.draw(frame, Duration::from_secs(2)))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("NetView"));
        assert!(text.contains("sshd"));
        assert!(text.contains("Selected connection"));
        assert!(text.contains("All columns:"));
        for header in [
            "LOCAL",
            "LOCAL PORT",
            "REMOTE",
            "REMOTE PORT",
            "STATE",
            "PID",
            "PROCESS",
        ] {
            assert!(text.contains(header), "missing {header}");
        }
        assert!(!text.contains("DIRECTION"));
        assert!(!text.contains("Direction:"));
        assert!(text.contains("test failure"));
    }

    #[test]
    fn renders_small_and_empty_terminals_without_panicking() {
        for (width, height) in [(80, 24), (40, 12), (1, 1)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut app = App::new(false, String::new());
            terminal
                .draw(|frame| app.draw(frame, Duration::from_secs(2)))
                .unwrap();
        }
    }
}
