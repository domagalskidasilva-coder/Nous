//! Conteúdo inline: achata texto, elementos de linha, imagens, controles de formulário
//! e caixas `inline-block` / `inline-flex` numa única string com faixas de estilo e
//! caixas inline (`U+FFFC`) para o Parley.

use std::ops::Range;
use std::sync::Arc;

use nous_dom::{NodeData, NodeId, local_name};
use nous_style::{ComputedStyle, Display};

use crate::boxes::{BoxBuilder, is_replaced_or_control};
use crate::forms::{
    ControlInfo, FormControlArea, extract_control_info, is_form_control, resolve_control_box_size,
};
use crate::{ImageMap, LayoutCx, LayoutEngine, LinkArea, PaintItem, resolve_img_box_size};

pub(crate) struct Span {
    pub range: Range<usize>,
    pub style: Arc<ComputedStyle>,
    pub link: u32,
}

pub(crate) enum InlineReplacementKind {
    Img {
        node: NodeId,
        style: Arc<ComputedStyle>,
        pad: [f32; 4],
        bw: [f32; 4],
        ml: f32,
        mt: f32,
    },
    Control {
        node: NodeId,
        style: Arc<ComputedStyle>,
        info: ControlInfo,
        pad: [f32; 4],
        bw: [f32; 4],
        ml: f32,
        mt: f32,
    },
    Subtree {
        items: Vec<PaintItem>,
        links: Vec<LinkArea>,
        controls: Vec<FormControlArea>,
        ml: f32,
        mt: f32,
    },
    MeasureOnly,
}

pub(crate) struct InlineReplacement {
    pub index: usize,
    pub width: f32,
    pub height: f32,
    pub baseline: Option<f32>,
    pub link: u32,
    pub kind: InlineReplacementKind,
}

pub(crate) struct InlineCollector<'a, 'b> {
    builder: &'b BoxBuilder<'a>,
    engine: &'b mut LayoutEngine,
    images: &'b ImageMap,
    link_urls: &'b mut Vec<String>,
    avail_w: f32,
    paint_subtrees: bool,
    pub text: String,
    pub spans: Vec<Span>,
    pub replacements: Vec<InlineReplacement>,
    /// O último caractere é um espaço colapsável (ou estamos no começo da linha).
    last_space: bool,
}

fn is_css_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')
}

impl<'a, 'b> InlineCollector<'a, 'b> {
    pub fn new(
        builder: &'b BoxBuilder<'a>,
        engine: &'b mut LayoutEngine,
        images: &'b ImageMap,
        link_urls: &'b mut Vec<String>,
        avail_w: f32,
        paint_subtrees: bool,
    ) -> Self {
        InlineCollector {
            builder,
            engine,
            images,
            link_urls,
            avail_w,
            paint_subtrees,
            text: String::new(),
            spans: Vec::new(),
            replacements: Vec::new(),
            last_space: true,
        }
    }

