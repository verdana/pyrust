use crossbeam_channel::Receiver;
use eframe::egui;

use crate::renderer::EguiCandidateApp;
use crate::UiUpdate;
use yas_config::UiConfig;

/// Run the candidate window UI in the current thread.
///
/// This function blocks until the window is closed or an error occurs.
pub fn run_ui_window(config: UiConfig, receiver: Receiver<UiUpdate>) {
    log::info!("run_ui_window called, starting egui...");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(true)
            .with_always_on_top()
            .with_inner_size([400.0, 300.0])
            .with_resizable(true)
            .with_visible(true)
            .with_title("inputd 候选词"),
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
        "inputd-candidate",
        options,
        Box::new(move |cc| {
            log::info!("eframe creation callback called");
            setup_chinese_fonts(cc);
            Box::new(EguiCandidateApp::new(config, receiver)) as Box<dyn eframe::App>
        }),
    ) {
        log::error!("Failed to run UI window: {e}");
    }

    log::info!("eframe::run_native returned");
}

fn setup_chinese_fonts(cc: &eframe::CreationContext<'_>) {
    #[allow(unused_mut)]
    let mut fonts = egui::FontDefinitions::default();

    // Try to load Windows Chinese font at runtime
    #[cfg(windows)]
    {
        let font_paths = [
            "C:\\Windows\\Fonts\\msyh.ttc",      // 微软雅黑
            "C:\\Windows\\Fonts\\simhei.ttf",    // 黑体
            "C:\\Windows\\Fonts\\simsun.ttc",    // 宋体
        ];

        for path in &font_paths {
            if let Ok(data) = std::fs::read(path) {
                log::info!("Loaded Chinese font from: {}", path);
                fonts.font_data.insert(
                    "chinese".to_owned(),
                    egui::FontData::from_owned(data),
                );
                fonts
                    .families
                    .entry(egui::FontFamily::Proportional)
                    .or_default()
                    .insert(0, "chinese".to_owned());
                break;
            }
        }
    }

    cc.egui_ctx.set_fonts(fonts);
}
