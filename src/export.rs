use std::fs::File;
use std::io::{self, Write};
use std::path::Path;
use chrono::Local;
use crate::model::{format_size, FileNode, FileTree};

pub fn export_text_report<P: AsRef<Path>>(
    path: P,
    tree: &FileTree,
    root_id: usize,
    filter_query: &str,
) -> io::Result<()> {
    let mut file = File::create(path)?;
    let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

    writeln!(file, "================================================================================")?;
    writeln!(file, "                        SPACEMAP - STORAGE AUDIT REPORT                         ")?;
    writeln!(file, "================================================================================")?;
    writeln!(file, "Generated:      {}", now)?;
    if let Some(root) = tree.get(root_id) {
        writeln!(file, "Audit Root:     {}", root.path.display())?;
        writeln!(file, "Subtree Size:   {}", format_size(root.size))?;
        writeln!(file, "Total Files:    {}", root.file_count)?;
        writeln!(file, "Total Dirs:     {}", root.dir_count)?;
    }
    if !filter_query.is_empty() {
        writeln!(file, "Active Filter:  {}", filter_query)?;
    }
    writeln!(file, "================================================================================")?;
    writeln!(file)?;

    // Top 20 largest files in this subtree
    writeln!(file, "----------------------------- TOP LARGEST FILES --------------------------------")?;
    writeln!(file, "{:<12}  {:<8}  {}", "SIZE", "TAG", "PATH")?;
    writeln!(file, "--------------------------------------------------------------------------------")?;

    let mut all_files = Vec::new();
    collect_files(tree, root_id, &mut all_files);
    all_files.sort_by(|a, b| b.size.cmp(&a.size));

    for node in all_files.iter().take(30) {
        let tag_str = match node.tag {
            Some(1) => "[RED]",
            Some(2) => "[GREEN]",
            Some(3) => "[YELLOW]",
            Some(4) => "[BLUE]",
            _ => "     ",
        };
        writeln!(file, "{:<12}  {:<8}  {}", format_size(node.size), tag_str, node.path.display())?;
    }

    writeln!(file)?;
    writeln!(file, "----------------------------- HIERARCHICAL TREE --------------------------------")?;
    write_tree_node(&mut file, tree, root_id, "", true)?;
    writeln!(file)?;
    writeln!(file, "================================================================================")?;
    writeln!(file, "                            END OF AUDIT REPORT                                 ")?;
    writeln!(file, "================================================================================")?;

    Ok(())
}

fn collect_files<'a>(tree: &'a FileTree, node_id: usize, list: &mut Vec<&'a FileNode>) {
    if let Some(node) = tree.get(node_id) {
        if !node.is_dir() {
            list.push(node);
        } else {
            for &cid in &node.children {
                collect_files(tree, cid, list);
            }
        }
    }
}

fn write_tree_node<W: Write>(
    w: &mut W,
    tree: &FileTree,
    node_id: usize,
    prefix: &str,
    is_last: bool,
) -> io::Result<()> {
    let Some(node) = tree.get(node_id) else {
        return Ok(());
    };

    let marker = if is_last { "└── " } else { "├── " };
    let kind_icon = if node.is_dir() { "📁 " } else { "   " };
    let tag_badge = match node.tag {
        Some(1) => " [TAG:RED]",
        Some(2) => " [TAG:GREEN]",
        Some(3) => " [TAG:YELLOW]",
        Some(4) => " [TAG:BLUE]",
        _ => "",
    };

    writeln!(
        w,
        "{}{}{}{:<32} {:>10}{}",
        prefix,
        marker,
        kind_icon,
        node.name,
        format_size(node.size),
        tag_badge
    )?;

    if node.is_dir() {
        let new_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
        let child_count = node.children.len();
        for (i, &cid) in node.children.iter().enumerate() {
            let last_child = i == child_count - 1;
            write_tree_node(w, tree, cid, &new_prefix, last_child)?;
        }
    }

    Ok(())
}
