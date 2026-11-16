use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::Rect;

use crate::actions::{
    copy_to_clipboard, delete_item, open_default, open_in_file_manager, open_terminal_here,
};
use crate::export::export_text_report;
use crate::model::{FileNode, FileTree};
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
    pub scan_error_count: usize,
    pub last_scan_error: Option<String>,
    pub path_to_id: HashMap<PathBuf, usize>,
    pub scanner_rx: Receiver<ScannerMsg>,
    pub scanner_controller: ScannerController,
    pub should_quit: bool,
    pub pending_editor_path: Option<PathBuf>,
    pub pending_g_key: bool,
    pub last_layout_rect: Rect,
    pub last_layout_update: Instant,
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
            scan_error_count: 0,
            last_scan_error: None,
            path_to_id,
            scanner_rx,
            scanner_controller,
            should_quit: false,
            pending_editor_path: None,
            pending_g_key: false,
            last_layout_rect: Rect::default(),
            last_layout_update: Instant::now()
                .checked_sub(Duration::from_secs(1))
                .unwrap_or_else(Instant::now),
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
        if let Some(t) = self.status_message_time {
            if t.elapsed() > Duration::from_secs(4) {
                self.status_message = None;
                self.status_message_time = None;
            }
        }

        let mut tree_changed = false;
        let mut scan_just_finished = false;
        while let Ok(msg) = self.scanner_rx.try_recv() {
            match msg {
                ScannerMsg::Batch(items) => {
                    for item in items {
                        if let Some(&existing_id) = self.path_to_id.get(&item.path) {
                            if self.tree.get(existing_id).is_some() {
                                self.tree.update_node_size(existing_id, item.size);
                                continue;
                            }
                        }
                        let Some(&parent_id) = self.path_to_id.get(&item.parent_path) else {
                            continue;
                        };
                        if self.tree.get(parent_id).is_none() {
                            continue;
                        }
                        let node = FileNode::new(
                            0,
                            item.path.clone(),
                            item.name,
                            item.kind,
                            item.size,
                            item.modified,
                        );
                        let new_id = self.tree.add_child(parent_id, node);
                        self.path_to_id.insert(item.path, new_id);
                    }
                    tree_changed = true;
                }
                ScannerMsg::Progress {
                    scanned_items,
                    current_path: _,
                    ..
                } => {
                    self.total_scanned_items = scanned_items;
                    self.scan_progress = format!("{} items", scanned_items);
                }
                ScannerMsg::Finished { .. } => {
                    self.is_scanning = false;
                    let root = &self.tree.nodes[self.tree.root_id];
                    self.total_scanned_items =
                        root.file_count + root.dir_count.saturating_sub(1);
                    tree_changed = true;
                    scan_just_finished = true;
                }
                ScannerMsg::LiveChange { path, .. } => {
                    if let Some(&node_id) = self.path_to_id.get(&path) {
                        if let Ok(metadata) = std::fs::symlink_metadata(&path) {
                            if metadata.file_type().is_file() {
                                self.tree.update_node_size(node_id, metadata.len());
                                self.tree.set_flash(node_id, Duration::from_millis(1500));
                                tree_changed = true;
                            }
                        } else {
                            let removed = self.tree.remove_node(node_id);
                            for p in removed {
                                self.path_to_id.remove(&p);
                            }
                            tree_changed = true;
                        }
                    }
                }
                ScannerMsg::Error(err) => {
                    self.scan_error_count += 1;
                    self.last_scan_error = Some(err.clone());
                    self.set_status(&format!(
                        "Skipped ({} total): {}",
                        self.scan_error_count, err
                    ));
                }
            }
        }

        if tree_changed
            && self.last_layout_rect.width > 0
            && (scan_just_finished
                || !self.is_scanning
                || self.last_layout_update.elapsed() >= Duration::from_millis(150))
        {
            self.recompute_layout(self.last_layout_rect);
            self.last_layout_update = Instant::now();
        }
    }

    pub fn recompute_layout(&mut self, canvas_rect: Rect) {
        self.last_layout_rect = canvas_rect;
        let tab = &mut self.tabs[self.active_tab_index];
        if canvas_rect.is_empty() {
            tab.treemap.tiles.clear();
            return;
        }
        if self.tree.get(tab.root_node_id).is_none() {
            tab.root_node_id = self.tree.root_id;
            tab.zoom_history.retain(|&id| self.tree.get(id).is_some());
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

        if !tab
            .treemap
            .tiles
            .iter()
            .any(|t| t.node_id == tab.selected_node_id)
        {
            tab.selected_node_id = tab
                .treemap
                .tiles
                .get(1)
                .or_else(|| tab.treemap.tiles.first())
                .map(|t| t.node_id)
                .unwrap_or(root_id);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind == KeyEventKind::Release {
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        match self.input_mode {
            InputMode::Normal => self.handle_normal_key(key),
            InputMode::Command => self.handle_command_key(key),
            InputMode::Filter => self.handle_filter_key(key),
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) {
        if let Some(modal) = &self.modal {
            self.pending_g_key = false;
            match modal {
                Modal::Help | Modal::Inspector => {
                    if key.code == KeyCode::Esc
                        || key.code == KeyCode::Char('?')
                        || key.code == KeyCode::Char('i')
                        || key.code == KeyCode::Char('q')
                    {
                        self.modal = None;
                    }
                    return;
                }
                Modal::ActionMenu => {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => self.modal = None,
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
                                if p == self.tree.nodes[self.tree.root_id].path {
                                    self.modal = None;
                                    self.set_status("Cannot delete scan root directory.");
                                } else {
                                    self.modal = Some(Modal::ConfirmDelete(p));
                                }
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
                                Ok(_) => {
                                    if let Some(&node_id) = self.path_to_id.get(&path) {
                                        let removed = self.tree.remove_node(node_id);
                                        for p in removed {
                                            self.path_to_id.remove(&p);
                                        }
                                        let root = &self.tree.nodes[self.tree.root_id];
                                        self.total_scanned_items = root.file_count
                                            + root.dir_count.saturating_sub(1);
                                        self.recompute_layout(self.last_layout_rect);
                                    }
                                    self.set_status(&format!("Deleted: {}", path.display()));
                                }
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

        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            self.pending_g_key = false;
            return;
        }

        if self.pending_g_key {
            self.pending_g_key = false;
            match key.code {
                KeyCode::Char('t') => {
                    self.next_tab();
                    return;
                }
                KeyCode::Char('T') => {
                    self.prev_tab();
                    return;
                }
                KeyCode::Char('g') => {
                    let root_id = self.tree.root_id;
                    self.current_tab_mut().root_node_id = root_id;
                    self.current_tab_mut().selected_node_id = root_id;
                    self.current_tab_mut().zoom_history.clear();
                    self.recompute_layout(self.last_layout_rect);
                    return;
                }
                _ => {}
            }
        }

        match key.code {
            KeyCode::Left | KeyCode::Char('h') => self.navigate(Direction::Left),
            KeyCode::Right | KeyCode::Char('l') => self.navigate(Direction::Right),
            KeyCode::Up | KeyCode::Char('k') => self.navigate(Direction::Up),
            KeyCode::Down | KeyCode::Char('j') => self.navigate(Direction::Down),

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

            KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('u') => {
                if self.current_tab_mut().zoom_out() {
                    self.recompute_layout(self.last_layout_rect);
                } else {
                    self.set_status("Already at top zoom level.");
                }
            }

            KeyCode::Home => {
                let root_id = self.tree.root_id;
                self.current_tab_mut().root_node_id = root_id;
                self.current_tab_mut().selected_node_id = root_id;
                self.current_tab_mut().zoom_history.clear();
                self.recompute_layout(self.last_layout_rect);
            }

            KeyCode::Char('1') => self.toggle_tag(1),
            KeyCode::Char('2') => self.toggle_tag(2),
            KeyCode::Char('3') => self.toggle_tag(3),
            KeyCode::Char('4') => self.toggle_tag(4),
            KeyCode::Char('0') | KeyCode::Char('x') => self.clear_tag(),

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

            KeyCode::Char('?') => self.modal = Some(Modal::Help),
            KeyCode::Char(' ') | KeyCode::Char('a') => self.modal = Some(Modal::ActionMenu),
            KeyCode::Char('i') => self.modal = Some(Modal::Inspector),
            KeyCode::Char('o') => self.action_open_default(),
            KeyCode::Char('e') | KeyCode::Char('v') => self.action_open_editor(),
            KeyCode::Char('t') => self.action_open_terminal(),
            KeyCode::Char('y') => self.action_copy_path(),
            KeyCode::Char('d') => {
                if let Some(p) = self.get_selected_path() {
                    if p == self.tree.nodes[self.tree.root_id].path {
                        self.set_status("Cannot delete scan root directory.");
                    } else {
                        self.modal = Some(Modal::ConfirmDelete(p));
                    }
                }
            }
            KeyCode::Char('r') => self.action_rescan_current(),

            KeyCode::Char('g') => {
                self.pending_g_key = true;
            }
            KeyCode::Tab => {
                self.next_tab();
            }
            KeyCode::BackTab => {
                self.prev_tab();
            }

            KeyCode::Char('/') => {
                self.input_mode = InputMode::Filter;
                self.filter_input = self.current_tab().filter_query.clone();
            }

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
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('u') {
            self.command_input.clear();
            return;
        }
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
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) =>
            {
                self.command_input.push(c);
            }
            _ => {}
        }
    }

    fn handle_filter_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('u') {
            self.filter_input.clear();
            return;
        }
        match key.code {
            KeyCode::Esc => {
                self.input_mode = InputMode::Normal;
                self.filter_input = self.current_tab().filter_query.clone();
            }
            KeyCode::Enter => {
                let filter_str = self.filter_input.trim().to_string();
                self.input_mode = InputMode::Normal;
                self.current_tab_mut().set_filter(&filter_str);
                self.recompute_layout(self.last_layout_rect);
                if filter_str.is_empty() {
                    self.set_status("Filter cleared.");
                } else {
                    self.set_status(&format!("Filter applied: {}", filter_str));
                }
            }
            KeyCode::Backspace => {
                self.filter_input.pop();
            }
            KeyCode::Char(c)
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER) =>
            {
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
                match export_text_report(filename, &self.tree, tab.root_node_id, &tab.filter_query)
                {
                    Ok(_) => {
                        self.set_status(&format!("Exported report to '{}' successfully!", filename))
                    }
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
                if query.is_empty() {
                    self.set_status("Filter cleared.");
                } else {
                    self.set_status(&format!("Filter set: {}", query));
                }
            }
            "noh" | "nofilter" => {
                self.current_tab_mut().set_filter("");
                self.filter_input.clear();
                self.recompute_layout(self.last_layout_rect);
                self.set_status("Filter cleared.");
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
            if !self.current_tab().filter_criteria.is_empty() {
                self.recompute_layout(self.last_layout_rect);
            }
        }
    }

    fn clear_tag(&mut self) {
        let tab = self.current_tab();
        let sel_id = tab.selected_node_id;
        if let Some(node) = self.tree.get_mut(sel_id) {
            node.tag = None;
            self.set_status("Tag cleared.");
            if !self.current_tab().filter_criteria.is_empty() {
                self.recompute_layout(self.last_layout_rect);
            }
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
            self.pending_editor_path = Some(path);
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
        let target_id = self
            .tree
            .get(tab.selected_node_id)
            .and_then(|n| if n.is_dir() { Some(n.id) } else { n.parent })
            .unwrap_or(tab.root_node_id);

        if let Some(node) = self.tree.get(target_id) {
            let path = node.path.clone();
            let removed = self.tree.clear_children(target_id);
            for p in removed {
                self.path_to_id.remove(&p);
            }
            self.recompute_layout(self.last_layout_rect);
            self.scanner_controller.rescan_subtree(path.clone());
            self.is_scanning = true;
            self.set_status(&format!("Rescanning subtree: {}", path.display()));
        }
    }
}
