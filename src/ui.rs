use std::{borrow::Cow, path::Path, time::SystemTime};

use crossterm::style::Colored;
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Padding, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, InputMode, Modal};
use crate::model::{FileClass, FileKind, FileNode, format_size};
use crate::treemap::VisibleTile;
use crate::view::ColorMode;

const PAPER: Color = Color::Rgb(248, 242, 233);
const CHROME: Color = Color::Rgb(235, 224, 208);
const MAT: Color = Color::Rgb(228, 209, 181);
const INK: Color = Color::Rgb(43, 36, 31);
const MUTED: Color = Color::Rgb(105, 85, 66);
const AMBER: Color = Color::Rgb(190, 130, 58);
const FOCUS: Color = Color::Rgb(31, 61, 86);
const DANGER: Color = Color::Rgb(146, 52, 45);

const MIN_WIDTH: u16 = 20;
const MIN_HEIGHT: u16 = 8;
const HEADER_ROWS: u16 = 3;
const FOOTER_ROWS: u16 = 2;

pub fn treemap_area(area: Rect) -> Rect {
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        return Rect::default();
    }

    let margin = if area.width >= 48 { 2 } else { 1 };
    Rect::new(
        area.x + margin,
        area.y + HEADER_ROWS,
        area.width.saturating_sub(2 * margin),
        area.height.saturating_sub(HEADER_ROWS + FOOTER_ROWS),
    )
}

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    fill(f, area, PAPER);
    let map = treemap_area(area);
    if map.is_empty() {
        f.render_widget(
            Paragraph::new("Resize terminal")
                .style(Style::new().fg(MUTED))
                .alignment(Alignment::Center),
            area,
        );
        return;
    }

    draw_header(f, app, Rect::new(map.x, area.y, map.width, 1));
    draw_breadcrumbs(f, app, Rect::new(map.x, area.y + 1, map.width, 1));
    f.render_widget(
        Paragraph::new("▄".repeat(map.width as usize)).style(Style::new().fg(AMBER)),
        Rect::new(map.x, area.y + 2, map.width, 1),
    );

    draw_treemap(f, app, map);
    draw_selection(f, app, Rect::new(map.x, map.bottom(), map.width, 1));
    draw_footer(f, app, Rect::new(map.x, map.bottom() + 1, map.width, 1));

    match &app.modal {
        Some(Modal::Help) => draw_help(f, area),
        Some(Modal::ActionMenu) => draw_actions(f, app, area),
        Some(Modal::Inspector) => draw_inspector(f, app, area),
        Some(Modal::ConfirmDelete(path)) => draw_confirm_delete(f, path, area),
        None => {}
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let mut count = format!("{} items", format_count(app.total_scanned_items));
    if app.scan_error_count > 0 && area.width >= 56 {
        count.push_str(&format!(" · {} skipped", format_count(app.scan_error_count)));
    }
    let status = match (app.is_scanning, area.width) {
        (_, 0..=25) => String::new(),
        (true, 44..) => format!("Scanning · {count}"),
        (true, _) => "Scanning".into(),
        (false, _) => count,
    };
    let (left, right) = split_right(area, status.as_str().width() as u16);
    f.render_widget(
        Paragraph::new(Span::styled(status, Style::new().fg(MUTED))),
        right,
    );

    let mut title = Line::from(Span::styled("Spacemap", bold(INK)));
    if app.tabs.len() > 1 {
        title.spans.push(Span::styled(
            format!(
                "  {} ({}/{})",
                app.current_tab().name,
                app.active_tab_index + 1,
                app.tabs.len()
            ),
            Style::new().fg(MUTED),
        ));
    }
    f.render_widget(Paragraph::new(title), left);
}

