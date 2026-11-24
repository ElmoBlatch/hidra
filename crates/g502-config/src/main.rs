//! `g502-config`: readonly desktop viewer for Logitech G502 mice.

use std::path::PathBuf;

use clap::Parser;
use g502_config::{MiceWindow, Source};
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
    /// Start in the light theme (default: dark).
    #[arg(long)]
    light: bool,
}

fn main() {
    let cli = Cli::parse();
    let source = match cli.replay.clone() {
        Some(log) => Source::Replay { log, segment: cli.segment },
        None => Source::Hardware,
    };

    gpui_kit::application().with_assets(gpui_kit::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        g502_config::theme::init(cx);
        if cli.light {
            g502_config::theme::toggle(cx);
        }
        g502_config::init(cx);

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size(px(1180.), px(820.)), cx))),
            window_min_size: Some(size(px(760.), px(520.))),
            titlebar: Some(TitlebarOptions { title: Some(SharedString::from("G502 Config")), ..Default::default() }),
            window_decorations: Some(WindowDecorations::Server),
            app_id: Some("g502-config".into()),
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
