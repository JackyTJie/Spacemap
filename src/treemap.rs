use std::collections::HashMap;
use crate::model::{FileNode, FileTree};
use ratatui::layout::{Position, Rect};

#[derive(Debug, Clone)]
pub struct VisibleTile {
    pub node_id: usize,
    pub rect: Rect,
    pub depth: usize,
    pub is_leaf: bool,
    pub is_dir: bool,
    pub display_size: u64,
    pub share_of_parent: f64,
    pub collapsed_label: Option<String>,
}

pub struct TreemapLayout {
    pub tiles: Vec<VisibleTile>,
}

impl Default for TreemapLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl TreemapLayout {
    pub fn new() -> Self {
        Self { tiles: Vec::new() }
    }

    pub fn compute<F>(
        &mut self,
        tree: &FileTree,
        root_id: usize,
        bounds: Rect,
        min_width: u16,
        min_height: u16,
        max_depth: usize,
        filter_fn: &F,
    ) where
        F: Fn(&FileNode) -> bool,
    {
        self.tiles.clear();
        if bounds.width == 0 || bounds.height == 0 {
            return;
        }

        let Some(root) = tree.get(root_id) else {
            return;
        };

        let mut effective_sizes = HashMap::new();
        let root_size = compute_effective_sizes(tree, root_id, filter_fn, &mut effective_sizes);

        self.tiles.push(VisibleTile {
            node_id: root_id,
            rect: bounds,
            depth: 0,
            is_leaf: true,
            is_dir: root.is_dir(),
            display_size: root_size,
            share_of_parent: 1.0,
            collapsed_label: None,
        });

        self.layout_children(
            tree,
            root_id,
            bounds,
            1,
            min_width,
            min_height,
            max_depth,
            &effective_sizes,
        );
        self.tiles[0].is_leaf = self.tiles.len() == 1;
    }

    #[allow(clippy::too_many_arguments)]
    fn layout_children(
        &mut self,
        tree: &FileTree,
        parent_id: usize,
        parent_rect: Rect,
        current_depth: usize,
        min_width: u16,
        min_height: u16,
        max_depth: usize,
        effective_sizes: &HashMap<usize, u64>,
    ) {
        if current_depth > max_depth {
            return;
        }

        let interior = if current_depth == 1 {
            parent_rect
        } else {
            let req_w = min_width.saturating_mul(4).max(16);
            let req_h = min_height.saturating_mul(3).max(6);
            if parent_rect.width < req_w || parent_rect.height < req_h {
                return;
            }
            Rect::new(
                parent_rect.x + 1,
                parent_rect.y + 1,
                parent_rect.width.saturating_sub(2),
                parent_rect.height.saturating_sub(2),
            )
        };

        if interior.width == 0 || interior.height == 0 {
            return;
        }

        let Some(parent) = tree.get(parent_id) else {
            return;
        };

        let mut children_info: Vec<(usize, u64, Option<String>)> = Vec::new();
        let mut total_child_size = 0u64;

        for &child_id in &parent.children {
            if let Some(&sz) = effective_sizes.get(&child_id) {
                if sz > 0 {
                    let (target_id, collapsed_label) =
                        collapse_single_child_chain(tree, child_id, sz, effective_sizes);
                    children_info.push((target_id, sz, collapsed_label));
                    total_child_size = total_child_size.saturating_add(sz);
                }
            }
        }

        if children_info.is_empty() || total_child_size == 0 {
            return;
        }

        children_info.sort_by(|a, b| b.1.cmp(&a.1));

        let parent_area = interior.width as f64 * interior.height as f64;
        let min_tile_area = if current_depth == 1 {
            (min_width as f64 * min_height as f64 * 0.75).max(4.0)
        } else {
            (min_width as f64 * min_height as f64 * 1.5).max(12.0)
        };

        let mut visible_children: Vec<(usize, u64, Option<String>)> = Vec::new();
        let mut visible_total_size = 0u64;

        for (idx, item) in children_info.into_iter().enumerate() {
            let expected_area = (item.1 as f64 / total_child_size as f64) * parent_area;
            if idx == 0 || expected_area >= min_tile_area || visible_children.len() < 3 {
                visible_total_size = visible_total_size.saturating_add(item.1);
                visible_children.push(item);
            }
            if visible_children.len() >= 48 {
                break;
            }
        }

        if visible_children.is_empty() || visible_total_size == 0 {
            return;
        }

        let rects = squarify(
            interior,
            &visible_children
                .iter()
                .map(|(_, sz, _)| *sz as f64)
                .collect::<Vec<_>>(),
            visible_total_size as f64,
        );

        for (i, (child_id, sz, collapsed_label)) in visible_children.into_iter().enumerate() {
            let mut child_rect = rects[i];
            if child_rect.width < 1 || child_rect.height < 1 {
                continue;
            }
            if current_depth > 1 && (child_rect.width < 4 || child_rect.height < 2) {
                continue;
            }

            let Some(child) = tree.get(child_id) else {
                continue;
            };

            let share = if total_child_size > 0 {
                sz as f64 / total_child_size as f64
            } else {
                0.0
            };

            if child_rect.width >= 5 && child_rect.right() < interior.right() {
                child_rect.width -= 1;
            }
            if child_rect.height >= 4 && child_rect.bottom() < interior.bottom() {
                child_rect.height -= 1;
            }

            let tile_index = self.tiles.len();
            self.tiles.push(VisibleTile {
                node_id: child_id,
                rect: child_rect,
                depth: current_depth,
                is_leaf: true,
                is_dir: child.is_dir(),
                display_size: sz,
                share_of_parent: share,
                collapsed_label,
            });

            if child.is_dir() && !child.children.is_empty() {
                self.layout_children(
                    tree,
                    child_id,
                    child_rect,
                    current_depth + 1,
                    min_width,
                    min_height,
                    max_depth,
                    effective_sizes,
                );
                self.tiles[tile_index].is_leaf = self.tiles.len() == tile_index + 1;
            }
        }
    }
}

