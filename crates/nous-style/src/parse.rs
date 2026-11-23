//! Parser de folhas de estilo e declarações (provisório, até a troca pelo Stylo).

use std::sync::Arc;

use crate::color::parse_color;
use crate::selector::{Selector, parse_selector_list};
use crate::values::{
    AlignItems, AlignSelf, BoxSizing, Display, FlexDirection, FlexWrap, FontFamily, GenericFamily,
    GridPlacement, JustifyContent, ListStyle, Overflow, Position, Rgba, TextAlign, WhiteSpace,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpecLength {
    Px(f32),
    Em(f32),
    Rem(f32),
    Percent(f32),
    Vw(f32),
    Vh(f32),
    Auto,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpecTrackBound {
    Length(SpecLength),
    Fr(f32),
    MinContent,
    MaxContent,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SpecTrackSize {
    Length(SpecLength),
    Fr(f32),
    MinContent,
    MaxContent,
    MinMax(SpecTrackBound, SpecTrackBound),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorValue {
    Rgba(Rgba),
    CurrentColor,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FontSizeValue {
    Length(SpecLength),
    Px(f32),
    Larger,
    Smaller,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FontWeightValue {
    Absolute(u16),
    Bolder,
    Lighter,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LineHeightValue {
    Normal,
    Number(f32),
    Length(SpecLength),
}

/// Lado de uma caixa: 0 topo, 1 direita, 2 baixo, 3 esquerda.
pub type Side = usize;

#[derive(Clone, Debug, PartialEq)]
pub enum Decl {
    Display(Display),
    Color(ColorValue),
    BackgroundColor(ColorValue),
    FontSize(FontSizeValue),
    FontWeight(FontWeightValue),
    FontStyle(bool),
    FontFamily(FontFamily),
    LineHeight(LineHeightValue),
    TextAlign(TextAlign),
    WhiteSpace(WhiteSpace),
    TextDecoration { underline: bool, line_through: bool },
    Margin(Side, SpecLength),
    Padding(Side, SpecLength),
    BorderWidth(Side, SpecLength),
    BorderStyle(Side, bool),
    BorderColor(Side, ColorValue),
    BorderRadius(SpecLength),
    Width(SpecLength),
    Height(SpecLength),
    MinWidth(SpecLength),
    MinHeight(SpecLength),
    MaxWidth(SpecLength),
    MaxHeight(SpecLength),
    BoxSizing(BoxSizing),
    OverflowX(Overflow),
    OverflowY(Overflow),
    Opacity(f32),
    Position(Position),
    Inset(Side, SpecLength),
    ZIndex(Option<i32>),
    FlexDirection(FlexDirection),
    FlexWrap(FlexWrap),
    JustifyContent(JustifyContent),
    AlignItems(AlignItems),
    AlignSelf(AlignSelf),
    FlexGrow(f32),
    FlexShrink(f32),
    FlexBasis(SpecLength),
    RowGap(SpecLength),
    ColumnGap(SpecLength),
    GridTemplateColumns(Vec<SpecTrackSize>),
    GridTemplateRows(Vec<SpecTrackSize>),
    GridColumn(GridPlacement),
    GridRow(GridPlacement),
    ListStyle(ListStyle),
    Visibility(bool),
    CustomProp { name: Box<str>, value: Box<str> },
    VarProp { name: Box<str>, value: Box<str> },
}

#[derive(Debug, Default)]
pub struct DeclBlock {
    pub normal: Vec<Decl>,
    pub important: Vec<Decl>,
}

/// Condição de `@media` já analisada; avaliada na cascata com o tamanho da janela.
#[derive(Clone, Debug, PartialEq)]
pub struct MediaQuery {
    pub negated: bool,
    pub media_ok: bool,
    pub min_width: Option<f32>,
    pub max_width: Option<f32>,
    pub unsupported: bool,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct MediaList(pub Vec<MediaQuery>);

impl MediaList {
    pub fn matches(&self, viewport_width: f32) -> bool {
        self.0.iter().any(|q| {
            let ok = q.media_ok
                && !q.unsupported
                && q.min_width.is_none_or(|w| viewport_width >= w)
                && q.max_width.is_none_or(|w| viewport_width <= w);
            ok != q.negated
        })
    }
}

pub struct ParsedRule {
    pub selectors: Vec<Selector>,
    pub decls: Arc<DeclBlock>,
    pub media: Option<Arc<MediaList>>,
}

// ------------------------------------------------------------------ folhas

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Acha o `}` que fecha um bloco cujo conteúdo começa em `s` (respeita aspas e chaves).
fn block_end(s: &str) -> usize {
    let mut depth = 0;
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), '\\') => {}
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '{') => depth += 1,
            (None, '}') if depth == 0 => return i,
            (None, '}') => depth -= 1,
            _ => {}
        }
    }
    s.len()
}

/// Acha o próximo `delim` fora de aspas/parênteses.
fn find_top_level(s: &str, delims: &[char]) -> Option<usize> {
    let mut depth = 0;
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth -= 1,
            (None, c) if depth == 0 && delims.contains(&c) => return Some(i),
            _ => {}
        }
    }
    None
}

pub fn parse_stylesheet(css: &str) -> Vec<ParsedRule> {
    let css = strip_comments(css);
    let mut rules = Vec::new();
    parse_rules(&css, None, &mut rules);
    rules
}

fn parse_rules(mut s: &str, media: Option<Arc<MediaList>>, out: &mut Vec<ParsedRule>) {
    loop {
        s = s.trim_start();
        if s.is_empty() {
            return;
        }
        if let Some(rest) = s.strip_prefix('@') {
            let Some(pos) = find_top_level(rest, &['{', ';']) else {
                return;
            };
            let prelude = rest[..pos].trim();
            if rest[pos..].starts_with(';') {
                s = &rest[pos + 1..];
                continue;
            }
            let body_start = pos + 1;
            let end = body_start + block_end(&rest[body_start..]);
            let body = &rest[body_start..end.min(rest.len())];
            let (name, cond) = prelude
                .split_once(char::is_whitespace)
                .unwrap_or((prelude, ""));
            match name.to_ascii_lowercase().as_str() {
                "media" => {
                    let list = Arc::new(parse_media_list(cond));
                    // @media dentro de @media: fica a condição mais interna (simplificação)
                    parse_rules(body, Some(list), out);
                }
                "supports" | "layer" | "document" | "-moz-document" => {
                    parse_rules(body, media.clone(), out)
                }
                _ => {} // @font-face, @keyframes, @page…: ainda não
            }
            s = rest.get(end + 1..).unwrap_or("");
            continue;
        }
        let Some(open) = find_top_level(s, &['{']) else {
            return;
        };
        let prelude = s[..open].trim();
        let body_start = open + 1;
        let end = body_start + block_end(&s[body_start..]);
        let body = &s[body_start..end.min(s.len())];
        if let Some(selectors) = parse_selector_list(prelude) {
            let decls = parse_declarations(body);
            if !decls.normal.is_empty() || !decls.important.is_empty() {
                out.push(ParsedRule {
                    selectors,
                    decls: Arc::new(decls),
                    media: media.clone(),
                });
            }
        }
        s = s.get(end + 1..).unwrap_or("");
    }
}

pub fn parse_media_list(input: &str) -> MediaList {
    let input = input.trim();
    if input.is_empty() {
        return MediaList(vec![MediaQuery {
            negated: false,
            media_ok: true,
            min_width: None,
            max_width: None,
            unsupported: false,
        }]);
    }
    let mut out = Vec::new();
    for q in input.split(',') {
        let q = q.trim().to_ascii_lowercase();
        let mut mq = MediaQuery {
            negated: false,
            media_ok: true,
            min_width: None,
            max_width: None,
            unsupported: false,
        };
        let mut rest = q.as_str();
        while !rest.is_empty() {
            rest = rest.trim_start();
            if let Some(r) = rest.strip_prefix('(') {
                let close = r.find(')').unwrap_or(r.len());
                let feature = &r[..close];
                rest = r.get(close + 1..).unwrap_or("");
                let (name, value) = feature.split_once(':').unwrap_or((feature, ""));
                let px = parse_length(value.trim()).and_then(|l| match l {
                    SpecLength::Px(v) => Some(v),
                    SpecLength::Em(v) | SpecLength::Rem(v) => Some(v * 16.0),
                    _ => None,
                });
                match (name.trim(), px) {
                    ("min-width", Some(v)) => mq.min_width = Some(v),
                    ("max-width", Some(v)) => mq.max_width = Some(v),
                    ("prefers-reduced-motion", _) | ("hover", _) | ("pointer", _) => {}
                    _ => mq.unsupported = true,
                }
                continue;
            }
            let (word, r) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
            rest = r;
            match word {
                "not" => mq.negated = true,
                "only" | "and" | "" => {}
                "screen" | "all" => {}
                _ => mq.media_ok = false, // print, speech…
            }
        }
        out.push(mq);
    }
    MediaList(out)
}

// ------------------------------------------------------------- declarações

pub fn parse_declarations(body: &str) -> DeclBlock {
    let mut block = DeclBlock::default();
    parse_declarations_into(body, &mut block);
    block
}

pub fn parse_declarations_into(body: &str, block: &mut DeclBlock) {
    block.normal.clear();
    block.important.clear();
    let mut rest = body;
    while !rest.trim().is_empty() {
        let end = find_top_level(rest, &[';']).unwrap_or(rest.len());
        let decl = &rest[..end];
        rest = rest.get(end + 1..).unwrap_or("");
        let Some((name, value)) = decl.split_once(':') else {
            continue;
        };
        let raw_name = name.trim();
        let mut value = value.trim();
        let mut important = false;
        if let Some(pos) = value.to_ascii_lowercase().rfind("!important")
            && value[pos..].trim().eq_ignore_ascii_case("!important")
        {
            important = true;
            value = value[..pos].trim();
        }
        let target = if important {
            &mut block.important
        } else {
            &mut block.normal
        };
        if raw_name.starts_with("--") {
            target.push(Decl::CustomProp {
                name: raw_name.into(),
                value: value.into(),
            });
            continue;
        }
        let name = raw_name.to_ascii_lowercase();
        parse_property(&name, value, target);
    }
}

/// Divide um valor em componentes separados por espaço, sem quebrar `rgb(1, 2, 3)`.
fn components(value: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = value.trim();
    while !rest.is_empty() {
        let end = find_top_level(rest, &[' ', '\t', '\n']).unwrap_or(rest.len());
        if end > 0 {
            out.push(&rest[..end]);
        }
        rest = rest[end..].trim_start();
    }
    out
}

pub fn parse_length(v: &str) -> Option<SpecLength> {
    let v = v.trim().to_ascii_lowercase();
    if v == "auto" {
        return Some(SpecLength::Auto);
    }
    // número = sinal opcional + dígitos e ponto; o resto é a unidade (sem expoente por ora)
    let split = v
        .char_indices()
        .find(|&(i, c)| !(c.is_ascii_digit() || c == '.' || (i == 0 && (c == '-' || c == '+'))))
        .map(|(i, _)| i)
        .unwrap_or(v.len());
    let (num, unit) = v.split_at(split);
    let n: f32 = num.parse().ok()?;
    Some(match unit {
        "" if n == 0.0 => SpecLength::Px(0.0),
        "px" => SpecLength::Px(n),
        "em" => SpecLength::Em(n),
        "ex" | "ch" => SpecLength::Em(n * 0.5),
        "rem" => SpecLength::Rem(n),
        "%" => SpecLength::Percent(n),
        "vw" => SpecLength::Vw(n),
        "vh" => SpecLength::Vh(n),
        "pt" => SpecLength::Px(n * 96.0 / 72.0),
        "pc" => SpecLength::Px(n * 16.0),
        "in" => SpecLength::Px(n * 96.0),
        "cm" => SpecLength::Px(n * 96.0 / 2.54),
        "mm" => SpecLength::Px(n * 96.0 / 25.4),
        _ => return None,
    })
}

fn parse_color_value(v: &str) -> Option<ColorValue> {
    if v.eq_ignore_ascii_case("currentcolor") {
        return Some(ColorValue::CurrentColor);
    }
    parse_color(v).map(ColorValue::Rgba)
}

/// Expande 1 a 4 valores para os quatro lados (topo, direita, baixo, esquerda).
fn four_sides<T: Copy>(vals: &[T]) -> Option<[T; 4]> {
    Some(match vals {
        [a] => [*a, *a, *a, *a],
        [a, b] => [*a, *b, *a, *b],
        [a, b, c] => [*a, *b, *c, *b],
        [a, b, c, d] => [*a, *b, *c, *d],
        _ => return None,
    })
}

fn side_of(name: &str) -> Option<Side> {
    Some(match name {
        "top" => 0,
        "right" => 1,
        "bottom" => 2,
        "left" => 3,
        _ => return None,
    })
}

fn border_width_keyword(v: &str) -> Option<SpecLength> {
    match v {
        "thin" => Some(SpecLength::Px(1.0)),
        "medium" => Some(SpecLength::Px(3.0)),
        "thick" => Some(SpecLength::Px(5.0)),
        _ => parse_length(v),
    }
}

fn is_border_style(v: &str) -> Option<bool> {
    match v {
        "none" | "hidden" => Some(false),
        "solid" | "dotted" | "dashed" | "double" | "groove" | "ridge" | "inset" | "outset" => {
            Some(true)
        }
        _ => None,
    }
}

fn parse_border_shorthand(value: &str, sides: &[Side], out: &mut Vec<Decl>) {
    let mut width = SpecLength::Px(3.0);
    let mut style = false;
    let mut color = ColorValue::CurrentColor;
    for c in components(value) {
        let lc = c.to_ascii_lowercase();
        if let Some(s) = is_border_style(&lc) {
            style = s;
        } else if let Some(w) = border_width_keyword(&lc) {
            width = w;
        } else if let Some(col) = parse_color_value(c) {
            color = col;
        } else {
            return;
        }
    }
    for &s in sides {
        out.push(Decl::BorderWidth(s, width));
        out.push(Decl::BorderStyle(s, style));
        out.push(Decl::BorderColor(s, color));
    }
}

fn parse_font_family(value: &str) -> FontFamily {
    let mut names: Vec<Arc<str>> = Vec::new();
    let mut generic = None;
    for part in value.split(',') {
        let name = part.trim().trim_matches(['"', '\'']).trim();
        match name.to_ascii_lowercase().as_str() {
            "serif" => generic = generic.or(Some(GenericFamily::Serif)),
            "sans-serif" | "system-ui" | "-apple-system" | "ui-sans-serif"
            | "blinkmacsystemfont" => generic = generic.or(Some(GenericFamily::SansSerif)),
            "monospace" | "ui-monospace" => generic = generic.or(Some(GenericFamily::Monospace)),
            "" => {}
            _ => names.push(name.into()),
        }
    }
    FontFamily {
        names: names.into(),
        generic: generic.unwrap_or(GenericFamily::Serif),
    }
}

fn parse_font_size(v: &str) -> Option<FontSizeValue> {
    Some(match v {
        "xx-small" => FontSizeValue::Px(9.0),
        "x-small" => FontSizeValue::Px(10.0),
        "small" => FontSizeValue::Px(13.0),
        "medium" => FontSizeValue::Px(16.0),
        "large" => FontSizeValue::Px(18.0),
        "x-large" => FontSizeValue::Px(24.0),
        "xx-large" => FontSizeValue::Px(32.0),
        "xxx-large" => FontSizeValue::Px(48.0),
        "larger" => FontSizeValue::Larger,
        "smaller" => FontSizeValue::Smaller,
        _ => match parse_length(v)? {
            SpecLength::Auto => return None,
            l => FontSizeValue::Length(l),
        },
    })
}

fn parse_font_weight(v: &str) -> Option<FontWeightValue> {
    Some(match v {
        "normal" => FontWeightValue::Absolute(400),
        "bold" => FontWeightValue::Absolute(700),
        "bolder" => FontWeightValue::Bolder,
        "lighter" => FontWeightValue::Lighter,
        _ => FontWeightValue::Absolute(v.parse::<u16>().ok()?.clamp(1, 1000)),
    })
}

fn parse_line_height(v: &str) -> Option<LineHeightValue> {
    if v == "normal" {
        return Some(LineHeightValue::Normal);
    }
    if let Ok(n) = v.parse::<f32>() {
        return Some(LineHeightValue::Number(n));
    }
    parse_length(v).map(LineHeightValue::Length)
}

fn parse_display(v: &str) -> Option<Display> {
    Some(match v {
        "none" => Display::None,
        "inline" | "contents" => Display::Inline,
        "inline-block" => Display::InlineBlock,
        "inline-flex" => Display::InlineFlex,
        "flex" => Display::Flex,
        "grid" | "inline-grid" => Display::Grid,
        "table" | "inline-table" => Display::Table,
        "table-row" => Display::TableRow,
        "table-cell" => Display::TableCell,
        "list-item" => Display::ListItem,
        "block" | "table-row-group" | "table-header-group" | "table-footer-group"
        | "table-caption" | "flow-root" => Display::Block,
        _ => return None,
    })
}

fn contains_var_call(v: &str) -> bool {
    v.as_bytes()
        .windows(4)
        .any(|w| w.eq_ignore_ascii_case(b"var("))
}

pub(crate) fn parse_property(name: &str, value: &str, out: &mut Vec<Decl>) {
    if contains_var_call(value) {
        out.push(Decl::VarProp {
            name: name.into(),
            value: value.into(),
        });
        return;
    }
    let lv = value.to_ascii_lowercase();
    let v = lv.as_str().trim();
    if matches!(v, "inherit" | "initial" | "unset" | "revert") || v.contains("calc(") {
        return;
    }
    match name {
        "display" => out.extend(parse_display(v).map(Decl::Display)),
        "color" => out.extend(parse_color_value(value).map(Decl::Color)),
        "background-color" => out.extend(parse_color_value(value).map(Decl::BackgroundColor)),
        "background" => {
            // pega só a cor do atalho
            let color = components(value)
                .into_iter()
                .rev()
                .find_map(parse_color_value)
                .unwrap_or(ColorValue::Rgba(Rgba::TRANSPARENT));
            out.push(Decl::BackgroundColor(color));
        }
        "font-size" => out.extend(parse_font_size(v).map(Decl::FontSize)),
        "font-weight" => out.extend(parse_font_weight(v).map(Decl::FontWeight)),
        "font-style" => out.push(Decl::FontStyle(v == "italic" || v == "oblique")),
        "font-family" => out.push(Decl::FontFamily(parse_font_family(value))),
        "font" => parse_font_shorthand(value, out),
        "line-height" => out.extend(parse_line_height(v).map(Decl::LineHeight)),
        "text-align" => out.extend(
            match v {
                "left" | "start" => Some(TextAlign::Left),
                "right" | "end" => Some(TextAlign::Right),
                "center" | "-webkit-center" => Some(TextAlign::Center),
                "justify" => Some(TextAlign::Justify),
                _ => None,
            }
            .map(Decl::TextAlign),
        ),
        "white-space" | "white-space-collapse" => out.extend(
            match v {
                "normal" => Some(WhiteSpace::Normal),
                "pre" => Some(WhiteSpace::Pre),
                "nowrap" => Some(WhiteSpace::NoWrap),
                "pre-wrap" | "break-spaces" => Some(WhiteSpace::PreWrap),
                "pre-line" => Some(WhiteSpace::PreLine),
                _ => None,
            }
            .map(Decl::WhiteSpace),
        ),
        "text-decoration" | "text-decoration-line" => out.push(Decl::TextDecoration {
            underline: v.contains("underline"),
            line_through: v.contains("line-through"),
        }),
        "margin" | "padding" => {
            let vals: Option<Vec<SpecLength>> =
                components(v).into_iter().map(parse_length).collect();
            if let Some(sides) = vals.as_deref().and_then(four_sides) {
                for (i, l) in sides.into_iter().enumerate() {
                    out.push(if name == "margin" {
                        Decl::Margin(i, l)
                    } else {
                        Decl::Padding(i, l)
                    });
                }
            }
        }
        "margin-block" | "margin-inline" | "padding-block" | "padding-inline" => {
            let vals: Option<Vec<SpecLength>> =
                components(v).into_iter().map(parse_length).collect();
            if let Some(vals) = vals.filter(|v| !v.is_empty()) {
                let (a, b) = (vals[0], *vals.get(1).unwrap_or(&vals[0]));
                let sides = if name.ends_with("block") {
                    [0, 2]
                } else {
                    [3, 1]
                };
                let margin = name.starts_with("margin");
                for (s, l) in sides.into_iter().zip([a, b]) {
                    out.push(if margin {
                        Decl::Margin(s, l)
                    } else {
                        Decl::Padding(s, l)
                    });
                }
            }
        }
        "border" => parse_border_shorthand(value, &[0, 1, 2, 3], out),
        "border-width" => {
            let vals: Option<Vec<SpecLength>> = components(v)
                .into_iter()
                .map(border_width_keyword)
                .collect();
            if let Some(sides) = vals.as_deref().and_then(four_sides) {
                for (i, l) in sides.into_iter().enumerate() {
                    out.push(Decl::BorderWidth(i, l));
                }
            }
        }
        "border-style" => {
            let vals: Option<Vec<bool>> = components(v).into_iter().map(is_border_style).collect();
            if let Some(sides) = vals.as_deref().and_then(four_sides) {
                for (i, s) in sides.into_iter().enumerate() {
                    out.push(Decl::BorderStyle(i, s));
                }
            }
        }
        "border-color" => {
            let vals: Option<Vec<ColorValue>> = components(value)
                .into_iter()
                .map(parse_color_value)
                .collect();
            if let Some(sides) = vals.as_deref().and_then(four_sides) {
                for (i, c) in sides.into_iter().enumerate() {
                    out.push(Decl::BorderColor(i, c));
                }
            }
        }
        "border-radius" => {
            if let Some(first) = components(v).first().and_then(|c| parse_length(c)) {
                out.push(Decl::BorderRadius(first));
            }
        }
        "width" => out.extend(parse_length(v).map(Decl::Width)),
        "height" => out.extend(parse_length(v).map(Decl::Height)),
        "min-width" => out.extend(parse_length(v).map(Decl::MinWidth)),
        "min-height" => out.extend(parse_length(v).map(Decl::MinHeight)),
        "max-width" => {
            if v == "none" {
                out.push(Decl::MaxWidth(SpecLength::Auto));
            } else {
                out.extend(parse_length(v).map(Decl::MaxWidth));
            }
        }
        "max-height" => {
            if v == "none" {
                out.push(Decl::MaxHeight(SpecLength::Auto));
            } else {
                out.extend(parse_length(v).map(Decl::MaxHeight));
            }
        }
        "box-sizing" => match v {
            "content-box" => out.push(Decl::BoxSizing(BoxSizing::ContentBox)),
            "border-box" => out.push(Decl::BoxSizing(BoxSizing::BorderBox)),
            _ => {}
        },
        "overflow" => {
            let vals: Option<Vec<Overflow>> =
                components(v).into_iter().map(parse_overflow).collect();
            if let Some(vals) = vals.filter(|v| !v.is_empty()) {
                let x = vals[0];
                let y = *vals.get(1).unwrap_or(&x);
                out.push(Decl::OverflowX(x));
                out.push(Decl::OverflowY(y));
            }
        }
        "overflow-x" => out.extend(parse_overflow(v).map(Decl::OverflowX)),
        "overflow-y" => out.extend(parse_overflow(v).map(Decl::OverflowY)),
        "opacity" => {
            let alpha = if let Some(p) = v.strip_suffix('%') {
                p.parse::<f32>().ok().map(|x| x / 100.0)
            } else {
                v.parse::<f32>().ok()
            };
            if let Some(a) = alpha {
                out.push(Decl::Opacity(a.clamp(0.0, 1.0)));
            }
        }
        "position" => match v {
            "static" => out.push(Decl::Position(Position::Static)),
            "relative" | "sticky" => out.push(Decl::Position(Position::Relative)),
            "absolute" => out.push(Decl::Position(Position::Absolute)),
            "fixed" => out.push(Decl::Position(Position::Fixed)),
            _ => {}
        },
        "top" => out.extend(parse_length(v).map(|l| Decl::Inset(0, l))),
        "right" => out.extend(parse_length(v).map(|l| Decl::Inset(1, l))),
        "bottom" => out.extend(parse_length(v).map(|l| Decl::Inset(2, l))),
        "left" => out.extend(parse_length(v).map(|l| Decl::Inset(3, l))),
        "inset" => {
            let vals: Option<Vec<SpecLength>> =
                components(v).into_iter().map(parse_length).collect();
            if let Some(sides) = vals.as_deref().and_then(four_sides) {
                for (i, l) in sides.into_iter().enumerate() {
                    out.push(Decl::Inset(i, l));
                }
            }
        }
        "inset-block" | "inset-inline" => {
            let vals: Option<Vec<SpecLength>> =
                components(v).into_iter().map(parse_length).collect();
            if let Some(vals) = vals.filter(|v| !v.is_empty()) {
                let (a, b) = (vals[0], *vals.get(1).unwrap_or(&vals[0]));
                let sides = if name.ends_with("block") {
                    [0, 2]
                } else {
                    [3, 1]
                };
                for (s, l) in sides.into_iter().zip([a, b]) {
                    out.push(Decl::Inset(s, l));
                }
            }
        }
        "z-index" => {
            if v == "auto" {
                out.push(Decl::ZIndex(None));
            } else if let Ok(z) = v.parse::<i32>() {
                out.push(Decl::ZIndex(Some(z)));
            }
        }
        "flex-direction" => out.extend(parse_flex_direction(v).map(Decl::FlexDirection)),
        "flex-wrap" => out.extend(parse_flex_wrap(v).map(Decl::FlexWrap)),
        "flex-flow" => {
            for c in components(v) {
                if let Some(d) = parse_flex_direction(c) {
                    out.push(Decl::FlexDirection(d));
                } else if let Some(w) = parse_flex_wrap(c) {
                    out.push(Decl::FlexWrap(w));
                }
            }
        }
        "justify-content" => out.extend(parse_justify_content(v).map(Decl::JustifyContent)),
        "align-items" => out.extend(parse_align_items(v).map(Decl::AlignItems)),
        "align-self" => out.extend(parse_align_self(v).map(Decl::AlignSelf)),
        "place-items" => {
            if let Some(first) = components(v).first().and_then(|c| parse_align_items(c)) {
                out.push(Decl::AlignItems(first));
            }
        }
        "flex-grow" => {
            if let Ok(g) = v.parse::<f32>()
                && g >= 0.0
            {
                out.push(Decl::FlexGrow(g));
            }
        }
        "flex-shrink" => {
            if let Ok(s) = v.parse::<f32>()
                && s >= 0.0
            {
                out.push(Decl::FlexShrink(s));
            }
        }
        "flex-basis" => {
            if v == "content" || v == "auto" {
                out.push(Decl::FlexBasis(SpecLength::Auto));
            } else {
                out.extend(parse_length(v).map(Decl::FlexBasis));
            }
        }
        "flex" => parse_flex_shorthand(v, out),
        "gap" | "grid-gap" => {
            let vals: Option<Vec<SpecLength>> =
                components(v).into_iter().map(parse_length).collect();
            if let Some(vals) = vals.filter(|v| !v.is_empty()) {
                let row = vals[0];
                let col = *vals.get(1).unwrap_or(&row);
                out.push(Decl::RowGap(row));
                out.push(Decl::ColumnGap(col));
            }
        }
        "row-gap" | "grid-row-gap" => out.extend(parse_length(v).map(Decl::RowGap)),
        "column-gap" | "grid-column-gap" => out.extend(parse_length(v).map(Decl::ColumnGap)),
        "grid-template-columns" => {
            if let Some(tracks) = parse_track_list(v) {
                out.push(Decl::GridTemplateColumns(tracks));
            }
        }
        "grid-template-rows" => {
            if let Some(tracks) = parse_track_list(v) {
                out.push(Decl::GridTemplateRows(tracks));
            }
        }
        "grid-column" => {
            if let Some(p) = parse_grid_placement(v) {
                out.push(Decl::GridColumn(p));
            }
        }
        "grid-row" => {
            if let Some(p) = parse_grid_placement(v) {
                out.push(Decl::GridRow(p));
            }
        }
        "list-style-type" | "list-style" => {
            let style = components(v).into_iter().find_map(|c| match c {
                "disc" => Some(ListStyle::Disc),
                "circle" => Some(ListStyle::Circle),
                "square" => Some(ListStyle::Square),
                "decimal" => Some(ListStyle::Decimal),
                "none" => Some(ListStyle::None),
                _ => None,
            });
            out.extend(style.map(Decl::ListStyle));
        }
        "visibility" => out.push(Decl::Visibility(v != "hidden" && v != "collapse")),
        _ => {
            // margin-top, padding-left, border-bottom, border-top-width…
            if let Some(rest) = name.strip_prefix("margin-") {
                if let (Some(s), Some(l)) = (side_of(rest), parse_length(v)) {
                    out.push(Decl::Margin(s, l));
                }
            } else if let Some(rest) = name.strip_prefix("padding-") {
                if let (Some(s), Some(l)) = (side_of(rest), parse_length(v)) {
                    out.push(Decl::Padding(s, l));
                }
            } else if let Some(rest) = name.strip_prefix("border-") {
                let (side, prop) = rest.split_once('-').unwrap_or((rest, ""));
                let Some(s) = side_of(side) else { return };
                match prop {
                    "" => parse_border_shorthand(value, &[s], out),
                    "width" => out.extend(border_width_keyword(v).map(|l| Decl::BorderWidth(s, l))),
                    "style" => out.extend(is_border_style(v).map(|b| Decl::BorderStyle(s, b))),
                    "color" => {
                        out.extend(parse_color_value(value).map(|c| Decl::BorderColor(s, c)))
                    }
                    _ => {}
                }
            }
        }
    }
}

/// `font: italic bold 16px/1.5 Arial, sans-serif`
fn parse_font_shorthand(value: &str, out: &mut Vec<Decl>) {
    let comps = components(value);
    let Some(size_idx) = comps.iter().position(|c| {
        let size = c.split('/').next().unwrap_or(c).to_ascii_lowercase();
        parse_font_size(&size).is_some()
    }) else {
        return;
    };
    let mut italic = false;
    let mut weight = FontWeightValue::Absolute(400);
    for c in &comps[..size_idx] {
        let lc = c.to_ascii_lowercase();
        match lc.as_str() {
            "italic" | "oblique" => italic = true,
            _ => {
                if let Some(w) = parse_font_weight(&lc) {
                    weight = w;
                }
            }
        }
    }
    let size_part = comps[size_idx].to_ascii_lowercase();
    let (size, lh) = match size_part.split_once('/') {
        Some((s, l)) => (s.to_owned(), parse_line_height(l)),
        None => (size_part.clone(), None),
    };
    let Some(size) = parse_font_size(&size) else {
        return;
    };
    let family_start = value
        .find(comps[size_idx])
        .map(|p| p + comps[size_idx].len())
        .unwrap_or(value.len());
    out.push(Decl::FontStyle(italic));
    out.push(Decl::FontWeight(weight));
    out.push(Decl::FontSize(size));
    out.push(Decl::LineHeight(lh.unwrap_or(LineHeightValue::Normal)));
    let fam = value[family_start..].trim();
    if !fam.is_empty() {
        out.push(Decl::FontFamily(parse_font_family(fam)));
    }
}

fn parse_overflow(v: &str) -> Option<Overflow> {
    Some(match v {
        "visible" => Overflow::Visible,
        "hidden" | "clip" => Overflow::Hidden,
        "scroll" => Overflow::Scroll,
        "auto" => Overflow::Auto,
        _ => return None,
    })
}

fn parse_flex_direction(v: &str) -> Option<FlexDirection> {
    Some(match v {
        "row" => FlexDirection::Row,
        "row-reverse" => FlexDirection::RowReverse,
        "column" => FlexDirection::Column,
        "column-reverse" => FlexDirection::ColumnReverse,
        _ => return None,
    })
}

fn parse_flex_wrap(v: &str) -> Option<FlexWrap> {
    Some(match v {
        "nowrap" => FlexWrap::NoWrap,
        "wrap" => FlexWrap::Wrap,
        "wrap-reverse" => FlexWrap::WrapReverse,
        _ => return None,
    })
}

fn parse_justify_content(v: &str) -> Option<JustifyContent> {
    Some(match v {
        "flex-start" | "start" | "left" | "normal" => JustifyContent::FlexStart,
        "flex-end" | "end" | "right" => JustifyContent::FlexEnd,
        "center" => JustifyContent::Center,
        "space-between" => JustifyContent::SpaceBetween,
        "space-around" => JustifyContent::SpaceAround,
        "space-evenly" => JustifyContent::SpaceEvenly,
        "stretch" => JustifyContent::Stretch,
        _ => return None,
    })
}

fn parse_align_items(v: &str) -> Option<AlignItems> {
    Some(match v {
        "stretch" | "normal" => AlignItems::Stretch,
        "flex-start" | "start" | "self-start" => AlignItems::FlexStart,
        "flex-end" | "end" | "self-end" => AlignItems::FlexEnd,
        "center" => AlignItems::Center,
        "baseline" => AlignItems::Baseline,
        _ => return None,
    })
}

fn parse_align_self(v: &str) -> Option<AlignSelf> {
    Some(match v {
        "auto" => AlignSelf::Auto,
        "stretch" | "normal" => AlignSelf::Stretch,
        "flex-start" | "start" | "self-start" => AlignSelf::FlexStart,
        "flex-end" | "end" | "self-end" => AlignSelf::FlexEnd,
        "center" => AlignSelf::Center,
        "baseline" => AlignSelf::Baseline,
        _ => return None,
    })
}

fn parse_unitless_nonneg(s: &str) -> Option<f32> {
    if s.is_empty()
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'.' || b == b'+' || b == b'-')
    {
        return None;
    }
    s.parse::<f32>().ok().filter(|n| *n >= 0.0)
}

