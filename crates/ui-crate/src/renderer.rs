use crossbeam_channel::Receiver;
use egui::{Color32, RichText};
use eframe::App;

use crate::candidate_window::CandidateWindow;
use crate::theme::ThemeColors;
use crate::UiUpdate;
use yas_config::UiConfig;

pub struct EguiCandidateApp {
    state: CandidateWindow,
    receiver: Receiver<UiUpdate>,
    pinyin_color: Color32,
    text_color: Color32,
}

impl EguiCandidateApp {
    pub fn new(config: UiConfig, receiver: Receiver<UiUpdate>) -> Self {
        let theme = ThemeColors::for_theme(config.theme);
        let pinyin_color = to_color32(theme.pinyin_text);
        let text_color = to_color32(theme.text);
        Self {
            state: CandidateWindow::new(config),
            receiver,
            pinyin_color,
            text_color,
        }
    }

    fn render_candidates(&self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new(&self.state.pinyin)
                .color(self.pinyin_color)
                .size(14.0),
        );
        ui.separator();

        for (i, candidate) in self.state.page_candidates().iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{}.", i + 1))
                        .color(self.pinyin_color)
                        .size(14.0),
                );
                ui.label(
                    RichText::new(&candidate.text)
                        .color(self.text_color)
                        .size(18.0),
                );
            });
        }
    }
}

impl App for EguiCandidateApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut had_update = false;
        while let Ok(update) = self.receiver.try_recv() {
            self.state.apply_update(update);
            had_update = true;
        }

        // Only repaint frequently when updates are arriving; slow poll when idle
        let interval = if had_update {
            std::time::Duration::from_millis(50)
        } else {
            std::time::Duration::from_millis(200)
        };
        ctx.request_repaint_after(interval);

        if self.state.visible {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_candidates(ui);
        });
    }
}

fn to_color32(rgba: [f32; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (rgba[0] * 255.0) as u8,
        (rgba[1] * 255.0) as u8,
        (rgba[2] * 255.0) as u8,
        (rgba[3] * 255.0) as u8,
    )
}