fn collapse_single_child_chain(
    tree: &FileTree,
    start_id: usize,
    start_size: u64,
    effective_sizes: &HashMap<usize, u64>,
) -> (usize, Option<String>) {
    let Some(start_node) = tree.get(start_id) else {
        return (start_id, None);
    };
    if !start_node.is_dir() {
        return (start_id, None);
    }

    let mut curr_id = start_id;
    let mut names = vec![start_node.name.clone()];

    while let Some(node) = tree.get(curr_id) {
        if !node.is_dir() || names.len() >= 6 {
            break;
        }
        let mut active_dirs = Vec::new();
        let mut active_files = 0usize;
        for &cid in &node.children {
            if let Some(&csz) = effective_sizes.get(&cid) {
                if csz > 0 {
                    if let Some(cnode) = tree.get(cid) {
                        if cnode.is_dir() && csz * 100 >= start_size * 98 {
                            active_dirs.push((cid, cnode.name.clone()));
                        } else {
                            active_files += 1;
                        }
                    }
                }
            }
        }

        if active_dirs.len() == 1 && active_files == 0 {
            let (next_id, next_name) = active_dirs.pop().unwrap();
            names.push(next_name);
            curr_id = next_id;
        } else {
            break;
        }
    }

    if names.len() > 1 {
        (curr_id, Some(names.join("/")))
    } else {
        (start_id, None)
    }
}

fn compute_effective_sizes<F>(
    tree: &FileTree,
    node_id: usize,
    filter_fn: &F,
    out: &mut HashMap<usize, u64>,
) -> u64
where
    F: Fn(&FileNode) -> bool,
{
    let Some(node) = tree.get(node_id) else {
        return 0;
    };

    if !node.is_dir() {
        let sz = if filter_fn(node) { node.size } else { 0 };
        if sz > 0 {
            out.insert(node_id, sz);
        }
        return sz;
    }

    let mut sum = 0u64;
    for &cid in &node.children {
        sum = sum.saturating_add(compute_effective_sizes(tree, cid, filter_fn, out));
    }

    let effective = if sum > 0 {
        sum
    } else if node.children.is_empty() && filter_fn(node) {
        node.size
    } else {
        0
    };

    if effective > 0 {
        out.insert(node_id, effective);
    }
    effective
}

