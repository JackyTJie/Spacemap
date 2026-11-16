#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use ratatui::layout::{Position, Rect};
    use spacemap::export::export_text_report;
    use spacemap::filter::FilterCriteria;
    use spacemap::model::{FileKind, FileNode, FileTree};
    use spacemap::treemap::{Direction, TreemapLayout};
    use spacemap::view::TabView;

    #[test]
    fn test_filter_parsing() {
        let f = FilterCriteria::parse("*.mp4;>100M;tag:1;!temp*");
        assert_eq!(f.name_patterns.len(), 2);
        assert_eq!(f.size_filters.len(), 1);
        assert!(f.tag_filter.is_some());
    }

    #[test]
    fn test_filter_quoted_filenames_and_invalid_sizes() {
        let f = FilterCriteria::parse("\"my file.mp4\" >-50M");
        assert_eq!(f.name_patterns.len(), 1);
        assert!(f.size_filters.is_empty());

        let node = FileNode::new(
            1,
            PathBuf::from("/test/my file.mp4"),
            "my file.mp4".to_string(),
            FileKind::File,
            1024,
            None,
        );
        assert!(f.matches(&node));
    }

    #[test]
    fn test_treemap_partitioning() {
        let mut tree = FileTree::new(PathBuf::from("/test"));
        let n1 = FileNode::new(1, PathBuf::from("/test/big"), "big".to_string(), FileKind::File, 1000, None);
        let n2 = FileNode::new(2, PathBuf::from("/test/small"), "small".to_string(), FileKind::File, 100, None);
        tree.add_child(0, n1);
        tree.add_child(0, n2);

        let mut layout = TreemapLayout::new();
        layout.compute(
            &tree,
            0,
            Rect { x: 0, y: 0, width: 80, height: 24 },
            2,
            1,
            5,
            &|_| true,
        );

        assert!(!layout.tiles.is_empty());
        let t_big = layout.tiles.iter().find(|t| t.node_id == 1).unwrap();
        let t_small = layout.tiles.iter().find(|t| t.node_id == 2).unwrap();
        let area_big = t_big.rect.width as u32 * t_big.rect.height as u32;
        let area_small = t_small.rect.width as u32 * t_small.rect.height as u32;
        assert!(area_big >= area_small);
    }

    #[test]
    fn test_filtered_treemap_prunes_empty_dirs_and_updates_display_size() {
        let mut tree = FileTree::new(PathBuf::from("/test"));
        let dir_a = tree.add_child(
            0,
            FileNode::new(0, PathBuf::from("/test/a"), "a".to_string(), FileKind::Directory, 0, None),
        );
        let dir_b = tree.add_child(
            0,
            FileNode::new(0, PathBuf::from("/test/b"), "b".to_string(), FileKind::Directory, 0, None),
        );
        tree.add_child(
            dir_a,
            FileNode::new(0, PathBuf::from("/test/a/keep.iso"), "keep.iso".to_string(), FileKind::File, 500, None),
        );
        tree.add_child(
            dir_a,
            FileNode::new(0, PathBuf::from("/test/a/skip.txt"), "skip.txt".to_string(), FileKind::File, 1500, None),
        );
        tree.add_child(
            dir_b,
            FileNode::new(0, PathBuf::from("/test/b/other.txt"), "other.txt".to_string(), FileKind::File, 2000, None),
        );

        let criteria = FilterCriteria::parse("*.iso");
        let mut layout = TreemapLayout::new();
        layout.compute(
            &tree,
            0,
            Rect::new(0, 0, 80, 24),
            2,
            1,
            5,
            &|node| criteria.matches(node),
        );

        assert!(layout.tiles.iter().any(|t| t.node_id == dir_a));
        assert!(!layout.tiles.iter().any(|t| t.node_id == dir_b));
        let tile_a = layout.tiles.iter().find(|t| t.node_id == dir_a).unwrap();
        assert_eq!(tile_a.display_size, 500);
    }

    #[test]
    fn test_remove_node_and_clear_children_update_tree_sizes() {
        let mut tree = FileTree::new(PathBuf::from("/test"));
        let dir_id = tree.add_child(
            0,
            FileNode::new(0, PathBuf::from("/test/sub"), "sub".to_string(), FileKind::Directory, 0, None),
        );
        let f1 = tree.add_child(
            dir_id,
            FileNode::new(0, PathBuf::from("/test/sub/f1"), "f1".to_string(), FileKind::File, 400, None),
        );
        let _f2 = tree.add_child(
            dir_id,
            FileNode::new(0, PathBuf::from("/test/sub/f2"), "f2".to_string(), FileKind::File, 600, None),
        );
        assert_eq!(tree.total_size, 1000);

        let removed = tree.remove_node(f1);
        assert_eq!(removed.len(), 1);
        assert_eq!(tree.total_size, 600);
        assert!(tree.get(f1).is_none());

        let cleared = tree.clear_children(dir_id);
        assert_eq!(cleared.len(), 1);
        assert_eq!(tree.total_size, 0);
        assert_eq!(tree.get(dir_id).unwrap().file_count, 0);
    }

    #[test]
    fn test_spatial_navigation() {
        let mut tree = FileTree::new(PathBuf::from("/test"));
        let n1 = FileNode::new(1, PathBuf::from("/test/left"), "left".to_string(), FileKind::File, 500, None);
        let n2 = FileNode::new(2, PathBuf::from("/test/right"), "right".to_string(), FileKind::File, 500, None);
        tree.add_child(0, n1);
        tree.add_child(0, n2);

        let mut layout = TreemapLayout::new();
        layout.compute(
            &tree,
            0,
            Rect { x: 0, y: 0, width: 80, height: 24 },
            2,
            1,
            5,
            &|_| true,
        );

        let left_tile = layout.tiles.iter().find(|t| t.node_id == 1).unwrap();
        let _right_tile = layout.tiles.iter().find(|t| t.node_id == 2).unwrap();

        let neighbor = layout.find_neighbor(left_tile.node_id, Direction::Right);
        assert!(neighbor.is_some());
    }

    #[test]
    fn test_tiny_tiles_stay_inside_their_parent_and_do_not_overlap() {
        let mut tree = FileTree::new("/test".into());
        for i in 0..200 {
            let name = format!("file-{i}");
            let path = PathBuf::from("/test").join(&name);
            let size = if i == 0 { 300 } else { 1 };
            tree.add_child(0, FileNode::new(0, path, name, FileKind::File, size, None));
        }

        let bounds = Rect::new(2, 3, 22, 7);
        let mut layout = TreemapLayout::new();
        layout.compute(&tree, 0, bounds, 4, 2, 12, &|_| true);
        let siblings = &layout.tiles[1..];
        assert!(siblings.len() > 1);
        for (index, tile) in siblings.iter().enumerate() {
            assert_eq!(tile.rect.intersection(bounds), tile.rect);
            assert!(siblings[..index].iter().all(|other| !other.rect.intersects(tile.rect)));
            assert_eq!(layout.node_at(Position::new(tile.rect.x, tile.rect.y)), Some(tile.node_id));
        }
    }

    #[test]
    fn test_child_covering_the_canvas_can_be_selected() {
        let mut tree = FileTree::new("/test".into());
        let child = FileNode::new(0, "/test/only".into(), "only".into(), FileKind::File, 42, None);
        let child_id = tree.add_child(0, child);
        let bounds = Rect::new(2, 3, 80, 24);
        let mut layout = TreemapLayout::new();
        layout.compute(&tree, 0, bounds, 4, 2, 12, &|_| true);
        assert_eq!(layout.node_at(Position::new(bounds.x, bounds.y)), Some(child_id));
        assert_eq!(layout.find_neighbor(0, Direction::Right), Some(child_id));
    }

    #[test]
    fn test_tab_view_zoom() {
        let mut tab = TabView::new(1, "Test".to_string(), 0);
        assert_eq!(tab.root_node_id, 0);

        tab.zoom_into(5);
        assert_eq!(tab.root_node_id, 5);
        assert_eq!(tab.zoom_history, vec![0]);

        let zoomed_out = tab.zoom_out();
        assert!(zoomed_out);
        assert_eq!(tab.root_node_id, 0);
        assert!(tab.zoom_history.is_empty());
    }

    #[test]
    fn test_export_report() {
        let mut tree = FileTree::new(PathBuf::from("/test"));
        let n1 = FileNode::new(1, PathBuf::from("/test/video.mp4"), "video.mp4".to_string(), FileKind::File, 1024 * 1024 * 50, None);
        let n2 = FileNode::new(2, PathBuf::from("/test/notes.txt"), "notes.txt".to_string(), FileKind::File, 1024, None);
        tree.add_child(0, n1);
        tree.add_child(0, n2);

        let temp_dir = std::env::temp_dir();
        let report_file = temp_dir.join("test_audit_report.txt");

        let res = export_text_report(&report_file, &tree, 0, "*.mp4");
        assert!(res.is_ok());

        let content = std::fs::read_to_string(&report_file).unwrap();
        assert!(content.contains("SPACEMAP - STORAGE AUDIT REPORT"));
        assert!(content.contains("video.mp4"));
        assert!(!content.contains("notes.txt"));
        assert!(content.contains("50.00 MB"));

        let _ = std::fs::remove_file(report_file);
    }

    #[test]
    fn test_single_child_folder_chain_collapses_into_one_tile() {
        let mut tree = FileTree::new(PathBuf::from("/test"));
        let d1 = tree.add_child(
            0,
            FileNode::new(0, PathBuf::from("/test/com.docker.docker"), "com.docker.docker".to_string(), FileKind::Directory, 0, None),
        );
        let d2 = tree.add_child(
            d1,
            FileNode::new(0, PathBuf::from("/test/com.docker.docker/Data"), "Data".to_string(), FileKind::Directory, 0, None),
        );
        let d3 = tree.add_child(
            d2,
            FileNode::new(0, PathBuf::from("/test/com.docker.docker/Data/vms"), "vms".to_string(), FileKind::Directory, 0, None),
        );
        let d4 = tree.add_child(
            d3,
            FileNode::new(0, PathBuf::from("/test/com.docker.docker/Data/vms/0"), "0".to_string(), FileKind::Directory, 0, None),
        );
        let d5 = tree.add_child(
            d4,
            FileNode::new(0, PathBuf::from("/test/com.docker.docker/Data/vms/0/data"), "data".to_string(), FileKind::Directory, 0, None),
        );
        let file_id = tree.add_child(
            d5,
            FileNode::new(0, PathBuf::from("/test/com.docker.docker/Data/vms/0/data/Docker.raw"), "Docker.raw".to_string(), FileKind::File, 50_000, None),
        );

        let mut layout = TreemapLayout::new();
        layout.compute(&tree, 0, Rect::new(0, 0, 80, 24), 5, 2, 4, &|_| true);

        let collapsed_tile = layout.tiles.iter().find(|t| t.node_id == d5).unwrap();
        assert_eq!(
            collapsed_tile.collapsed_label.as_deref(),
            Some("com.docker.docker/Data/vms/0/data")
        );
        assert!(layout.tiles.iter().any(|t| t.node_id == file_id));
        assert!(!layout.tiles.iter().any(|t| t.node_id == d1));
    }
}
