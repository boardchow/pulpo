use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, RwLock};

use ratatui::style::Color;
use syntect::highlighting::{FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings};

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Palette {
    bg: u32,
    editor_bg: u32,
    border: u32,
    accent: u32,
    on_accent: u32,
    fg: u32,
    muted: u32,
    dim: u32,
    selection: u32,
    hover: u32,
    ansi: [u32; 16],
    text_selection: u32,
}

const PULPO_ANSI: [u32; 16] = [
    0x232429, 0xEB5757, 0x4CB782, 0xFCC581, 0x8FA4FF, 0xF1ADE0, 0x58EBDD, 0xD4D4D4, 0x595A5C, 0xF2777A, 0x6FD3A0, 0xFDD6A3, 0x94A8FF,
    0xF5C4EA, 0x55CDFF, 0xFFFFFF,
];

const PULPO_DARK: Palette = Palette {
    bg: 0x090909,
    editor_bg: 0x101012,
    border: 0x232429,
    accent: 0x5E6AD2,
    on_accent: 0xFFFFFF,
    fg: 0xE3E4E7,
    muted: 0x97979A,
    dim: 0x595A5C,
    selection: 0x1E2025,
    hover: 0x16171A,
    ansi: PULPO_ANSI,
    text_selection: mix(0x5E6AD2, 0x101012, 0x70),
};

impl Palette {
    fn from_terminal(bg: u32, fg: u32, ansi: [u32; 16]) -> Palette {
        if (bg, fg, ansi) == (PULPO_DARK.bg, PULPO_DARK.fg, PULPO_DARK.ansi) {
            return PULPO_DARK;
        }
        let accent = ansi[4];
        let editor_bg = mix(fg, bg, 8);
        Palette {
            bg,
            editor_bg,
            border: mix(fg, bg, 34),
            accent,
            on_accent: if luminance(accent) > 0.55 { bg } else { 0xFFFFFF },
            fg,
            muted: mix(fg, bg, 166),
            dim: mix(fg, bg, 94),
            selection: mix(accent, bg, 48),
            hover: mix(fg, bg, 16),
            ansi,
            text_selection: mix(accent, editor_bg, 0x70),
        }
    }
}

const fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn mix(fg: u32, bg: u32, alpha: u32) -> u32 {
    const fn channel(fg: u32, bg: u32, shift: u32, alpha: u32) -> u32 {
        let (f, b) = ((fg >> shift) & 0xFF, (bg >> shift) & 0xFF);
        (f * alpha + b * (255 - alpha)) / 255
    }
    (channel(fg, bg, 16, alpha) << 16) | (channel(fg, bg, 8, alpha) << 8) | channel(fg, bg, 0, alpha)
}

fn luminance(hex: u32) -> f32 {
    let channel = |shift: u32| ((hex >> shift) & 0xFF) as f32 / 255.0;
    0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
}

static CURRENT: RwLock<Palette> = RwLock::new(PULPO_DARK);
static SYNTAX: RwLock<Option<Arc<Theme>>> = RwLock::new(None);

fn palette() -> Palette {
    *CURRENT.read().unwrap_or_else(|e| e.into_inner())
}

fn set(palette: Palette) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = palette;
    *SYNTAX.write().unwrap_or_else(|e| e.into_inner()) = None;
}

pub fn bg() -> Color {
    rgb(palette().bg)
}
pub fn editor_bg() -> Color {
    rgb(palette().editor_bg)
}
pub fn border() -> Color {
    rgb(palette().border)
}
pub fn accent() -> Color {
    rgb(palette().accent)
}
pub fn on_accent() -> Color {
    rgb(palette().on_accent)
}
pub fn fg() -> Color {
    rgb(palette().fg)
}
pub fn muted() -> Color {
    rgb(palette().muted)
}
pub fn dim() -> Color {
    rgb(palette().dim)
}
pub fn selection() -> Color {
    rgb(palette().selection)
}
pub fn hover() -> Color {
    rgb(palette().hover)
}
pub fn blue() -> Color {
    rgb(palette().ansi[4])
}
pub fn green() -> Color {
    rgb(palette().ansi[2])
}
pub fn red() -> Color {
    rgb(palette().ansi[1])
}
pub fn yellow() -> Color {
    rgb(palette().ansi[3])
}
pub fn magenta() -> Color {
    rgb(palette().ansi[5])
}
pub fn sky() -> Color {
    rgb(palette().ansi[14])
}
pub fn added_bg() -> Color {
    let p = palette();
    rgb(mix(p.ansi[2], p.editor_bg, 0x26))
}
pub fn removed_bg() -> Color {
    let p = palette();
    rgb(mix(p.ansi[1], p.editor_bg, 0x26))
}
pub fn find_bg() -> Color {
    let p = palette();
    rgb(mix(p.ansi[3], p.editor_bg, 0x55))
}
pub fn mark_bg() -> Color {
    let p = palette();
    rgb(mix(p.accent, p.editor_bg, 0x4D))
}
pub fn text_selection() -> Color {
    rgb(palette().text_selection)
}
pub fn ansi() -> [Color; 16] {
    palette().ansi.map(rgb)
}

