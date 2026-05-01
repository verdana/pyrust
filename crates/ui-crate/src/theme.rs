use yas_config::Theme;

pub struct ThemeColors {
    pub background: [f32; 4],
    pub text: [f32; 4],
    pub candidate_bg: [f32; 4],
    pub candidate_selected_bg: [f32; 4],
    pub pinyin_text: [f32; 4],
    pub border: [f32; 4],
}

impl ThemeColors {
    pub fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Light => Self::light(),
            Theme::Dark => Self::dark(),
            Theme::Auto => Self::light(), // default to light
        }
    }

    fn light() -> Self {
        Self {
            background: [0.18, 0.18, 0.18, 0.95],
            text: [1.0, 1.0, 1.0, 1.0],
            candidate_bg: [0.22, 0.22, 0.22, 1.0],
            candidate_selected_bg: [0.25, 0.35, 0.5, 1.0],
            pinyin_text: [0.7, 0.7, 0.7, 1.0],
            border: [0.35, 0.35, 0.35, 1.0],
        }
    }

    fn dark() -> Self {
        Self {
            background: [0.15, 0.15, 0.15, 0.95],
            text: [1.0, 1.0, 1.0, 1.0],
            candidate_bg: [0.2, 0.2, 0.2, 1.0],
            candidate_selected_bg: [0.25, 0.35, 0.5, 1.0],
            pinyin_text: [0.6, 0.6, 0.6, 1.0],
            border: [0.3, 0.3, 0.3, 1.0],
        }
    }
}
