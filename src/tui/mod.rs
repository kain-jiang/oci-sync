//! Full-screen TUI: two panes (shortcuts | artifacts), a details footer and
//! modal dialogs for pull/delete. Keymap documented in docs/interaction.md §3.

use std::time::{Duration, Instant};

use anyhow::{Result, anyhow};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, Cell, Clear, List, ListItem, ListState, Paragraph, Row, Table, TableState,
};
use tokio::sync::mpsc;

use crate::cache::{self, Activity, ActivityType};
use crate::config::{self, Config};
use crate::oci::{ArtifactInfo, OciClient};
use crate::output::format_bytes;

const HELP_TEXT: &str = "\
┌─ Navigation ─────────────────────────────────────────┐
│ Tab / ← → / h l      switch pane (shortcuts|artifacts) │
│ ↑ ↓ / j k            move selection                   │
│ Enter (shortcuts)    load artifacts of that repo      │
│ /                    search/filter tags               │
│ s                    sort artifacts by size (toggle)  │
│ r                    refresh artifacts                │
│ ?                    toggle this help                 │
├─ Actions ─────────────────────────────────────────────┤
│ p                    pull selected artifact (dialog)  │
│ d                    delete selected artifact (dialog)│
├─ Misc ────────────────────────────────────────────────┤
│ Esc                  close dialog / clear search      │
│ q / Ctrl+C           quit                             │
└──────────────────────────────────────────────────────┘";

enum Focus {
    Shortcuts,
    Artifacts,
}

enum Dialog {
    None,
    Pull {
        path: String,
        path_cursor: usize,
        passphrase: String,
        pass_cursor: usize,
        busy: bool,
    },
    Delete {
        busy: bool,
    },
    Help,
}

enum TuiMsg {
    ArtifactsLoaded {
        repo: String,
        artifacts: Vec<ArtifactInfo>,
    },
    LoadFailed(String),
    OpDone(String),
    OpFailed(String),
}

struct TuiApp {
    cfg: Config,
    shortcuts: Vec<(String, String)>,
    shortcut_state: ListState,
    artifacts: Vec<ArtifactInfo>,
    artifact_state: TableState,
    artifacts_repo: Option<String>,
    focus: Focus,
    search: String,
    search_active: bool,
    sort_by_size: bool,
    dialog: Dialog,
    toast: Option<(String, bool, Instant)>, // (msg, is_error, shown_at)
    quit: bool,
    tx: mpsc::UnboundedSender<TuiMsg>,
    rx: mpsc::UnboundedReceiver<TuiMsg>,
}

/// Launch the TUI. Returns when the user quits.
pub async fn run() -> Result<()> {
    let cfg = config::load()?;
    let (tx, rx) = mpsc::unbounded_channel();
    let mut app = TuiApp::new(cfg, tx, rx);

    let mut terminal = ratatui::init();
    let result = app.run_loop(&mut terminal).await;
    ratatui::restore();
    result
}

impl TuiApp {
    fn new(
        cfg: Config,
        tx: mpsc::UnboundedSender<TuiMsg>,
        rx: mpsc::UnboundedReceiver<TuiMsg>,
    ) -> Self {
        let shortcuts = cfg.all_shortcuts();
        let mut shortcut_state = ListState::default();
        if !shortcuts.is_empty() {
            shortcut_state.select(Some(0));
        }
        Self {
            cfg,
            shortcuts,
            shortcut_state,
            artifacts: Vec::new(),
            artifact_state: TableState::default(),
            artifacts_repo: None,
            focus: Focus::Shortcuts,
            search: String::new(),
            search_active: false,
            sort_by_size: false,
            dialog: Dialog::None,
            toast: None,
            quit: false,
            tx,
            rx,
        }
    }

