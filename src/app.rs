use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;

use crate::actions::{copy_to_clipboard, delete_item, open_default, open_in_editor, open_in_file_manager, open_terminal_here};
use crate::export::export_text_report;
use crate::model::{FileKind, FileNode, FileTree};
use crate::scanner::{start_scanner, ScannerController, ScannerMsg};
use crate::treemap::Direction;
use crate::view::{ColorMode, TabView};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Command,
    Filter,
}

#[derive(Debug, Clone)]
pub enum Modal {
    Help,
    ActionMenu,
    Inspector,
    ConfirmDelete(PathBuf),
}

pub struct App {
    pub tree: FileTree,
    pub tabs: Vec<TabView>,
    pub active_tab_index: usize,
    pub input_mode: InputMode,
    pub modal: Option<Modal>,
    pub command_input: String,
    pub filter_input: String,
    pub status_message: Option<String>,
    pub status_message_time: Option<Instant>,
    pub is_scanning: bool,
    pub scan_progress: String,
    pub total_scanned_items: usize,
    pub path_to_id: HashMap<PathBuf, usize>,
    pub scanner_rx: Receiver<ScannerMsg>,
    pub scanner_controller: ScannerController,
    pub should_quit: bool,
    pub last_layout_rect: Rect,
}

impl App {
    pub fn new(root_path: PathBuf) -> Self {
        let canonical_root = std::fs::canonicalize(&root_path).unwrap_or(root_path);
        let tree = FileTree::new(canonical_root.clone());
        let mut path_to_id = HashMap::new();
        path_to_id.insert(canonical_root.clone(), 0);

        let (scanner_rx, scanner_controller) = start_scanner(canonical_root);

        let initial_tab = TabView::new(1, "Overview".to_string(), 0);

        Self {
            tree,
            tabs: vec![initial_tab],
            active_tab_index: 0,
            input_mode: InputMode::Normal,
            modal: None,
            command_input: String::new(),
            filter_input: String::new(),
            status_message: None,
            status_message_time: None,
            is_scanning: true,
            scan_progress: "0 items".to_string(),
            total_scanned_items: 0,
            path_to_id,
            scanner_rx,
            scanner_controller,
            should_quit: false,
            last_layout_rect: Rect::default(),
        }
    }

    pub fn current_tab(&self) -> &TabView {
        &self.tabs[self.active_tab_index]
    }

    pub fn current_tab_mut(&mut self) -> &mut TabView {
        &mut self.tabs[self.active_tab_index]
    }

    pub fn set_status(&mut self, msg: &str) {
        self.status_message = Some(msg.to_string());
        self.status_message_time = Some(Instant::now());
    }

    pub fn update(&mut self) {
        // Clear old status message after 4 seconds
        if let Some(t) = self.status_message_time {
            if t.elapsed() > Duration::from_secs(4) {
                self.status_message = None;
                self.status_message_time = None;
            }
        }

        // Process incoming messages from scanner
        let mut tree_changed = false;
        while let Ok(msg) = self.scanner_rx.try_recv() {
            match msg {
                ScannerMsg::Batch(items) => {
                    for item in items {
                        let parent_id = self.path_to_id.get(&item.parent_path).copied().unwrap_or(0);
                        let kind = if item.is_dir { FileKind::Directory } else { FileKind::File };
                        let node = FileNode::new(
                            0, // Will be set by add_child
                            item.path.clone(),
                            item.name,
                            kind,
                            item.size,
                            item.modified,
                        );
                        let new_id = self.tree.add_child(parent_id, node);
                        self.path_to_id.insert(item.path, new_id);
                    }
                    tree_changed = true;
                }
                ScannerMsg::Progress { scanned_items, current_path: _, .. } => {
                    self.total_scanned_items = scanned_items;
                    self.scan_progress = format!("{} items", scanned_items);
                }
                ScannerMsg::Finished { total_items, .. } => {
                    self.is_scanning = false;
                    self.total_scanned_items = total_items;
                    tree_changed = true;
                }
                ScannerMsg::LiveChange { path, .. } => {
                    if let Some(&node_id) = self.path_to_id.get(&path) {
                        if let Ok(metadata) = std::fs::metadata(&path) {
                            self.tree.update_node_size(node_id, metadata.len());
                            self.tree.set_flash(node_id, Duration::from_millis(1500));
                            tree_changed = true;
                        }
                    }
                }
                ScannerMsg::Error(err) => {
                    self.set_status(&format!("Warning: {}", err));
                }
            }
        }

        if tree_changed && self.last_layout_rect.width > 0 {
            self.recompute_layout(self.last_layout_rect);
        }
    }

