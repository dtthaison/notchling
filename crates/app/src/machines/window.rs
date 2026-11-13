use gpui::{
    prelude::*, px, size, App, Application, Bounds, Context, TitlebarOptions, WindowBounds,
    WindowHandle, WindowKind, WindowOptions,
};

use super::cli::Command;
use super::data::Setup;
use super::model::Parts;
use super::renders::{WINDOW_MIN, WINDOW_SIZE};
use super::view::MachinesView;
use crate::dock::Policy;
use crate::mac::dock;

pub fn run_command(command: Command) -> i32 {
    match command {
        Command::Run {
            data_root,
            live,
            vault,
        } => match Setup::for_run(data_root.as_deref(), live, vault) {
            Some(setup) => run(setup),
            None => {
                eprintln!("notchling-machines: no home folder to read from");
                1
            }
        },
        Command::Render {
            view,
            out,
            data_root,
        } => crate::mac::render::run(&view, &out, data_root.as_deref()),
        Command::Version | Command::Help => 0,
    }
}

pub fn run(setup: Setup) -> i32 {
    if setup.live {
        eprintln!("notchling-machines: --live: ssh, gh and open may run");
    }
    if setup.vault {
        eprintln!("notchling-machines: --vault: the Secret Vault's values may be read");
    }
    let app = Application::new().with_assets(notchling_kit::ui::KitAssets);
    app.run(move |cx: &mut App| {
        dock::register(cx);
        dock::apply(Policy::Regular);
        dock::set_main_menu(cx, Policy::Regular);
        let parts = setup.parts();
        if open(cx, parts).is_none() {
            std::process::exit(1);
        }
        cx.activate(true);
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
    });
    0
}

pub fn open(cx: &mut App, parts: Parts) -> Option<WindowHandle<MachinesView>> {
    let bounds = Bounds::centered(None, size(px(WINDOW_SIZE.0), px(WINDOW_SIZE.1)), cx);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        titlebar: Some(TitlebarOptions {
            title: Some("Machines".into()),
            appears_transparent: false,
            traffic_light_position: None,
        }),
        window_min_size: Some(size(px(WINDOW_MIN.0), px(WINDOW_MIN.1))),
        kind: WindowKind::Normal,
        ..Default::default()
    };
    cx.open_window(options, |_, cx| {
        cx.new(|cx: &mut Context<MachinesView>| MachinesView::live(parts, cx))
    })
    .map_err(|e| eprintln!("open the Machines window: {e}"))
    .ok()
}
