use yas_config::Theme;

pub struct ThemeColors {
    pub background: [f32; 4],
    pub text: [f32; 4],
    pub pinyin_text: [f32; 4],
    pub index_color: [f32; 4],
    pub hover_bg: [f32; 4],
    pub separator: [f32; 4],
}

impl ThemeColors {
    pub fn for_theme(theme: Theme) -> Self {
        match theme {
            Theme::Light => Self::light(),
            Theme::Dark => Self::dark(),
            Theme::Auto => Self::light(),
        }
    }

    fn light() -> Self {
        Self {
            background: [1.0, 1.0, 1.0, 0.92],
            text: [0.10, 0.10, 0.10, 1.0],
            pinyin_text: [0.60, 0.60, 0.60, 1.0],
            index_color: [0.69, 0.69, 0.69, 1.0],
            hover_bg: [0.89, 0.93, 0.98, 1.0],
            separator: [0.90, 0.90, 0.90, 1.0],
        }
    }

    fn dark() -> Self {
        Self {
            background: [0.12, 0.12, 0.12, 0.90],
            text: [0.96, 0.96, 0.96, 1.0],
            pinyin_text: [0.53, 0.53, 0.53, 1.0],
            index_color: [0.40, 0.40, 0.40, 1.0],
            hover_bg: [0.16, 0.23, 0.31, 1.0],
            separator: [0.20, 0.20, 0.20, 1.0],
        }
    }
}

/// Convert [f32; 4] RGBA to Win32 COLORREF (0x00BBGGRR).
pub fn to_colorref(rgba: [f32; 4]) -> u32 {
    let r = (rgba[0] * 255.0) as u32;
    let g = (rgba[1] * 255.0) as u32;
    let b = (rgba[2] * 255.0) as u32;
    (b << 16) | (g << 8) | r
}