    pub fn recompute_layout(&mut self, canvas_rect: Rect) {
        self.last_layout_rect = canvas_rect;
        let tab = &mut self.tabs[self.active_tab_index];
        if canvas_rect.is_empty() {
            tab.treemap.tiles.clear();
            return;
        }
        let root_id = tab.root_node_id;
        let min_wh = tab.detail_level;
        let filter_criteria = tab.filter_criteria.clone();

        tab.treemap.compute(
            &self.tree,
            root_id,
            canvas_rect,
            min_wh,
            (min_wh / 2).max(1),
            12,
            &|node| filter_criteria.matches(node),
        );

        // Ensure selected_node_id is valid
        if !tab.treemap.tiles.iter().any(|t| t.node_id == tab.selected_node_id) {
            tab.selected_node_id = tab.treemap.tiles.first().map(|t| t.node_id).unwrap_or(root_id);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        match self.input_mode {
            InputMode::Normal => self.handle_normal_key(key),
            InputMode::Command => self.handle_command_key(key),
            InputMode::Filter => self.handle_filter_key(key),
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) {
        // Modal handling first
        if let Some(modal) = &self.modal {
            match modal {
                Modal::Help | Modal::Inspector => {
                    if key.code == KeyCode::Esc || key.code == KeyCode::Char('?') || key.code == KeyCode::Char('i') {
                        self.modal = None;
                    }
                    return;
                }
                Modal::ActionMenu => {
                    match key.code {
                        KeyCode::Esc => self.modal = None,
                        KeyCode::Char('1') => {
                            self.modal = None;
                            self.action_open_default();
                        }
                        KeyCode::Char('2') => {
                            self.modal = None;
                            self.action_open_file_manager();
                        }
                        KeyCode::Char('3') => {
                            self.modal = None;
                            self.action_open_editor();
                        }
                        KeyCode::Char('4') => {
                            self.modal = None;
                            self.action_open_terminal();
                        }
                        KeyCode::Char('5') => {
                            self.modal = None;
                            self.action_copy_path();
                        }
                        KeyCode::Char('6') => {
                            self.modal = Some(Modal::Inspector);
                        }
                        KeyCode::Char('7') => {
                            self.modal = None;
                            self.action_rescan_current();
                        }
                        KeyCode::Char('8') => {
                            let path = self.get_selected_path();
                            if let Some(p) = path {
                                self.modal = Some(Modal::ConfirmDelete(p));
                            } else {
                                self.modal = None;
                            }
                        }
                        _ => {}
                    }
                    return;
                }
                Modal::ConfirmDelete(path) => {
                    let path = path.clone();
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            match delete_item(&path) {
                                Ok(_) => self.set_status(&format!("Deleted: {}", path.display())),
                                Err(e) => self.set_status(&format!("Error: {}", e)),
                            }
                            self.modal = None;
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                            self.modal = None;
                            self.set_status("Deletion canceled.");
                        }
                        _ => {}
                    }
                    return;
                }
            }
        }

        // Standard Normal mode bindings
        match key.code {
            // Spatial 2D navigation: arrows or vim hjkl
            KeyCode::Left | KeyCode::Char('h') => self.navigate(Direction::Left),
            KeyCode::Right | KeyCode::Char('l') => self.navigate(Direction::Right),
            KeyCode::Up | KeyCode::Char('k') => self.navigate(Direction::Up),
            KeyCode::Down | KeyCode::Char('j') => self.navigate(Direction::Down),

            // Zoom in: Enter (expand folder branch to fill work area)
            KeyCode::Enter => {
                let tab = self.current_tab();
                let sel_id = tab.selected_node_id;
                if let Some(node) = self.tree.get(sel_id) {
                    if node.is_dir() {
                        self.current_tab_mut().zoom_into(sel_id);
                        self.recompute_layout(self.last_layout_rect);
                    } else {
                        self.set_status(&format!("File selected: {}", node.name));
                    }
                }
            }

            // Zoom out: Esc / Backspace / u
            KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('u') => {
                if self.current_tab_mut().zoom_out() {
                    self.recompute_layout(self.last_layout_rect);
                } else {
                    self.set_status("Already at top zoom level.");
                }
            }

            // Home: Jump to view root
            KeyCode::Home => {
                let root_id = self.tree.root_id;
                self.current_tab_mut().root_node_id = root_id;
                self.current_tab_mut().selected_node_id = root_id;
                self.current_tab_mut().zoom_history.clear();
                self.recompute_layout(self.last_layout_rect);
            }

            // Tagging: 1, 2, 3, 4
            KeyCode::Char('1') => self.toggle_tag(1),
            KeyCode::Char('2') => self.toggle_tag(2),
            KeyCode::Char('3') => self.toggle_tag(3),
            KeyCode::Char('4') => self.toggle_tag(4),
            KeyCode::Char('0') | KeyCode::Char('x') => self.clear_tag(),

            // More / Less detail: + / -
            KeyCode::Char('+') | KeyCode::Char('=') => {
                let tab = self.current_tab_mut();
                if tab.detail_level > 2 {
                    tab.detail_level -= 1;
                    self.recompute_layout(self.last_layout_rect);
                }
            }
            KeyCode::Char('-') => {
                let tab = self.current_tab_mut();
                if tab.detail_level < 15 {
                    tab.detail_level += 1;
                    self.recompute_layout(self.last_layout_rect);
                }
            }

            // Color mode toggle: f
            KeyCode::Char('f') => {
                let tab = self.current_tab_mut();
                tab.color_mode = match tab.color_mode {
                    ColorMode::Flat => ColorMode::FileClasses,
                    ColorMode::FileClasses => ColorMode::Depth,
                    ColorMode::Depth => ColorMode::Flat,
                };
                let message = match tab.color_mode {
                    ColorMode::Flat => "Colors · Classic",
                    ColorMode::FileClasses => "Colors · File types",
                    ColorMode::Depth => "Colors · Depth",
                };
                self.set_status(message);
            }

            // Modals & Actions
            KeyCode::Char('?') => self.modal = Some(Modal::Help),
            KeyCode::Char(' ') | KeyCode::Char('a') => self.modal = Some(Modal::ActionMenu),
            KeyCode::Char('i') => self.modal = Some(Modal::Inspector),
            KeyCode::Char('o') => self.action_open_default(),
            KeyCode::Char('e') | KeyCode::Char('v') => self.action_open_editor(),
            KeyCode::Char('t') => self.action_open_terminal(),
            KeyCode::Char('y') => self.action_copy_path(),
            KeyCode::Char('d') => {
                if let Some(p) = self.get_selected_path() {
                    self.modal = Some(Modal::ConfirmDelete(p));
                }
            }
            KeyCode::Char('r') => self.action_rescan_current(),

            // Tabs: gt / gT
            KeyCode::Char('g') => {
                // If followed by t or T
                // In simplified single-key handler:
            }
            KeyCode::Tab => {
                self.next_tab();
            }
            KeyCode::BackTab => {
                self.prev_tab();
            }

            // Neovim search / filter prompt: /
            KeyCode::Char('/') => {
                self.input_mode = InputMode::Filter;
                self.filter_input.clear();
            }

            // Neovim command prompt: :
            KeyCode::Char(':') => {
                self.input_mode = InputMode::Command;
                self.command_input.clear();
            }

            KeyCode::Char('q') => {
                self.should_quit = true;
            }

            _ => {}
        }
    }