    pub fn collect(&mut self, node: NodeId, link: u32) {
        let doc = self.builder.doc;
        match &doc.node(node).data {
            NodeData::Text(t) => {
                if let Some(style) = self.builder.style_of(node) {
                    let style = style.clone();
                    self.push_text(t, &style, link);
                }
            }
            NodeData::Element(el) => {
                let Some(style) = self.builder.style_of(node) else {
                    return;
                };
                if style.display == Display::None {
                    return;
                }
                let style = style.clone();
                if el.is(&local_name!("br")) {
                    self.push_raw("\n", &style, link);
                    self.last_space = true;
                    return;
                }

                let link = match el.attr("href") {
                    Some(href) if el.is(&local_name!("a")) || el.is(&local_name!("area")) => {
                        if self.paint_subtrees {
                            self.link_urls.push(href.to_owned());
                            self.link_urls.len() as u32
                        } else {
                            link.max(1)
                        }
                    }
                    _ => link,
                };

                if el.is(&local_name!("img")) {
                    if let Some((w, h, pad, bw)) = resolve_img_box_size(
                        el,
                        &style,
                        self.images.get(&node),
                        self.avail_w,
                        None,
                    ) {
                        let ml = style.margin[3].resolve_or_zero(self.avail_w).max(0.0);
                        let mr = style.margin[1].resolve_or_zero(self.avail_w).max(0.0);
                        let mt = style.margin[0].resolve_or_zero(self.avail_w).max(0.0);
                        let mb = style.margin[2].resolve_or_zero(self.avail_w).max(0.0);
                        let idx = self.push_object_replacement(&style, link);
                        self.replacements.push(InlineReplacement {
                            index: idx,
                            width: w + ml + mr,
                            height: h + mt + mb,
                            baseline: None,
                            link,
                            kind: if self.paint_subtrees {
                                InlineReplacementKind::Img {
                                    node,
                                    style,
                                    pad,
                                    bw,
                                    ml,
                                    mt,
                                }
                            } else {
                                InlineReplacementKind::MeasureOnly
                            },
                        });
                    } else if let Some(alt) = el.attr("alt").filter(|a| !a.trim().is_empty()) {
                        let alt = alt.to_owned();
                        self.push_text(&alt, &style, link);
                    }
                    return;
                }

                if is_form_control(el) {
                    if let Some(info) = extract_control_info(doc, node) {
                        let (w, h, pad, bw) =
                            resolve_control_box_size(self.engine, &info, &style, self.avail_w, None);
                        let ml = style.margin[3].resolve_or_zero(self.avail_w).max(0.0);
                        let mr = style.margin[1].resolve_or_zero(self.avail_w).max(0.0);
                        let mt = style.margin[0].resolve_or_zero(self.avail_w).max(0.0);
                        let mb = style.margin[2].resolve_or_zero(self.avail_w).max(0.0);
                        let idx = self.push_object_replacement(&style, link);
                        self.replacements.push(InlineReplacement {
                            index: idx,
                            width: w + ml + mr,
                            height: h + mt + mb,
                            baseline: Some(mt + h * 0.76),
                            link,
                            kind: if self.paint_subtrees {
                                InlineReplacementKind::Control {
                                    node,
                                    style,
                                    info,
                                    pad,
                                    bw,
                                    ml,
                                    mt,
                                }
                            } else {
                                InlineReplacementKind::MeasureOnly
                            },
                        });
                    }
                    return;
                }

                if matches!(style.display, Display::InlineBlock | Display::InlineFlex)
                    && !is_replaced_or_control(el)
                {
                    let ml = style.margin[3].resolve_or_zero(self.avail_w).max(0.0);
                    let mr = style.margin[1].resolve_or_zero(self.avail_w).max(0.0);
                    let mt = style.margin[0].resolve_or_zero(self.avail_w).max(0.0);
                    let mb = style.margin[2].resolve_or_zero(self.avail_w).max(0.0);
                    let box_item = self.builder.build_block(node);
                    let mut sub_cx = LayoutCx::new(
                        self.builder,
                        self.engine,
                        self.images,
                        self.builder.doc.document_element().unwrap_or(node),
                        self.avail_w,
                    );
                    sub_cx.link_urls = std::mem::take(self.link_urls);
                    let (w, h) =
                        sub_cx.layout_inline_atomic(&box_item, self.avail_w, self.paint_subtrees);
                    let (sub_items, sub_links, sub_controls, sub_urls) =
                        (sub_cx.items, sub_cx.links, sub_cx.controls, sub_cx.link_urls);
                    *self.link_urls = sub_urls;
                    let idx = self.push_object_replacement(&style, link);
                    self.replacements.push(InlineReplacement {
                        index: idx,
                        width: w + ml + mr,
                        height: h + mt + mb,
                        baseline: None,
                        link,
                        kind: if self.paint_subtrees {
                            InlineReplacementKind::Subtree {
                                items: sub_items,
                                links: sub_links,
                                controls: sub_controls,
                                ml,
                                mt,
                            }
                        } else {
                            InlineReplacementKind::MeasureOnly
                        },
                    });
                    return;
                }

                let children: Vec<NodeId> = doc.children(node).collect();
                for c in children {
                    self.collect(c, link);
                }
            }
            _ => {}
        }
    }

    fn push_object_replacement(&mut self, style: &Arc<ComputedStyle>, link: u32) -> usize {
        let start = self.text.len();
        self.text.push('\u{FFFC}');
        self.last_space = false;
        self.push_span(start, style, link);
        start
    }

    fn push_span(&mut self, start: usize, style: &Arc<ComputedStyle>, link: u32) {
        let end = self.text.len();
        if end > start {
            self.spans.push(Span {
                range: start..end,
                style: style.clone(),
                link,
            });
        }
    }

    fn push_raw(&mut self, s: &str, style: &Arc<ComputedStyle>, link: u32) {
        let start = self.text.len();
        self.text.push_str(s);
        self.push_span(start, style, link);
    }

    fn push_text(&mut self, t: &str, style: &Arc<ComputedStyle>, link: u32) {
        let ws = style.white_space;
        let start = self.text.len();
        if ws.collapses_spaces() {
            let keep_newlines = ws.keeps_newlines();
            for ch in t.chars() {
                if is_css_space(ch) {
                    if ch == '\n' && keep_newlines {
                        // pre-line: espaços antes da quebra somem, a quebra fica
                        while self.text.len() > start && self.text.ends_with(' ') {
                            self.text.pop();
                        }
                        self.text.push('\n');
                        self.last_space = true;
                    } else if !self.last_space {
                        self.text.push(' ');
                        self.last_space = true;
                    }
                } else {
                    self.text.push(ch);
                    self.last_space = false;
                }
            }
        } else {
            for ch in t.chars() {
                match ch {
                    '\r' => {}
                    // tab-size padrão do CSS é 8; 4 espaços ficam mais perto do que se vê
                    '\t' => self.text.push_str("    "),
                    c => self.text.push(c),
                }
            }
            self.last_space = self.text.ends_with('\n');
        }
        self.push_span(start, style, link);
    }

    /// Remove o espaço colapsável do fim e devolve texto, faixas e caixas inline.
    pub fn finish(mut self) -> (String, Vec<Span>, Vec<InlineReplacement>) {
        let trailing_collapsible = self
            .spans
            .last()
            .is_some_and(|s| s.style.white_space.collapses_spaces());
        if trailing_collapsible {
            while self.text.ends_with(' ') {
                self.text.pop();
            }
        }
        let len = self.text.len();
        self.spans.retain_mut(|s| {
            s.range.end = s.range.end.min(len);
            s.range.start < s.range.end
        });
        (self.text, self.spans, self.replacements)
    }
}
