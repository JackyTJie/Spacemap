use std::error::Error;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, MouseButton, MouseEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Position, Rect},
};

use spacemap::app::{App, InputMode, Modal};
use spacemap::ui::{draw, treemap_area};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("Spacemap - Interactive Terminal Treemap Disk Usage Analyzer");
        println!();
        println!("USAGE:");
        println!("    spacemap [PATH]");
        println!();
        println!("ARGS:");
        println!("    <PATH>    Path to analyze [default: current directory]");
        println!();
        println!("KEYBINDINGS (Neovim-inspired):");
        println!("    Up/Down/Left/Right or h/j/k/l  Move selection across treemap blocks spatially");
        println!("    Enter               Zoom into selected directory (drill-down)");
        println!("    Esc / Backspace / u Zoom out to parent directory / level");
        println!("    Home                Return to root view");
        println!("    + / -               Show more / fewer nested tiles");
        println!("    f                   Cycle colors (Classic, File Types, Depth)");
        println!(
            "    1, 2, 3, 4          Toggle 4-color tags (Red, Green, Yellow, Blue)"
        );
        println!("    0 or x              Clear tag on selected item");
        println!("    Space or a          Open Explorer-style action menu / context tools");
        println!("    o                   Open with default system application (xdg-open)");
        println!("    e or v              Open in editor ($EDITOR or nvim)");
        println!("    t                   Open terminal in directory ($SHELL)");
        println!("    y                   Copy absolute path to clipboard");
        println!("    d                   Delete / trash selected item (with confirmation)");
        println!("    i                   Inspect file properties modal");
        println!("    r                   Secondary scan refinement on current folder");
        println!("    /                   Search / Filter prompt (*.iso, >500M, <7d, tag:1)");
        println!("    :                   Neovim command-line (:w <file>, :tabnew, :tabclose, :q)");
        println!("    ?                   Show help and cheat sheet modal");
        println!();
        return Ok(());
    }

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("spacemap 0.1.0 (Ubuntu x86_64)");
        return Ok(());
    }

    let root_path = if args.len() > 1 && !args[1].starts_with('-') {
        PathBuf::from(&args[1])
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    };

    if !root_path.exists() {
        eprintln!("Error: Path '{}' does not exist.", root_path.display());
        std::process::exit(1);
    }

    // Terminal initialization
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let app = App::new(root_path);
    let res = run_app(&mut terminal, app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Application Error: {:?}", err);
    }

    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, mut app: App) -> io::Result<()> {
    let tick_rate = Duration::from_millis(30);
    let mut last_tick = Instant::now();
    let mut last_click = None;

    loop {
        // App update for scanner messages and background events
        app.update();

        // Render UI
        terminal.draw(|f| {
            let canvas_rect = treemap_area(f.area());
            if app.last_layout_rect != canvas_rect {
                app.recompute_layout(canvas_rect);
            }
            draw(f, &app);
        })?;

        if app.should_quit {
            app.scanner_controller.stop();
            break;
        }

        let timeout = tick_rate
            .checked_sub(last_tick.elapsed())
            .unwrap_or_else(|| Duration::from_millis(0));

        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => {
                    last_click = None;
                    app.handle_key(key);
                }
                Event::Mouse(mouse_event)
                    if app.modal.is_none() && app.input_mode == InputMode::Normal =>
                {
                    let MouseEventKind::Down(button @ (MouseButton::Left | MouseButton::Right)) =
                        mouse_event.kind
                    else {
                        continue;
                    };
                    let pos = Position::new(mouse_event.column, mouse_event.row);
                    let Some(node_id) = app.current_tab().treemap.node_at(pos) else {
                        last_click = None;
                        continue;
                    };
                    app.current_tab_mut().selected_node_id = node_id;

                    if button == MouseButton::Left {
                        let now = Instant::now();
                        let double_click = last_click.is_some_and(|(time, id)| {
                            id == node_id && now.duration_since(time) < Duration::from_millis(400)
                        });
                        if double_click && app.tree.get(node_id).is_some_and(|node| node.is_dir()) {
                            app.current_tab_mut().zoom_into(node_id);
                            app.recompute_layout(app.last_layout_rect);
                            last_click = None;
                        } else {
                            last_click = Some((now, node_id));
                        }
                    } else {
                        app.modal = Some(Modal::ActionMenu);
                        last_click = None;
                    }
                }
                Event::Resize(_, _) => {
                    last_click = None;
                    app.last_layout_rect = Rect::default(); // Force recompute
                }
                _ => {}
            }
        }

        if last_tick.elapsed() >= tick_rate {
            last_tick = Instant::now();
        }
    }

    Ok(())
}