#[derive(Clone, Copy)]
enum Ink {
    Fg,
    Soft,
    Dim,
    Ansi(usize),
}

const ITALIC: u8 = 1;
const BOLD: u8 = 2;

/// Pulpo Dark's tokenColors, as its terminal colors, which is how other themes get theirs.
const TOKENS: &[(&str, Ink, u8)] = &[
    ("comment, punctuation.definition.comment", Ink::Dim, ITALIC),
    (
        "keyword, keyword.control, storage, storage.type, storage.modifier, keyword.operator.new, \
         keyword.operator.expression, keyword.operator.logical.python, variable.language.this, \
         variable.language.self, variable.language.super, support.function.kernel.ruby, keyword.other.special-method",
        Ink::Ansi(6),
        0,
    ),
    (
        "keyword.operator, punctuation, meta.brace, punctuation.separator, punctuation.terminator, \
         punctuation.definition.string, punctuation.definition.variable, punctuation.section",
        Ink::Soft,
        0,
    ),
    (
        "entity.name.type, entity.name.class, entity.name.namespace, entity.other.inherited-class, support.class, \
         support.type, entity.name.type.module, storage.type.java, storage.type.cs",
        Ink::Ansi(14),
        0,
    ),
    ("constant, variable.other.constant, constant.language, support.constant, variable.other.enummember, constant.other.symbol", Ink::Ansi(12), 0),
    ("constant.numeric", Ink::Ansi(12), 0),
    ("string, string.quoted, string.template, string.regexp, markup.inline.raw", Ink::Ansi(5), 0),
    ("constant.character.escape, punctuation.section.embedded, punctuation.definition.template-expression", Ink::Ansi(6), 0),
    ("entity.name.function, support.function, meta.function-call entity.name.function, variable.function, entity.name.method", Ink::Ansi(3), 0),
    (
        "variable.parameter, meta.parameter, variable.other.readwrite.instance, variable.other.readwrite.class, \
         variable.other.readwrite.global, variable.other.property, meta.object-literal.key, support.type.property-name",
        Ink::Ansi(4),
        0,
    ),
    ("variable, variable.other, variable.other.readwrite", Ink::Fg, 0),
    ("entity.name.tag", Ink::Ansi(6), 0),
    ("entity.other.attribute-name", Ink::Ansi(4), 0),
    ("meta.decorator, entity.name.function.decorator, punctuation.decorator", Ink::Ansi(3), 0),
    ("markup.heading, entity.name.section", Ink::Ansi(14), BOLD),
    ("markup.underline.link, string.other.link", Ink::Ansi(4), 0),
    ("markup.inserted", Ink::Ansi(2), 0),
    ("markup.deleted", Ink::Ansi(1), 0),
    ("invalid, invalid.illegal", Ink::Ansi(1), 0),
];

pub fn syntax() -> Arc<Theme> {
    if let Some(theme) = SYNTAX.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return theme.clone();
    }
    let theme = Arc::new(build_syntax(&palette()));
    *SYNTAX.write().unwrap_or_else(|e| e.into_inner()) = Some(theme.clone());
    theme
}

fn build_syntax(p: &Palette) -> Theme {
    let color = |hex: u32| syntect::highlighting::Color { r: (hex >> 16) as u8, g: (hex >> 8) as u8, b: hex as u8, a: 0xFF };
    let ink = |ink: Ink| match ink {
        Ink::Fg => p.fg,
        // Pulpo Dark's punctuation is its color 7; other themes fade their foreground.
        Ink::Soft if *p == PULPO_DARK => p.ansi[7],
        Ink::Soft => mix(p.fg, p.bg, 235),
        Ink::Dim => p.dim,
        Ink::Ansi(i) => p.ansi[i],
    };
    let scopes = TOKENS
        .iter()
        .filter_map(|&(scope, fg, style)| {
            let mut font_style = FontStyle::empty();
            if style & ITALIC != 0 {
                font_style |= FontStyle::ITALIC;
            }
            if style & BOLD != 0 {
                font_style |= FontStyle::BOLD;
            }
            Some(ThemeItem {
                scope: ScopeSelectors::from_str(scope).ok()?,
                style: StyleModifier { foreground: Some(color(ink(fg))), background: None, font_style: Some(font_style) },
            })
        })
        .collect();
    Theme {
        name: Some("Pulpo".into()),
        settings: ThemeSettings { foreground: Some(color(p.fg)), background: Some(color(p.editor_bg)), ..ThemeSettings::default() },
        scopes,
        ..Theme::default()
    }
}

pub const FOLLOW: &str = "ghostty";
pub const PULPO: &str = "Pulpo Dark";
pub fn apply(name: &str) -> Result<(), String> {
    set(load(name)?);
    Ok(())
}

fn load(name: &str) -> Result<Palette, String> {
    match name {
        PULPO => Ok(PULPO_DARK),
        FOLLOW => ghostty_theme().map_or(Ok(PULPO_DARK), |theme| from_ghostty(&theme)),
        theme => from_ghostty(theme),
    }
}

