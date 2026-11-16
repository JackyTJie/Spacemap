use std::path::PathBuf;
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Directory,
    File,
    Symlink,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileClass {
    Directory,
    Archive,
    Media,
    Document,
    Code,
    Binary,
    DiskImage,
    Other,
}

impl FileClass {
    pub fn from_extension(ext: &str) -> Self {
        let ext = ext.to_lowercase();
        match ext.as_str() {
            "zip" | "tar" | "gz" | "bz2" | "xz" | "zst" | "7z" | "rar" | "deb" | "rpm" => {
                FileClass::Archive
            }
            "iso" | "img" | "qcow2" | "vmdk" | "dmg" => FileClass::DiskImage,
            "mp4" | "mkv" | "avi" | "mov" | "webm" | "mp3" | "flac" | "wav" | "ogg" | "png"
            | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" => FileClass::Media,
            "pdf" | "doc" | "docx" | "txt" | "md" | "rst" | "xlsx" | "csv" | "pptx" | "epub" => {
                FileClass::Document
            }
            "rs" | "py" | "js" | "ts" | "jsx" | "tsx" | "c" | "cpp" | "h" | "hpp" | "cc" | "go"
            | "java" | "kt" | "swift" | "html" | "css" | "scss" | "json" | "yaml" | "yml"
            | "toml" | "xml" | "sh" | "bash" | "zsh" | "lua" | "sql" | "vim" => FileClass::Code,
            "exe" | "so" | "dll" | "dylib" | "bin" | "o" | "a" | "class" | "wasm" => {
                FileClass::Binary
            }
            _ => FileClass::Other,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FileNode {
    pub id: usize,
    pub path: PathBuf,
    pub name: String,
    pub kind: FileKind,
    pub class: FileClass,
    pub size: u64,
    pub direct_size: u64,
    pub children: Vec<usize>,
    pub parent: Option<usize>,
    pub modified: Option<SystemTime>,
    pub tag: Option<u8>,
    pub flash_until: Option<SystemTime>,
    pub file_count: usize,
    pub dir_count: usize,
    pub deleted: bool,
}

impl FileNode {
    pub fn new(
        id: usize,
        path: PathBuf,
        name: String,
        kind: FileKind,
        direct_size: u64,
        modified: Option<SystemTime>,
    ) -> Self {
        let class = if kind == FileKind::Directory {
            FileClass::Directory
        } else {
            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            FileClass::from_extension(ext)
        };

        let (file_count, dir_count) = if kind == FileKind::Directory {
            (0, 1)
        } else {
            (1, 0)
        };

        Self {
            id,
            path,
            name,
            kind,
            class,
            size: direct_size,
            direct_size,
            children: Vec::new(),
            parent: None,
            modified,
            tag: None,
            flash_until: None,
            file_count,
            dir_count,
            deleted: false,
        }
    }

    pub fn is_dir(&self) -> bool {
        self.kind == FileKind::Directory
    }
}

pub struct FileTree {
    pub nodes: Vec<FileNode>,
    pub root_id: usize,
    pub total_size: u64,
}

impl FileTree {
    pub fn new(root_path: PathBuf) -> Self {
        let root_name = root_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("/")
            .to_string();
        let root_node = FileNode::new(0, root_path, root_name, FileKind::Directory, 0, None);
        Self {
            nodes: vec![root_node],
            root_id: 0,
            total_size: 0,
        }
    }

    pub fn get(&self, id: usize) -> Option<&FileNode> {
        self.nodes.get(id).filter(|n| !n.deleted)
    }

    pub fn get_mut(&mut self, id: usize) -> Option<&mut FileNode> {
        self.nodes.get_mut(id).filter(|n| !n.deleted)
    }

    pub fn add_child(&mut self, parent_id: usize, mut node: FileNode) -> usize {
        let new_id = self.nodes.len();
        node.id = new_id;
        node.parent = Some(parent_id);
        let node_size = node.direct_size;
        let is_dir = node.is_dir();

        self.nodes.push(node);
        self.nodes[parent_id].children.push(new_id);

        let mut curr = Some(parent_id);
        while let Some(pid) = curr {
            let parent = &mut self.nodes[pid];
            parent.size = parent.size.saturating_add(node_size);
            if is_dir {
                parent.dir_count += 1;
            } else {
                parent.file_count += 1;
            }
            curr = parent.parent;
        }

        self.total_size = self.nodes[self.root_id].size;
        new_id
    }

    pub fn update_node_size(&mut self, node_id: usize, new_direct_size: u64) {
        if node_id >= self.nodes.len() || self.nodes[node_id].deleted || self.nodes[node_id].is_dir() {
            return;
        }
        let old_size = self.nodes[node_id].direct_size;
        if old_size == new_direct_size {
            return;
        }
        self.nodes[node_id].direct_size = new_direct_size;

        if old_size < new_direct_size {
            let diff = new_direct_size - old_size;
            let mut curr = Some(node_id);
            while let Some(id) = curr {
                self.nodes[id].size = self.nodes[id].size.saturating_add(diff);
                curr = self.nodes[id].parent;
            }
        } else {
            let diff = old_size - new_direct_size;
            let mut curr = Some(node_id);
            while let Some(id) = curr {
                self.nodes[id].size = self.nodes[id].size.saturating_sub(diff);
                curr = self.nodes[id].parent;
            }
        }
        self.total_size = self.nodes[self.root_id].size;
    }

    pub fn remove_node(&mut self, node_id: usize) -> Vec<PathBuf> {
        if node_id == self.root_id || node_id >= self.nodes.len() || self.nodes[node_id].deleted {
            return Vec::new();
        }

        let removed_size = self.nodes[node_id].size;
        let removed_files = self.nodes[node_id].file_count;
        let removed_dirs = self.nodes[node_id].dir_count;
        let parent_id = self.nodes[node_id].parent;

        if let Some(pid) = parent_id {
            self.nodes[pid].children.retain(|&cid| cid != node_id);
        }

        let mut curr = parent_id;
        while let Some(id) = curr {
            let ancestor = &mut self.nodes[id];
            ancestor.size = ancestor.size.saturating_sub(removed_size);
            ancestor.file_count = ancestor.file_count.saturating_sub(removed_files);
            ancestor.dir_count = ancestor.dir_count.saturating_sub(removed_dirs);
            curr = ancestor.parent;
        }

        let mut removed_paths = Vec::new();
        let mut stack = vec![node_id];
        while let Some(id) = stack.pop() {
            if id < self.nodes.len() && !self.nodes[id].deleted {
                self.nodes[id].deleted = true;
                removed_paths.push(self.nodes[id].path.clone());
                let children = std::mem::take(&mut self.nodes[id].children);
                stack.extend(children);
            }
        }

        self.total_size = self.nodes[self.root_id].size;
        removed_paths
    }

    pub fn clear_children(&mut self, dir_id: usize) -> Vec<PathBuf> {
        if dir_id >= self.nodes.len() || self.nodes[dir_id].deleted || !self.nodes[dir_id].is_dir() {
            return Vec::new();
        }

        let children = std::mem::take(&mut self.nodes[dir_id].children);
        let removed_size = self.nodes[dir_id].size.saturating_sub(self.nodes[dir_id].direct_size);
        let removed_files = self.nodes[dir_id].file_count;
        let removed_dirs = self.nodes[dir_id].dir_count.saturating_sub(1);

        let mut curr = Some(dir_id);
        while let Some(id) = curr {
            let node = &mut self.nodes[id];
            node.size = node.size.saturating_sub(removed_size);
            node.file_count = node.file_count.saturating_sub(removed_files);
            node.dir_count = node.dir_count.saturating_sub(removed_dirs);
            curr = node.parent;
        }

        let mut removed_paths = Vec::new();
        let mut stack = children;
        while let Some(id) = stack.pop() {
            if id < self.nodes.len() && !self.nodes[id].deleted {
                self.nodes[id].deleted = true;
                removed_paths.push(self.nodes[id].path.clone());
                let sub = std::mem::take(&mut self.nodes[id].children);
                stack.extend(sub);
            }
        }

        self.total_size = self.nodes[self.root_id].size;
        removed_paths
    }

    pub fn set_flash(&mut self, node_id: usize, duration: std::time::Duration) {
        if let Some(node) = self.get_mut(node_id) {
            node.flash_until = Some(SystemTime::now() + duration);
        }
    }
}

pub fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