fn parse_flex_basis_token(s: &str) -> Option<SpecLength> {
    if s == "content" || s == "auto" {
        Some(SpecLength::Auto)
    } else {
        parse_length(s)
    }
}

fn parse_flex_shorthand(v: &str, out: &mut Vec<Decl>) {
    match v {
        "none" => {
            out.push(Decl::FlexGrow(0.0));
            out.push(Decl::FlexShrink(0.0));
            out.push(Decl::FlexBasis(SpecLength::Auto));
            return;
        }
        "auto" => {
            out.push(Decl::FlexGrow(1.0));
            out.push(Decl::FlexShrink(1.0));
            out.push(Decl::FlexBasis(SpecLength::Auto));
            return;
        }
        "initial" => {
            out.push(Decl::FlexGrow(0.0));
            out.push(Decl::FlexShrink(1.0));
            out.push(Decl::FlexBasis(SpecLength::Auto));
            return;
        }
        _ => {}
    }
    let comps = components(v);
    match comps.as_slice() {
        [a] => {
            if let Some(g) = parse_unitless_nonneg(a) {
                out.push(Decl::FlexGrow(g));
                out.push(Decl::FlexShrink(1.0));
                out.push(Decl::FlexBasis(SpecLength::Px(0.0)));
            } else if let Some(b) = parse_flex_basis_token(a) {
                out.push(Decl::FlexGrow(1.0));
                out.push(Decl::FlexShrink(1.0));
                out.push(Decl::FlexBasis(b));
            }
        }
        [a, b] => {
            let Some(g) = parse_unitless_nonneg(a) else {
                return;
            };
            if let Some(s) = parse_unitless_nonneg(b) {
                out.push(Decl::FlexGrow(g));
                out.push(Decl::FlexShrink(s));
                out.push(Decl::FlexBasis(SpecLength::Px(0.0)));
            } else if let Some(basis) = parse_flex_basis_token(b) {
                out.push(Decl::FlexGrow(g));
                out.push(Decl::FlexShrink(1.0));
                out.push(Decl::FlexBasis(basis));
            }
        }
        [a, b, c] => {
            if let (Some(g), Some(s), Some(basis)) = (
                parse_unitless_nonneg(a),
                parse_unitless_nonneg(b),
                parse_flex_basis_token(c),
            ) {
                out.push(Decl::FlexGrow(g));
                out.push(Decl::FlexShrink(s));
                out.push(Decl::FlexBasis(basis));
            }
        }
        _ => {}
    }
}

