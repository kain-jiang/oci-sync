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
│ /                    search/filter tags                   │
│ s                    sort: name → size ↓ → size ↑ (cycle) │
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

/// Artifact sort mode; `s` cycles Name → SizeDesc → SizeAsc → Name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum SortMode {
    #[default]
    Name,
    SizeDesc,
    SizeAsc,
}

impl SortMode {
    fn next(self) -> Self {
        match self {
            SortMode::Name => SortMode::SizeDesc,
            SortMode::SizeDesc => SortMode::SizeAsc,
            SortMode::SizeAsc => SortMode::Name,
        }
    }

    fn label(self) -> &'static str {
        match self {
            SortMode::Name => "name",
            SortMode::SizeDesc => "size ↓",
            SortMode::SizeAsc => "size ↑",
        }
    }
}

/// Focus within the pull dialog; Tab cycles Path → Passphrase → Buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PullField {
    #[default]
    Path,
    Passphrase,
    Buttons,
}

enum Dialog {
    None,
    Pull {
        path: String,
        passphrase: String,
        field: PullField,
        /// 0 = Pull, 1 = Cancel (active only when `field == Buttons`)
        button: usize,
        busy: bool,
    },
    Delete {
        /// 0 = Delete, 1 = Cancel (default focus: Cancel — safety first)
        button: usize,
        busy: bool,
    },
    Help {
        scroll: usize,
    },
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
    sort: SortMode,
    loading: bool,
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
            sort: SortMode::Name,
            loading: false,
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
                    self.loading = false;
                    if self.artifacts_repo.as_deref() == Some(repo.as_str()) {
                        self.artifacts = artifacts;
                        self.artifact_state = TableState::default();
                        if !self.artifacts.is_empty() {
                            self.artifact_state.select(Some(0));
                        }
                        self.apply_sort();
                    }
                }
                TuiMsg::LoadFailed(e) => {
                    self.loading = false;
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
            Dialog::Help { scroll } => match key.code {
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
                    action = DialogAction::Close
                }
                KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
                _ => {}
            },
            Dialog::Delete { button, busy } => {
                if *busy {
                    return;
                }
                match key.code {
                    // Buttons: Delete(0) / Cancel(1), Cancel focused by default
                    KeyCode::Char('y') | KeyCode::Enter if *button == 0 => {
                        action = DialogAction::ConfirmDelete
                    }
                    KeyCode::Enter if *button == 1 => action = DialogAction::Close,
                    KeyCode::Char('n') | KeyCode::Char('q') | KeyCode::Esc => {
                        action = DialogAction::Close
                    }
                    KeyCode::Left | KeyCode::Right | KeyCode::Char('h') | KeyCode::Char('l') => {
                        *button = if *button == 0 { 1 } else { 0 };
                    }
                    _ => {}
                }
            }
            Dialog::Pull {
                path,
                passphrase,
                field,
                button,
                busy,
            } => {
                if *busy {
                    return;
                }
                match key.code {
                    KeyCode::Esc => action = DialogAction::Close,
                    KeyCode::Tab => {
                        *field = match field {
                            PullField::Path => PullField::Passphrase,
                            PullField::Passphrase => PullField::Buttons,
                            PullField::Buttons => PullField::Path,
                        };
                    }
                    KeyCode::Backspace => match field {
                        PullField::Path => {
                            path.pop();
                        }
                        PullField::Passphrase => {
                            passphrase.pop();
                        }
                        PullField::Buttons => {}
                    },
                    KeyCode::Left | KeyCode::Char('h') if *field == PullField::Buttons => {
                        *button = if *button == 0 { 1 } else { 0 };
                    }
                    KeyCode::Right | KeyCode::Char('l') if *field == PullField::Buttons => {
                        *button = if *button == 0 { 1 } else { 0 };
                    }
                    KeyCode::Enter => {
                        if *field == PullField::Buttons {
                            if *button == 0 {
                                // "Pull"
                                if path.is_empty() {
                                    self.toast = Some((
                                        "local path is required".into(),
                                        true,
                                        Instant::now(),
                                    ));
                                } else {
                                    action =
                                        DialogAction::StartPull(path.clone(), passphrase.clone());
                                }
                            } else {
                                // "Cancel"
                                action = DialogAction::Close;
                            }
                        }
                    }
                    KeyCode::Char(c) => match field {
                        PullField::Path => path.push(c),
                        PullField::Passphrase => passphrase.push(c),
                        PullField::Buttons => {}
                    },
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
                    passphrase: passphrase.clone(),
                    field: PullField::Path,
                    button: 0,
                    busy: true,
                };
                self.do_pull(path, passphrase).await;
            }
            DialogAction::ConfirmDelete => {
                self.dialog = Dialog::Delete {
                    button: 1,
                    busy: true,
                };
                self.do_delete().await;
            }
            DialogAction::None => {}
        }
        if had_action
            || matches!(
                self.dialog,
                Dialog::Help { .. } | Dialog::Delete { .. } | Dialog::Pull { .. }
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
                self.sort = self.sort.next();
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
                        passphrase: String::new(),
                        field: PullField::Path,
                        button: 0,
                        busy: false,
                    };
                }
            }
            KeyCode::Char('d') => {
                if matches!(self.focus, Focus::Artifacts) && !self.artifacts.is_empty() {
                    // Cancel is the default focus (safety first).
                    self.dialog = Dialog::Delete {
                        button: 1,
                        busy: false,
                    };
                }
            }
            KeyCode::Char('?') => self.dialog = Dialog::Help { scroll: 0 },
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
        self.loading = true;
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
        match self.sort {
            SortMode::Name => self.artifacts.sort_by(|a, b| a.tag.cmp(&b.tag)),
            SortMode::SizeDesc => self.artifacts.sort_by_key(|b| std::cmp::Reverse(b.size)),
            SortMode::SizeAsc => self.artifacts.sort_by_key(|b| b.size),
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
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(6),
                Constraint::Length(1),
            ])
            .split(area);

        self.draw_top_bar(frame, main[0]);
        self.draw_panes(frame, main[1]);
        self.draw_details(frame, main[2]);
        self.draw_status_bar(frame, main[3]);

        match &self.dialog {
            Dialog::Help { scroll } => self.draw_help(frame, area, *scroll),
            Dialog::Pull {
                path,
                passphrase,
                field,
                button,
                ..
            } => self.draw_pull_dialog(frame, area, path, passphrase, *field, *button),
            Dialog::Delete { button, .. } => self.draw_delete_dialog(frame, area, *button),
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
            Line::from(format!(
                " ARTIFACTS ({repo_title}){} ",
                if self.loading {
                    spinner_char().to_string()
                } else {
                    String::new()
                }
            ))
            .style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        } else {
            Line::from(format!(
                " ARTIFACTS ({repo_title}){} ",
                if self.loading {
                    spinner_char().to_string()
                } else {
                    String::new()
                }
            ))
        };
        let table = Table::new(rows, widths)
            .header(header)
            .block(Block::new().borders(Borders::ALL).title(title))
            .row_highlight_style(Style::new().bg(Color::DarkGray))
            .column_spacing(1);
        let mut state = self.artifact_state;
        frame.render_stateful_widget(table, panes[1], &mut state);

        // Loading indicator inside the artifacts pane.
        if self.loading && self.filtered_artifacts().is_empty() {
            let inner = Rect {
                x: panes[1].x + 1,
                y: panes[1].y + 1,
                width: panes[1].width.saturating_sub(2),
                height: panes[1].height.saturating_sub(2),
            };
            frame.render_widget(
                Paragraph::new(format!("{} Loading tags...", spinner_char()))
                    .style(Style::new().fg(Color::DarkGray)),
                inner,
            );
        }
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

    fn draw_top_bar(&self, frame: &mut Frame, area: Rect) {
        let halves = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);
        let style = Style::new().fg(Color::Black).bg(Color::Cyan);
        let left = format!(" oci-sync v{} ", crate::VERSION);
        let host = self
            .selected_shortcut()
            .map(|(_, r)| r.split('/').next().unwrap_or("").to_string())
            .unwrap_or_default();
        let right = format!(" {host}   Q 退出 ");
        frame.render_widget(Paragraph::new(left).style(style), halves[0]);
        frame.render_widget(
            Paragraph::new(right)
                .style(style)
                .alignment(ratatui::layout::Alignment::Right),
            halves[1],
        );
    }

    fn draw_status_bar(&self, frame: &mut Frame, area: Rect) {
        let hints = format!(
            " Tab/←→ switch  ↑↓/jk move  Enter load  / search  s sort({})  p pull  d delete  r refresh  ? help  q quit ",
            self.sort.label()
        );
        let style = Style::new().fg(Color::Black).bg(if self.search_active {
            Color::Yellow
        } else {
            Color::Blue
        });
        frame.render_widget(Paragraph::new(hints).style(style), area);
    }

    fn draw_help(&self, frame: &mut Frame, area: Rect, scroll: usize) {
        // Full-screen overlay (95% of the area) with scrolling.
        let popup = centered_rect(95, 95, area);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(HELP_TEXT)
                .block(
                    Block::new()
                        .borders(Borders::ALL)
                        .title(" HELP — ↑↓/jk scroll, Esc close ")
                        .border_style(Style::new().fg(Color::Cyan)),
                )
                .scroll((scroll as u16, 0)),
            popup,
        );
    }

    fn draw_pull_dialog(
        &self,
        frame: &mut Frame,
        area: Rect,
        path: &str,
        passphrase: &str,
        field: PullField,
        button: usize,
    ) {
        let popup = centered_rect(60, 42, area);
        frame.render_widget(Clear, popup);
        let inner = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(popup);

        let path_border = if field == PullField::Path {
            Style::new().fg(Color::Yellow)
        } else {
            Style::new().fg(Color::Cyan)
        };
        frame.render_widget(
            Paragraph::new(path.to_string()).block(
                Block::new()
                    .borders(Borders::ALL)
                    .title(" Local path ")
                    .border_style(path_border),
            ),
            inner[0],
        );
        let masked: String = passphrase.chars().map(|_| '*').collect();
        let pass_border = if field == PullField::Passphrase {
            Style::new().fg(Color::Yellow)
        } else {
            Style::new().fg(Color::Cyan)
        };
        frame.render_widget(
            Paragraph::new(masked).block(
                Block::new()
                    .borders(Borders::ALL)
                    .title(" Passphrase (optional) ")
                    .border_style(pass_border),
            ),
            inner[1],
        );

        // Buttons: [ Pull ] [ Cancel ], navigated with ←/→ when focused.
        let btn_style = |selected: bool| {
            if field == PullField::Buttons && selected {
                Style::new().fg(Color::Black).bg(Color::Green)
            } else {
                Style::new().fg(Color::DarkGray)
            }
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" [ Pull ] ", btn_style(button == 0)),
                Span::styled(" [ Cancel ] ", btn_style(button == 1)),
            ])),
            inner[2],
        );
        frame.render_widget(
            Paragraph::new("Tab: next field   ←/→: choose button   Enter: confirm   Esc: cancel")
                .style(Style::new().fg(Color::DarkGray)),
            inner[3],
        );
    }

    fn draw_delete_dialog(&self, frame: &mut Frame, area: Rect, button: usize) {
        let popup = centered_rect(58, 38, area);
        frame.render_widget(Clear, popup);
        let inner = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(3)])
            .split(popup);

        let text = match self.selected_artifact() {
            Some(a) => format!(
                "Delete {}:{} ({})\n\nThis cannot be undone.",
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
            inner[0],
        );

        // [Delete] [Cancel] — Cancel focused by default (safety first).
        let btn_style = |selected: bool| {
            if selected {
                Style::new().fg(Color::Black).bg(if button == 0 {
                    Color::Red
                } else {
                    Color::DarkGray
                })
            } else {
                Style::new().fg(Color::DarkGray)
            }
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" [ Delete ] ", btn_style(button == 0)),
                Span::styled(" [ Cancel ] ", btn_style(button == 1)),
            ])),
            inner[1],
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

/// Rotating spinner frame for the TUI (driven by wall-clock time).
fn spinner_char() -> char {
    const FRAMES: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let i = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
        / 80) as usize
        % FRAMES.len();
    FRAMES[i]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_mode_cycles() {
        assert_eq!(SortMode::Name.next(), SortMode::SizeDesc);
        assert_eq!(SortMode::SizeDesc.next(), SortMode::SizeAsc);
        assert_eq!(SortMode::SizeAsc.next(), SortMode::Name);
    }

    #[test]
    fn sort_mode_labels() {
        assert_eq!(SortMode::Name.label(), "name");
        assert_eq!(SortMode::SizeDesc.label(), "size ↓");
        assert_eq!(SortMode::SizeAsc.label(), "size ↑");
    }

    #[test]
    fn spinner_frames_are_ascii_braille() {
        let c = spinner_char();
        assert!(('⠋'..='⠏').contains(&c) || "⠙⠹⠸⠼⠴⠦⠧⠇".contains(c));
    }
}
