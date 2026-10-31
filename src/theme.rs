//! Colors from the Linear Dark VS Code theme.

use std::str::FromStr;

use ratatui::style::Color;
use syntect::highlighting::{FontStyle, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSettings};

/// Side bar, panels, terminal.
pub const BG: Color = rgb(0x090909);
/// Editor (the viewer).
pub const EDITOR_BG: Color = rgb(0x101012);
pub const BORDER: Color = rgb(0x232429);
pub const ACCENT: Color = rgb(0x5E6AD2);
pub const FG: Color = rgb(0xE3E4E7);
pub const MUTED: Color = rgb(0x97979A);
pub const DIM: Color = rgb(0x595A5C);
pub const SELECTION: Color = rgb(0x1E2025);
pub const HOVER: Color = rgb(0x16171A);
pub const WHITE: Color = rgb(0xFFFFFF);

pub const BLUE: Color = rgb(0x8FA4FF);
pub const GREEN: Color = rgb(0x4CB782);
pub const RED: Color = rgb(0xEB5757);
pub const YELLOW: Color = rgb(0xFCC581);
pub const MAGENTA: Color = rgb(0xF1ADE0);
pub const SKY: Color = rgb(0x55CDFF);

/// The theme's translucent backgrounds, blended over the editor background.
pub const ADDED_BG: Color = mix(0x4CB782, 0x101012, 0x26);
pub const REMOVED_BG: Color = mix(0xEB5757, 0x101012, 0x26);
pub const FIND_BG: Color = mix(0xFCC581, 0x101012, 0x55);
pub const MARK_BG: Color = mix(0x5E6AD2, 0x101012, 0x4D);
/// Selected text in the editor.
pub const TEXT_SELECTION: Color = mix(0x5E6AD2, 0x101012, 0x70);

/// terminal.ansi* colors, for programs running in the embedded terminals.
pub const ANSI: [Color; 16] = [
    rgb(0x232429),
    rgb(0xEB5757),
    rgb(0x4CB782),
    rgb(0xFCC581),
    rgb(0x8FA4FF),
    rgb(0xF1ADE0),
    rgb(0x58EBDD),
    rgb(0xD4D4D4),
    rgb(0x595A5C),
    rgb(0xF2777A),
    rgb(0x6FD3A0),
    rgb(0xFDD6A3),
    rgb(0x94A8FF),
    rgb(0xF5C4EA),
    rgb(0x55CDFF),
    rgb(0xFFFFFF),
];

const fn rgb(hex: u32) -> Color {
    Color::Rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// `fg` at `alpha` (0-255) over `bg`.
const fn mix(fg: u32, bg: u32, alpha: u32) -> Color {
    const fn channel(fg: u32, bg: u32, shift: u32, alpha: u32) -> u8 {
        let (f, b) = ((fg >> shift) & 0xFF, (bg >> shift) & 0xFF);
        ((f * alpha + b * (255 - alpha)) / 255) as u8
    }
    Color::Rgb(channel(fg, bg, 16, alpha), channel(fg, bg, 8, alpha), channel(fg, bg, 0, alpha))
}

const ITALIC: u8 = 1;
const BOLD: u8 = 2;

/// The theme's tokenColors: scopes, foreground, style.
const TOKENS: &[(&str, u32, u8)] = &[
    ("comment, punctuation.definition.comment", 0x585A5C, ITALIC),
    (
        "keyword, keyword.control, storage, storage.type, storage.modifier, keyword.operator.new, \
         keyword.operator.expression, keyword.operator.logical.python, variable.language.this, \
         variable.language.self, variable.language.super, support.function.kernel.ruby, keyword.other.special-method",
        0x58EBDD,
        0,
    ),
    (
        "keyword.operator, punctuation, meta.brace, punctuation.separator, punctuation.terminator, \
         punctuation.definition.string, punctuation.definition.variable, punctuation.section",
        0xD4D4D4,
        0,
    ),
    (
        "entity.name.type, entity.name.class, entity.name.namespace, entity.other.inherited-class, support.class, \
         support.type, entity.name.type.module, storage.type.java, storage.type.cs",
        0x55CDFF,
        0,
    ),
    ("constant, variable.other.constant, constant.language, support.constant, variable.other.enummember, constant.other.symbol", 0x94A8FF, 0),
    ("constant.numeric", 0x94A8FF, 0),
    ("string, string.quoted, string.template, string.regexp, markup.inline.raw", 0xF1ADE0, 0),
    ("constant.character.escape, punctuation.section.embedded, punctuation.definition.template-expression", 0x58EBDD, 0),
    ("entity.name.function, support.function, meta.function-call entity.name.function, variable.function, entity.name.method", 0xFCC581, 0),
    (
        "variable.parameter, meta.parameter, variable.other.readwrite.instance, variable.other.readwrite.class, \
         variable.other.readwrite.global, variable.other.property, meta.object-literal.key, support.type.property-name",
        0x8FA4FF,
        0,
    ),
    ("variable, variable.other, variable.other.readwrite", 0xE3E4E7, 0),
    ("entity.name.tag", 0x58EBDD, 0),
    ("entity.other.attribute-name", 0x8FA4FF, 0),
    ("meta.decorator, entity.name.function.decorator, punctuation.decorator", 0xFCC581, 0),
    ("markup.heading, entity.name.section", 0x55CDFF, BOLD),
    ("markup.underline.link, string.other.link", 0x8FA4FF, 0),
    ("markup.inserted", 0x4CB782, 0),
    ("markup.deleted", 0xEB5757, 0),
    ("invalid, invalid.illegal", 0xEB5757, 0),
];

/// Syntax highlighting theme built from the token colors.
pub fn syntax() -> Theme {
    let color = |hex: u32| syntect::highlighting::Color { r: (hex >> 16) as u8, g: (hex >> 8) as u8, b: hex as u8, a: 0xFF };
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
                style: StyleModifier { foreground: Some(color(fg)), background: None, font_style: Some(font_style) },
            })
        })
        .collect();
    Theme {
        name: Some("Linear Dark".into()),
        settings: ThemeSettings { foreground: Some(color(0xE3E4E7)), background: Some(color(0x101012)), ..ThemeSettings::default() },
        scopes,
        ..Theme::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blends_and_parses_every_scope() {
        assert_eq!(mix(0xFFFFFF, 0x000000, 0xFF), Color::Rgb(255, 255, 255));
        assert_eq!(mix(0xFFFFFF, 0x000000, 0), Color::Rgb(0, 0, 0));
        assert_eq!(syntax().scopes.len(), TOKENS.len());
    }
}
