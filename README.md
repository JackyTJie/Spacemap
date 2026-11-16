# Spacemap

Inspired by SpaceSniffer, bringing its intuitive Treemap disk visualization to the Linux terminal.

A high-performance, interactive terminal treemap disk usage analyzer written in **Rust**, replicating and enhancing the classic Windows **SpaceSniffer** experience directly within your Linux terminal.

---

## Core Mechanics Inspired by SpaceSniffer

Drawing inspiration from the visual mechanics and design of SpaceSniffer, Spacemap incorporates its core architecture and interaction principles:

1. **Squarified Treemap Layout**:
   - Hierarchical treemap model:
     - Directory containers (composite nodes with nested children)
     - File elements (leaf elements sized proportionally to byte length)
     - Free & unallocated space representation
   - Implements Mark Bruls, Kees Huizing, and Jarke J. van Wijk's **Squarified Treemap** algorithm, partitioning rectangular space while minimizing high aspect ratios.
2. **Visual Hierarchy & Styling**:
   - Signature warm amber / terracotta color scheme for directories with high-contrast borders.
   - File classification color mode categorizing files by MIME/extension (Media, Code, Archives, Documents, Binaries).
   - Dynamic drill-down & zooming: Single selection highlights, zoom navigation delves into any branch.
3. **Multi-Threaded Background Scanning**:
   - Disk scanning runs concurrently with UI rendering. Users can explore and navigate the partial tree immediately without waiting for the full disk scan to finish.
4. **Live Change Detection & Flashing**:
   - Hooks into OS filesystem events (e.g. inotify). When files are modified, created, or resized in the background, matching treemap blocks flash visibly.
5. **4-Color Tagging & Filtering**:
   - Temporary 4-color bookmarks (Red, Green, Yellow, Blue) to classify files during cleanup.
   - Filter query engine parsing name masks (`*.iso`), size bounds (`>500M`), and age (`<7d`).

---

## Terminal (TUI) Visual & Interactive Architecture

This Linux CLI version implements key SpaceSniffer features directly inside your terminal with **Ratatui** and **Crossterm**:

- **Terminal-Optimized Squarified Treemap**:
  - Automatically accounts for terminal character cell aspect ratio (columns are roughly half the physical width of rows, normalized via `CELL_ASPECT = 2.0`).
  - Classic sand-colored folders and steel-blue files, with solid tiles sized by disk usage.
  - Smaller tiles keep their area, while titles and nested folders appear when space permits. Select a tile for its path, size and share.
- **2D Spatial Navigation**:
  - Uses geometric proximity and angle weighting to jump smoothly between adjacent treemap blocks in 2D space.
- **Neovim-Inspired Interaction**:
  - Modal interaction (`NORMAL`, `COMMAND`, `FILTER`).
  - `h`/`j`/`k`/`l` and `Up`/`Down`/`Left`/`Right` arrow keys for spatial navigation.
  - `/` for instant filtering.
  - `:` for Neovim-style command line (`:w report.txt`, `:tabnew`, `:q`).
- **Explorer-Style Context Menu & Tool Launching**:
  - `Space` or `a` opens the action menu:
    - `1`: Open with default system app (`xdg-open`)
    - `2`: Open containing folder in file manager (Nautilus, Dolphin, etc.)
    - `3`: Open file/dir in `$EDITOR` / `nvim`
    - `4`: Open terminal shell in target directory (`$SHELL`)
    - `5`: Copy absolute path to clipboard (`arboard` / `wl-copy` / `xclip`)
    - `6`: Inspect file properties & metadata modal
    - `7`: Secondary rescan refinement of selected branch
    - `8`: Safe deletion with confirmation prompt
- **Live inotify Filesystem Tracking**:
  - Flashes updated blocks with bright amber/yellow when external processes write or delete files.
- **Multiple Views & Tabs**:
  - Open multiple tabs on the same scan tree with independent zoom levels and filters.
- **Custom Audit Text Export**:
  - `:w <filename>` or `:export <filename>` generates a comprehensive text report of the current zoomed/filtered view with top largest files and hierarchical tree.

---

## Keybindings Reference

| Key | Mode | Description |
|---|---|---|
| `Up` `Down` `Left` `Right` / `k` `j` `h` `l` | Normal | Navigate spatially across 2D treemap tiles |
| `Enter` | Normal | Zoom in: Expand selected folder to fill the view |
| `Esc` / `Backspace` / `u` | Normal | Zoom out: Return to upper level / parent directory |
| `Home` | Normal | Jump back to root view |
| `1`, `2`, `3`, `4` | Normal | Toggle SpaceSniffer 4-color tags (Red, Green, Yellow, Blue) |
| `0` / `x` | Normal | Clear tag on selected item |
| `+` / `-` | Normal | Show more / fewer nested tiles |
| `f` | Normal | Cycle Classic, File Types and Depth colors |
| `Space` / `a` | Normal | Open Explorer-style Actions Menu |
| `o` | Normal | Open item with default application (`xdg-open`) |
| `e` / `v` | Normal | Edit with `$EDITOR` (or `nvim`) |
| `t` | Normal | Launch terminal in selected directory |
| `y` | Normal | Copy absolute path to clipboard |
| `d` | Normal | Delete selected item (with confirmation modal) |
| `i` | Normal | Open file properties / metadata inspector |
| `r` | Normal | Secondary scan refinement on selected branch |
| `Tab` / `BackTab` | Normal | Switch between tabs / views |
| `/` | Normal | Open Filter Prompt (e.g. `*.iso`, `>500M`, `<7d`, `tag:1`) |
| `:` | Normal | Open Command-Line Mode |
| `?` | Normal | Open Help & Keybindings cheat sheet |
| `q` | Normal | Quit application |

---

## Build & Installation

### Requirements
- Linux (Ubuntu 20.04+)
- Rust & Cargo (1.75+)

### Build Release Binary
```bash
cargo build --release
```

The optimized binary is located at `target/release/spacemap`.

### Install to System PATH
```bash
mkdir -p ~/.local/bin
cp target/release/spacemap ~/.local/bin/spacemap
```

---

## Usage Examples

### 1. Analyze Current Directory
```bash
spacemap
```

### 2. Analyze a Specific Mount Point or Directory
```bash
spacemap /home/tj
spacemap /var/log
```

### 3. Neovim Commands Inside App
- `:w audit.txt` - Export detailed storage audit report to text file
- `:tabnew` - Create a new view on the current scan
- `:tabclose` - Close active view tab
- `:filter *.tar.gz;>100M` - Filter by pattern and size
- `:q` - Quit

---

## License

This project is licensed under the MIT License - see the LICENSE file for details.
