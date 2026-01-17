use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap, Tabs},
    Terminal,
};
use std::{fs, path::{PathBuf}, process::Command, io::{self, Write}};
use chrono::Local;

#[derive(PartialEq, Clone, Copy)]
enum Focus { Categories, Subfolders, Files }

#[derive(PartialEq)]
enum InputMode { Normal, NewCat, NewFolder, NewNote, ConfirmDelete }

struct App {
    vault_root: PathBuf,
    categories: Vec<String>,
    subfolders: Vec<String>,
    files: Vec<PathBuf>,
    selected_cat: String,
    selected_sub: Option<String>,
    sub_state: ListState,
    file_state: ListState,
    focus: Focus,
    input_mode: InputMode,
    input_buffer: String,
    should_quit: bool,
    last_sync: String,
}

impl App {
    fn new() -> Result<Self> {
        let mut vault_root = dirs::home_dir().context("Home dir not found")?;
        vault_root.push(".knot_vault");
        if !vault_root.exists() { fs::create_dir_all(&vault_root)?; }
        
        // Initial init if not exists
        if !vault_root.join(".git").exists() {
            let _ = Command::new("git").arg("init").current_dir(&vault_root).status();
        }

        let mut app = Self {
            vault_root,
            categories: Vec::new(),
            subfolders: Vec::new(),
            files: Vec::new(),
            selected_cat: "[Root]".to_string(),
            selected_sub: None,
            sub_state: ListState::default(),
            file_state: ListState::default(),
            focus: Focus::Categories,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            should_quit: false,
            last_sync: "Manual".into(),
        };
        app.hard_refresh()?;
        Ok(app)
    }

    fn hard_refresh(&mut self) -> Result<()> {
        let mut cats = vec!["[Root]".to_string()];
        if let Ok(entries) = fs::read_dir(&self.vault_root) {
            for entry in entries.flatten() {
                if entry.path().is_dir() && !entry.file_name().to_string_lossy().starts_with('.') {
                    cats.push(entry.file_name().to_string_lossy().to_string());
                }
            }
        }
        cats.sort();
        self.categories = cats;

        if !self.categories.contains(&self.selected_cat) {
            self.selected_cat = "[Root]".to_string();
        }

        let cat_path = if self.selected_cat == "[Root]" { self.vault_root.clone() } else { self.vault_root.join(&self.selected_cat) };
        let mut subs = Vec::new();
        if let Ok(entries) = fs::read_dir(&cat_path) {
            for entry in entries.flatten() {
                if entry.path().is_dir() {
                    subs.push(entry.file_name().to_string_lossy().to_string());
                }
            }
        }
        subs.sort();
        self.subfolders = subs;

        if let Some(ref sub_name) = self.selected_sub {
            if let Some(pos) = self.subfolders.iter().position(|s| s == sub_name) {
                self.sub_state.select(Some(pos));
            } else {
                self.selected_sub = None;
                self.sub_state.select(if self.subfolders.is_empty() { None } else { Some(0) });
            }
        }

        let mut file_path = cat_path;
        if let Some(si) = self.sub_state.selected() {
            if si < self.subfolders.len() {
                file_path.push(&self.subfolders[si]);
            }
        }

        let mut files = Vec::new();
        if let Ok(entries) = fs::read_dir(&file_path) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() && !p.file_name().unwrap().to_string_lossy().starts_with('.') {
                    files.push(p);
                }
            }
        }
        files.sort_by_key(|p| std::cmp::Reverse(fs::metadata(p).and_then(|m| m.modified()).unwrap_or(std::time::SystemTime::UNIX_EPOCH)));
        self.files = files;
        
        if self.file_state.selected().map_or(true, |i| i >= self.files.len()) {
            self.file_state.select(if self.files.is_empty() { None } else { Some(0) });
        }
        Ok(())
    }

    fn manual_sync(&mut self) -> Result<()> {
        let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        
        // Temporarily leave TUI to show Git output
        execute!(io::stdout(), LeaveAlternateScreen)?;
        disable_raw_mode()?;

        println!("\n--- STARTING GIT SYNC ---");
        let _ = Command::new("git").arg("add").arg(".").current_dir(&self.vault_root).status();
        let _ = Command::new("git").arg("commit").arg("-m").arg(format!("Manual Sync: {}", now)).current_dir(&self.vault_root).status();
        
        println!("Pushing to remote...");
        let status = Command::new("git").arg("push").current_dir(&self.vault_root).status();
        
        if let Ok(s) = status {
            if s.success() { println!("\n✅ Sync Successful!"); }
            else { println!("\n❌ Sync Failed. Check your network or remote settings."); }
        }

        print!("\nPress [ENTER] to return to KNOT...");
        io::stdout().flush()?;
        let mut temp = String::new();
        io::stdin().read_line(&mut temp)?;

        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        self.last_sync = now;
        Ok(())
    }
}

