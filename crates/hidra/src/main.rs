//! `hidra`: readonly desktop viewer for Logitech G502 mice.

// A release build on Windows opens no console window beside the viewer.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use std::path::PathBuf;

use clap::Parser;
use hidra::{MiceWindow, Source};
use gpui_kit::{
    AppContext as _, Bounds, SharedString, TitlebarOptions, WindowBounds, WindowDecorations, WindowOptions, px, size,
};

#[derive(Parser)]
#[command(version, about = "Readonly desktop viewer for Logitech G502 mice")]
struct Cli {
    /// Show devices from a `ratbagd --verbose=raw` capture instead of hardware.
    #[arg(long)]
    replay: Option<PathBuf>,
    /// Device name within the replay capture (default: every device in it).
    #[arg(long, requires = "replay")]
    segment: Option<String>,
    /// Use a simulated G502 Proteus Spectrum instead of hardware. Edits are
    /// kept in memory only, so this is safe to play with.
    #[arg(long, conflicts_with = "replay")]
    demo: bool,
    /// Start in the light theme (default: dark).
    #[arg(long)]
    light: bool,
}

fn main() {
    let cli = Cli::parse();
    let source = match (cli.demo, cli.replay.clone()) {
        (true, _) => match Source::demo() {
            Ok(source) => source,
            Err(e) => {
                eprintln!("hidra: couldn't start demo mode: {e}");
                std::process::exit(1);
            }
        },
        (false, Some(log)) => Source::Replay { log, segment: cli.segment },
        (false, None) => Source::Hardware,
    };

    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        hidra::theme::init(cx);
        if cli.light {
            hidra::theme::toggle(cx);
        }
        hidra::init(cx);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1440.), px(900.)), cx))),
            window_min_size: Some(size(px(760.), px(520.))),
            titlebar: Some(TitlebarOptions { title: Some(SharedString::from("hidra")), ..Default::default() }),
            window_decorations: Some(WindowDecorations::Server),
            app_id: Some("hidra".into()),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| MiceWindow::new(source, window, cx))
        })
        .expect("failed to open the window");

        // Closing the only window ends the application.
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
}