fn draw_breadcrumbs(f: &mut Frame, app: &App, area: Rect) {
    fill(f, area, CHROME);
    let tab = app.current_tab();
    let Some(root) = app.tree.get(tab.root_node_id) else {
        return;
    };

    let root_display_size = tab
        .treemap
        .tiles
        .first()
        .map_or(root.size, |t| t.display_size);
    let mut summary = format_size(root_display_size);
    if tab.root_node_id != app.tree.root_id && area.width >= 52 {
        let share = 100.0 * root_display_size as f64 / app.tree.total_size.max(1) as f64;
        summary.push_str(&format!(" · {share:.1}% of scan"));
    }
    let (left, right) = split_right(area, summary.as_str().width() as u16);
    f.render_widget(Paragraph::new(Span::styled(summary, bold(INK))), right);

    let width = left.width as usize;
    let mut trail = root.name.clone();
    let mut ancestor = root.parent;
    while let Some(parent) = ancestor.and_then(|id| app.tree.get(id)) {
        if parent.name.as_str().width() + 3 + trail.as_str().width() > width {
            if trail.as_str().width() + 4 <= width {
                trail.insert_str(0, "… › ");
            }
            break;
        }
        trail.insert_str(0, &format!("{} › ", parent.name));
        ancestor = parent.parent;
    }

    let mut line = Line::from(Span::styled(fit(&trail, width), bold(INK)));
    if !tab.filter_query.is_empty() && width > line.width() + 3 {
        let available = width - line.width() - 3;
        line.spans.push(Span::styled(
            format!("  /{}", fit(&tab.filter_query, available)),
            Style::new().fg(FOCUS),
        ));
    }
    f.render_widget(Paragraph::new(line), left);
}

fn draw_treemap(f: &mut Frame, app: &App, area: Rect) {
    fill(f, area, MAT);
    let tab = app.current_tab();
    let now = SystemTime::now();
    let monochrome = Colored::ansi_color_disabled_memoized();

    if tab.treemap.tiles.len() <= 1 {
        let message = if app.is_scanning {
            "Scanning this folder…"
        } else if !tab.filter_query.is_empty() {
            "No matching items"
        } else {
            "No items with disk usage"
        };
        let row = Rect::new(area.x, area.y + area.height / 2, area.width, 1);
        f.render_widget(
            Paragraph::new(message)
                .style(Style::new().fg(MUTED))
                .alignment(Alignment::Center),
            row,
        );
        return;
    }

    for tile in tab.treemap.tiles.iter().skip(1) {
        if let Some(node) = app.tree.get(tile.node_id) {
            draw_tile(
                f,
                tile,
                node,
                tile.node_id == tab.selected_node_id,
                node.flash_until.is_some_and(|until| until > now),
                tab.color_mode,
                monochrome,
            );
        }
    }
}

fn draw_tile(
    f: &mut Frame,
    tile: &VisibleTile,
    node: &FileNode,
    selected: bool,
    flashing: bool,
    mode: ColorMode,
    monochrome: bool,
) {
    let area = tile.rect;
    let outline = (selected && area.width >= 5 && area.height >= 3)
        || (monochrome && area.width >= 3 && area.height >= 2);
    let compact_focus = selected && !outline;
    let background = if compact_focus {
        FOCUS
    } else if flashing {
        Color::Rgb(250, 219, 157)
    } else {
        tile_fill(tile, node.class, mode)
    };
    fill(f, area, background);
    if monochrome && (selected || flashing) && !outline {
        f.buffer_mut()
            .set_style(area, Style::new().add_modifier(Modifier::REVERSED));
    }
    if outline {
        let border_type = match (monochrome, selected, flashing) {
            (true, true, _) => BorderType::Thick,
            (true, false, true) => BorderType::Double,
            _ => BorderType::Plain,
        };
        f.render_widget(
            Block::new()
                .borders(Borders::ALL)
                .border_type(border_type)
                .border_style(Style::new().fg(FOCUS)),
            area,
        );
    }

    let tag_width = u16::from(node.tag.is_some());
    if let Some(tag) = node.tag {
        let (_, color) = get_tag_color(tag);
        let x = area.x + u16::from(outline);
        if x < area.right() {
            f.render_widget(
                Paragraph::new(Span::styled("▪", bold(color))),
                Rect::new(x, area.y, 1, 1),
            );
        }
    }

    if area.width < 8 || area.height < 2 {
        return;
    }

    let left = tag_width + 1;
    let width = area.width.saturating_sub(left + 1) as usize;
    if width == 0 {
        return;
    }
    let title_row = Rect::new(area.x + left, area.y, width as u16, 1);
    let size = format_size(tile.display_size);
    let title_fg = if compact_focus { PAPER } else { INK };
    let details_fg = if compact_focus {
        PAPER
    } else {
        Color::Rgb(65, 51, 41)
    };

    let label_str = tile.collapsed_label.as_deref().unwrap_or(&node.name);
    let mut title = Line::from(Span::styled(fit(label_str, width), bold(title_fg)));
    let inline_size = title.width() + 2 + size.as_str().width() <= width;
    if inline_size {
        title.spans.push(Span::styled(
            format!("  {size}"),
            Style::new().fg(details_fg),
        ));
    }
    if outline {
        let label_area = Rect::new(title_row.x, title_row.y, title.width() as u16, 1);
        f.render_widget(Clear, label_area);
        fill(f, label_area, background);
    }
    f.render_widget(Paragraph::new(title), title_row);

    if !inline_size && tile.is_leaf && area.height >= 4 && size.as_str().width() <= width {
        f.render_widget(
            Paragraph::new(Span::styled(size, Style::new().fg(details_fg))),
            Rect::new(title_row.x, title_row.y + 1, title_row.width, 1),
        );
    }
}