fn squarify(container: Rect, weights: &[f64], total_weight: f64) -> Vec<Rect> {
    let mut result = vec![Rect::default(); weights.len()];
    if weights.is_empty() || container.width == 0 || container.height == 0 || total_weight <= 0.0 {
        return result;
    }

    let total_area = (container.width as f64 * 0.5) * (container.height as f64);
    let areas: Vec<f64> = weights
        .iter()
        .map(|&w| (w / total_weight) * total_area)
        .collect();

    let mut remaining_rect = (
        container.x as f64,
        container.y as f64,
        container.width as f64,
        container.height as f64,
    );

    let mut start_idx = 0;
    while start_idx < areas.len() {
        let (rx, ry, rw, rh) = remaining_rect;
        if rw <= 0.0 || rh <= 0.0 {
            break;
        }

        let norm_w = rw * 0.5;
        let norm_h = rh;
        let along_horizontal = norm_w < norm_h;

        let side = norm_w.min(norm_h);
        let mut row_area = areas[start_idx];
        let mut end_idx = start_idx + 1;
        while end_idx < areas.len() {
            let next_area = areas[end_idx];
            let largest = areas[start_idx];
            let current_worst = worst_aspect_ratio(largest, areas[end_idx - 1], row_area, side);
            let next_worst = worst_aspect_ratio(largest, next_area, row_area + next_area, side);

            if next_worst <= current_worst {
                row_area += next_area;
                end_idx += 1;
            } else {
                break;
            }
        }

        if along_horizontal {
            let row_h = if end_idx == areas.len() {
                rh
            } else {
                (row_area / norm_w).min(rh)
            };
            let mut curr_x = rx;

            for idx in start_idx..end_idx {
                let next_x = if idx + 1 == end_idx {
                    rx + rw
                } else {
                    (curr_x + rw * areas[idx] / row_area).min(rx + rw)
                };
                result[idx] = rounded_rect(curr_x, ry, next_x, ry + row_h);
                curr_x = next_x;
            }

            remaining_rect = (rx, ry + row_h, rw, rh - row_h);
        } else {
            let row_w = if end_idx == areas.len() {
                rw
            } else {
                (row_area / rh / 0.5).min(rw)
            };
            let mut curr_y = ry;

            for idx in start_idx..end_idx {
                let next_y = if idx + 1 == end_idx {
                    ry + rh
                } else {
                    (curr_y + rh * areas[idx] / row_area).min(ry + rh)
                };
                result[idx] = rounded_rect(rx, curr_y, rx + row_w, next_y);
                curr_y = next_y;
            }

            remaining_rect = (rx + row_w, ry, rw - row_w, rh);
        }

        start_idx = end_idx;
    }

    result
}

fn rounded_rect(left: f64, top: f64, right: f64, bottom: f64) -> Rect {
    let x = left.round().max(0.0) as u16;
    let y = top.round().max(0.0) as u16;
    let r = right.round().max(0.0) as u16;
    let b = bottom.round().max(0.0) as u16;
    Rect::new(x, y, r.saturating_sub(x), b.saturating_sub(y))
}

fn worst_aspect_ratio(largest: f64, smallest: f64, total_row_area: f64, side_length: f64) -> f64 {
    if side_length <= 0.0 || total_row_area <= 0.0 || smallest <= 0.0 {
        return f64::MAX;
    }
    let s2 = side_length * side_length;
    let a2 = total_row_area * total_row_area;
    (s2 * largest / a2).max(a2 / (s2 * smallest))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl TreemapLayout {
    pub fn node_at(&self, pos: Position) -> Option<usize> {
        self.tiles
            .iter()
            .rev()
            .find(|tile| tile.rect.contains(pos))
            .map(|tile| tile.node_id)
    }

    pub fn find_neighbor(&self, current_id: usize, direction: Direction) -> Option<usize> {
        let current_tile = self.tiles.iter().find(|t| t.node_id == current_id)?;
        let cur_rect = current_tile.rect;
        if current_tile.depth == 0 {
            if let Some(child) = self
                .tiles
                .iter()
                .skip(1)
                .find(|t| t.is_leaf)
                .or_else(|| self.tiles.get(1))
            {
                return Some(child.node_id);
            }
        }
        let cx = cur_rect.x as f64 + cur_rect.width as f64 / 2.0;
        let cy = cur_rect.y as f64 + cur_rect.height as f64 / 2.0;

        let mut best_id = None;
        let mut min_distance = f64::MAX;

        for tile in self.tiles.iter().skip(1) {
            if tile.node_id == current_id || !tile.is_leaf {
                continue;
            }
            let t_rect = tile.rect;
            let tx = t_rect.x as f64 + t_rect.width as f64 / 2.0;
            let ty = t_rect.y as f64 + t_rect.height as f64 / 2.0;

            let dx = tx - cx;
            let dy = (ty - cy) * 2.0;

            let is_in_direction = match direction {
                Direction::Left => dx < -0.5,
                Direction::Right => dx > 0.5,
                Direction::Up => dy < -0.5,
                Direction::Down => dy > 0.5,
            };

            if is_in_direction {
                let (primary_dist, perp_dist) = match direction {
                    Direction::Left | Direction::Right => (dx.abs(), dy.abs()),
                    Direction::Up | Direction::Down => (dy.abs(), dx.abs()),
                };

                let score = primary_dist + perp_dist * 2.5;
                if score < min_distance {
                    min_distance = score;
                    best_id = Some(tile.node_id);
                }
            }
        }

        best_id
    }
}
