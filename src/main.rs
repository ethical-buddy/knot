use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Terminal,
};
use std::{fs, path::PathBuf, process::Command, io, time::SystemTime};

#[derive(PartialEq)]
enum InputMode { Normal, NewNote, NewCategory, ConfirmDelete, Searching }

#[derive(PartialEq)]
enum Focus { Categories, Notes }

struct App {
    vault_path: PathBuf,
    categories: Vec<String>,
    notes: Vec<PathBuf>,
    cat_state: ListState,
    note_state: ListState,
    focus: Focus,
    input_mode: InputMode,
    input_buffer: String,
    search_query: String,
    should_quit: bool,
    zen_mode: bool,
}

impl App {
    fn new() -> Result<Self> {
        let mut vault_path = dirs::home_dir().context("Home dir not found")?;
        vault_path.push(".knot_vault");
        if !vault_path.exists() { fs::create_dir_all(&vault_path)?; }

        let mut app = Self {
            vault_path,
            categories: Vec::new(),
            notes: Vec::new(),
            cat_state: ListState::default(),
            note_state: ListState::default(),
            focus: Focus::Categories,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            search_query: String::new(),
            should_quit: false,
            zen_mode: false,
        };
        app.refresh_categories()?;
        Ok(app)
    }

    fn refresh_categories(&mut self) -> Result<()> {
        let mut cats = vec!["[Root]".to_string()];
        if let Ok(entries) = fs::read_dir(&self.vault_path) {
            let mut entries: Vec<_> = entries.flatten().collect();
            entries.sort_by_key(|e| e.file_name()); // Consistent order
            for entry in entries {
                if entry.path().is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if self.search_query.is_empty() || name.to_lowercase().contains(&self.search_query.to_lowercase()) {
                        cats.push(name);
                    }
                }
            }
        }
        self.categories = cats;
        
        // Ensure selection is valid
        if self.cat_state.selected().is_none() && !self.categories.is_empty() {
            self.cat_state.select(Some(0));
        }
        
        self.refresh_notes()
    }

    fn refresh_notes(&mut self) -> Result<()> {
        let cat_idx = self.cat_state.selected().unwrap_or(0);
        let path = if cat_idx == 0 || self.categories.len() <= cat_idx { 
            self.vault_path.clone() 
        } else { 
            self.vault_path.join(&self.categories[cat_idx]) 
        };

        let mut notes = Vec::new();
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let filename = entry.file_name().to_string_lossy().to_string();
                if entry.path().is_file() && filename.ends_with(".md") {
                    if self.search_query.is_empty() || filename.to_lowercase().contains(&self.search_query.to_lowercase()) {
                        notes.push(entry.path());
                    }
                }
            }
        }
        notes.sort_by_key(|p| std::cmp::Reverse(fs::metadata(p).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH)));
        self.notes = notes;

        // Auto-correct selection if list shrinks
        if let Some(selected) = self.note_state.selected() {
            if selected >= self.notes.len() {
                self.note_state.select(if self.notes.is_empty() { None } else { Some(0) });
            }
        } else if !self.notes.is_empty() {
            self.note_state.select(Some(0));
        }
        Ok(())
    }

    fn get_stats(&self) -> (usize, usize) {
        if let Some(idx) = self.note_state.selected() {
            if idx < self.notes.len() {
                if let Ok(content) = fs::read_to_string(&self.notes[idx]) {
                    let words = content.split_whitespace().count();
                    return (words, (words as f32 / 200.0).ceil() as usize);
                }
            }
        }
        (0, 0)
    }
}

fn parse_md(content: &str) -> Text<'_> {
    let mut lines = Vec::new();
    for line in content.lines() {
        if line.starts_with("# ") {
            lines.push(Line::from(vec![Span::styled(format!(" # {} ", &line[2..]), Style::default().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD))]));
        } else if line.starts_with("## ") {
            lines.push(Line::from(vec![Span::styled(line, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))]));
        } else if line.starts_with("### ") {
            lines.push(Line::from(vec![Span::styled(line, Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))]));
        } else if line.starts_with("#### ") {
            lines.push(Line::from(vec![Span::styled(line, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))]));
        } else {
            lines.push(Line::from(line));
        }
    }
    Text::from(lines)
}