fn from_ghostty(theme: &str) -> Result<Palette, String> {
    let path = find(theme).ok_or_else(|| format!("no Ghostty theme named {theme:?}"))?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&text).ok_or_else(|| format!("{}: not a Ghostty theme", path.display()))
}

/// A Ghostty theme file: `background`, `foreground`, `palette = N=#hex`; missing colors are xterm's.
fn parse(text: &str) -> Option<Palette> {
    const XTERM: [u32; 16] = [
        0x000000, 0xCD0000, 0x00CD00, 0xCDCD00, 0x0000EE, 0xCD00CD, 0x00CDCD, 0xE5E5E5, 0x7F7F7F, 0xFF0000, 0x00FF00, 0xFFFF00, 0x5C5CFF,
        0xFF00FF, 0x00FFFF, 0xFFFFFF,
    ];
    let hex = |v: &str| u32::from_str_radix(v.trim().trim_start_matches('#'), 16).ok();
    let (mut bg, mut fg, mut ansi) = (None, None, XTERM);
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        match key.trim() {
            "background" => bg = hex(value),
            "foreground" => fg = hex(value),
            "palette" => {
                if let Some((i, color)) = value.split_once('=')
                    && let (Ok(i), Some(color)) = (i.trim().parse::<usize>(), hex(color))
                    && i < 16
                {
                    ansi[i] = color;
                }
            }
            _ => {}
        }
    }
    Some(Palette::from_terminal(bg?, fg?, ansi))
}

/// Ghostty's `theme`; a `light:X,dark:Y` pair picks by the macOS appearance.
pub fn ghostty_theme() -> Option<String> {
    let mut theme = None;
    for path in crate::doctor::ghostty_configs() {
        for line in std::fs::read_to_string(path).unwrap_or_default().lines() {
            if let Some((key, value)) = line.split_once('=')
                && key.trim() == "theme"
            {
                theme = Some(value.trim().trim_matches('"').to_string());
            }
        }
    }
    let theme = theme.filter(|t| !t.is_empty())?;
    if !theme.contains(':') {
        return Some(theme);
    }
    let dark = std::process::Command::new("defaults")
        .args(["read", "-g", "AppleInterfaceStyle"])
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "Dark");
    let want = if dark { "dark" } else { "light" };
    theme.split(',').find_map(|part| part.split_once(':').filter(|(k, _)| k.trim() == want).map(|(_, v)| v.trim().to_string()))
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

fn user_dirs() -> Vec<PathBuf> {
    let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"));
    vec![xdg.join("ghostty/themes"), home().join("Library/Application Support/com.mitchellh.ghostty/themes")]
}

fn bundled_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::env::var_os("GHOSTTY_RESOURCES_DIR").map(|d| PathBuf::from(d).join("themes")).into_iter().collect();
    dirs.push(PathBuf::from("/Applications/Ghostty.app/Contents/Resources/ghostty/themes"));
    dirs
}

fn find(theme: &str) -> Option<PathBuf> {
    let path = PathBuf::from(theme);
    if path.is_absolute() {
        return path.is_file().then_some(path);
    }
    user_dirs().into_iter().chain(bundled_dirs()).map(|dir| dir.join(theme)).find(|p| p.is_file())
}

fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"));
    base.join("pulpo/config.toml")
}

pub fn chosen() -> String {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|table| table.get("theme").and_then(|t| t.as_str()).map(String::from))
        .unwrap_or_else(|| FOLLOW.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blends_and_parses_every_scope() {
        assert_eq!(mix(0xFFFFFF, 0x000000, 0xFF), 0xFFFFFF);
        assert_eq!(mix(0xFFFFFF, 0x000000, 0), 0);
        assert_eq!(build_syntax(&PULPO_DARK).scopes.len(), TOKENS.len());
    }

    #[test]
    fn reads_ghostty_themes() {
        let text = "background = #1a1b26\nforeground = #c0caf5\npalette = 4=#7aa2f7\npalette = 2 = #9ece6a\n# a comment\n";
        let p = parse(text).unwrap();
        assert_eq!((p.bg, p.fg, p.accent, p.ansi[2]), (0x1A1B26, 0xC0CAF5, 0x7AA2F7, 0x9ECE6A));
        assert_eq!(p.ansi[1], 0xCD0000);
        assert!(parse("palette = 0=#000000").is_none());
    }

    #[test]
    fn pulpo_darks_colors_are_pulpo_dark() {
        let mut text = String::from("background = #090909\nforeground = #e3e4e7\n");
        for (i, c) in PULPO_ANSI.iter().enumerate() {
            text.push_str(&format!("palette = {i}=#{c:06x}\n"));
        }
        assert_eq!(parse(&text), Some(PULPO_DARK));
    }

    #[test]
    fn light_themes_get_dark_text_on_a_light_accent() {
        let light = Palette::from_terminal(0xFFFCF0, 0x100F0F, [0xFFFCF0; 16]);
        assert_eq!(light.on_accent, 0xFFFCF0);
        assert!(luminance(light.muted) > luminance(light.fg) && luminance(light.muted) < luminance(light.bg));
    }
}