fn tile_fill(tile: &VisibleTile, class: FileClass, mode: ColorMode) -> Color {
    const SAND: [Color; 3] = [
        Color::Rgb(201, 164, 127),
        Color::Rgb(222, 191, 153),
        Color::Rgb(180, 145, 114),
    ];
    const BLUE: [Color; 3] = [
        Color::Rgb(141, 172, 196),
        Color::Rgb(168, 193, 210),
        Color::Rgb(129, 157, 181),
    ];
    let shades = if tile.is_dir { SAND } else { BLUE };

    match mode {
        ColorMode::Flat => shades[(tile.node_id + tile.depth) % shades.len()],
        ColorMode::Depth => shades[tile.depth.saturating_sub(1) % shades.len()],
        ColorMode::FileClasses => match class {
            FileClass::Archive | FileClass::DiskImage => Color::Rgb(207, 155, 135),
            FileClass::Media => Color::Rgb(176, 155, 192),
            FileClass::Document => Color::Rgb(164, 181, 142),
            FileClass::Code => Color::Rgb(145, 180, 177),
            FileClass::Binary => Color::Rgb(193, 148, 151),
            _ => shades[(tile.node_id + tile.depth) % shades.len()],
        },
    }
}

fn draw_selection(f: &mut Frame, app: &App, area: Rect) {
    fill(f, area, CHROME);
    let tab = app.current_tab();
    let Some(node) = app.tree.get(tab.selected_node_id) else {
        return;
    };

    let tile_opt = tab.treemap.tiles.iter().find(|t| t.node_id == node.id);
    let shown_size = tile_opt.map_or(node.size, |t| t.display_size);
    let size = format_size(shown_size);
    let mut summary = Line::from(Span::styled(size, bold(INK)));
    if area.width >= 44
        && node.id != tab.root_node_id
        && let Some(tile) = tile_opt
    {
        summary.spans.push(Span::styled(
            format!(" · {:.1}%", tile.share_of_parent * 100.0),
            Style::new().fg(MUTED),
        ));
    }
    if area.width >= 70 && node.is_dir() {
        summary.spans.push(Span::styled(
            format!(
                " · {} items",
                format_count(node.file_count + node.dir_count.saturating_sub(1))
            ),
            Style::new().fg(MUTED),
        ));
    }
    if let Some(tag) = node.tag {
        let (name, color) = get_tag_color(tag);
        let chip = format!(" {name} ");
        if area.width as usize > summary.width() + chip.as_str().width() + 6 {
            summary.spans.push(Span::raw("  "));
            summary
                .spans
                .push(Span::styled(chip, bold(PAPER).bg(color)));
        }
    }

    let (left, right) = split_right(area, summary.width() as u16);
    let path = node.path.to_string_lossy();
    let width = left.width.saturating_sub(2) as usize;
    let label = if path.as_ref().width() <= width {
        path.as_ref()
    } else {
        &node.name
    };
    let line = Line::from(vec![
        Span::styled("› ", bold(FOCUS)),
        Span::styled(fit(label, width), Style::new().fg(INK)),
    ]);
    f.render_widget(Paragraph::new(line), left);
    f.render_widget(Paragraph::new(summary), right);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let width = area.width as usize;
    let line = match app.input_mode {
        InputMode::Command | InputMode::Filter => {
            let (label, input, hint) = if app.input_mode == InputMode::Filter {
                ("/ ", &app.filter_input, "e.g. *.iso, >100M, tag:1")
            } else {
                (": ", &app.command_input, "e.g. :w report.txt")
            };
            f.render_widget(
                Paragraph::new(Span::styled(label, bold(FOCUS))),
                Rect::new(area.x, area.y, 2.min(area.width), 1),
            );
            let input_area = Rect::new(
                area.x.saturating_add(2).min(area.right()),
                area.y,
                area.width.saturating_sub(2),
                1,
            );
            let mut line = Line::from(vec![
                Span::styled(input, Style::new().fg(INK)),
                Span::styled("▏", bold(FOCUS)),
            ]);
            if line.width() > input_area.width as usize {
                line = line.right_aligned();
            } else if input.is_empty() && hint.width() + 5 <= width {
                line.spans
                    .push(Span::styled(format!("  {hint}"), Style::new().fg(MUTED)));
            }
            f.render_widget(Paragraph::new(line), input_area);
            return;
        }
        InputMode::Normal => {
            let (_, help) = split_right(area, 6);
            f.render_widget(Paragraph::new(shortcuts(&[("?", "Help")])), help);
            let available = width.saturating_sub(8);
            if let Some(msg) = &app.status_message {
                Line::from(Span::styled(fit(msg, available), Style::new().fg(INK)))
            } else {
                let keys = [
                    ("Enter", "Zoom in"),
                    ("Esc", "Zoom out"),
                    ("/", "Filter"),
                    ("Space", "Actions"),
                    ("+/−", "Detail"),
                    ("Tab", "Views"),
                ];
                let mut line = Line::default();
                for entry in keys {
                    if entry.0 == "Tab" && app.tabs.len() == 1 {
                        continue;
                    }
                    let hint = shortcuts(&[entry]);
                    let gap = if line.spans.is_empty() { 0 } else { 3 };
                    if line.width() + gap + hint.width() <= available {
                        if gap > 0 {
                            line.spans.push(Span::raw("   "));
                        }
                        line.spans.extend(hint.spans);
                    }
                }
                line
            }
        }
    };
    f.render_widget(Paragraph::new(line), area);
}