fn parse_track_bound(s: &str) -> Option<SpecTrackBound> {
    match s {
        "min-content" => Some(SpecTrackBound::MinContent),
        "max-content" => Some(SpecTrackBound::MaxContent),
        _ if s.ends_with("fr") => s[..s.len() - 2]
            .parse::<f32>()
            .ok()
            .filter(|n| *n >= 0.0)
            .map(SpecTrackBound::Fr),
        _ => parse_length(s).map(SpecTrackBound::Length),
    }
}

fn parse_single_track(s: &str) -> Option<SpecTrackSize> {
    match s {
        "min-content" => Some(SpecTrackSize::MinContent),
        "max-content" => Some(SpecTrackSize::MaxContent),
        _ if s.ends_with("fr") => s[..s.len() - 2]
            .parse::<f32>()
            .ok()
            .filter(|n| *n >= 0.0)
            .map(SpecTrackSize::Fr),
        _ => parse_length(s).map(SpecTrackSize::Length),
    }
}

fn parse_track_into(s: &str, out: &mut Vec<SpecTrackSize>) -> Option<()> {
    let comps = components(s);
    if comps.is_empty() {
        return None;
    }
    for c in comps {
        if let Some(inner) = c.strip_prefix("repeat(").and_then(|r| r.strip_suffix(')')) {
            let comma = find_top_level(inner, &[','])?;
            let count_str = inner[..comma].trim();
            let pat_str = inner[comma + 1..].trim();
            let mut pat = Vec::new();
            parse_track_into(pat_str, &mut pat)?;
            let count = if let Ok(n) = count_str.parse::<usize>() {
                if n == 0 {
                    return None;
                }
                n.min(1024)
            } else if matches!(count_str, "auto-fill" | "auto-fit") {
                1
            } else {
                return None;
            };
            for _ in 0..count {
                out.extend_from_slice(&pat);
            }
        } else if let Some(inner) = c.strip_prefix("minmax(").and_then(|r| r.strip_suffix(')')) {
            let comma = find_top_level(inner, &[','])?;
            let min_b = parse_track_bound(inner[..comma].trim())?;
            let max_b = parse_track_bound(inner[comma + 1..].trim())?;
            out.push(SpecTrackSize::MinMax(min_b, max_b));
        } else {
            out.push(parse_single_track(c)?);
        }
    }
    Some(())
}