    async fn run_loop(
        &mut self,
        terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        while !self.quit {
            terminal.draw(|f| self.ui(f))?;
            self.drain_messages();

            if event::poll(Duration::from_millis(150)).map_err(|e| anyhow!("event poll: {e}"))? {
                if let Event::Key(key) = event::read().map_err(|e| anyhow!("event read: {e}"))? {
                    if key.kind == KeyEventKind::Press {
                        self.handle_key(key).await;
                    }
                }
            }

            // expire toasts
            if let Some((_, _, at)) = self.toast {
                if at.elapsed() > Duration::from_secs(4) {
                    self.toast = None;
                }
            }
        }
        Ok(())
    }

    fn drain_messages(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                TuiMsg::ArtifactsLoaded { repo, artifacts } => {
                    if self.artifacts_repo.as_deref() == Some(repo.as_str()) {
                        self.artifacts = artifacts;
                        self.artifact_state = TableState::default();
                        if !self.artifacts.is_empty() {
                            self.artifact_state.select(Some(0));
                        }
                    }
                }
                TuiMsg::LoadFailed(e) => {
                    self.toast = Some((format!("load failed: {e}"), true, Instant::now()))
                }
                TuiMsg::OpDone(m) => self.toast = Some((m, false, Instant::now())),
                TuiMsg::OpFailed(e) => {
                    self.toast = Some((format!("failed: {e}"), true, Instant::now()))
                }
            }
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) {
        // Global quit first (unless typing in an input field).
        let typing = matches!(self.dialog, Dialog::Pull { .. }) || self.search_active;
        if key.code == KeyCode::Char('q') && !typing
            || key.code == KeyCode::Char('c')
                && key.modifiers.contains(KeyModifiers::CONTROL)
                && !typing
        {
            self.quit = true;
            return;
        }

        // Dialog handling: compute an action while borrowing, apply after the
        // borrow ends (avoids E0506 on `self.dialog` reassignment).
        enum DialogAction {
            None,
            Close,
            StartPull(String, String),
            ConfirmDelete,
        }

        let mut action = DialogAction::None;
        match &mut self.dialog {
            Dialog::Help => {
                if key.code == KeyCode::Esc
                    || key.code == KeyCode::Char('?')
                    || key.code == KeyCode::Char('q')
                {
                    action = DialogAction::Close;
                }
            }
            Dialog::Delete { busy } => {
                if *busy {
                    return;
                }
                match key.code {
                    KeyCode::Char('y') | KeyCode::Enter => action = DialogAction::ConfirmDelete,
                    KeyCode::Char('n') | KeyCode::Char('q') | KeyCode::Esc => {
                        action = DialogAction::Close
                    }
                    _ => {}
                }
            }
            Dialog::Pull {
                path,
                path_cursor,
                passphrase,
                pass_cursor,
                busy,
            } => {
                if *busy {
                    return;
                }
                match key.code {
                    KeyCode::Esc => action = DialogAction::Close,
                    KeyCode::Tab => {
                        std::mem::swap(path, passphrase);
                        std::mem::swap(path_cursor, pass_cursor);
                    }
                    KeyCode::Backspace => {
                        if *path_cursor > 0 && !path.is_empty() {
                            path.pop();
                            *path_cursor = path.chars().count();
                        } else if *path_cursor == 0 && !passphrase.is_empty() {
                            passphrase.pop();
                            *pass_cursor = passphrase.chars().count();
                        }
                    }
                    KeyCode::Enter => {
                        if path.is_empty() {
                            self.toast =
                                Some(("local path is required".into(), true, Instant::now()));
                        } else {
                            action = DialogAction::StartPull(path.clone(), passphrase.clone());
                        }
                    }
                    KeyCode::Char(c) => {
                        if *path_cursor == 0 {
                            path.push(c);
                            *path_cursor = path.chars().count();
                        } else {
                            passphrase.push(c);
                            *pass_cursor = passphrase.chars().count();
                        }
                    }
                    _ => {}
                }
            }
            Dialog::None => {}
        }

        let had_action = !matches!(action, DialogAction::None);
        match action {
            DialogAction::Close => self.dialog = Dialog::None,
            DialogAction::StartPull(path, passphrase) => {
                self.dialog = Dialog::Pull {
                    path: path.clone(),
                    path_cursor: path.chars().count(),
                    passphrase: passphrase.clone(),
                    pass_cursor: passphrase.chars().count(),
                    busy: true,
                };
                self.do_pull(path, passphrase).await;
            }
            DialogAction::ConfirmDelete => {
                self.dialog = Dialog::Delete { busy: true };
                self.do_delete().await;
            }
            DialogAction::None => {}
        }
        if had_action
            || matches!(
                self.dialog,
                Dialog::Help | Dialog::Delete { .. } | Dialog::Pull { .. }
            )
        {
            return;
        }

        // Search mode
        if self.search_active {
            match key.code {
                KeyCode::Esc => {
                    self.search_active = false;
                    self.search.clear();
                }
                KeyCode::Enter => self.search_active = false,
                KeyCode::Backspace => {
                    self.search.pop();
                }
                KeyCode::Char(c) => self.search.push(c),
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Tab | KeyCode::Left | KeyCode::Char('h') => self.switch_focus_back(),
            KeyCode::Right | KeyCode::Char('l') => self.switch_focus_fwd(),
            KeyCode::Up | KeyCode::Char('k') => self.move_sel(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_sel(1),
            KeyCode::Enter => {
                if matches!(self.focus, Focus::Shortcuts) {
                    self.load_artifacts();
                }
            }
            KeyCode::Char('/') => {
                self.search_active = true;
                self.search.clear();
            }
            KeyCode::Char('s') => {
                self.sort_by_size = !self.sort_by_size;
                self.apply_sort();
            }
            KeyCode::Char('r') => {
                if self.artifacts_repo.is_some() {
                    self.load_artifacts();
                }
            }
            KeyCode::Char('p') => {
                if matches!(self.focus, Focus::Artifacts) && !self.artifacts.is_empty() {
                    self.dialog = Dialog::Pull {
                        path: std::env::current_dir()
                            .map(|p| p.display().to_string())
                            .unwrap_or_else(|_| ".".to_string()),
                        path_cursor: 0,
                        passphrase: String::new(),
                        pass_cursor: 0,
                        busy: false,
                    };
                }
            }
            KeyCode::Char('d') => {
                if matches!(self.focus, Focus::Artifacts) && !self.artifacts.is_empty() {
                    self.dialog = Dialog::Delete { busy: false };
                }
            }
            KeyCode::Char('?') => self.dialog = Dialog::Help,
            KeyCode::Esc => {
                if matches!(self.focus, Focus::Artifacts) {
                    self.focus = Focus::Shortcuts;
                }
            }
            _ => {}
        }
    }

    fn switch_focus_fwd(&mut self) {
        self.focus = match self.focus {
            Focus::Shortcuts => Focus::Artifacts,
            Focus::Artifacts => Focus::Shortcuts,
        };
    }

    fn switch_focus_back(&mut self) {
        self.switch_focus_fwd();
    }

    fn move_sel(&mut self, delta: i32) {
        match self.focus {
            Focus::Shortcuts => {
                if self.shortcuts.is_empty() {
                    return;
                }
                let len = self.shortcuts.len();
                let cur = self.shortcut_state.selected().unwrap_or(0) as i32;
                let next = (cur + delta).clamp(0, len as i32 - 1) as usize;
                self.shortcut_state.select(Some(next));
            }
            Focus::Artifacts => {
                if self.artifacts.is_empty() {
                    return;
                }
                let len = self.artifacts.len();
                let cur = self.artifact_state.selected().unwrap_or(0) as i32;
                let next = (cur + delta).clamp(0, len as i32 - 1) as usize;
                self.artifact_state.select(Some(next));
            }
        }
    }

    fn selected_shortcut(&self) -> Option<(String, String)> {
        let idx = self.shortcut_state.selected()?;
        self.shortcuts.get(idx).cloned()
    }

    fn selected_artifact(&self) -> Option<ArtifactInfo> {
        let idx = self.artifact_state.selected()?;
        self.artifacts.get(idx).cloned()
    }

    fn load_artifacts(&mut self) {
        let Some((name, repo)) = self.selected_shortcut() else {
            return;
        };
        self.artifacts_repo = Some(name.clone());
        self.artifacts = Vec::new();
        self.artifact_state = TableState::default();
        self.toast = Some((format!("loading {repo} ..."), false, Instant::now()));

        let host = repo.split('/').next().unwrap_or("").to_string();
        let cfg = self.cfg.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = async {
                let client = OciClient::new(&host, &cfg)?;
                let artifacts = client.list_repo(&repo).await?;
                Ok::<Vec<ArtifactInfo>, anyhow::Error>(artifacts)
            }
            .await;
            match result {
                Ok(artifacts) => {
                    let _ = tx.send(TuiMsg::ArtifactsLoaded {
                        repo: name,
                        artifacts,
                    });
                }
                Err(e) => {
                    let _ = tx.send(TuiMsg::LoadFailed(e.to_string()));
                }
            }
        });
    }

    fn apply_sort(&mut self) {
        if self.sort_by_size {
            self.artifacts.sort_by(|a, b| b.size.cmp(&a.size));
        } else {
            self.artifacts.sort_by(|a, b| a.tag.cmp(&b.tag));
        }
    }

    fn filtered_artifacts(&self) -> Vec<ArtifactInfo> {
        if self.search.is_empty() {
            return self.artifacts.clone();
        }
        let q = self.search.to_lowercase();
        self.artifacts
            .iter()
            .filter(|a| a.tag.to_lowercase().contains(&q))
            .cloned()
            .collect()
    }

    async fn do_pull(&mut self, path: String, passphrase: String) {
        let Some(artifact) = self.selected_artifact() else {
            self.dialog = Dialog::None;
            return;
        };
        let Some((_, repo)) = self.selected_shortcut() else {
            self.dialog = Dialog::None;
            return;
        };
        let host = repo.split('/').next().unwrap_or("").to_string();
        let full_ref = format!("{repo}:{}", artifact.tag);
        let cfg = self.cfg.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = async {
                let client = OciClient::new(&host, &cfg)?;
                let encrypted = client.is_encrypted(&artifact.repo, &artifact.tag).await?;
                if encrypted && passphrase.is_empty() {
                    return Err(anyhow!("content is encrypted, provide a passphrase"));
                }
                let pulled = client.pull(&artifact.repo, &artifact.tag).await?;
                let data = if pulled.encrypted {
                    crate::crypto::decrypt(&pulled.data, &passphrase)?
                } else {
                    pulled.data
                };
                crate::archive::unpack(&data, std::path::Path::new(&path))?;
                let _ = cache::add(Activity {
                    kind: ActivityType::Pull,
                    timestamp: chrono::Local::now(),
                    remote_ref: full_ref,
                    local_path: Some(path),
                    labels: vec![],
                    success: true,
                    error: None,
                });
                Ok::<_, anyhow::Error>(())
            }
            .await;
            match result {
                Ok(()) => {
                    let _ = tx.send(TuiMsg::OpDone("pull successful ✓".into()));
                }
                Err(e) => {
                    let _ = tx.send(TuiMsg::OpFailed(e.to_string()));
                }
            }
        });
        self.dialog = Dialog::None;
    }

    async fn do_delete(&mut self) {
        let Some(artifact) = self.selected_artifact() else {
            self.dialog = Dialog::None;
            return;
        };
        let Some((_, repo)) = self.selected_shortcut() else {
            self.dialog = Dialog::None;
            return;
        };
        let host = repo.split('/').next().unwrap_or("").to_string();
        let full_ref = format!("{repo}:{}", artifact.tag);
        let cfg = self.cfg.clone();
        let tx = self.tx.clone();
        tokio::spawn(async move {
            let result = async {
                let client = OciClient::new(&host, &cfg)?;
                client.delete(&artifact.repo, &artifact.tag).await?;
                let _ = cache::add(Activity {
                    kind: ActivityType::Delete,
                    timestamp: chrono::Local::now(),
                    remote_ref: full_ref,
                    local_path: None,
                    labels: vec![],
                    success: true,
                    error: None,
                });
                Ok::<_, anyhow::Error>(())
            }
            .await;
            match result {
                Ok(()) => {
                    let _ = tx.send(TuiMsg::OpDone("delete successful ✓".into()));
                }
                Err(e) => {
                    let _ = tx.send(TuiMsg::OpFailed(e.to_string()));
                }
            }
        });
        self.dialog = Dialog::None;
        // refresh the list shortly after deletion
        let tx = self.tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1200)).await;
            let _ = tx.send(TuiMsg::OpDone(String::new()));
        });
    }

    // ------------------------------------------------------------ ui

    fn ui(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let main = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(6),
                Constraint::Length(1),
            ])
            .split(area);

        self.draw_panes(frame, main[0]);
        self.draw_details(frame, main[1]);
        self.draw_status_bar(frame, main[2]);

        match &self.dialog {
            Dialog::Help => self.draw_help(frame, area),
            Dialog::Pull {
                path, passphrase, ..
            } => self.draw_pull_dialog(frame, area, path, passphrase),
            Dialog::Delete { .. } => self.draw_delete_dialog(frame, area),
            Dialog::None => {}
        }

        if let Some((msg, is_error, _)) = &self.toast {
            if !msg.is_empty() {
                self.draw_toast(frame, area, msg, *is_error);
            }
        }
    }

    fn draw_panes(&self, frame: &mut Frame, area: Rect) {
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(32), Constraint::Percentage(68)])
            .split(area);

        // Left: shortcuts
        let items: Vec<ListItem> = self
            .shortcuts
            .iter()
            .map(|(name, repo)| {
                ListItem::new(Line::from(vec![
                    Span::styled(name.clone(), Style::new().add_modifier(Modifier::BOLD)),
                    Span::raw("  "),
                    Span::styled(repo.clone(), Style::new().fg(Color::DarkGray)),
                ]))
            })
            .collect();
        let title = if matches!(self.focus, Focus::Shortcuts) {
            Line::from(" SHORTCUTS ")
                .style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        } else {
            Line::from(" SHORTCUTS ")
        };
        let list = List::new(items)
            .block(Block::new().borders(Borders::ALL).title(title))
            .highlight_style(
                Style::new()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            );
        let mut state = self.shortcut_state;
        frame.render_stateful_widget(list, panes[0], &mut state);

        // Right: artifacts
        let repo_title = self
            .artifacts_repo
            .clone()
            .unwrap_or_else(|| "press Enter to load".to_string());
        let rows: Vec<Row> = self
            .filtered_artifacts()
            .iter()
            .map(|a| {
                let lock = if a.encrypted { "🔒 " } else { "" };
                let labels = a
                    .labels
                    .iter()
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect::<Vec<_>>()
                    .join(",");
                Row::new(vec![
                    Cell::from(format!("{lock}{}", a.tag)),
                    Cell::from(if a.encrypted { "yes" } else { "no" }),
                    Cell::from(format_bytes(a.size)),
                    Cell::from(a.version.clone()),
                    Cell::from(labels),
                ])
            })
            .collect();
        let widths = [
            Constraint::Length(22),
            Constraint::Length(9),
            Constraint::Length(10),
            Constraint::Length(8),
            Constraint::Min(10),
        ];
        let header = Row::new(vec!["TAG", "ENCRYPTED", "SIZE", "VERSION", "LABELS"])
            .style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD));
        let title = if matches!(self.focus, Focus::Artifacts) {
            Line::from(format!(" ARTIFACTS ({repo_title}) "))
                .style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        } else {
            Line::from(format!(" ARTIFACTS ({repo_title}) "))
        };
        let table = Table::new(rows, widths)
            .header(header)
            .block(Block::new().borders(Borders::ALL).title(title))
            .row_highlight_style(Style::new().bg(Color::DarkGray))
            .column_spacing(1);
        let mut state = self.artifact_state;
        frame.render_stateful_widget(table, panes[1], &mut state);
    }

    fn draw_details(&self, frame: &mut Frame, area: Rect) {
        let text = match self.selected_artifact() {
            Some(a) => vec![
                Line::from(vec![
                    Span::styled(
                        "Details: ",
                        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(format!("{}:{}", a.repo, a.tag)),
                ]),
                Line::from(format!(
                    "Full name: {}    Digest: {}    Version: {}    Size: {}",
                    a.full_name,
                    a.digest,
                    a.version,
                    format_bytes(a.size)
                )),
                Line::from(format!(
                    "Encrypted: {}    Labels: {}",
                    if a.encrypted {
                        "yes (AES-256-GCM)"
                    } else {
                        "no"
                    },
                    if a.labels.is_empty() {
                        "-".to_string()
                    } else {
                        a.labels
                            .iter()
                            .map(|(k, v)| format!("{k}={v}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                )),
            ],
            None => vec![Line::from(
                "Select a shortcut and press Enter, then pick an artifact.",
            )],
        };
        frame.render_widget(
            Paragraph::new(text).block(Block::new().borders(Borders::ALL).title(" DETAILS ")),
            area,
        );
    }

    fn draw_status_bar(&self, frame: &mut Frame, area: Rect) {
        let hints = " Tab/←→ switch  ↑↓/jk move  Enter load  / search  s sort  p pull  d delete  r refresh  ? help  q quit ";
        let style = Style::new().fg(Color::Black).bg(if self.search_active {
            Color::Yellow
        } else {
            Color::Blue
        });
        frame.render_widget(Paragraph::new(hints).style(style), area);
    }

    fn draw_help(&self, frame: &mut Frame, area: Rect) {
        let popup = centered_rect(70, 80, area);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(HELP_TEXT).block(
                Block::new()
                    .borders(Borders::ALL)
                    .title(" HELP ")
                    .border_style(Style::new().fg(Color::Cyan)),
            ),
            popup,
        );
    }

    fn draw_pull_dialog(&self, frame: &mut Frame, area: Rect, path: &str, passphrase: &str) {
        let popup = centered_rect(60, 40, area);
        frame.render_widget(Clear, popup);
        let inner = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(popup);

        frame.render_widget(
            Paragraph::new(path.to_string()).block(
                Block::new()
                    .borders(Borders::ALL)
                    .title(" Local path ")
                    .border_style(Style::new().fg(Color::Cyan)),
            ),
            inner[0],
        );
        let masked: String = passphrase.chars().map(|_| '*').collect();
        frame.render_widget(
            Paragraph::new(masked).block(
                Block::new()
                    .borders(Borders::ALL)
                    .title(" Passphrase (optional) ")
                    .border_style(Style::new().fg(Color::Cyan)),
            ),
            inner[1],
        );
        frame.render_widget(
            Paragraph::new("Enter: pull   Tab: switch field   Esc: cancel")
                .style(Style::new().fg(Color::DarkGray)),
            inner[2],
        );
    }

    fn draw_delete_dialog(&self, frame: &mut Frame, area: Rect) {
        let popup = centered_rect(55, 35, area);
        frame.render_widget(Clear, popup);
        let text = match self.selected_artifact() {
            Some(a) => format!(
                "Delete {}:{} ({})\n\nThis cannot be undone. Press y to confirm, n to cancel.",
                a.repo, a.tag, a.digest
            ),
            None => "Nothing selected".to_string(),
        };
        frame.render_widget(
            Paragraph::new(text).block(
                Block::new()
                    .borders(Borders::ALL)
                    .title(" DELETE ")
                    .border_style(Style::new().fg(Color::Red)),
            ),
            popup,
        );
    }

    fn draw_toast(&self, frame: &mut Frame, area: Rect, msg: &str, is_error: bool) {
        let popup = centered_rect(60, 15, area);
        frame.render_widget(Clear, popup);
        let color = if is_error { Color::Red } else { Color::Green };
        frame.render_widget(
            Paragraph::new(msg.to_string()).style(Style::new().fg(color)),
            popup,
        );
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
