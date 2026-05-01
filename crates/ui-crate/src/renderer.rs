use crossbeam_channel::{Receiver, Sender};
use egui::{Color32, RichText, Rounding, Sense};
use eframe::App;

use crate::candidate_window::CandidateWindow;
use crate::theme::{to_color32, ThemeColors};
use crate::{UiAction, UiUpdate};
use yas_config::UiConfig;

pub struct EguiCandidateApp {
    state: CandidateWindow,
    receiver: Receiver<UiUpdate>,
    action_tx: Sender<UiAction>,
    background_color: Color32,
    pinyin_color: Color32,
    text_color: Color32,
    index_color: Color32,
    hover_color: Color32,
    font_size: f32,
}

impl EguiCandidateApp {
    pub fn new(
        config: UiConfig,
        receiver: Receiver<UiUpdate>,
        action_tx: Sender<UiAction>,
    ) -> Self {
        let theme = ThemeColors::for_theme(config.theme);
        let font_size = config.font_size.max(10) as f32;
        Self {
            state: CandidateWindow::new(config),
            receiver,
            action_tx,
            background_color: to_color32(theme.background),
            pinyin_color: to_color32(theme.pinyin_text),
            text_color: to_color32(theme.text),
            index_color: to_color32(theme.index_color),
            hover_color: to_color32(theme.hover_bg),
            font_size,
        }
    }

    fn render_candidates(&mut self, ui: &mut egui::Ui) {
        let pinyin_size = (self.font_size - 2.0).max(11.0);
        let index_size = (self.font_size - 1.0).max(11.0);

        // Pinyin row — dimmed, above candidates
        if !self.state.pinyin.is_empty() {
            ui.label(
                RichText::new(&self.state.pinyin)
                    .color(self.pinyin_color)
                    .size(pinyin_size),
            );
            ui.add_space(6.0);
            // Subtle separator line
            ui.separator();
            ui.add_space(4.0);
        }

        let candidates = self.state.page_candidates();
        if candidates.is_empty() {
            return;
        }

        let mut new_hover: Option<usize> = None;

        ui.horizontal(|ui| {
            for (i, candidate) in candidates.iter().enumerate() {
                let global_idx = self.state.page * self.state.per_page() + i;

                // Build rich text: dim index + normal text
                let index_str = format!("{}.", i + 1);
                let display = format!("{}.{}", i + 1, candidate.text);

                // Clickable label
                let resp = ui.add(
                    egui::Label::new(
                        RichText::new(&display)
                            .color(self.text_color)
                            .size(self.font_size),
                    )
                    .sense(Sense::click()),
                );

                // Paint dimmed index number on top
                let label_rect = resp.rect;
                let index_pos = label_rect.left_top();
                ui.painter().text(
                    index_pos,
                    egui::Align2::LEFT_TOP,
                    &index_str,
                    egui::FontId::proportional(index_size),
                    self.index_color,
                );

                if resp.hovered() {
                    new_hover = Some(global_idx);
                    // Hover background (painted first, so it's behind text)
                    let hover_rect = label_rect.expand2(egui::vec2(6.0, 4.0));
                    ui.painter().rect_filled(
                        hover_rect,
                        Rounding::same(8.0),
                        self.hover_color,
                    );
                    // Re-paint text on top of hover
                    ui.painter().text(
                        index_pos,
                        egui::Align2::LEFT_TOP,
                        &index_str,
                        egui::FontId::proportional(index_size),
                        self.index_color,
                    );
                    ui.painter().text(
                        egui::pos2(index_pos.x + ui.painter().layout_no_wrap(index_str.clone(), egui::FontId::proportional(index_size), self.index_color).size().x, index_pos.y),
                        egui::Align2::LEFT_TOP,
                        &candidate.text,
                        egui::FontId::proportional(self.font_size),
                        self.text_color,
                    );
                }

                if resp.clicked() {
                    let _ = self.action_tx.send(UiAction::SelectCandidate(global_idx));
                }

                // Spacing between candidates
                ui.add_space(16.0);
            }
        });

        self.state.set_hovered(new_hover);
    }

    fn frame(&self) -> egui::Frame {
        egui::Frame::default()
            .rounding(Rounding::same(16.0))
            .fill(self.background_color)
            .inner_margin(egui::Margin::symmetric(18.0, 10.0))
    }
}

impl App for EguiCandidateApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(update) = self.receiver.try_recv() {
            self.state.apply_update(update);
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(50));

        let has_content = self.state.visible && !self.state.candidates.is_empty();

        if has_content {
            let candidate_count = self.state.page_candidates().len().max(1) as f32;
            let width = (candidate_count * self.font_size * 3.8 + 40.0).clamp(150.0, 900.0);
            let pinyin_height = if self.state.pinyin.is_empty() { 0.0 } else { self.font_size + 18.0 };
            let height = pinyin_height + self.font_size + 28.0;
            ctx.send_viewport_cmd(egui::viewport::ViewportCommand::InnerSize(
                egui::vec2(width, height),
            ));

            egui::CentralPanel::default()
                .frame(self.frame())
                .show(ctx, |ui| {
                    self.render_candidates(ui);
                });
        } else {
            // Keep window tiny when idle; decorations/transparent toggle can't
            // be done reliably, so we just make it near-invisible.
            ctx.send_viewport_cmd(egui::viewport::ViewportCommand::InnerSize(
                egui::vec2(1.0, 1.0),
            ));
        }
    }
}
