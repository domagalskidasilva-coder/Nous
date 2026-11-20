//! Layout do Nous: transforma DOM + estilos numa lista de coisas para desenhar
//! (retângulos, bordas e blocos de texto já quebrados em linhas) e nas áreas de link.
//!
//! Estado atual: fluxo de blocos (com margens colapsando e centralização por
//! `margin: auto`) e texto inline com o Parley. Flex, grid e tabelas usam o Taffy
//! no próximo passo da Fase 1; por enquanto viram blocos empilhados.

mod boxes;
mod inline;

use std::borrow::Cow;

pub use parley;
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontStyle, FontWeight, Layout,
    LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty, TextWrapMode,
};

use nous_dom::{Document, NodeId};
use nous_style::{
    ComputedStyle, Display, GenericFamily, Length, ListStyle, Rgba, StyleMap, TextAlign,
};

use boxes::{BlockBox, BlockContent, BoxBuilder};
use inline::InlineCollector;

/// "Pincel" do texto: a cor e, se for link, o número do link (0 = sem link).
#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub struct TextBrush {
    pub color: [u8; 4],
    pub link: u32,
}

/// O único sistema de fontes do navegador: a interface e as páginas usam o mesmo,
/// então cada fonte é carregada e guardada uma vez só.
pub struct LayoutEngine {
    pub font_cx: FontContext,
    pub layout_cx: LayoutContext<TextBrush>,
}

impl LayoutEngine {
    pub fn new() -> Self {
        LayoutEngine {
            font_cx: FontContext::new(),
            layout_cx: LayoutContext::new(),
        }
    }
}

impl Default for LayoutEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Rect { x, y, w, h }
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }
}

pub enum PaintItem {
    Rect {
        rect: Rect,
        color: Rgba,
        radius: f32,
    },
    Border {
        rect: Rect,
        widths: [f32; 4],
        colors: [Rgba; 4],
    },
    /// O layout fica numa caixa: ele tem ~430 bytes e deixaria cada retângulo e borda
    /// da lista desse tamanho também.
    Text {
        x: f32,
        y: f32,
        bounds: Rect,
        layout: Box<Layout<TextBrush>>,
    },
}