fn get_relative_time(path: &PathBuf) -> String {
    if let Ok(m) = fs::metadata(path) {
        if let Ok(t) = m.modified() {
            if let Ok(e) = t.elapsed() {
                let s = e.as_secs();
                if s < 60 { return "now".into(); }
                if s < 3600 { return format!("{}m", s/60); }
                if s < 86400 { return format!("{}h", s/3600); }
                return format!("{}d", s/86400);
            }
        }
    }
    "---".into()
}

fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut app = App::new()?;
    terminal.clear()?;

    let palette = [Color::Blue, Color::Green, Color::Yellow, Color::Magenta, Color::Cyan, Color::Red];

    while !app.should_quit {
        terminal.draw(|f| {
            let area = f.size();
            let chunks = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(3)]).split(area);
            
            let header = if app.input_mode == InputMode::Searching { format!(" 🔍 FILTER: {}_ ", app.search_query) } else { " 📔 KNOT ".into() };
            f.render_widget(Paragraph::new(header).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan))), chunks[0]);

            let body_constraints = if app.zen_mode { [Constraint::Max(0), Constraint::Max(0), Constraint::Percentage(100)] } 
                                   else { [Constraint::Percentage(22), Constraint::Percentage(33), Constraint::Percentage(45)] };
            let body = Layout::default().direction(Direction::Horizontal).constraints(body_constraints).split(chunks[1]);

            if !app.zen_mode {
                // Category List with Cycling Colors
                let cats: Vec<ListItem> = app.categories.iter().enumerate().map(|(i, c)| {
                    let color = palette[i % palette.len()];
                    ListItem::new(format!("  📂 {}", c)).style(Style::default().fg(color))
                }).collect();
                f.render_stateful_widget(List::new(cats).block(Block::default().borders(Borders::ALL).title(" Folders ").border_style(if app.focus == Focus::Categories { Style::default().fg(Color::Yellow) } else { Style::default() })).highlight_style(Style::default().bg(Color::Rgb(50,50,50))), body[0], &mut app.cat_state);

                // Note List
                let notes: Vec<ListItem> = app.notes.iter().map(|n| {
                    let name = n.file_name().unwrap_or_default().to_string_lossy();
                    ListItem::new(Line::from(vec![Span::raw(format!("  {} ", name)), Span::styled(get_relative_time(n), Style::default().fg(Color::DarkGray))]))
                }).collect();
                f.render_stateful_widget(List::new(notes).block(Block::default().borders(Borders::ALL).title(" Notes ").border_style(if app.focus == Focus::Notes { Style::default().fg(Color::Yellow) } else { Style::default() })).highlight_style(Style::default().bg(Color::Rgb(50,50,50))), body[1], &mut app.note_state);
            }

            // Preview
            let (words, mins) = app.get_stats();
            let content = app.note_state.selected().and_then(|i| app.notes.get(i)).and_then(|p| fs::read_to_string(p).ok()).unwrap_or_default();
            f.render_widget(Paragraph::new(parse_md(&content)).block(Block::default().borders(Borders::ALL).title(format!(" 📝 Preview [{} words | {}m] ", words, mins))).wrap(Wrap { trim: true }), body[2]);

            // Footer
            let footer = match app.input_mode {
                InputMode::Normal => " [/] Search | [z] Zen | [Tab] Focus | [c] New Cat | [n] New Note | [d] Delete | [Enter] Helix ",
                InputMode::ConfirmDelete => " ! DELETE? [y] Yes | [n] No ",
                _ => " Typing... [Enter] Save | [Esc] Cancel ",
            };
            f.render_widget(Paragraph::new(footer).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray))), chunks[2]);

            // Modals
            if matches!(app.input_mode, InputMode::NewNote | InputMode::NewCategory) {
                let m_area = centered_rect(40, 15, area);
                f.render_widget(Clear, m_area);
                f.render_widget(Paragraph::new(app.input_buffer.as_str()).block(Block::default().borders(Borders::ALL).title(" Name Item ")), m_area);
            }
        })?;

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match app.input_mode {
                    InputMode::Normal => match key.code {
                        KeyCode::Char('q') => app.should_quit = true,
                        KeyCode::Char('z') => app.zen_mode = !app.zen_mode,
                        KeyCode::Char('/') => { app.input_mode = InputMode::Searching; app.search_query.clear(); }
                        KeyCode::Tab => app.focus = if app.focus == Focus::Categories { Focus::Notes } else { Focus::Categories },
                        KeyCode::Char('c') => { app.input_mode = InputMode::NewCategory; app.input_buffer.clear(); }
                        KeyCode::Char('n') => { app.input_mode = InputMode::NewNote; app.input_buffer.clear(); }
                        KeyCode::Char('d') => app.input_mode = InputMode::ConfirmDelete,
                        KeyCode::Enter | KeyCode::Char('e') => {
                            if let Some(i) = app.note_state.selected() {
                                if i < app.notes.len() {
                                    disable_raw_mode()?; execute!(io::stdout(), LeaveAlternateScreen)?;
                                    let _ = Command::new("helix").arg(&app.notes[i]).status();
                                    enable_raw_mode()?; execute!(io::stdout(), EnterAlternateScreen)?;
                                    terminal.clear()?; app.refresh_notes()?;
                                }
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.focus == Focus::Categories && !app.categories.is_empty() {
                                let next = (app.cat_state.selected().unwrap_or(0) + 1) % app.categories.len();
                                app.cat_state.select(Some(next)); app.refresh_notes()?;
                            } else if !app.notes.is_empty() {
                                let next = (app.note_state.selected().unwrap_or(0) + 1) % app.notes.len();
                                app.note_state.select(Some(next));
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.focus == Focus::Categories && !app.categories.is_empty() {
                                let i = app.cat_state.selected().unwrap_or(0);
                                let ni = if i == 0 { app.categories.len() - 1 } else { i - 1 };
                                app.cat_state.select(Some(ni)); app.refresh_notes()?;
                            } else if !app.notes.is_empty() {
                                let i = app.note_state.selected().unwrap_or(0);
                                let ni = if i == 0 { app.notes.len() - 1 } else { i - 1 };
                                app.note_state.select(Some(ni));
                            }
                        }
                        _ => {}
                    },
                    InputMode::Searching => match key.code {
                        KeyCode::Enter | KeyCode::Esc => { app.input_mode = InputMode::Normal; }
                        KeyCode::Backspace => { app.search_query.pop(); app.refresh_categories()?; }
                        KeyCode::Char(c) => { app.search_query.push(c); app.refresh_categories()?; }
                        _ => {}
                    },
                    InputMode::ConfirmDelete => match key.code {
                        KeyCode::Char('y') => {
                            if app.focus == Focus::Categories {
                                let i = app.cat_state.selected().unwrap_or(0);
                                if i != 0 { let _ = fs::remove_dir_all(app.vault_path.join(&app.categories[i])); }
                            } else if let Some(i) = app.note_state.selected() {
                                if i < app.notes.len() { let _ = fs::remove_file(&app.notes[i]); }
                            }
                            app.refresh_categories()?; app.input_mode = InputMode::Normal;
                        }
                        _ => app.input_mode = InputMode::Normal,
                    },
                    _ => match key.code {
                        KeyCode::Enter => {
                            if !app.input_buffer.is_empty() {
                                if app.input_mode == InputMode::NewCategory {
                                    let _ = fs::create_dir_all(app.vault_path.join(&app.input_buffer));
                                } else {
                                    let cat_idx = app.cat_state.selected().unwrap_or(0);
                                    let path = if cat_idx == 0 { app.vault_path.clone() } else { app.vault_path.join(&app.categories[cat_idx]) };
                                    let _ = fs::write(path.join(format!("{}.md", app.input_buffer)), "# New Note\n");
                                }
                            }
                            app.input_buffer.clear(); app.input_mode = InputMode::Normal; app.refresh_categories()?;
                        }
                        KeyCode::Esc => app.input_mode = InputMode::Normal,
                        KeyCode::Backspace => { app.input_buffer.pop(); }
                        KeyCode::Char(c) => { app.input_buffer.push(c); }
                        _ => {}
                    }
                }
            }
        }
    }
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    disable_raw_mode()?;
    Ok(())
}

fn centered_rect(px: u16, py: u16, r: Rect) -> Rect {
    let v = Layout::default().direction(Direction::Vertical).constraints([Constraint::Percentage((100-py)/2), Constraint::Percentage(py), Constraint::Percentage((100-py)/2)]).split(r);
    Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage((100-px)/2), Constraint::Percentage(px), Constraint::Percentage((100-px)/2)]).split(v[1])[1]
}