    fn handle_command_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.command_input.clear();
            }
            KeyCode::Enter => {
                let cmd = std::mem::take(&mut self.command_input);
                self.input_mode = InputMode::Normal;
                self.execute_command(&cmd);
            }
            KeyCode::Backspace => {
                self.command_input.pop();
                if self.command_input.is_empty() {
                    self.input_mode = InputMode::Normal;
                }
            }
            KeyCode::Char(c) => {
                self.command_input.push(c);
            }
            _ => {}
        }
    }

    fn handle_filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
            }
            KeyCode::Enter => {
                let filter_str = self.filter_input.clone();
                self.input_mode = InputMode::Normal;
                self.current_tab_mut().set_filter(&filter_str);
                self.recompute_layout(self.last_layout_rect);
                self.set_status(&format!("Filter applied: {}", filter_str));
            }
            KeyCode::Backspace => {
                self.filter_input.pop();
            }
            KeyCode::Char(c) => {
                self.filter_input.push(c);
            }
            _ => {}
        }
    }

    fn execute_command(&mut self, cmd: &str) {
        let parts: Vec<&str> = cmd.trim().split_whitespace().collect();
        if parts.is_empty() {
            return;
        }

        match parts[0] {
            "q" | "quit" => self.should_quit = true,
            "w" | "export" => {
                let filename = parts.get(1).copied().unwrap_or("spacemap_report.txt");
                let tab = self.current_tab();
                match export_text_report(filename, &self.tree, tab.root_node_id, &tab.filter_query) {
                    Ok(_) => self.set_status(&format!("Exported report to '{}' successfully!", filename)),
                    Err(e) => self.set_status(&format!("Export failed: {}", e)),
                }
            }
            "tabnew" => {
                let new_id = self.tabs.len() + 1;
                let cur_root = self.current_tab().root_node_id;
                let new_tab = TabView::new(new_id, format!("View {}", new_id), cur_root);
                self.tabs.push(new_tab);
                self.active_tab_index = self.tabs.len() - 1;
                self.recompute_layout(self.last_layout_rect);
                self.set_status("Created new view tab.");
            }
            "tabclose" => {
                if self.tabs.len() > 1 {
                    self.tabs.remove(self.active_tab_index);
                    if self.active_tab_index >= self.tabs.len() {
                        self.active_tab_index = self.tabs.len() - 1;
                    }
                    self.recompute_layout(self.last_layout_rect);
                    self.set_status("Closed tab.");
                } else {
                    self.set_status("Cannot close only remaining tab.");
                }
            }
            "filter" => {
                let query = parts[1..].join(" ");
                self.current_tab_mut().set_filter(&query);
                self.recompute_layout(self.last_layout_rect);
                self.set_status(&format!("Filter set: {}", query));
            }
            "rescan" => {
                self.action_rescan_current();
            }
            "help" => {
                self.modal = Some(Modal::Help);
            }
            _ => {
                self.set_status(&format!("Unknown command: :{}", cmd));
            }
        }
    }

    fn navigate(&mut self, dir: Direction) {
        let tab = self.current_tab();
        let cur_sel = tab.selected_node_id;
        if let Some(next_id) = tab.treemap.find_neighbor(cur_sel, dir) {
            self.current_tab_mut().selected_node_id = next_id;
        }
    }

    fn toggle_tag(&mut self, tag_num: u8) {
        let tab = self.current_tab();
        let sel_id = tab.selected_node_id;
        if let Some(node) = self.tree.get_mut(sel_id) {
            if node.tag == Some(tag_num) {
                node.tag = None;
                self.set_status("Tag cleared.");
            } else {
                node.tag = Some(tag_num);
                let (name, _) = crate::ui::get_tag_color(tag_num);
                self.set_status(&format!("Tagged as {}", name));
            }
        }
    }

    fn clear_tag(&mut self) {
        let tab = self.current_tab();
        let sel_id = tab.selected_node_id;
        if let Some(node) = self.tree.get_mut(sel_id) {
            node.tag = None;
            self.set_status("Tag cleared.");
        }
    }

    pub fn next_tab(&mut self) {
        if self.tabs.len() > 1 {
            self.active_tab_index = (self.active_tab_index + 1) % self.tabs.len();
            self.recompute_layout(self.last_layout_rect);
        }
    }

    pub fn prev_tab(&mut self) {
        if self.tabs.len() > 1 {
            if self.active_tab_index == 0 {
                self.active_tab_index = self.tabs.len() - 1;
            } else {
                self.active_tab_index -= 1;
            }
            self.recompute_layout(self.last_layout_rect);
        }
    }

    fn get_selected_path(&self) -> Option<PathBuf> {
        let tab = self.current_tab();
        self.tree.get(tab.selected_node_id).map(|n| n.path.clone())
    }

    fn action_open_default(&mut self) {
        if let Some(path) = self.get_selected_path() {
            match open_default(&path) {
                Ok(_) => self.set_status(&format!("Opened: {}", path.display())),
                Err(e) => self.set_status(&format!("Error: {}", e)),
            }
        }
    }

    fn action_open_file_manager(&mut self) {
        if let Some(path) = self.get_selected_path() {
            match open_in_file_manager(&path) {
                Ok(_) => self.set_status(&format!("Opened file manager for: {}", path.display())),
                Err(e) => self.set_status(&format!("Error: {}", e)),
            }
        }
    }

    fn action_open_editor(&mut self) {
        if let Some(path) = self.get_selected_path() {
            match open_in_editor(&path) {
                Ok(_) => self.set_status(&format!("Opening in editor: {}", path.display())),
                Err(e) => self.set_status(&format!("Error: {}", e)),
            }
        }
    }

    fn action_open_terminal(&mut self) {
        if let Some(path) = self.get_selected_path() {
            match open_terminal_here(&path) {
                Ok(_) => self.set_status(&format!("Opened terminal in: {}", path.display())),
                Err(e) => self.set_status(&format!("Error: {}", e)),
            }
        }
    }

    fn action_copy_path(&mut self) {
        if let Some(path) = self.get_selected_path() {
            match copy_to_clipboard(&path) {
                Ok(_) => self.set_status(&format!("Copied to clipboard: {}", path.display())),
                Err(e) => self.set_status(&format!("Error: {}", e)),
            }
        }
    }

    fn action_rescan_current(&mut self) {
        let tab = self.current_tab();
        let target_node = self.tree.get(tab.selected_node_id).or_else(|| self.tree.get(tab.root_node_id));
        if let Some(node) = target_node {
            let path = node.path.clone();
            self.scanner_controller.rescan_subtree(path.clone());
            self.is_scanning = true;
            self.set_status(&format!("Rescanning subtree: {}", path.display()));
        }
    }
}