impl PaintItem {
    pub fn bounds(&self) -> Rect {
        match self {
            PaintItem::Rect { rect, .. } | PaintItem::Border { rect, .. } => *rect,
            PaintItem::Text { bounds, .. } => *bounds,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinkArea {
    pub rect: Rect,
    pub link: u32,
}

pub struct PageLayout {
    pub width: f32,
    pub height: f32,
    pub canvas_color: Rgba,
    pub items: Vec<PaintItem>,
    pub links: Vec<LinkArea>,
    /// `href` original de cada link; o link `n` está em `link_urls[n - 1]`.
    pub link_urls: Vec<String>,
}

impl PageLayout {
    pub fn empty(width: f32) -> Self {
        PageLayout {
            width,
            height: 0.0,
            canvas_color: Rgba::WHITE,
            items: Vec::new(),
            links: Vec::new(),
            link_urls: Vec::new(),
        }
    }

    /// Link sob o ponto (coordenadas da página), se houver.
    pub fn link_at(&self, x: f32, y: f32) -> Option<&str> {
        let area = self.links.iter().rev().find(|l| l.rect.contains(x, y))?;
        self.link_urls
            .get(area.link.checked_sub(1)? as usize)
            .map(String::as_str)
    }

    /// Estimativa de memória do layout (para `nous://memoria`). Os layouts de texto do
    /// Parley não expõem o tamanho exato; contamos ~48 bytes por byte de texto
    /// (glifos, clusters e linhas).
    pub fn approx_heap_bytes(&self) -> usize {
        let mut total = self.items.capacity() * std::mem::size_of::<PaintItem>()
            + self.links.capacity() * std::mem::size_of::<LinkArea>()
            + self
                .link_urls
                .iter()
                .map(|u| u.capacity() + 24)
                .sum::<usize>();
        for item in &self.items {
            if let PaintItem::Text { layout, .. } = item {
                total += std::mem::size_of::<Layout<TextBrush>>() + layout.text_len() * 48;
            }
        }
        total
    }
}

/// Faz o layout do documento inteiro numa janela de `viewport_width` pixels de largura.
pub fn layout_document(
    doc: &Document,
    styles: &StyleMap,
    engine: &mut LayoutEngine,
    viewport_width: f32,
) -> PageLayout {
    let viewport_width = viewport_width.max(0.0);
    let Some(root) = doc.document_element() else {
        return PageLayout::empty(viewport_width);
    };
    let Some(root_style) = styles.get(root) else {
        return PageLayout::empty(viewport_width);
    };
    if root_style.display == Display::None {
        return PageLayout::empty(viewport_width);
    }

    let canvas_color = canvas_color(doc, styles, root);
    let builder = BoxBuilder { doc, styles };
    let root_box = builder.build_block(root);
    let mut cx = LayoutCx {
        builder: &builder,
        engine,
        root,
        items: Vec::new(),
        links: Vec::new(),
        link_urls: Vec::new(),
    };
    let mt = cx.margin_top(&root_box, viewport_width);
    let mb = cx.margin_bottom(&root_box, viewport_width);
    let h = cx.layout_block(&root_box, 0.0, viewport_width, mt);
    PageLayout {
        width: viewport_width,
        height: mt + h + mb,
        canvas_color,
        items: cx.items,
        links: cx.links,
        link_urls: cx.link_urls,
    }
}

/// Fundo da "tela": o do `<html>`; se for transparente, o do `<body>` (como manda o CSS).
fn canvas_color(doc: &Document, styles: &StyleMap, root: NodeId) -> Rgba {
    let opaque = |n: NodeId| {
        styles
            .get(n)
            .map(|s| s.background_color)
            .filter(|c| !c.is_transparent())
    };
    opaque(root)
        .or_else(|| doc.body().and_then(opaque))
        .unwrap_or(Rgba::WHITE)
}

/// Junta duas margens verticais adjacentes (colapso de margens do CSS).
fn collapse(a: f32, b: f32) -> f32 {
    if a >= 0.0 && b >= 0.0 {
        a.max(b)
    } else if a <= 0.0 && b <= 0.0 {
        a.min(b)
    } else {
        a + b
    }
}

fn brush_color(c: Rgba, visible: bool) -> [u8; 4] {
    if visible {
        [c.r, c.g, c.b, c.a]
    } else {
        [0, 0, 0, 0]
    }
}

/// `font-family` em sintaxe CSS para o Parley: nomes entre aspas e a genérica no fim.
fn family_source(style: &ComputedStyle) -> String {
    let mut out = String::new();
    for name in style.font_family.names.iter() {
        out.push('"');
        out.push_str(&name.replace('"', ""));
        out.push_str("\", ");
    }
    out.push_str(match style.font_family.generic {
        GenericFamily::SansSerif => "sans-serif",
        GenericFamily::Serif => "serif",
        GenericFamily::Monospace => "monospace",
    });
    out
}

fn line_height(style: &ComputedStyle) -> LineHeight {
    match style.line_height {
        nous_style::LineHeight::Normal => LineHeight::NORMAL,
        nous_style::LineHeight::Number(n) => LineHeight::FontSizeRelative(n),
        nous_style::LineHeight::Px(px) => LineHeight::Absolute(px),
    }
}

fn alignment(style: &ComputedStyle) -> Alignment {
    match style.text_align {
        TextAlign::Left => Alignment::Left,
        TextAlign::Right => Alignment::Right,
        TextAlign::Center => Alignment::Center,
        TextAlign::Justify => Alignment::Justify,
    }
}

struct LayoutCx<'a, 'e> {
    builder: &'a BoxBuilder<'a>,
    engine: &'e mut LayoutEngine,
    root: NodeId,
    items: Vec<PaintItem>,
    links: Vec<LinkArea>,
    link_urls: Vec<String>,
}

impl LayoutCx<'_, '_> {
    fn padding(b: &BlockBox, cb_w: f32) -> [f32; 4] {
        if b.is_anonymous() {
            return [0.0; 4];
        }
        b.style.padding.map(|p| p.resolve_or_zero(cb_w).max(0.0))
    }

    fn borders(b: &BlockBox) -> [f32; 4] {
        if b.is_anonymous() {
            [0.0; 4]
        } else {
            b.style.border_width
        }
    }

    fn margin_top(&self, b: &BlockBox, cb_w: f32) -> f32 {
        if b.is_anonymous() {
            0.0
        } else {
            b.style.margin[0].resolve_or_zero(cb_w)
        }
    }

    fn margin_bottom(&self, b: &BlockBox, cb_w: f32) -> f32 {
        if b.is_anonymous() {
            0.0
        } else {
            b.style.margin[2].resolve_or_zero(cb_w)
        }
    }

    fn is_root(&self, b: &BlockBox) -> bool {
        b.node == Some(self.root)
    }

    /// A margem de cima do primeiro filho "atravessa" este bloco?
    fn top_collapses_through(&self, b: &BlockBox, cb_w: f32) -> bool {
        !b.is_anonymous()
            && !self.is_root(b)
            && Self::padding(b, cb_w)[0] == 0.0
            && Self::borders(b)[0] == 0.0
    }

    fn bottom_collapses_through(&self, b: &BlockBox, cb_w: f32) -> bool {
        !b.is_anonymous()
            && !self.is_root(b)
            && Self::padding(b, cb_w)[2] == 0.0
            && Self::borders(b)[2] == 0.0
            && b.style.height.is_auto()
            && b.style.min_height.resolve_or_zero(cb_w) <= 0.0
    }

    /// Margem de cima já somada com a do primeiro filho, quando ela atravessa.
    fn collapsed_top(&self, b: &BlockBox, cb_w: f32) -> f32 {
        let own = self.margin_top(b, cb_w);
        if self.top_collapses_through(b, cb_w)
            && let BlockContent::Blocks(children) = &b.content
            && let Some(first) = children.first()
        {
            let (_, w) = self.horizontal(b, 0.0, cb_w);
            return collapse(own, self.collapsed_top(first, w));
        }
        own
    }

    fn collapsed_bottom(&self, b: &BlockBox, cb_w: f32) -> f32 {
        let own = self.margin_bottom(b, cb_w);
        if self.bottom_collapses_through(b, cb_w)
            && let BlockContent::Blocks(children) = &b.content
            && let Some(last) = children.last()
        {
            let (_, w) = self.horizontal(b, 0.0, cb_w);
            return collapse(own, self.collapsed_bottom(last, w));
        }
        own
    }

    /// Posição x da borda esquerda e largura do conteúdo, resolvendo `width`,
    /// `max-width` e margens `auto`.
    fn horizontal(&self, b: &BlockBox, cb_x: f32, cb_w: f32) -> (f32, f32) {
        if b.is_anonymous() {
            return (cb_x, cb_w);
        }
        let s = &b.style;
        let pad = Self::padding(b, cb_w);
        let bw = Self::borders(b);
        let extra = pad[1] + pad[3] + bw[1] + bw[3];
        let ml = s.margin[3].resolve(cb_w);
        let mr = s.margin[1].resolve(cb_w);
        let avail = cb_w - ml.unwrap_or(0.0) - mr.unwrap_or(0.0) - extra;
        let mut constrained = false;
        let mut w = match s.width.resolve(cb_w) {
            Some(w) => {
                constrained = true;
                w
            }
            None => avail,
        };
        if let Some(max) = s.max_width.resolve(cb_w)
            && w > max
        {
            w = max;
            constrained = true;
        }
        let w = w.max(0.0);
        let used = w + extra;
        let left = if constrained {
            match (ml, mr) {
                (None, None) => ((cb_w - used) / 2.0).max(0.0),
                (None, Some(r)) => (cb_w - used - r).max(0.0),
                (Some(l), _) => l,
            }
        } else {
            ml.unwrap_or(0.0)
        };
        (cb_x + left, w)
    }

    fn placeholder(&mut self) -> usize {
        self.items.push(PaintItem::Rect {
            rect: Rect::default(),
            color: Rgba::TRANSPARENT,
            radius: 0.0,
        });
        self.items.len() - 1
    }

    /// Faz o layout de um bloco cuja borda de cima fica em `y`. Devolve a altura da
    /// caixa de borda.
    fn layout_block(&mut self, b: &BlockBox, cb_x: f32, cb_w: f32, y: f32) -> f32 {
        let s = b.style.clone();
        let pad = Self::padding(b, cb_w);
        let bw = Self::borders(b);
        let (x, content_w) = self.horizontal(b, cb_x, cb_w);
        let content_x = x + bw[3] + pad[3];
        let content_y = y + bw[0] + pad[0];
        let paints_box = !b.is_anonymous() && s.visible;
        let bg_slot =
            (paints_box && !s.background_color.is_transparent()).then(|| self.placeholder());
        let border_slot = (paints_box && bw.iter().any(|&w| w > 0.0)).then(|| self.placeholder());

        let content_h = match &b.content {
            BlockContent::Blocks(children) => {
                self.layout_children(b, children, content_x, content_w, content_y, cb_w)
            }
            BlockContent::Inline(nodes) => {
                self.layout_inline(&s, nodes, content_x, content_y, content_w)
            }
            BlockContent::Empty => 0.0,
        };
        if s.display == Display::ListItem && !b.is_anonymous() {
            self.list_marker(b, content_x, content_y);
        }

        let mut h = match s.height {
            Length::Px(px) => px,
            _ => content_h,
        };
        if let Length::Px(min) = s.min_height {
            h = h.max(min);
        }
        let border_box = Rect::new(
            x,
            y,
            content_w + pad[1] + pad[3] + bw[1] + bw[3],
            h + pad[0] + pad[2] + bw[0] + bw[2],
        );
        if let Some(i) = bg_slot {
            self.items[i] = PaintItem::Rect {
                rect: border_box,
                color: s.background_color,
                radius: s.border_radius,
            };
        }
        if let Some(i) = border_slot {
            self.items[i] = PaintItem::Border {
                rect: border_box,
                widths: bw,
                colors: s.border_color,
            };
        }
        border_box.h
    }

    fn layout_children(
        &mut self,
        parent: &BlockBox,
        children: &[BlockBox],
        x: f32,
        w: f32,
        y: f32,
        parent_cb_w: f32,
    ) -> f32 {
        let hoist_top = self.top_collapses_through(parent, parent_cb_w);
        let hoist_bottom = self.bottom_collapses_through(parent, parent_cb_w);
        let mut cursor = y;
        let mut prev_mb: Option<f32> = None;
        for child in children {
            let mt = self.collapsed_top(child, w);
            let gap = match prev_mb {
                None if hoist_top => 0.0,
                None => mt,
                Some(pmb) => collapse(pmb, mt),
            };
            let child_y = cursor + gap;
            let h = self.layout_block(child, x, w, child_y);
            cursor = child_y + h;
            prev_mb = Some(self.collapsed_bottom(child, w));
        }
        if !hoist_bottom {
            cursor += prev_mb.unwrap_or(0.0);
        }
        (cursor - y).max(0.0)
    }

    /// Monta um layout do Parley com o estilo base do bloco e as faixas de estilo.
    fn build_text_layout(
        &mut self,
        base: &ComputedStyle,
        text: &str,
        spans: &[inline::Span],
        max_width: Option<f32>,
    ) -> Layout<TextBrush> {
        let engine = &mut *self.engine;
        let mut builder = engine
            .layout_cx
            .ranged_builder(&mut engine.font_cx, text, 1.0, true);
        builder.push_default(StyleProperty::FontSize(base.font_size));
        builder.push_default(StyleProperty::FontFamily(FontFamily::Source(Cow::Owned(
            family_source(base),
        ))));
        builder.push_default(StyleProperty::FontWeight(FontWeight::new(
            base.font_weight as f32,
        )));
        builder.push_default(StyleProperty::FontStyle(if base.italic {
            FontStyle::Italic
        } else {
            FontStyle::Normal
        }));
        builder.push_default(StyleProperty::LineHeight(line_height(base)));
        builder.push_default(StyleProperty::Brush(TextBrush {
            color: brush_color(base.color, base.visible),
            link: 0,
        }));
        if !base.white_space.wraps() {
            builder.push_default(StyleProperty::TextWrapMode(TextWrapMode::NoWrap));
        }

        for span in spans {
            let st = &span.style;
            let r = span.range.clone();
            if st.font_size != base.font_size {
                builder.push(StyleProperty::FontSize(st.font_size), r.clone());
            }
            if st.font_family != base.font_family {
                builder.push(
                    StyleProperty::FontFamily(FontFamily::Source(Cow::Owned(family_source(st)))),
                    r.clone(),
                );
            }
            if st.font_weight != base.font_weight {
                builder.push(
                    StyleProperty::FontWeight(FontWeight::new(st.font_weight as f32)),
                    r.clone(),
                );
            }
            if st.italic != base.italic {
                let fs = if st.italic {
                    FontStyle::Italic
                } else {
                    FontStyle::Normal
                };
                builder.push(StyleProperty::FontStyle(fs), r.clone());
            }
            if st.line_height != base.line_height {
                builder.push(StyleProperty::LineHeight(line_height(st)), r.clone());
            }
            let brush = TextBrush {
                color: brush_color(st.color, st.visible),
                link: span.link,
            };
            if brush.color != brush_color(base.color, base.visible) || span.link != 0 {
                builder.push(StyleProperty::Brush(brush), r.clone());
            }
            if st.underline {
                builder.push(StyleProperty::Underline(true), r.clone());
            }
            if st.line_through {
                builder.push(StyleProperty::Strikethrough(true), r.clone());
            }
            if st.white_space.wraps() != base.white_space.wraps() {
                let mode = if st.white_space.wraps() {
                    TextWrapMode::Wrap
                } else {
                    TextWrapMode::NoWrap
                };
                builder.push(StyleProperty::TextWrapMode(mode), r);
            }
        }
        let mut layout = builder.build(text);
        layout.set_alignment(alignment(base), AlignmentOptions::default());
        layout.break_all_lines(max_width);
        layout
    }

    fn layout_inline(
        &mut self,
        style: &ComputedStyle,
        nodes: &[NodeId],
        x: f32,
        y: f32,
        w: f32,
    ) -> f32 {
        let mut collector = InlineCollector::new(self.builder, &mut self.link_urls);
        for &n in nodes {
            collector.collect(n, 0);
        }
        let (text, spans) = collector.finish();
        if text.is_empty() {
            return 0.0;
        }
        let layout = self.build_text_layout(style, &text, &spans, Some(w.max(1.0)));
        let h = layout.height();
        for line in layout.lines() {
            let m = line.metrics();
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(run) = item else {
                    continue;
                };
                let link = run.style().brush.link;
                if link != 0 {
                    self.links.push(LinkArea {
                        rect: Rect::new(
                            x + run.offset(),
                            y + m.block_min_coord,
                            run.advance(),
                            m.block_max_coord - m.block_min_coord,
                        ),
                        link,
                    });
                }
            }
        }
        let bounds = Rect::new(x, y, w.max(layout.full_width()), h);
        self.items.push(PaintItem::Text {
            x,
            y,
            bounds,
            layout: Box::new(layout),
        });
        h
    }

    /// Marcador de item de lista (•, ◦, ▪ ou "n.") à esquerda do conteúdo.
    fn list_marker(&mut self, b: &BlockBox, content_x: f32, content_y: f32) {
        let s = b.style.clone();
        let marker = match s.list_style {
            ListStyle::None => return,
            ListStyle::Disc => "•".to_owned(),
            ListStyle::Circle => "◦".to_owned(),
            ListStyle::Square => "▪".to_owned(),
            ListStyle::Decimal => format!("{}.", self.list_index(b)),
        };
        let mut base = s.as_ref().clone();
        base.white_space = nous_style::WhiteSpace::Pre;
        base.text_align = TextAlign::Left;
        let layout = self.build_text_layout(&base, &marker, &[], None);
        let mw = layout.full_width();
        let mx = content_x - mw - base.font_size * 0.5;
        let bounds = Rect::new(mx, content_y, mw, layout.height());
        self.items.push(PaintItem::Text {
            x: mx,
            y: content_y,
            bounds,
            layout: Box::new(layout),
        });
    }

    /// Número do item numa lista ordenada (respeita `<ol start>`).
    fn list_index(&self, b: &BlockBox) -> i64 {
        let doc = self.builder.doc;
        let Some(node) = b.node else { return 1 };
        let Some(parent) = doc.node(node).parent else {
            return 1;
        };
        let start = doc
            .element(parent)
            .and_then(|e| e.attr("start"))
            .and_then(|s| s.trim().parse::<i64>().ok())
            .unwrap_or(1);
        let before = doc
            .children(parent)
            .take_while(|&c| c != node)
            .filter(|&c| {
                self.builder
                    .style_of(c)
                    .is_some_and(|st| doc.element(c).is_some() && st.display == Display::ListItem)
            })
            .count() as i64;
        start + before
    }
}

#[cfg(test)]
mod tests;
