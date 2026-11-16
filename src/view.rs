use crate::filter::FilterCriteria;
use crate::treemap::TreemapLayout;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Flat,        // Classic SpaceSniffer warm amber / brown
    FileClasses, // Category-based: media, code, archives, etc.
    Depth,       // Color shifted by directory depth
}

pub struct TabView {
    pub id: usize,
    pub name: String,
    pub root_node_id: usize,
    pub zoom_history: Vec<usize>,
    pub selected_node_id: usize,
    pub filter_query: String,
    pub filter_criteria: FilterCriteria,
    pub color_mode: ColorMode,
    pub detail_level: u16, // Base size threshold for expanding nested folders.
    pub treemap: TreemapLayout,
}

impl TabView {
    pub fn new(id: usize, name: String, root_node_id: usize) -> Self {
        Self {
            id,
            name,
            root_node_id,
            zoom_history: Vec::new(),
            selected_node_id: root_node_id,
            filter_query: String::new(),
            filter_criteria: FilterCriteria::default(),
            color_mode: ColorMode::Flat,
            detail_level: 4,
            treemap: TreemapLayout::new(),
        }
    }

    pub fn zoom_into(&mut self, node_id: usize) {
        if node_id != self.root_node_id {
            self.zoom_history.push(self.root_node_id);
            self.root_node_id = node_id;
            self.selected_node_id = node_id;
        }
    }

    pub fn zoom_out(&mut self) -> bool {
        if let Some(prev) = self.zoom_history.pop() {
            self.root_node_id = prev;
            self.selected_node_id = prev;
            true
        } else {
            false
        }
    }

    pub fn set_filter(&mut self, query: &str) {
        self.filter_query = query.to_string();
        self.filter_criteria = FilterCriteria::parse(query);
    }
}
