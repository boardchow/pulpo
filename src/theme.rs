use std::str::FromStr;
use std::sync::{Arc, LazyLock};

use ratatui::style::Color;
use syntect::highlighting::{FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings};

struct Palette {
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

pub fn bg() -> Color {
    rgb(PULPO_DARK.bg)
}
pub fn editor_bg() -> Color {
    rgb(PULPO_DARK.editor_bg)
}
pub fn border() -> Color {
    rgb(PULPO_DARK.border)
}
pub fn accent() -> Color {
    rgb(PULPO_DARK.accent)
}
pub fn on_accent() -> Color {
    rgb(PULPO_DARK.on_accent)
}
pub fn fg() -> Color {
    rgb(PULPO_DARK.fg)
}
pub fn muted() -> Color {
    rgb(PULPO_DARK.muted)
}
pub fn dim() -> Color {
    rgb(PULPO_DARK.dim)
}
pub fn selection() -> Color {
    rgb(PULPO_DARK.selection)
}
pub fn hover() -> Color {
    rgb(PULPO_DARK.hover)
}
pub fn blue() -> Color {
    rgb(PULPO_DARK.ansi[4])
}
pub fn green() -> Color {
    rgb(PULPO_DARK.ansi[2])
}
pub fn red() -> Color {
    rgb(PULPO_DARK.ansi[1])
}
pub fn yellow() -> Color {
    rgb(PULPO_DARK.ansi[3])
}
pub fn magenta() -> Color {
    rgb(PULPO_DARK.ansi[5])
}
pub fn sky() -> Color {
    rgb(PULPO_DARK.ansi[14])
}
pub fn added_bg() -> Color {
    let p = PULPO_DARK;
    rgb(mix(p.ansi[2], p.editor_bg, 0x26))
}
pub fn removed_bg() -> Color {
    let p = PULPO_DARK;
    rgb(mix(p.ansi[1], p.editor_bg, 0x26))
}
pub fn find_bg() -> Color {
    let p = PULPO_DARK;
    rgb(mix(p.ansi[3], p.editor_bg, 0x55))
}
pub fn mark_bg() -> Color {
    let p = PULPO_DARK;
    rgb(mix(p.accent, p.editor_bg, 0x4D))
}
pub fn text_selection() -> Color {
    rgb(PULPO_DARK.text_selection)
}
pub fn ansi() -> [Color; 16] {
    PULPO_DARK.ansi.map(rgb)
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

/// Pulpo Dark's tokenColors, as its terminal colors.
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
    static SYNTAX: LazyLock<Arc<Theme>> = LazyLock::new(|| Arc::new(build_syntax(&PULPO_DARK)));
    SYNTAX.clone()
}

fn build_syntax(p: &Palette) -> Theme {
    let color = |hex: u32| syntect::highlighting::Color { r: (hex >> 16) as u8, g: (hex >> 8) as u8, b: hex as u8, a: 0xFF };
    let ink = |ink: Ink| match ink {
        Ink::Fg => p.fg,
        Ink::Soft => p.ansi[7],
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blends_and_parses_every_scope() {
        assert_eq!(mix(0xFFFFFF, 0x000000, 0xFF), 0xFFFFFF);
        assert_eq!(mix(0xFFFFFF, 0x000000, 0), 0);
        assert_eq!(build_syntax(&PULPO_DARK).scopes.len(), TOKENS.len());
    }
}
