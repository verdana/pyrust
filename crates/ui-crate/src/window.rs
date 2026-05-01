use crossbeam_channel::{Receiver, Sender};
use eframe::egui;

use crate::renderer::EguiCandidateApp;
use crate::{UiAction, UiUpdate};
use yas_config::UiConfig;

/// Run the candidate window UI in the current thread.
/// Blocks until the window is closed or an error occurs.
pub fn run_ui_window(
    config: UiConfig,
    receiver: Receiver<UiUpdate>,
    action_tx: Sender<UiAction>,
) {
    log::info!("run_ui_window called, starting egui...");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_always_on_top()
            .with_resizable(false)
            .with_visible(true)
            .with_taskbar(false)
            .with_inner_size([400.0, 60.0])
            .with_title("pyrust candidate"),
        event_loop_builder: Some(Box::new(|builder| {
            #[cfg(windows)]
            {
                use winit::platform::windows::EventLoopBuilderExtWindows;
                builder.with_any_thread(true);
            }
            #[cfg(not(windows))]
            let _ = builder;
        })),
        ..Default::default()
    };

    log::info!("Calling eframe::run_native...");

    if let Err(e) = eframe::run_native(
        "pyrust-candidate",
        options,
        Box::new(move |cc| {
            log::info!("eframe creation callback called");
            setup_chinese_fonts(cc, &config);
            Box::new(EguiCandidateApp::new(config, receiver, action_tx)) as Box<dyn eframe::App>
        }),
    ) {
        log::error!("Failed to run UI window: {e}");
    }

    log::info!("eframe::run_native returned");
}

fn setup_chinese_fonts(cc: &eframe::CreationContext<'_>, config: &UiConfig) {
    let mut fonts = egui::FontDefinitions::default();

    // If user specified a font family, try to load it first
    let mut loaded = false;
    if !config.font_family.is_empty() {
        loaded = try_load_font(&mut fonts, &config.font_family);
        if !loaded {
            log::warn!("Failed to load configured font '{}', falling back", config.font_family);
        }
    }

    // Fall back to system fonts
    if !loaded {
        #[cfg(windows)]
        {
            for family in &["msyh.ttc", "simhei.ttf", "simsun.ttc"] {
                if try_load_font(&mut fonts, family) {
                    break;
                }
            }
        }
    }

    // Adjust font size
    if config.font_size > 0 {
        for (_, font_data) in fonts.font_data.iter_mut() {
            font_data.tweak.scale = config.font_size as f32 / 18.0;
        }
    }

    cc.egui_ctx.set_fonts(fonts);
}

fn try_load_font(fonts: &mut egui::FontDefinitions, family: &str) -> bool {
    #[cfg(windows)]
    {
        // Try without path prefix first (just the filename)
        let paths = [
            format!("C:\\Windows\\Fonts\\{}", family),
            family.to_string(),
        ];
        for path in &paths {
            if let Ok(data) = std::fs::read(path) {
                log::info!("Loaded font: {}", path);
                fonts.font_data.insert(
                    "chinese".to_owned(),
                    egui::FontData::from_owned(data),
                );
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "chinese".to_owned());
                return true;
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = fonts;
        let _ = family;
    }
    false
}