fn shortcuts(keys: &[(&str, &str)]) -> Line<'static> {
    let mut line = Line::default();
    for (index, (key, label)) in keys.iter().enumerate() {
        if index > 0 {
            line.spans.push(Span::raw("   "));
        }
        line.spans
            .push(Span::styled((*key).to_owned(), bold(FOCUS)));
        line.spans
            .push(Span::styled(format!(" {label}"), Style::new().fg(MUTED)));
    }
    line
}

fn draw_help(f: &mut Frame, area: Rect) {
    let lines = vec![
        shortcuts(&[("Arrows / h j k l", "Move selection")]),
        shortcuts(&[("Enter", "Zoom in"), ("Esc / Backspace / u", "Zoom out")]),
        shortcuts(&[("Home / gg", "Scan root"), ("+ / -", "Detail"), ("f", "Colors")]),
        shortcuts(&[("Tab / gt / gT", "Switch views")]),
        Line::default(),
        shortcuts(&[("/", "Filter"), (":", "Command"), ("Space / a", "Actions")]),
        shortcuts(&[("1–4", "Tag"), ("0 / x", "Clear tag"), ("i", "Properties")]),
        shortcuts(&[
            ("o", "Open"),
            ("e / v", "Edit"),
            ("t", "Terminal"),
            ("y", "Copy path"),
        ]),
        shortcuts(&[("r", "Rescan"), ("d", "Delete…"), ("q", "Quit")]),
        Line::default(),
        shortcuts(&[
            (":tabnew / :tabclose", "Manage views"),
            (":w <file>", "Export"),
        ]),
        shortcuts(&[("Esc / ?", "Close help")]),
    ];
    dialog(f, area, "Keyboard shortcuts", 62, lines, MUTED);
}

fn draw_actions(f: &mut Frame, app: &App, area: Rect) {
    let title = app
        .tree
        .get(app.current_tab().selected_node_id)
        .map_or("Actions", |node| &node.name);
    let actions = [
        ("1", "Open with default application"),
        ("2", "Open in file manager"),
        ("3", "Edit in $EDITOR"),
        ("4", "Open terminal here"),
        ("5", "Copy absolute path"),
        ("6", "Inspect properties"),
        ("7", "Rescan folder"),
        ("8", "Delete…"),
        ("Esc", "Close menu"),
    ];
    let lines = actions
        .into_iter()
        .map(|action| shortcuts(&[action]))
        .collect();
    dialog(f, area, title, 44, lines, MUTED);
}

