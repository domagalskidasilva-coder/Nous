//! Estilo do Nous: cascata CSS que produz um [`ComputedStyle`] por elemento.
//!
//! Estado atual: cascata própria e enxuta (seletores, especificidade, `!important`,
//! `@media` por largura, herança). Ela existe para o motor desenhar páginas desde já;
//! o próximo passo do plano é colocar o **Stylo** (o CSS do Firefox) atrás desta mesma API.

pub mod color;
pub mod parse;
pub mod selector;
pub mod values;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use nous_dom::{Document, LocalName, NodeData, NodeId};

use parse::{
    ColorValue, FontSizeValue, FontWeightValue, LineHeightValue, SpecLength, SpecTrackBound,
    SpecTrackSize,
};
pub use parse::{Decl, DeclBlock, MediaList};
use selector::Selector;
pub use values::*;

/// Folha de estilo padrão do navegador (subconjunto da folha da especificação HTML).
pub const UA_STYLESHEET: &str = include_str!("ua.css");

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Origin {
    UserAgent = 0,
    Author = 1,
}

struct Rule {
    selector: Selector,
    specificity: u32,
    origin: Origin,
    order: u32,
    decls: Arc<DeclBlock>,
    media: Option<Arc<MediaList>>,
}

/// Todas as regras de uma página, indexadas pela parte mais à direita do seletor
/// (id, classe, tag ou universal), para não testar todas as regras em todo elemento.
#[derive(Default)]
pub struct Stylist {
    rules: Vec<Rule>,
    by_id: HashMap<Box<str>, Vec<u32>>,
    by_class: HashMap<Box<str>, Vec<u32>>,
    by_tag: HashMap<LocalName, Vec<u32>>,
    universal: Vec<u32>,
}

impl Stylist {
    pub fn with_user_agent_sheet() -> Self {
        let mut s = Stylist::default();
        s.add_sheet(UA_STYLESHEET, Origin::UserAgent, None);
        s
    }

    pub fn add_author_sheet(&mut self, css: &str, media: Option<Arc<MediaList>>) {
        self.add_sheet(css, Origin::Author, media);
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    fn add_sheet(&mut self, css: &str, origin: Origin, sheet_media: Option<Arc<MediaList>>) {
        for parsed in parse::parse_stylesheet(css) {
            // @media da regra tem precedência; se não houver, vale a do <link media>
            let media = parsed.media.or_else(|| sheet_media.clone());
            for selector in parsed.selectors {
                if selector.rightmost().never {
                    continue;
                }
                let idx = self.rules.len() as u32;
                let right = selector.rightmost();
                if let Some(id) = &right.id {
                    self.by_id.entry(id.clone()).or_default().push(idx);
                } else if let Some(class) = right.classes.first() {
                    self.by_class.entry(class.clone()).or_default().push(idx);
                } else if let Some(tag) = &right.tag {
                    self.by_tag.entry(tag.clone()).or_default().push(idx);
                } else {
                    self.universal.push(idx);
                }
                self.rules.push(Rule {
                    specificity: selector.specificity(),
                    selector,
                    origin,
                    order: idx,
                    decls: parsed.decls.clone(),
                    media: media.clone(),
                });
            }
        }
    }

    fn matching_rules(&self, doc: &Document, node: NodeId, viewport: Viewport, out: &mut Vec<u32>) {
        out.clear();
        let Some(el) = doc.element(node) else { return };
        let mut consider = |list: Option<&Vec<u32>>| {
            for &i in list.into_iter().flatten() {
                let r = &self.rules[i as usize];
                if r.media.as_ref().is_some_and(|m| !m.matches(viewport.width)) {
                    continue;
                }
                if selector::matches(doc, node, &r.selector) {
                    out.push(i);
                }
            }
        };
        if let Some(id) = el.id() {
            consider(self.by_id.get(id));
        }
        if let Some(classes) = el.attr("class") {
            for c in classes.split_ascii_whitespace() {
                consider(self.by_class.get(c));
            }
        }
        consider(self.by_tag.get(el.local_name()));
        consider(Some(&self.universal));
        out.sort_unstable();
        out.dedup();
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub width: f32,
    pub height: f32,
}

/// Estilo computado de cada nó. Nós de texto apontam para o estilo do pai.
pub struct StyleMap {
    styles: Vec<Option<Arc<ComputedStyle>>>,
}

impl StyleMap {
    pub fn get(&self, node: NodeId) -> Option<&Arc<ComputedStyle>> {
        self.styles.get(node.index())?.as_ref()
    }

    pub fn approx_heap_bytes(&self) -> usize {
        let mut seen = HashSet::new();
        let mut unique = 0usize;
        for s in self.styles.iter().flatten() {
            if seen.insert(Arc::as_ptr(s) as usize) {
                unique += 1;
            }
        }
        self.styles.capacity() * std::mem::size_of::<Option<Arc<ComputedStyle>>>()
            + unique * std::mem::size_of::<ComputedStyle>()
    }
}

struct CustomPropsNode {
    parent: Option<Arc<CustomPropsNode>>,
    entries: Vec<(Box<str>, Box<str>)>,
}

impl CustomPropsNode {
    fn get(&self, name: &str) -> Option<&str> {
        if let Some((_, val)) = self.entries.iter().rev().find(|(k, _)| &**k == name) {
            return Some(val);
        }
        self.parent.as_deref().and_then(|p| p.get(name))
    }
}

fn is_css_ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || !c.is_ascii()
}

fn find_matching_paren_quoted(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') if depth == 0 => return Some(i),
            (None, ')') => depth -= 1,
            _ => {}
        }
    }
    None
}