fn parse_track_list(v: &str) -> Option<Vec<SpecTrackSize>> {
    if v == "none" {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    parse_track_into(v, &mut out)?;
    Some(out)
}

fn parse_grid_placement(v: &str) -> Option<GridPlacement> {
    if v == "auto" {
        return Some(GridPlacement::default());
    }
    let (start_s, end_s) = match v.split_once('/') {
        Some((a, b)) => (a.trim(), Some(b.trim())),
        None => (v.trim(), None),
    };
    let parse_span = |s: &str| -> Option<u16> {
        s.strip_prefix("span")
            .and_then(|r| r.trim().parse::<u16>().ok())
            .filter(|n| *n > 0)
    };
    if end_s.is_none()
        && let Some(sp) = parse_span(start_s)
    {
        return Some(GridPlacement {
            start: None,
            span: Some(sp),
        });
    }
    let start = if start_s == "auto" {
        None
    } else {
        start_s.parse::<i16>().ok()
    };
    let span = match end_s {
        None => None,
        Some("auto") => None,
        Some(es) => {
            if let Some(sp) = parse_span(es) {
                Some(sp)
            } else if let (Some(st), Ok(en)) = (start, es.parse::<i16>())
                && en > st
            {
                Some((en - st) as u16)
            } else {
                None
            }
        }
    };
    if start.is_none() && span.is_none() {
        return None;
    }
    Some(GridPlacement { start, span })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folha_com_media_e_comentarios() {
        let css = "/* x */ body { margin: 0; color: #333 }\n\
                   @media (max-width: 600px) { p { margin: 1em 2em } }\n\
                   @font-face { font-family: X }\n\
                   a:hover { color: red } div{padding:1px 2px 3px 4px !important}";
        let rules = parse_stylesheet(css);
        assert_eq!(rules.len(), 4);
        assert_eq!(rules[0].decls.normal.len(), 5);
        assert!(rules[1].media.as_ref().unwrap().matches(500.0));
        assert!(!rules[1].media.as_ref().unwrap().matches(800.0));
        assert_eq!(rules[3].decls.important.len(), 4);
    }

    #[test]
    fn comprimentos() {
        assert_eq!(parse_length("1.5em"), Some(SpecLength::Em(1.5)));
        assert_eq!(parse_length("10px"), Some(SpecLength::Px(10.0)));
        assert_eq!(parse_length("0"), Some(SpecLength::Px(0.0)));
        assert_eq!(parse_length("50%"), Some(SpecLength::Percent(50.0)));
        assert_eq!(parse_length("2rem"), Some(SpecLength::Rem(2.0)));
        assert_eq!(parse_length("abc"), None);
    }

    #[test]
    fn flex_grid_e_atalhos() {
        let d = parse_declarations(
            "display: flex; flex: 1 0 20px; gap: 8px 16px; \
             grid-template-columns: 100px repeat(2, 1fr 20%) auto; \
             box-sizing: border-box; opacity: 50%; overflow: hidden scroll; \
             position: relative; inset: 4px 8px; z-index: 10; \
             background: url(x.png) rgba(10, 20, 30, 0.5); border-radius: 50%;",
        );
        assert!(d.normal.iter().any(|x| matches!(x, Decl::Display(Display::Flex))));
        assert!(d.normal.iter().any(|x| matches!(x, Decl::FlexGrow(g) if (*g - 1.0).abs() < 1e-5)));
        assert!(d.normal.iter().any(|x| matches!(x, Decl::FlexShrink(s) if *s == 0.0)));
        assert!(d.normal.iter().any(|x| matches!(x, Decl::FlexBasis(SpecLength::Px(20.0)))));
        assert!(d.normal.iter().any(|x| matches!(x, Decl::RowGap(SpecLength::Px(8.0)))));
        assert!(d.normal.iter().any(|x| matches!(x, Decl::ColumnGap(SpecLength::Px(16.0)))));
        assert!(d.normal.iter().any(|x| matches!(x, Decl::BoxSizing(BoxSizing::BorderBox))));
        assert!(d.normal.iter().any(|x| matches!(x, Decl::Opacity(a) if (*a - 0.5).abs() < 1e-5)));
        assert!(d.normal.iter().any(|x| matches!(x, Decl::OverflowX(Overflow::Hidden))));
        assert!(d.normal.iter().any(|x| matches!(x, Decl::OverflowY(Overflow::Scroll))));
        assert!(d.normal.iter().any(|x| matches!(x, Decl::Position(Position::Relative))));
        assert!(d.normal.iter().any(|x| matches!(x, Decl::ZIndex(Some(10)))));
        let cols = d.normal.iter().find_map(|x| match x {
            Decl::GridTemplateColumns(t) => Some(t),
            _ => None,
        }).unwrap();
        assert_eq!(cols.len(), 6);
    }
}