fn main() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout))?;

    let mut app = App::new()?;
    let colors = [Color::Cyan, Color::Magenta, Color::Green, Color::Yellow, Color::Blue];

    while !app.should_quit {
        terminal.draw(|f| {
            let area = f.size();
            let chunks = Layout::default().direction(Direction::Vertical).constraints([
                Constraint::Length(3), 
                Constraint::Length(3), 
                Constraint::Min(0),    
                Constraint::Length(3), 
            ]).split(area);

            f.render_widget(Paragraph::new(format!(" 🚀 KNOT v2 | Last Sync: {} ", app.last_sync))
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray))), chunks[0]);

            let cat_idx = app.categories.iter().position(|c| c == &app.selected_cat).unwrap_or(0);
            let tabs = Tabs::new(app.categories.iter().enumerate().map(|(i, c)| {
                let color = colors[i % colors.len()];
                if i == cat_idx { Line::from(vec![Span::styled(format!(" {} ", c), Style::default().bg(color).fg(Color::Black).add_modifier(Modifier::BOLD))]) }
                else { Line::from(vec![Span::styled(format!(" {} ", c), Style::default().fg(color))]) }
            }).collect())
            .block(Block::default().borders(Borders::ALL).title(" Categories "))
            .select(cat_idx);
            f.render_widget(tabs, chunks[1]);

            let main_chunks = Layout::default().direction(Direction::Horizontal).constraints([
                Constraint::Percentage(20),
                Constraint::Percentage(30),
                Constraint::Percentage(50),
            ]).split(chunks[2]);

            let sub_list = List::new(app.subfolders.iter().map(|s| ListItem::new(format!("  {} ", s))).collect::<Vec<_>>())
                .block(Block::default().borders(Borders::ALL).title(" Folders ")
                .border_style(if app.focus == Focus::Subfolders { Style::default().fg(Color::Yellow) } else { Style::default() }))
                .highlight_style(Style::default().bg(Color::Rgb(40,40,40)));
            f.render_stateful_widget(sub_list, main_chunks[0], &mut app.sub_state);

            let file_list = List::new(app.files.iter().map(|p| ListItem::new(format!(" 📄 {} ", p.file_name().unwrap().to_string_lossy()))).collect::<Vec<_>>())
                .block(Block::default().borders(Borders::ALL).title(" Notes ")
                .border_style(if app.focus == Focus::Files { Style::default().fg(Color::Yellow) } else { Style::default() }))
                .highlight_style(Style::default().bg(Color::Rgb(40,40,40)));
            f.render_stateful_widget(file_list, main_chunks[1], &mut app.file_state);

            let preview = if let Some(i) = app.file_state.selected() {
                fs::read_to_string(&app.files[i]).unwrap_or_else(|_| "Error reading file".into())
            } else { "---".into() };
            f.render_widget(Paragraph::new(preview).block(Block::default().borders(Borders::ALL).title(" Preview ")).wrap(Wrap{trim:true}), main_chunks[2]);

            let footer = match app.input_mode {
                InputMode::Normal => " [TAB] Focus | [S] Sync to Cloud | [C/F/N] New | [D] Delete | [Enter] Edit ",
                InputMode::ConfirmDelete => " !!! PERMANENT DELETE? [y/n] !!! ",
                _ => " Name: [ENTER] Save | [ESC] Cancel ",
            };
            f.render_widget(Paragraph::new(footer).block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray))), chunks[3]);

            if app.input_mode != InputMode::Normal && app.input_mode != InputMode::ConfirmDelete {
                let box_area = centered_rect(50, 15, area);
                f.render_widget(Clear, box_area);
                f.render_widget(Paragraph::new(app.input_buffer.as_str()).block(Block::default().borders(Borders::ALL).title(" Input ")), box_area);
            }
        })?;

        if let Event::Key(key) = event::read()? {
            if key.kind == KeyEventKind::Press {
                match app.input_mode {
                    InputMode::Normal => match key.code {
                        KeyCode::Char('q') => app.should_quit = true,
                        KeyCode::Char('S') => { app.manual_sync()?; terminal.clear()?; }
                        KeyCode::Tab => app.focus = match app.focus { 
                            Focus::Categories => Focus::Subfolders, 
                            Focus::Subfolders => Focus::Files, 
                            Focus::Files => Focus::Categories 
                        },
                        KeyCode::Char('h') | KeyCode::Left => {
                            let cur_idx = app.categories.iter().position(|c| c == &app.selected_cat).unwrap_or(0);
                            let new_idx = if cur_idx == 0 { app.categories.len() - 1 } else { cur_idx - 1 };
                            app.selected_cat = app.categories[new_idx].clone();
                            app.hard_refresh()?;
                        }
                        KeyCode::Char('l') | KeyCode::Right => {
                            let cur_idx = app.categories.iter().position(|c| c == &app.selected_cat).unwrap_or(0);
                            let new_idx = (cur_idx + 1) % app.categories.len();
                            app.selected_cat = app.categories[new_idx].clone();
                            app.hard_refresh()?;
                        }
                        KeyCode::Char('j') | KeyCode::Down => {
                            match app.focus {
                                Focus::Subfolders if !app.subfolders.is_empty() => {
                                    let i = (app.sub_state.selected().unwrap_or(0) + 1) % app.subfolders.len();
                                    app.sub_state.select(Some(i));
                                    app.selected_sub = Some(app.subfolders[i].clone());
                                }
                                Focus::Files if !app.files.is_empty() => {
                                    let i = (app.file_state.selected().unwrap_or(0) + 1) % app.files.len();
                                    app.file_state.select(Some(i));
                                }
                                _ => {}
                            }
                            app.hard_refresh()?;
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            match app.focus {
                                Focus::Subfolders if !app.subfolders.is_empty() => {
                                    let i = if app.sub_state.selected().unwrap_or(0) == 0 { app.subfolders.len()-1 } else { app.sub_state.selected().unwrap()-1 };
                                    app.sub_state.select(Some(i));
                                    app.selected_sub = Some(app.subfolders[i].clone());
                                }
                                Focus::Files if !app.files.is_empty() => {
                                    let i = if app.file_state.selected().unwrap_or(0) == 0 { app.files.len()-1 } else { app.file_state.selected().unwrap()-1 };
                                    app.file_state.select(Some(i));
                                }
                                _ => {}
                            }
                            app.hard_refresh()?;
                        }
                        KeyCode::Char('C') => { app.input_mode = InputMode::NewCat; app.input_buffer.clear(); }
                        KeyCode::Char('F') => { app.input_mode = InputMode::NewFolder; app.input_buffer.clear(); }
                        KeyCode::Char('N') => { app.input_mode = InputMode::NewNote; app.input_buffer.clear(); }
                        KeyCode::Char('D') => { app.input_mode = InputMode::ConfirmDelete; }
                        KeyCode::Enter if app.focus == Focus::Files => {
                            if let Some(i) = app.file_state.selected() {
                                execute!(io::stdout(), LeaveAlternateScreen)?; disable_raw_mode()?;
                                let _ = Command::new("helix").arg(&app.files[i]).status();
                                enable_raw_mode()?; execute!(io::stdout(), EnterAlternateScreen)?;
                                app.hard_refresh()?;
                                terminal.clear()?;
                            }
                        }
                        _ => {}
                    },
                    InputMode::ConfirmDelete => match key.code {
                        KeyCode::Char('y') => {
                            let path = match app.focus {
                                Focus::Categories if app.selected_cat != "[Root]" => Some(app.vault_root.join(&app.selected_cat)),
                                Focus::Subfolders => app.sub_state.selected().map(|i| app.vault_root.join(&app.selected_cat).join(&app.subfolders[i])),
                                Focus::Files => app.file_state.selected().map(|i| app.files[i].clone()),
                                _ => None,
                            };
                            if let Some(p) = path {
                                if p.is_dir() { let _ = fs::remove_dir_all(p); } else { let _ = fs::remove_file(p); }
                                if app.focus == Focus::Categories { app.selected_cat = "[Root]".to_string(); }
                            }
                            app.input_mode = InputMode::Normal; app.hard_refresh()?;
                            terminal.clear()?;
                        },
                        _ => app.input_mode = InputMode::Normal,
                    },
                    _ => match key.code {
                        KeyCode::Enter => {
                            let buf = app.input_buffer.clone();
                            if !buf.is_empty() {
                                let base = if app.selected_cat == "[Root]" { app.vault_root.clone() } else { app.vault_root.join(&app.selected_cat) };
                                match app.input_mode {
                                    InputMode::NewCat => { let _ = fs::create_dir_all(app.vault_root.join(&buf)); app.selected_cat = buf; }
                                    InputMode::NewFolder => { let _ = fs::create_dir_all(base.join(&buf)); app.selected_sub = Some(buf); }
                                    InputMode::NewNote => {
                                        let mut p = base;
                                        if let Some(ref s) = app.selected_sub { p.push(s); }
                                        let _ = fs::write(p.join(format!("{}.md", buf)), "# New Note");
                                    }
                                    _ => {}
                                }
                            }
                            app.input_mode = InputMode::Normal; app.hard_refresh()?;
                            terminal.clear()?;
                        }
                        KeyCode::Esc => app.input_mode = InputMode::Normal,
                        KeyCode::Char(c) => app.input_buffer.push(c),
                        KeyCode::Backspace => { app.input_buffer.pop(); }
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