fn draw_inspector(f: &mut Frame, app: &App, area: Rect) {
    let Some(node) = app.tree.get(app.current_tab().selected_node_id) else {
        return;
    };
    let modified = node
        .modified
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .and_then(|d| chrono::DateTime::from_timestamp(d.as_secs() as i64, 0))
        .map_or_else(
            || "Unknown".to_owned(),
            |t| t.format("%Y-%m-%d %H:%M:%S").to_string(),
        );
    let kind_label = match node.kind {
        FileKind::Directory => "Directory",
        FileKind::File => "File",
        FileKind::Symlink => "Symlink",
        FileKind::Other => "Special",
    };
    let fields = [
        ("Path", node.path.to_string_lossy().into_owned()),
        ("Type", kind_label.into()),
        ("Size", format_size(node.size)),
        ("Direct size", format!("{} bytes", node.direct_size)),
        ("Files", format_count(node.file_count)),
        ("Directories", format_count(node.dir_count)),
        ("Modified", modified),
        (
            "Tag",
            node.tag.map_or("None", |t| get_tag_color(t).0).to_owned(),
        ),
    ];
    let mut lines: Vec<_> = fields
        .into_iter()
        .map(|(name, value)| {
            Line::from(vec![
                Span::styled(format!("{name:<13}"), Style::new().fg(MUTED)),
                Span::raw(value),
            ])
        })
        .collect();
    lines.push(Line::default());
    lines.push(shortcuts(&[("Esc / i", "Close properties")]));
    dialog(f, area, &node.name, 64, lines, MUTED);
}

fn draw_confirm_delete(f: &mut Frame, path: &Path, area: Rect) {
    let lines = vec![
        Line::from("Delete or move this item to trash?"),
        Line::from(Span::styled(path.to_string_lossy(), bold(INK))),
        Line::default(),
        shortcuts(&[("y", "Confirm delete"), ("n / Esc", "Cancel")]),
    ];
    dialog(f, area, "Confirm deletion", 64, lines, DANGER);
}

fn dialog(f: &mut Frame, area: Rect, title: &str, width: u16, lines: Vec<Line<'_>>, accent: Color) {
    if area.width < 6 || area.height < 4 {
        return;
    }
    let width = width.min(area.width.saturating_sub(2)).max(6);
    let inner_width = width.saturating_sub(4).max(1);
    let content_rows: usize = lines
        .iter()
        .map(|line| line.width().div_ceil(inner_width as usize).max(1))
        .sum();
    let height = (content_rows + 4).min(area.height as usize) as u16;
    let popup = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let title = format!(" {} ", fit(title, inner_width as usize));
    let block = Block::new()
        .title(Span::styled(title, bold(accent)))
        .borders(Borders::ALL)
        .border_style(Style::new().fg(accent))
        .padding(Padding::uniform(1))
        .style(Style::new().fg(INK).bg(PAPER));
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(block),
        popup,
    );
}

fn fill(f: &mut Frame, area: Rect, color: Color) {
    f.render_widget(Block::new().style(Style::new().bg(color)), area);
}

fn bold(color: Color) -> Style {
    Style::new().fg(color).add_modifier(Modifier::BOLD)
}

fn split_right(area: Rect, width: u16) -> (Rect, Rect) {
    let width = width.min(area.width);
    let left = Rect::new(area.x, area.y, area.width.saturating_sub(width + 2), 1);
    let right = Rect::new(area.right() - width, area.y, width, 1);
    (left, right)
}

fn fit(value: &str, width: usize) -> Cow<'_, str> {
    if width == 0 {
        return Cow::Borrowed("");
    }
    if value.width() <= width {
        return Cow::Borrowed(value);
    }

    let mut remaining = width - 1;
    let mut result = String::new();
    for grapheme in Span::raw(value).styled_graphemes(Style::new()) {
        let Some(rest) = remaining.checked_sub(grapheme.symbol.width()) else {
            break;
        };
        remaining = rest;
        result.push_str(grapheme.symbol);
    }
    result.push('…');
    Cow::Owned(result)
}

fn format_count(value: usize) -> String {
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    grouped
}

pub fn get_tag_color(tag: u8) -> (&'static str, Color) {
    match tag {
        1 => ("Red", DANGER),
        2 => ("Green", Color::Rgb(63, 104, 61)),
        3 => ("Yellow", Color::Rgb(128, 89, 31)),
        4 => ("Blue", Color::Rgb(49, 93, 132)),
        _ => ("None", MUTED),
    }
}