fn split_first_top_comma(s: &str) -> Option<(&str, &str)> {
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth -= 1,
            (None, ',') if depth == 0 => return Some((&s[..i], &s[i + 1..])),
            _ => {}
        }
    }
    None
}

fn resolve_css_vars(
    input: &str,
    vars: Option<&CustomPropsNode>,
    depth: u8,
    visiting: &mut Vec<String>,
) -> Option<String> {
    if depth > 32 {
        return None;
    }
    if !input
        .as_bytes()
        .windows(4)
        .any(|w| w.eq_ignore_ascii_case(b"var("))
    {
        return Some(input.to_owned());
    }
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    let mut quote: Option<char> = None;
    let bytes = input.as_bytes();
    while i < bytes.len() {
        let ch = input[i..].chars().next()?;
        if let Some(q) = quote {
            out.push(ch);
            if ch == q {
                quote = None;
            }
            i += ch.len_utf8();
            continue;
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            out.push(ch);
            i += 1;
            continue;
        }
        let is_var_start = i + 4 <= bytes.len()
            && bytes[i..i + 4].eq_ignore_ascii_case(b"var(")
            && (i == 0
                || input[..i]
                    .chars()
                    .next_back()
                    .is_none_or(|p| !is_css_ident_char(p)));
        if is_var_start {
            let rest = &input[i + 4..];
            let close = find_matching_paren_quoted(rest)?;
            let inner = &rest[..close];
            let (raw_name, fallback) = match split_first_top_comma(inner) {
                Some((n, fb)) => (n.trim(), Some(fb.trim_start())),
                None => (inner.trim(), None),
            };
            if !raw_name.starts_with("--") {
                return None;
            }
            let mut resolved_val = None;
            if !visiting.iter().any(|v| v == raw_name)
                && let Some(raw_var) = vars.and_then(|m| m.get(raw_name))
            {
                visiting.push(raw_name.to_owned());
                resolved_val = resolve_css_vars(raw_var, vars, depth + 1, visiting);
                visiting.pop();
            }
            if resolved_val.is_none()
                && let Some(fb) = fallback
            {
                resolved_val = resolve_css_vars(fb, vars, depth + 1, visiting);
            }
            let val = resolved_val?;
            out.push_str(&val);
            i += 4 + close + 1;
        } else {
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    Some(out)
}

/// Calcula o estilo de todos os nós do documento.
pub fn compute_styles(doc: &Document, stylist: &Stylist, viewport: Viewport) -> StyleMap {
    let mut styles: Vec<Option<Arc<ComputedStyle>>> = vec![None; doc.len()];
    let mut node_vars: Vec<Option<Arc<CustomPropsNode>>> = vec![None; doc.len()];
    let root_style = Arc::new(ComputedStyle {
        display: Display::Block,
        ..ComputedStyle::initial()
    });
    let mut matched = Vec::new();
    let mut decls: Vec<Decl> = Vec::new();
    let mut inline_block = DeclBlock::default();
    let mut local_vars: Vec<(Box<str>, Box<str>)> = Vec::new();
    let mut expanded_decls: Vec<Decl> = Vec::new();
    let mut var_parse_buf: Vec<Decl> = Vec::new();
    let mut visiting: Vec<String> = Vec::new();
    let mut child_style_cache: HashMap<usize, Arc<ComputedStyle>> = HashMap::new();
    let mut root_font_size = 16.0;

    for node in doc.descendants(NodeId::DOCUMENT) {
        let parent_id = doc.node(node).parent;
        let parent_style = match parent_id {
            Some(p) => styles[p.index()]
                .clone()
                .unwrap_or_else(|| root_style.clone()),
            None => {
                styles[node.index()] = Some(root_style.clone());
                continue;
            }
        };
        let parent_vars = parent_id.and_then(|p| node_vars[p.index()].clone());
        let style = match &doc.node(node).data {
            NodeData::Element(el) => {
                stylist.matching_rules(doc, node, viewport, &mut matched);
                // ordem da cascata: origem, especificidade, ordem no código
                matched.sort_by_key(|&i| {
                    let r = &stylist.rules[i as usize];
                    (r.origin, r.specificity, r.order)
                });
                let has_inline = if let Some(inline_str) = el.attr("style") {
                    parse::parse_declarations_into(inline_str, &mut inline_block);
                    true
                } else {
                    inline_block.normal.clear();
                    inline_block.important.clear();
                    false
                };
                decls.clear();
                local_vars.clear();
                let mut has_var_prop = false;

                let mut push_decl = |d: &Decl| match d {
                    Decl::CustomProp { name, value } => {
                        if let Some((_, existing)) = local_vars.iter_mut().find(|(k, _)| k == name)
                        {
                            *existing = value.clone();
                        } else {
                            local_vars.push((name.clone(), value.clone()));
                        }
                    }
                    Decl::VarProp { .. } => {
                        has_var_prop = true;
                        decls.push(d.clone());
                    }
                    _ => decls.push(d.clone()),
                };

                for &i in &matched {
                    for d in &stylist.rules[i as usize].decls.normal {
                        push_decl(d);
                    }
                }
                if has_inline {
                    for d in &inline_block.normal {
                        push_decl(d);
                    }
                }
                // !important: autor antes, depois agente do usuário (ordem inversa)
                for &i in &matched {
                    let r = &stylist.rules[i as usize];
                    if r.origin == Origin::Author {
                        for d in &r.decls.important {
                            push_decl(d);
                        }
                    }
                }
                if has_inline {
                    for d in &inline_block.important {
                        push_decl(d);
                    }
                }
                for &i in &matched {
                    let r = &stylist.rules[i as usize];
                    if r.origin == Origin::UserAgent {
                        for d in &r.decls.important {
                            push_decl(d);
                        }
                    }
                }

                let elem_vars = if local_vars.is_empty() {
                    parent_vars
                } else {
                    Some(Arc::new(CustomPropsNode {
                        parent: parent_vars,
                        entries: std::mem::take(&mut local_vars),
                    }))
                };
                let is_root = parent_id == Some(NodeId::DOCUMENT);

                let computed_arc =
                    if !is_root && decls.is_empty() && !has_presentational_hints(el) {
                        if parent_style.has_initial_non_inherited() {
                            parent_style.clone()
                        } else {
                            let key = Arc::as_ptr(&parent_style) as usize;
                            child_style_cache
                                .entry(key)
                                .or_insert_with(|| {
                                    Arc::new(ComputedStyle::inherit_from(&parent_style))
                                })
                                .clone()
                        }
                    } else {
                        let final_decls: &[Decl] = if has_var_prop {
                            expanded_decls.clear();
                            for d in &decls {
                                match d {
                                    Decl::VarProp { name, value } => {
                                        visiting.clear();
                                        if let Some(resolved) = resolve_css_vars(
                                            value,
                                            elem_vars.as_deref(),
                                            0,
                                            &mut visiting,
                                        ) {
                                            var_parse_buf.clear();
                                            parse::parse_property(
                                                name,
                                                &resolved,
                                                &mut var_parse_buf,
                                            );
                                            expanded_decls.append(&mut var_parse_buf);
                                        }
                                    }
                                    other => expanded_decls.push(other.clone()),
                                }
                            }
                            &expanded_decls
                        } else {
                            &decls
                        };
                        let mut computed =
                            compute(&parent_style, final_decls, root_font_size, viewport);
                        presentational_hints(el, &mut computed);
                        if is_root {
                            root_font_size = computed.font_size;
                        }
                        Arc::new(computed)
                    };
                node_vars[node.index()] = elem_vars;
                computed_arc
            }
            _ => {
                node_vars[node.index()] = parent_vars;
                parent_style
            }
        };
        styles[node.index()] = Some(style);
    }
    StyleMap { styles }
}

fn has_presentational_hints(el: &nous_dom::ElementData) -> bool {
    el.attr("bgcolor").is_some()
        || el.attr("align").is_some()
        || el.attr("width").is_some()
        || (&**el.local_name() == "font" && el.attr("color").is_some())
        || (&**el.local_name() == "img" && el.attr("height").is_some())
}

/// Atributos HTML antigos que viram estilo (`<font color>`, `<td bgcolor>`, `align`…).
fn presentational_hints(el: &nous_dom::ElementData, s: &mut ComputedStyle) {
    if let Some(c) = el.attr("bgcolor").and_then(color::parse_color) {
        s.background_color = c;
    }
    if &**el.local_name() == "font"
        && let Some(c) = el.attr("color").and_then(color::parse_color)
    {
        s.color = c;
    }
    if let Some(a) = el.attr("align") {
        match a.to_ascii_lowercase().as_str() {
            "center" | "middle" => s.text_align = TextAlign::Center,
            "right" => s.text_align = TextAlign::Right,
            _ => {}
        }
    }
    if let Some(w) = el.attr("width")
        && let Some(l) = parse_html_length(w)
    {
        match l {
            SpecLength::Px(v) if s.width.is_auto() => s.width = Length::Px(v),
            SpecLength::Percent(p) if s.width.is_auto() => s.width = Length::Percent(p),
            _ => {}
        }
    }
    if &**el.local_name() == "img"
        && let Some(h) = el.attr("height")
        && let Some(l) = parse_html_length(h)
    {
        match l {
            SpecLength::Px(v) if s.height.is_auto() => s.height = Length::Px(v),
            SpecLength::Percent(p) if s.height.is_auto() => s.height = Length::Percent(p),
            _ => {}
        }
    }
}

fn parse_html_length(raw: &str) -> Option<SpecLength> {
    let raw = raw.trim();
    parse::parse_length(if raw.ends_with('%') {
        raw
    } else {
        raw.trim_end_matches("px")
    })
    .or_else(|| raw.parse::<f32>().ok().map(SpecLength::Px))
}

fn resolve_len(l: SpecLength, font_size: f32, root_font: f32, vp: Viewport) -> Length {
    match l {
        SpecLength::Px(v) => Length::Px(v),
        SpecLength::Em(v) => Length::Px(v * font_size),
        SpecLength::Rem(v) => Length::Px(v * root_font),
        SpecLength::Percent(p) => Length::Percent(p),
        SpecLength::Vw(v) => Length::Px(v * vp.width / 100.0),
        SpecLength::Vh(v) => Length::Px(v * vp.height / 100.0),
        SpecLength::Auto => Length::Auto,
    }
}

fn resolve_track_bound(b: SpecTrackBound, font_size: f32, root_font: f32, vp: Viewport) -> TrackBound {
    match b {
        SpecTrackBound::Length(l) => match resolve_len(l, font_size, root_font, vp) {
            Length::Px(px) => TrackBound::Px(px),
            Length::Percent(p) => TrackBound::Percent(p),
            Length::Auto => TrackBound::Auto,
        },
        SpecTrackBound::Fr(f) => TrackBound::Fr(f),
        SpecTrackBound::MinContent => TrackBound::MinContent,
        SpecTrackBound::MaxContent => TrackBound::MaxContent,
    }
}

fn resolve_track(t: SpecTrackSize, font_size: f32, root_font: f32, vp: Viewport) -> TrackSize {
    match t {
        SpecTrackSize::Length(l) => match resolve_len(l, font_size, root_font, vp) {
            Length::Px(px) => TrackSize::Px(px),
            Length::Percent(p) => TrackSize::Percent(p),
            Length::Auto => TrackSize::Auto,
        },
        SpecTrackSize::Fr(f) => TrackSize::Fr(f),
        SpecTrackSize::MinContent => TrackSize::MinContent,
        SpecTrackSize::MaxContent => TrackSize::MaxContent,
        SpecTrackSize::MinMax(a, b) => TrackSize::MinMax(
            resolve_track_bound(a, font_size, root_font, vp),
            resolve_track_bound(b, font_size, root_font, vp),
        ),
    }
}

fn compute(parent: &ComputedStyle, decls: &[Decl], root_font: f32, vp: Viewport) -> ComputedStyle {
    let mut s = ComputedStyle::inherit_from(parent);
    // 1ª passada: tamanho da fonte (outras unidades `em` dependem dele)
    for d in decls {
        if let Decl::FontSize(fs) = d {
            s.font_size = match *fs {
                FontSizeValue::Px(px) => px,
                FontSizeValue::Larger => parent.font_size * 1.2,
                FontSizeValue::Smaller => parent.font_size / 1.2,
                FontSizeValue::Length(l) => match l {
                    SpecLength::Em(v) => v * parent.font_size,
                    SpecLength::Percent(p) => p / 100.0 * parent.font_size,
                    other => resolve_len(other, parent.font_size, root_font, vp)
                        .resolve(parent.font_size)
                        .unwrap_or(parent.font_size),
                },
            };
        }
    }
    let fs = s.font_size;
    let len = |l: SpecLength| resolve_len(l, fs, root_font, vp);
    let mut border_style = [false; 4];
    let mut border_width = [3.0f32; 4];
    let mut border_color: [Option<ColorValue>; 4] = [None; 4];
    let mut color = s.color;

    for d in decls {
        match d {
            Decl::Color(ColorValue::Rgba(c)) => color = *c,
            Decl::Color(ColorValue::CurrentColor) => color = parent.color,
            _ => {}
        }
    }
    s.color = color;
    let resolve_color = |c: ColorValue| match c {
        ColorValue::Rgba(c) => c,
        ColorValue::CurrentColor => color,
    };

    for d in decls {
        match d {
            Decl::Display(v) => s.display = *v,
            Decl::BackgroundColor(c) => s.background_color = resolve_color(*c),
            Decl::FontWeight(w) => {
                s.font_weight = match w {
                    FontWeightValue::Absolute(v) => *v,
                    FontWeightValue::Bolder => {
                        if parent.font_weight < 600 {
                            700
                        } else {
                            900
                        }
                    }
                    FontWeightValue::Lighter => {
                        if parent.font_weight > 500 {
                            400
                        } else {
                            100
                        }
                    }
                }
            }
            Decl::FontStyle(i) => s.italic = *i,
            Decl::FontFamily(f) => s.font_family = f.clone(),
            Decl::LineHeight(lh) => {
                s.line_height = match lh {
                    LineHeightValue::Normal => LineHeight::Normal,
                    LineHeightValue::Number(n) => LineHeight::Number(*n),
                    LineHeightValue::Length(l) => match len(*l) {
                        Length::Px(px) => LineHeight::Px(px),
                        Length::Percent(p) => LineHeight::Px(p / 100.0 * fs),
                        Length::Auto => LineHeight::Normal,
                    },
                }
            }
            Decl::TextAlign(a) => s.text_align = *a,
            Decl::WhiteSpace(w) => s.white_space = *w,
            Decl::TextDecoration {
                underline,
                line_through,
            } => {
                s.underline = *underline;
                s.line_through = *line_through;
            }
            Decl::Margin(side, l) => s.margin[*side] = len(*l),
            Decl::Padding(side, l) => s.padding[*side] = len(*l),
            Decl::BorderWidth(side, l) => {
                border_width[*side] = len(*l).resolve(0.0).unwrap_or(3.0).max(0.0)
            }
            Decl::BorderStyle(side, visible) => border_style[*side] = *visible,
            Decl::BorderColor(side, c) => border_color[*side] = Some(*c),
            Decl::BorderRadius(l) => {
                let r = len(*l);
                s.border_radius_length = r;
                s.border_radius = r.resolve(0.0).unwrap_or(0.0);
            }
            Decl::Width(l) => s.width = len(*l),
            Decl::Height(l) => s.height = len(*l),
            Decl::MinWidth(l) => s.min_width = len(*l),
            Decl::MinHeight(l) => s.min_height = len(*l),
            Decl::MaxWidth(l) => s.max_width = len(*l),
            Decl::MaxHeight(l) => s.max_height = len(*l),
            Decl::BoxSizing(b) => s.box_sizing = *b,
            Decl::OverflowX(o) => s.overflow_x = *o,
            Decl::OverflowY(o) => s.overflow_y = *o,
            Decl::Opacity(a) => s.opacity = *a,
            Decl::Position(p) => s.position = *p,
            Decl::Inset(side, l) => s.inset[*side] = len(*l),
            Decl::ZIndex(z) => s.z_index = *z,
            Decl::FlexDirection(d) => s.flex_direction = *d,
            Decl::FlexWrap(w) => s.flex_wrap = *w,
            Decl::JustifyContent(j) => s.justify_content = *j,
            Decl::AlignItems(a) => s.align_items = *a,
            Decl::AlignSelf(a) => s.align_self = *a,
            Decl::FlexGrow(g) => s.flex_grow = *g,
            Decl::FlexShrink(sh) => s.flex_shrink = *sh,
            Decl::FlexBasis(b) => s.flex_basis = len(*b),
            Decl::RowGap(g) => s.row_gap = len(*g),
            Decl::ColumnGap(g) => s.column_gap = len(*g),
            Decl::GridTemplateColumns(ts) => {
                s.grid_template_columns = ts
                    .iter()
                    .map(|&t| resolve_track(t, fs, root_font, vp))
                    .collect();
            }
            Decl::GridTemplateRows(ts) => {
                s.grid_template_rows = ts
                    .iter()
                    .map(|&t| resolve_track(t, fs, root_font, vp))
                    .collect();
            }
            Decl::GridColumn(g) => s.grid_column = *g,
            Decl::GridRow(g) => s.grid_row = *g,
            Decl::ListStyle(l) => s.list_style = *l,
            Decl::Visibility(v) => s.visible = *v,
            Decl::Color(_)
            | Decl::FontSize(_)
            | Decl::CustomProp { .. }
            | Decl::VarProp { .. } => {}
        }
    }
    for i in 0..4 {
        s.border_width[i] = if border_style[i] {
            border_width[i]
        } else {
            0.0
        };
        s.border_color[i] = border_color[i].map(resolve_color).unwrap_or(color);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use nous_dom::{ElementData, QualName, ns};

    fn element(doc: &mut Document, parent: NodeId, name: &str, attrs: &[(&str, &str)]) -> NodeId {
        let id = doc.create(NodeData::Element(ElementData {
            name: QualName::new(None, ns!(html), LocalName::from(name)),
            attrs: attrs
                .iter()
                .map(|(k, v)| nous_dom::Attribute {
                    name: QualName::new(None, ns!(), LocalName::from(*k)),
                    value: (*v).into(),
                })
                .collect(),
            template_contents: None,
        }));
        doc.append(parent, id);
        id
    }

    #[test]
    fn cascata_basica() {
        let mut doc = Document::new();
        let html = element(&mut doc, NodeId::DOCUMENT, "html", &[]);
        let body = element(&mut doc, html, "body", &[]);
        let p = element(
            &mut doc,
            body,
            "p",
            &[("class", "x"), ("style", "color: blue")],
        );
        let h1 = element(&mut doc, body, "h1", &[("id", "t")]);
        let mut stylist = Stylist::with_user_agent_sheet();
        stylist.add_author_sheet(
            "body { font-size: 20px; color: red } .x { margin-top: 2em } #t { color: green !important } h1 { color: black }",
            None,
        );
        let vp = Viewport {
            width: 800.0,
            height: 600.0,
        };
        let styles = compute_styles(&doc, &stylist, vp);
        let ps = styles.get(p).unwrap();
        assert_eq!(ps.display, Display::Block);
        assert_eq!(ps.color, Rgba::new(0, 0, 255, 255));
        assert_eq!(ps.margin[0], Length::Px(40.0));
        assert_eq!(ps.font_size, 20.0);
        let hs = styles.get(h1).unwrap();
        assert_eq!(hs.color, Rgba::new(0, 128, 0, 255));
        assert_eq!(hs.font_size, 40.0);
        assert_eq!(hs.font_weight, 700);
        assert_eq!(styles.get(body).unwrap().margin[3], Length::Px(8.0));
    }

    #[test]
    fn custom_properties_e_sharing() {
        let mut doc = Document::new();
        let html = element(&mut doc, NodeId::DOCUMENT, "html", &[]);
        let body = element(&mut doc, html, "body", &[]);
        let wrap = element(&mut doc, body, "div", &[("class", "wrap")]);
        let child = element(&mut doc, wrap, "p", &[("class", "card")]);
        let s1 = element(&mut doc, child, "span", &[]);
        let s2 = element(&mut doc, s1, "span", &[]);
        let s3 = element(&mut doc, child, "span", &[]);

        let mut stylist = Stylist::with_user_agent_sheet();
        stylist.add_author_sheet(
            ":root { --fg: #112233; --pad: 12px; --chain: var(--pad); } \
             .wrap { --fg: rgb(10 20 30); } \
             .card { color: var(--fg); padding: var(--chain) var(--missing, 6px); \
                     background: var(--missing, #abcdef); opacity: 0.75; \
                     --loop-a: var(--loop-b); --loop-b: var(--loop-a); \
                     border-color: var(--loop-a, #00ff00); }",
            None,
        );
        let vp = Viewport {
            width: 800.0,
            height: 600.0,
        };
        let styles = compute_styles(&doc, &stylist, vp);
        let cs = styles.get(child).unwrap();
        assert_eq!(cs.color, Rgba::new(10, 20, 30, 255));
        assert_eq!(cs.padding[0], Length::Px(12.0));
        assert_eq!(cs.padding[1], Length::Px(6.0));
        assert_eq!(cs.background_color, Rgba::new(0xab, 0xcd, 0xef, 255));
        assert!((cs.opacity - 0.75).abs() < 1e-5);
        assert_eq!(cs.border_color[0], Rgba::new(0, 255, 0, 255));

        // Unstyled spans share child Arc<ComputedStyle> safely
        assert!(Arc::ptr_eq(styles.get(s1).unwrap(), styles.get(s2).unwrap()));
        assert!(Arc::ptr_eq(styles.get(s1).unwrap(), styles.get(s3).unwrap()));
        assert!(!Arc::ptr_eq(styles.get(child).unwrap(), styles.get(s1).unwrap()));
    }
}
