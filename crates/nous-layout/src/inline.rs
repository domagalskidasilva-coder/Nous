//! Conteúdo inline: achata texto e elementos de linha numa única string, com faixas
//! de estilo, aplicando o colapso de espaços do CSS (`white-space`).

use std::ops::Range;
use std::sync::Arc;

use nous_dom::{NodeData, NodeId, local_name};
use nous_style::{ComputedStyle, Display};

use crate::boxes::BoxBuilder;

pub(crate) struct Span {
    pub range: Range<usize>,
    pub style: Arc<ComputedStyle>,
    pub link: u32,
}

pub(crate) struct InlineCollector<'a, 'b> {
    builder: &'b BoxBuilder<'a>,
    link_urls: &'b mut Vec<String>,
    pub text: String,
    pub spans: Vec<Span>,
    /// O último caractere é um espaço colapsável (ou estamos no começo da linha).
    last_space: bool,
}

fn is_css_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0c')
}

impl<'a, 'b> InlineCollector<'a, 'b> {
    pub fn new(builder: &'b BoxBuilder<'a>, link_urls: &'b mut Vec<String>) -> Self {
        InlineCollector {
            builder,
            link_urls,
            text: String::new(),
            spans: Vec::new(),
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
                if el.is(&local_name!("img")) {
                    // imagens ainda não são desenhadas: mostra o texto alternativo
                    if let Some(alt) = el.attr("alt").filter(|a| !a.trim().is_empty()) {
                        let alt = alt.to_owned();
                        self.push_text(&alt, &style, link);
                    }
                    return;
                }
                let link = match el.attr("href") {
                    Some(href) if el.is(&local_name!("a")) || el.is(&local_name!("area")) => {
                        self.link_urls.push(href.to_owned());
                        self.link_urls.len() as u32
                    }
                    _ => link,
                };
                let children: Vec<NodeId> = doc.children(node).collect();
                for c in children {
                    self.collect(c, link);
                }
            }
            _ => {}
        }
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

    /// Remove o espaço colapsável do fim e devolve texto e faixas.
    pub fn finish(mut self) -> (String, Vec<Span>) {
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
        (self.text, self.spans)
    }
}
