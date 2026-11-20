//! Estilo do Nous: cascata CSS que produz um [`ComputedStyle`] por elemento.
//!
//! Estado atual: cascata própria e enxuta (seletores, especificidade, `!important`,
//! `@media` por largura, herança). Ela existe para o motor desenhar páginas desde já;
//! o próximo passo do plano é colocar o **Stylo** (o CSS do Firefox) atrás desta mesma API.

pub mod color;
pub mod parse;
pub mod selector;
pub mod values;

use std::collections::HashMap;
use std::sync::Arc;

use nous_dom::{Document, LocalName, NodeData, NodeId};

use parse::{ColorValue, FontSizeValue, FontWeightValue, LineHeightValue, SpecLength};
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
        self.styles.capacity() * std::mem::size_of::<Option<Arc<ComputedStyle>>>()
            + self.styles.iter().flatten().count() * std::mem::size_of::<ComputedStyle>()
    }
}

/// Calcula o estilo de todos os nós do documento.
pub fn compute_styles(doc: &Document, stylist: &Stylist, viewport: Viewport) -> StyleMap {
    let mut styles: Vec<Option<Arc<ComputedStyle>>> = vec![None; doc.len()];
    let root_style = Arc::new(ComputedStyle {
        display: Display::Block,
        ..ComputedStyle::initial()
    });
    let mut matched = Vec::new();
    let mut root_font_size = 16.0;

    for node in doc.descendants(NodeId::DOCUMENT) {
        let parent_style = match doc.node(node).parent {
            Some(p) => styles[p.index()]
                .clone()
                .unwrap_or_else(|| root_style.clone()),
            None => {
                styles[node.index()] = Some(root_style.clone());
                continue;
            }
        };
        let style = match &doc.node(node).data {
            NodeData::Element(el) => {
                stylist.matching_rules(doc, node, viewport, &mut matched);
                // ordem da cascata: origem, especificidade, ordem no código
                matched.sort_by_key(|&i| {
                    let r = &stylist.rules[i as usize];
                    (r.origin, r.specificity, r.order)
                });
                let inline = el.attr("style").map(parse::parse_declarations);
                let normal = matched
                    .iter()
                    .flat_map(|&i| stylist.rules[i as usize].decls.normal.iter())
                    .chain(inline.iter().flat_map(|b| b.normal.iter()));
                // !important: autor antes, depois agente do usuário (ordem inversa)
                let important_author = matched
                    .iter()
                    .filter(|&&i| stylist.rules[i as usize].origin == Origin::Author)
                    .flat_map(|&i| stylist.rules[i as usize].decls.important.iter())
                    .chain(inline.iter().flat_map(|b| b.important.iter()));
                let important_ua = matched
                    .iter()
                    .filter(|&&i| stylist.rules[i as usize].origin == Origin::UserAgent)
                    .flat_map(|&i| stylist.rules[i as usize].decls.important.iter());
                let decls: Vec<&Decl> =
                    normal.chain(important_author).chain(important_ua).collect();
                let is_root = doc.node(node).parent == Some(NodeId::DOCUMENT);
                let mut computed = compute(&parent_style, &decls, root_font_size, viewport);
                presentational_hints(el, &mut computed);
                if is_root {
                    root_font_size = computed.font_size;
                }
                Arc::new(computed)
            }
            _ => parent_style,
        };
        styles[node.index()] = Some(style);
    }
    StyleMap { styles }
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
        && let Some(l) = parse::parse_length(if w.ends_with('%') {
            w
        } else {
            w.trim_end_matches("px")
        })
        .or_else(|| w.parse::<f32>().ok().map(SpecLength::Px))
    {
        match l {
            SpecLength::Px(v) if s.width.is_auto() => s.width = Length::Px(v),
            SpecLength::Percent(p) if s.width.is_auto() => s.width = Length::Percent(p),
            _ => {}
        }
    }
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

fn compute(parent: &ComputedStyle, decls: &[&Decl], root_font: f32, vp: Viewport) -> ComputedStyle {
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
            Decl::BorderRadius(l) => s.border_radius = len(*l).resolve(0.0).unwrap_or(0.0),
            Decl::Width(l) => s.width = len(*l),
            Decl::Height(l) => s.height = len(*l),
            Decl::MinHeight(l) => s.min_height = len(*l),
            Decl::MaxWidth(l) => s.max_width = len(*l),
            Decl::ListStyle(l) => s.list_style = *l,
            Decl::Visibility(v) => s.visible = *v,
            Decl::Color(_) | Decl::FontSize(_) => {}
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
}
