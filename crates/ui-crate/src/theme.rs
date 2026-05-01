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

    /// WeChat-style light theme: white bg, dark text, light blue hover
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

    /// WeChat-style dark theme: dark bg, light text, dark blue hover
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

pub fn to_color32(rgba: [f32; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        (rgba[0] * 255.0) as u8,
        (rgba[1] * 255.0) as u8,
        (rgba[2] * 255.0) as u8,
        (rgba[3] * 255.0) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_theme_has_light_background() {
        let t = ThemeColors::light();
        assert!(t.background[0] > 0.9);
        assert!(t.background[1] > 0.9);
        assert!(t.background[2] > 0.9);
    }

    #[test]
    fn dark_theme_has_dark_background() {
        let t = ThemeColors::dark();
        assert!(t.background[0] < 0.2);
        assert!(t.background[1] < 0.2);
        assert!(t.background[2] < 0.2);
    }

    #[test]
    fn light_theme_has_dark_text() {
        let t = ThemeColors::light();
        assert!(t.text[0] < 0.15);
        assert!(t.text[1] < 0.15);
        assert!(t.text[2] < 0.15);
    }

    #[test]
    fn dark_theme_has_light_text() {
        let t = ThemeColors::dark();
        assert!(t.text[0] > 0.9);
        assert!(t.text[1] > 0.9);
        assert!(t.text[2] > 0.9);
    }

    #[test]
    fn for_theme_light_returns_light() {
        let t = ThemeColors::for_theme(Theme::Light);
        assert!(t.background[0] > 0.9);
    }

    #[test]
    fn for_theme_dark_returns_dark() {
        let t = ThemeColors::for_theme(Theme::Dark);
        assert!(t.background[0] < 0.2);
    }

    #[test]
    fn for_theme_auto_defaults_to_light() {
        let t = ThemeColors::for_theme(Theme::Auto);
        assert!(t.background[0] > 0.9);
    }

    #[test]
    fn all_colors_in_valid_range() {
        for theme in &[ThemeColors::light(), ThemeColors::dark()] {
            for field in &[theme.background, theme.text, theme.pinyin_text, theme.index_color, theme.hover_bg, theme.separator] {
                for &c in field.iter() {
                    assert!(c >= 0.0 && c <= 1.0, "color value {c} out of range");
                }
            }
        }
    }

    #[test]
    fn light_vs_dark_are_different() {
        let light = ThemeColors::light();
        let dark = ThemeColors::dark();
        assert!(light.background != dark.background);
        assert!(light.text != dark.text);
    }

    #[test]
    fn hover_bg_contrasts_against_background() {
        for theme in &[ThemeColors::light(), ThemeColors::dark()] {
            let bg_luma = theme.background[0] * 0.299 + theme.background[1] * 0.587 + theme.background[2] * 0.114;
            let hover_luma = theme.hover_bg[0] * 0.299 + theme.hover_bg[1] * 0.587 + theme.hover_bg[2] * 0.114;
            assert!((bg_luma - hover_luma).abs() > 0.05, "hover must contrast against background");
        }
    }

    #[test]
    fn to_color32_produces_valid_color() {
        let c = to_color32([1.0, 0.0, 0.5, 0.8]);
        assert!(c.a() > 0);
    }

    #[test]
    fn to_color32_black_transparent() {
        let c = to_color32([0.0, 0.0, 0.0, 0.0]);
        assert_eq!(c, egui::Color32::TRANSPARENT);
    }
}
