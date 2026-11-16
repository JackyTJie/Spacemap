use crate::model::{FileNode, FileTree};
use ratatui::layout::{Position, Rect};

#[derive(Debug, Clone)]
pub struct VisibleTile {
    pub node_id: usize,
    pub rect: Rect,
    pub depth: usize,
    pub is_leaf: bool, // true if it has no visible nested children inside its rect
    pub is_dir: bool,
    pub share_of_parent: f64,
}

pub struct TreemapLayout {
    pub tiles: Vec<VisibleTile>,
}

impl TreemapLayout {
    pub fn new() -> Self {
        Self { tiles: Vec::new() }
    }

    /// Computes nested squarified treemap layout for a given root node within `bounds`.
    /// `filter_fn` determines whether a node is visible according to active filters.
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

        // Root tile itself
        self.tiles.push(VisibleTile {
            node_id: root_id,
            rect: bounds,
            depth: 0,
            is_leaf: true,
            is_dir: root.is_dir(),
            share_of_parent: 1.0,
        });

        // Recursively lay out children inside root's interior
        self.layout_children(
            tree, root_id, bounds, 1, min_width, min_height, max_depth, filter_fn,
        );
        self.tiles[0].is_leaf = self.tiles.len() == 1;
    }

    fn layout_children<F>(
        &mut self,
        tree: &FileTree,
        parent_id: usize,
        parent_rect: Rect,
        current_depth: usize,
        min_width: u16,
        min_height: u16,
        max_depth: usize,
        filter_fn: &F,
    ) where
        F: Fn(&FileNode) -> bool,
    {
        if current_depth > max_depth {
            return;
        }

        let interior = if current_depth == 1 {
            // The breadcrumb labels the canvas, so the root needs no title row.
            parent_rect
        } else {
            // Only expand folders with room for a title and legible child tiles.
            if parent_rect.width < min_width.saturating_mul(5).max(6)
                || parent_rect.height < min_height.saturating_mul(4).max(4)
            {
                return;
            }
            Rect::new(
                parent_rect.x + 1,
                parent_rect.y + 1,
                parent_rect.width - 2,
                parent_rect.height - 2,
            )
        };

        let Some(parent) = tree.get(parent_id) else {
            return;
        };

        // Collect visible children matching filter
        let mut children_info: Vec<(usize, u64)> = Vec::new();
        let mut total_child_size = 0u64;

        for &child_id in &parent.children {
            if let Some(child) = tree.get(child_id) {
                if filter_fn(child) && child.size > 0 {
                    children_info.push((child_id, child.size));
                    total_child_size += child.size;
                }
            }
        }

        if children_info.is_empty() || total_child_size == 0 {
            return;
        }

        // Sort descending by size
        children_info.sort_by(|a, b| b.1.cmp(&a.1));

        // Squarified treemap partitioning
        let rects = squarify(
            interior,
            &children_info
                .iter()
                .map(|(_, sz)| *sz as f64)
                .collect::<Vec<_>>(),
            total_child_size as f64,
        );

        for (i, &(child_id, sz)) in children_info.iter().enumerate() {
            let mut child_rect = rects[i];
            if child_rect.width < 1 || child_rect.height < 1 {
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

            // Leave a seam between larger siblings while keeping small tiles selectable.
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
                share_of_parent: share,
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
                    filter_fn,
                );
                self.tiles[tile_index].is_leaf = self.tiles.len() == tile_index + 1;
            }
        }
    }
}

/// Squarified treemap algorithm adjusted for terminal character aspect ratio.
/// In terminals, 1 character row is roughly twice as tall as 1 character column is wide.
/// So we scale width by 0.5 for metric aspect calculations.
fn squarify(container: Rect, weights: &[f64], total_weight: f64) -> Vec<Rect> {
    let mut result = vec![Rect::default(); weights.len()];
    if weights.is_empty() || container.width == 0 || container.height == 0 || total_weight <= 0.0 {
        return result;
    }

    let total_area = (container.width as f64 * 0.5) * (container.height as f64);
    // Normalize weights to areas
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

        // Terminal normalized width & height
        let norm_w = rw * 0.5;
        let norm_h = rh;
        let along_horizontal = norm_w < norm_h; // slice along the shorter side

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

        // Lay out the current row
        if along_horizontal {
            // Side along width, subdivide vertically
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
            // Side along height, subdivide horizontally
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

// Round shared edges once so siblings stay disjoint even below one cell.
fn rounded_rect(left: f64, top: f64, right: f64, bottom: f64) -> Rect {
    let x = left.round() as u16;
    let y = top.round() as u16;
    Rect::new(x, y, right.round() as u16 - x, bottom.round() as u16 - y)
}

fn worst_aspect_ratio(largest: f64, smallest: f64, total_row_area: f64, side_length: f64) -> f64 {
    let s2 = side_length * side_length;
    let a2 = total_row_area * total_row_area;
    (s2 * largest / a2).max(a2 / (s2 * smallest))
}

/// 2D Direction for spatial navigation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl TreemapLayout {
    /// Children are painted after their parents, so hit-test in reverse paint order.
    pub fn node_at(&self, pos: Position) -> Option<usize> {
        self.tiles
            .iter()
            .rev()
            .find(|tile| tile.rect.contains(pos))
            .map(|tile| tile.node_id)
    }

    /// Finds the closest adjacent tile in the given 2D direction from the currently selected tile.
    pub fn find_neighbor(&self, current_id: usize, direction: Direction) -> Option<usize> {
        // Find visible tile corresponding to current_id
        let current_tile = self.tiles.iter().find(|t| t.node_id == current_id)?;
        let cur_rect = current_tile.rect;
        if current_tile.depth == 0
            && let Some(child) = self.tiles.get(1).filter(|child| child.rect == cur_rect)
        {
            return Some(child.node_id);
        }
        let cx = cur_rect.x as f64 + cur_rect.width as f64 / 2.0;
        let cy = cur_rect.y as f64 + cur_rect.height as f64 / 2.0;

        let mut best_id = None;
        let mut min_distance = f64::MAX;

        for tile in &self.tiles {
            if tile.node_id == current_id {
                continue;
            }
            let t_rect = tile.rect;
            let tx = t_rect.x as f64 + t_rect.width as f64 / 2.0;
            let ty = t_rect.y as f64 + t_rect.height as f64 / 2.0;

            let dx = tx - cx;
            let dy = (ty - cy) * 2.0; // Terminal cell aspect compensation

            let is_in_direction = match direction {
                Direction::Left => dx < -0.5,
                Direction::Right => dx > 0.5,
                Direction::Up => dy < -0.5,
                Direction::Down => dy > 0.5,
            };

            if is_in_direction {
                // Penalize perpendicular deviation to prefer direct neighbors
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
