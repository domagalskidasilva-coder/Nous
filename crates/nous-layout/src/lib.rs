//! Layout do Nous: transforma DOM + estilos numa lista de coisas para desenhar
//! (retângulos, bordas, imagens e blocos de texto já quebrados em linhas), além
//! de áreas de link e controles de formulário.
//!
//! Usa fluxo de blocos CSS (colapso de margens, `margin: auto`, `box-sizing`,
//! posicionamento relativo/absoluto), Taffy para Flexbox/Grid/Tabelas e Parley
//! para texto e caixas inline (`<img>`, controles, `inline-block`/`inline-flex`).

mod boxes;
mod flex_grid;
mod forms;
mod inline;

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub use forms::{FormControlArea, FormControlKind};
pub use parley;
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, FontStyle, FontWeight, InlineBox,
    InlineBoxKind, Layout, LayoutContext, LineHeight, PositionedLayoutItem, StyleProperty,
    TextWrapMode, VerticalAlign,
};

use nous_dom::{Document, ElementData, NodeId, local_name};
use nous_style::{
    BoxSizing, ComputedStyle, Display, GenericFamily, Length, ListStyle, Overflow, Position, Rgba,
    StyleMap, TextAlign,
};

use boxes::{BlockBox, BlockContent, BoxBuilder};
use forms::{
    extract_control_info, is_form_control, paint_control_full, resolve_control_box_size,
};
use inline::{InlineCollector, InlineReplacement, InlineReplacementKind};

/// Imagem já decodificada em RGBA8 pronto para enviar à GPU.
#[derive(Clone, Debug)]
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

pub type ImageMap = HashMap<NodeId, DecodedImage>;

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
    Image {
        rect: Rect,
        image: DecodedImage,
        radius: f32,
    },
    PushClip(Rect),
    PopClip,
    PushOpacity(f32),
    PopOpacity,
}

impl PaintItem {
    pub fn bounds(&self) -> Rect {
        match self {
            PaintItem::Rect { rect, .. }
            | PaintItem::Border { rect, .. }
            | PaintItem::Image { rect, .. }
            | PaintItem::PushClip(rect) => *rect,
            PaintItem::Text { bounds, .. } => *bounds,
            PaintItem::PopClip | PaintItem::PushOpacity(_) | PaintItem::PopOpacity => {
                Rect::default()
            }
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
    pub controls: Vec<FormControlArea>,
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
            controls: Vec::new(),
        }
    }

    /// Link sob o ponto (coordenadas da página), se houver.
    pub fn link_at(&self, x: f32, y: f32) -> Option<&str> {
        let area = self.links.iter().rev().find(|l| l.rect.contains(x, y))?;
        self.link_urls
            .get(area.link.checked_sub(1)? as usize)
            .map(String::as_str)
    }

    /// Estimativa de memória do layout (para `nous://memoria`).
    pub fn approx_heap_bytes(&self) -> usize {
        let mut total = self.items.capacity() * std::mem::size_of::<PaintItem>()
            + self.links.capacity() * std::mem::size_of::<LinkArea>()
            + self
                .link_urls
                .iter()
                .map(|u| u.capacity() + 24)
                .sum::<usize>()
            + self.controls.capacity() * std::mem::size_of::<FormControlArea>()
            + self
                .controls
                .iter()
                .map(|c| c.value.capacity() + c.placeholder.capacity())
                .sum::<usize>();
        let mut seen_images = HashSet::new();
        for item in &self.items {
            match item {
                PaintItem::Text { layout, .. } => {
                    total += std::mem::size_of::<Layout<TextBrush>>() + layout.text_len() * 48;
                }
                PaintItem::Image { image, .. } => {
                    let ptr = Arc::as_ptr(&image.rgba) as *const u8 as usize;
                    if seen_images.insert(ptr) {
                        total += image.rgba.len();
                    }
                }
                _ => {}
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
    layout_document_with_images(doc, styles, engine, viewport_width, &ImageMap::new())
}

/// Faz o layout do documento com mapa de imagens já decodificadas.
pub fn layout_document_with_images(
    doc: &Document,
    styles: &StyleMap,
    engine: &mut LayoutEngine,
    viewport_width: f32,
    images: &ImageMap,
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
    let mut cx = LayoutCx::new(&builder, engine, images, root, viewport_width);
    let mt = cx.margin_top(&root_box, viewport_width);
    let mb = cx.margin_bottom(&root_box, viewport_width);
    let h = cx.layout_block(&root_box, 0.0, viewport_width, mt);
    cx.items.shrink_to_fit();
    cx.links.shrink_to_fit();
    cx.link_urls.shrink_to_fit();
    cx.controls.shrink_to_fit();
    PageLayout {
        width: viewport_width,
        height: mt + h + mb,
        canvas_color,
        items: cx.items,
        links: cx.links,
        link_urls: cx.link_urls,
        controls: cx.controls,
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

pub(crate) fn brush_color(c: Rgba, visible: bool) -> [u8; 4] {
    if visible {
        [c.r, c.g, c.b, c.a]
    } else {
        [0, 0, 0, 0]
    }
}

/// `font-family` em sintaxe CSS para o Parley: nomes entre aspas e a genérica no fim.
pub(crate) fn family_source(style: &ComputedStyle) -> String {
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

pub(crate) fn line_height(style: &ComputedStyle) -> LineHeight {
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

pub(crate) fn resolve_border_radius(style: &ComputedStyle, rect: Rect) -> f32 {
    match style.border_radius_length {
        Length::Percent(p) => (rect.w.min(rect.h) * p / 100.0).max(0.0),
        _ => style.border_radius.max(0.0),
    }
}

pub(crate) fn clip_rect_for(style: &ComputedStyle, padding_box: Rect) -> Option<Rect> {
    let cx = matches!(
        style.overflow_x,
        Overflow::Hidden | Overflow::Scroll | Overflow::Auto
    );
    let cy = matches!(
        style.overflow_y,
        Overflow::Hidden | Overflow::Scroll | Overflow::Auto
    );
    if !cx && !cy {
        return None;
    }
    Some(Rect::new(
        if cx { padding_box.x } else { -1_000_000.0 },
        if cy { padding_box.y } else { -1_000_000.0 },
        if cx { padding_box.w } else { 2_000_000.0 },
        if cy { padding_box.h } else { 2_000_000.0 },
    ))
}

pub(crate) fn translate_outputs(
    items: &mut [PaintItem],
    links: &mut [LinkArea],
    controls: &mut [FormControlArea],
    dx: f32,
    dy: f32,
) {
    if dx == 0.0 && dy == 0.0 {
        return;
    }
    for item in items {
        match item {
            PaintItem::Rect { rect, .. }
            | PaintItem::Border { rect, .. }
            | PaintItem::Image { rect, .. }
            | PaintItem::PushClip(rect) => {
                rect.x += dx;
                rect.y += dy;
            }
            PaintItem::Text { x, y, bounds, .. } => {
                *x += dx;
                *y += dy;
                bounds.x += dx;
                bounds.y += dy;
            }
            PaintItem::PopClip | PaintItem::PushOpacity(_) | PaintItem::PopOpacity => {}
        }
    }
    for link in links {
        link.rect.x += dx;
        link.rect.y += dy;
    }
    for ctrl in controls {
        ctrl.rect.x += dx;
        ctrl.rect.y += dy;
    }
}

pub(crate) fn resolve_img_box_size(
    _el: &ElementData,
    style: &ComputedStyle,
    img: Option<&DecodedImage>,
    cb_w: f32,
    cb_h: Option<f32>,
) -> Option<(f32, f32, [f32; 4], [f32; 4])> {
    let pad = style.padding.map(|p| p.resolve_or_zero(cb_w).max(0.0));
    let bw = style.border_width;
    let extra_w = pad[1] + pad[3] + bw[1] + bw[3];
    let extra_h = pad[0] + pad[2] + bw[0] + bw[2];
    let to_cw = |w: f32| {
        if style.box_sizing == BoxSizing::BorderBox {
            (w - extra_w).max(0.0)
        } else {
            w.max(0.0)
        }
    };
    let to_ch = |h: f32| {
        if style.box_sizing == BoxSizing::BorderBox {
            (h - extra_h).max(0.0)
        } else {
            h.max(0.0)
        }
    };

    let spec_w = style.width.resolve(cb_w).map(to_cw);
    let spec_h = match style.height {
        Length::Px(px) => Some(to_ch(px)),
        Length::Percent(p) => cb_h.map(|h| to_ch(h * p / 100.0)),
        Length::Auto => None,
    };
    let max_w = style.max_width.resolve(cb_w).map(to_cw);
    let min_w = style.min_width.resolve(cb_w).map(to_cw);
    let resolve_len_h = |l: Length| match l {
        Length::Px(px) => Some(to_ch(px)),
        Length::Percent(p) => cb_h.map(|h| to_ch(h * p / 100.0)),
        Length::Auto => None,
    };
    let max_h = resolve_len_h(style.max_height);
    let min_h = resolve_len_h(style.min_height);

    let (mut cw, mut ch) = if let Some(im) = img {
        let nw = im.width as f32;
        let nh = im.height as f32;
        if nw <= 0.0 || nh <= 0.0 {
            (spec_w.unwrap_or(0.0), spec_h.unwrap_or(0.0))
        } else {
            match (spec_w, spec_h) {
                (Some(w), Some(h)) => (w, h),
                (Some(w), None) => (w, w * nh / nw),
                (None, Some(h)) => (h * nw / nh, h),
                (None, None) => (nw, nh),
            }
        }
    } else if let (Some(w), Some(h)) = (spec_w, spec_h) {
        (w, h)
    } else {
        return None;
    };

    if let Some(mx) = max_w
        && cw > mx
    {
        if spec_h.is_none()
            && let Some(im) = img
            && im.width > 0
        {
            ch = mx * (im.height as f32 / im.width as f32);
        }
        cw = mx;
    }
    if let Some(mn) = min_w
        && cw < mn
    {
        cw = mn;
    }
    if let Some(mx) = max_h
        && ch > mx
    {
        if spec_w.is_none()
            && let Some(im) = img
            && im.height > 0
        {
            cw = mx * (im.width as f32 / im.height as f32);
        }
        ch = mx;
    }
    if let Some(mn) = min_h
        && ch < mn
    {
        ch = mn;
    }
    Some((cw.max(0.0) + extra_w, ch.max(0.0) + extra_h, pad, bw))
}

pub(crate) fn paint_img_content(
    cx: &mut LayoutCx<'_, '_>,
    node: NodeId,
    style: &ComputedStyle,
    content_box: Rect,
    border_box: Rect,
) {
    if !style.visible || content_box.w <= 0.0 || content_box.h <= 0.0 {
        return;
    }
    let radius = resolve_border_radius(style, border_box);
    if let Some(img) = cx.images.get(&node)
        && img.width > 0
        && img.height > 0
    {
        cx.items.push(PaintItem::Image {
            rect: content_box,
            image: img.clone(),
            radius,
        });
        return;
    }
    if let Some(el) = cx.builder.doc.element(node) {
        if style.border_width.iter().all(|&w| w == 0.0) {
            cx.items.push(PaintItem::Border {
                rect: border_box,
                widths: [1.0; 4],
                colors: [Rgba::new(200, 200, 210, 255); 4],
            });
        }
        if let Some(alt) = el.attr("alt").filter(|a| !a.trim().is_empty()) {
            cx.items.push(PaintItem::PushClip(content_box));
            let layout = cx.build_text_layout(style, alt, &[], &[], Some(content_box.w.max(1.0)));
            cx.items.push(PaintItem::Text {
                x: content_box.x,
                y: content_box.y,
                bounds: content_box,
                layout: Box::new(layout),
            });
            cx.items.push(PaintItem::PopClip);
        }
    }
}

pub(crate) struct LayoutCx<'a, 'e> {
    pub builder: &'a BoxBuilder<'a>,
    pub engine: &'e mut LayoutEngine,
    pub images: &'a ImageMap,
    pub root: NodeId,
    pub viewport_width: f32,
    pub items: Vec<PaintItem>,
    pub links: Vec<LinkArea>,
    pub link_urls: Vec<String>,
    pub controls: Vec<FormControlArea>,
}

impl<'a, 'e> LayoutCx<'a, 'e> {
    pub fn new(
        builder: &'a BoxBuilder<'a>,
        engine: &'e mut LayoutEngine,
        images: &'a ImageMap,
        root: NodeId,
        viewport_width: f32,
    ) -> Self {
        LayoutCx {
            builder,
            engine,
            images,
            root,
            viewport_width,
            items: Vec::new(),
            links: Vec::new(),
            link_urls: Vec::new(),
            controls: Vec::new(),
        }
    }

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
            && b.style.display == Display::Block
            && b.style.overflow_x == Overflow::Visible
            && b.style.overflow_y == Overflow::Visible
            && Self::padding(b, cb_w)[0] == 0.0
            && Self::borders(b)[0] == 0.0
    }

    fn bottom_collapses_through(&self, b: &BlockBox, cb_w: f32) -> bool {
        !b.is_anonymous()
            && !self.is_root(b)
            && b.style.display == Display::Block
            && b.style.overflow_x == Overflow::Visible
            && b.style.overflow_y == Overflow::Visible
            && Self::padding(b, cb_w)[2] == 0.0
            && Self::borders(b)[2] == 0.0
            && b.style.height.is_auto()
            && b.style.max_height.is_auto()
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
    /// `min-width`, `max-width`, `box-sizing` e margens `auto`.
    fn horizontal(&self, b: &BlockBox, cb_x: f32, cb_w: f32) -> (f32, f32) {
        if b.is_anonymous() {
            return (cb_x, cb_w);
        }
        let s = &b.style;
        let pad = Self::padding(b, cb_w);
        let bw = Self::borders(b);
        let extra = pad[1] + pad[3] + bw[1] + bw[3];
        let to_cw = |spec_w: f32| {
            if s.box_sizing == BoxSizing::BorderBox {
                (spec_w - extra).max(0.0)
            } else {
                spec_w.max(0.0)
            }
        };
        let ml = s.margin[3].resolve(cb_w);
        let mr = s.margin[1].resolve(cb_w);
        let avail = cb_w - ml.unwrap_or(0.0) - mr.unwrap_or(0.0) - extra;
        let mut constrained = false;
        let mut w = match s.width.resolve(cb_w) {
            Some(w) => {
                constrained = true;
                to_cw(w)
            }
            None => avail,
        };
        if let Some(max) = s.max_width.resolve(cb_w).map(to_cw)
            && w > max
        {
            w = max;
            constrained = true;
        }
        if let Some(min) = s.min_width.resolve(cb_w).map(to_cw)
            && w < min
        {
            w = min;
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

    fn relative_offset(s: &ComputedStyle, cb_w: f32, cb_h: Option<f32>) -> (f32, f32) {
        if s.position != Position::Relative {
            return (0.0, 0.0);
        }
        let dx = if !s.inset[3].is_auto() {
            s.inset[3].resolve_or_zero(cb_w)
        } else if !s.inset[1].is_auto() {
            -s.inset[1].resolve_or_zero(cb_w)
        } else {
            0.0
        };
        let resolve_y = |l: Length| match l {
            Length::Px(px) => Some(px),
            Length::Percent(p) => cb_h.map(|h| h * p / 100.0),
            Length::Auto => None,
        };
        let dy = if let Some(top) = resolve_y(s.inset[0]) {
            top
        } else if let Some(bottom) = resolve_y(s.inset[2]) {
            -bottom
        } else {
            0.0
        };
        (dx, dy)
    }

    /// Mede a largura/altura do conteúdo (sem padding/border do próprio `b`) para o Taffy.
    pub(crate) fn measure_block_content_size(
        &mut self,
        b: &BlockBox,
        avail_w: taffy::AvailableSpace,
    ) -> (f32, f32) {
        if let Some(node) = b.node
            && let Some(el) = self.builder.doc.element(node)
        {
            let cb_w = match avail_w {
                taffy::AvailableSpace::Definite(w) => w,
                _ => self.viewport_width,
            };
            if el.is(&local_name!("img")) {
                if let Some((bw, bh, pad, border)) =
                    resolve_img_box_size(el, &b.style, self.images.get(&node), cb_w, None)
                {
                    let cw = (bw - pad[1] - pad[3] - border[1] - border[3]).max(0.0);
                    let ch = (bh - pad[0] - pad[2] - border[0] - border[2]).max(0.0);
                    return (cw, ch);
                }
                if let Some(alt) = el.attr("alt").filter(|a| !a.trim().is_empty()) {
                    let max_w = match avail_w {
                        taffy::AvailableSpace::Definite(w) => Some(w.max(1.0)),
                        _ => None,
                    };
                    let l = self.build_text_layout(&b.style, alt, &[], &[], max_w);
                    return (l.full_width(), l.height());
                }
                return (0.0, 0.0);
            }
            if is_form_control(el)
                && let Some(info) = extract_control_info(self.builder.doc, node)
            {
                let max_w = match avail_w {
                    taffy::AvailableSpace::Definite(w) => Some(w),
                    _ => None,
                };
                return forms::measure_control_content(self.engine, &info, &b.style, max_w);
            }
        }

        if matches!(
            b.style.display,
            Display::Flex | Display::InlineFlex | Display::Grid | Display::Table
        ) {
            let w_ref = match avail_w {
                taffy::AvailableSpace::Definite(w) => w,
                _ => self.viewport_width,
            };
            let (bw, bh) =
                flex_grid::layout_taffy_container(self, b, 0.0, 0.0, w_ref, None, false);
            let pad = Self::padding(b, w_ref);
            let borders = Self::borders(b);
            let cw = (bw - pad[1] - pad[3] - borders[1] - borders[3]).max(0.0);
            let ch = (bh - pad[0] - pad[2] - borders[0] - borders[2]).max(0.0);
            return (cw, ch);
        }

        match &b.content {
            BlockContent::Empty => (0.0, 0.0),
            BlockContent::Inline(nodes) => {
                let ref_w = match avail_w {
                    taffy::AvailableSpace::Definite(w) => w,
                    _ => self.viewport_width,
                };
                let mut dummy_urls = Vec::new();
                let mut collector = InlineCollector::new(
                    self.builder,
                    self.engine,
                    self.images,
                    &mut dummy_urls,
                    ref_w,
                    false,
                );
                for &n in nodes {
                    collector.collect(n, 0);
                }
                let (text, spans, replacements) = collector.finish();
                if text.is_empty() {
                    return (0.0, 0.0);
                }
                match avail_w {
                    taffy::AvailableSpace::MinContent => {
                        let mut layout =
                            self.build_text_layout(&b.style, &text, &spans, &replacements, None);
                        let min_w = layout.calculate_content_widths().min.max(1.0);
                        layout.break_all_lines(Some(min_w));
                        (min_w, layout.height())
                    }
                    taffy::AvailableSpace::MaxContent => {
                        let layout =
                            self.build_text_layout(&b.style, &text, &spans, &replacements, None);
                        (layout.full_width(), layout.height())
                    }
                    taffy::AvailableSpace::Definite(w) => {
                        let layout = self.build_text_layout(
                            &b.style,
                            &text,
                            &spans,
                            &replacements,
                            Some(w.max(1.0)),
                        );
                        (layout.full_width(), layout.height())
                    }
                }
            }
            BlockContent::Blocks(children) => {
                let ref_w = match avail_w {
                    taffy::AvailableSpace::Definite(w) => w,
                    _ => self.viewport_width,
                };
                let mut max_w = 0.0f32;
                let mut total_h = 0.0f32;
                let mut prev_mb: Option<f32> = None;
                for child in children {
                    let mt = self.collapsed_top(child, ref_w);
                    let gap = match prev_mb {
                        None => mt,
                        Some(pmb) => collapse(pmb, mt),
                    };
                    let c_pad = Self::padding(child, ref_w);
                    let c_bw = Self::borders(child);
                    let c_extra_w = c_pad[1] + c_pad[3] + c_bw[1] + c_bw[3];
                    let c_extra_h = c_pad[0] + c_pad[2] + c_bw[0] + c_bw[2];
                    let c_avail = match avail_w {
                        taffy::AvailableSpace::Definite(w) => taffy::AvailableSpace::Definite(
                            (w - c_extra_w
                                - child.style.margin[3].resolve_or_zero(ref_w)
                                - child.style.margin[1].resolve_or_zero(ref_w))
                            .max(0.0),
                        ),
                        other => other,
                    };
                    let (cw, ch) = self.measure_block_content_size(child, c_avail);
                    let spec_w = child.style.width.resolve(ref_w).map(|sw| {
                        if child.style.box_sizing == BoxSizing::BorderBox {
                            sw
                        } else {
                            sw + c_extra_w
                        }
                    });
                    let child_outer_w = spec_w.unwrap_or(cw + c_extra_w)
                        + child.style.margin[3].resolve_or_zero(ref_w)
                        + child.style.margin[1].resolve_or_zero(ref_w);
                    max_w = max_w.max(child_outer_w);
                    let child_h = match child.style.height {
                        Length::Px(px) => {
                            if child.style.box_sizing == BoxSizing::BorderBox {
                                px
                            } else {
                                px + c_extra_h
                            }
                        }
                        _ => ch + c_extra_h,
                    };
                    total_h += gap + child_h;
                    prev_mb = Some(self.collapsed_bottom(child, ref_w));
                }
                total_h += prev_mb.unwrap_or(0.0);
                let out_w = match avail_w {
                    taffy::AvailableSpace::Definite(w) => w.max(max_w),
                    _ => max_w,
                };
                (out_w, total_h.max(0.0))
            }
        }
    }

    /// Faz o layout de um elemento `inline-block` ou `inline-flex` na origem `(0, 0)`.
    pub(crate) fn layout_inline_atomic(
        &mut self,
        b: &BlockBox,
        avail_w: f32,
        paint: bool,
    ) -> (f32, f32) {
        if b.style.display == Display::InlineFlex {
            let width_override = b.style.width.resolve(avail_w).map(|w| {
                if b.style.box_sizing == BoxSizing::BorderBox {
                    w
                } else {
                    let pad = Self::padding(b, avail_w);
                    let bw = Self::borders(b);
                    w + pad[1] + pad[3] + bw[1] + bw[3]
                }
            });
            return flex_grid::layout_taffy_container(
                self,
                b,
                0.0,
                0.0,
                avail_w,
                width_override,
                paint,
            );
        }

        // InlineBlock
        let pad = Self::padding(b, avail_w);
        let bw = Self::borders(b);
        let extra_w = pad[1] + pad[3] + bw[1] + bw[3];
        let extra_h = pad[0] + pad[2] + bw[0] + bw[2];
        let to_cw = |w: f32| {
            if b.style.box_sizing == BoxSizing::BorderBox {
                (w - extra_w).max(0.0)
            } else {
                w.max(0.0)
            }
        };
        let mut cw = if let Some(w) = b.style.width.resolve(avail_w) {
            to_cw(w)
        } else {
            let (max_cw, _) =
                self.measure_block_content_size(b, taffy::AvailableSpace::MaxContent);
            max_cw.min((avail_w - extra_w).max(0.0))
        };
        if let Some(mx) = b.style.max_width.resolve(avail_w).map(to_cw) {
            cw = cw.min(mx);
        }
        if let Some(mn) = b.style.min_width.resolve(avail_w).map(to_cw) {
            cw = cw.max(mn);
        }
        let border_w = cw + extra_w;
        if paint {
            let mut sub_style_owned = (*b.style).clone();
            sub_style_owned.width = Length::Px(if b.style.box_sizing == BoxSizing::BorderBox {
                border_w
            } else {
                cw
            });
            sub_style_owned.margin = [Length::Px(0.0); 4];
            let sub_box = BlockBox {
                node: b.node,
                style: Arc::new(sub_style_owned),
                content: match &b.content {
                    BlockContent::Blocks(v) => {
                        let _ = v;
                        return self.paint_inline_block_direct(b, cw, pad, bw, extra_h);
                    }
                    BlockContent::Inline(nodes) => BlockContent::Inline(nodes.clone()),
                    BlockContent::Empty => BlockContent::Empty,
                },
            };
            let h = self.layout_block(&sub_box, 0.0, border_w, 0.0);
            (border_w, h)
        } else {
            let (_, ch) =
                self.measure_block_content_size(b, taffy::AvailableSpace::Definite(cw));
            let h = resolve_box_height(&b.style, ch, pad[0] + pad[2] + bw[0] + bw[2], None);
            (border_w, h + extra_h)
        }
    }

    fn paint_inline_block_direct(
        &mut self,
        b: &BlockBox,
        content_w: f32,
        pad: [f32; 4],
        bw: [f32; 4],
        extra_h: f32,
    ) -> (f32, f32) {
        let s = &b.style;
        let border_w = content_w + pad[1] + pad[3] + bw[1] + bw[3];
        let pushed_opacity = s.opacity < 1.0;
        if pushed_opacity {
            self.items
                .push(PaintItem::PushOpacity(s.opacity.clamp(0.0, 1.0)));
        }
        let paints = s.visible;
        let bg_slot = (paints && !s.background_color.is_transparent()).then(|| self.placeholder());
        let border_slot = (paints && bw.iter().any(|&w| w > 0.0)).then(|| self.placeholder());
        let clip_slot = clip_rect_for(s, Rect::default()).map(|_| self.placeholder());
        let content_h = match &b.content {
            BlockContent::Blocks(children) => {
                self.layout_children(b, children, bw[3] + pad[3], content_w, bw[0] + pad[0], border_w)
            }
            BlockContent::Inline(nodes) => {
                self.layout_inline(s, nodes, bw[3] + pad[3], bw[0] + pad[0], content_w)
            }
            BlockContent::Empty => 0.0,
        };
        let h = resolve_box_height(s, content_h, extra_h, None);
        let border_box = Rect::new(0.0, 0.0, border_w, h + extra_h);
        if let Some(i) = bg_slot {
            self.items[i] = PaintItem::Rect {
                rect: border_box,
                color: s.background_color,
                radius: resolve_border_radius(s, border_box),
            };
        }
        if let Some(i) = border_slot {
            self.items[i] = PaintItem::Border {
                rect: border_box,
                widths: bw,
                colors: s.border_color,
            };
        }
        if let Some(ci) = clip_slot {
            let pb = Rect::new(
                bw[3],
                bw[0],
                (border_box.w - bw[1] - bw[3]).max(0.0),
                (border_box.h - bw[0] - bw[2]).max(0.0),
            );
            self.items[ci] =
                PaintItem::PushClip(clip_rect_for(s, pb).unwrap_or(pb));
            self.items.push(PaintItem::PopClip);
        }
        if pushed_opacity {
            self.items.push(PaintItem::PopOpacity);
        }
        (border_box.w, border_box.h)
    }

    pub(crate) fn paint_block_content_at(
        &mut self,
        b: &BlockBox,
        content_x: f32,
        content_y: f32,
        content_w: f32,
        border_box_w: f32,
    ) {
        match &b.content {
            BlockContent::Blocks(children) => {
                self.layout_children(
                    b,
                    children,
                    content_x,
                    content_w,
                    content_y,
                    border_box_w,
                );
            }
            BlockContent::Inline(nodes) => {
                self.layout_inline(&b.style, nodes, content_x, content_y, content_w);
            }
            BlockContent::Empty => {}
        }
        if b.style.display == Display::ListItem && !b.is_anonymous() {
            self.list_marker(b, content_x, content_y);
        }
    }

    /// Faz o layout de um bloco cuja borda de cima fica em `y`. Devolve a altura da
    /// caixa de borda.
    fn layout_block(&mut self, b: &BlockBox, cb_x: f32, cb_w: f32, y: f32) -> f32 {
        let s = b.style.clone();
        if matches!(
            s.display,
            Display::Flex | Display::InlineFlex | Display::Grid | Display::Table
        ) && !b.is_anonymous()
        {
            let pad = Self::padding(b, cb_w);
            let bw = Self::borders(b);
            let (x, content_w) = self.horizontal(b, cb_x, cb_w);
            let border_w = content_w + pad[1] + pad[3] + bw[1] + bw[3];
            let width_override = if s.display == Display::Table && s.width.is_auto() {
                None
            } else {
                Some(border_w)
            };
            let items_start = self.items.len();
            let links_start = self.links.len();
            let controls_start = self.controls.len();
            let (_, bh) = flex_grid::layout_taffy_container(
                self,
                b,
                x,
                y,
                cb_w,
                width_override,
                true,
            );
            let (dx, dy) = Self::relative_offset(&s, cb_w, None);
            translate_outputs(
                &mut self.items[items_start..],
                &mut self.links[links_start..],
                &mut self.controls[controls_start..],
                dx,
                dy,
            );
            return bh;
        }

        // Elemento substituído (<img> ou controle) em nível de bloco
        if let Some(node) = b.node
            && let Some(el) = self.builder.doc.element(node)
        {
            if el.is(&local_name!("img")) {
                let (x, cw) = self.horizontal(b, cb_x, cb_w);
                let items_start = self.items.len();
                let links_start = self.links.len();
                let controls_start = self.controls.len();
                let (bw_px, bh_px, pad, bw) = resolve_img_box_size(
                    el,
                    &s,
                    self.images.get(&node),
                    cb_w,
                    None,
                )
                .unwrap_or_else(|| {
                    let p = Self::padding(b, cb_w);
                    let border = Self::borders(b);
                    (
                        cw + p[1] + p[3] + border[1] + border[3],
                        p[0] + p[2] + border[0] + border[2],
                        p,
                        border,
                    )
                });
                let border_box = Rect::new(x, y, bw_px, bh_px);
                let content_box = Rect::new(
                    x + bw[3] + pad[3],
                    y + bw[0] + pad[0],
                    (bw_px - pad[1] - pad[3] - bw[1] - bw[3]).max(0.0),
                    (bh_px - pad[0] - pad[2] - bw[0] - bw[2]).max(0.0),
                );
                let pushed_opacity = s.opacity < 1.0;
                if pushed_opacity {
                    self.items
                        .push(PaintItem::PushOpacity(s.opacity.clamp(0.0, 1.0)));
                }
                if s.visible && !s.background_color.is_transparent() {
                    self.items.push(PaintItem::Rect {
                        rect: border_box,
                        color: s.background_color,
                        radius: resolve_border_radius(&s, border_box),
                    });
                }
                if s.visible && bw.iter().any(|&w| w > 0.0) {
                    self.items.push(PaintItem::Border {
                        rect: border_box,
                        widths: bw,
                        colors: s.border_color,
                    });
                }
                paint_img_content(self, node, &s, content_box, border_box);
                if pushed_opacity {
                    self.items.push(PaintItem::PopOpacity);
                }
                let (dx, dy) = Self::relative_offset(&s, cb_w, None);
                translate_outputs(
                    &mut self.items[items_start..],
                    &mut self.links[links_start..],
                    &mut self.controls[controls_start..],
                    dx,
                    dy,
                );
                return border_box.h;
            }
            if is_form_control(el)
                && let Some(info) = extract_control_info(self.builder.doc, node)
            {
                let (x, _) = self.horizontal(b, cb_x, cb_w);
                let items_start = self.items.len();
                let links_start = self.links.len();
                let controls_start = self.controls.len();
                let (w, h, pad, bw) =
                    resolve_control_box_size(self.engine, &info, &s, cb_w, None);
                let border_box = Rect::new(x, y, w, h);
                paint_control_full(self, node, &info, &s, border_box, pad, bw);
                let (dx, dy) = Self::relative_offset(&s, cb_w, None);
                translate_outputs(
                    &mut self.items[items_start..],
                    &mut self.links[links_start..],
                    &mut self.controls[controls_start..],
                    dx,
                    dy,
                );
                return h;
            }
        }

        let pad = Self::padding(b, cb_w);
        let bw = Self::borders(b);
        let (x, content_w) = self.horizontal(b, cb_x, cb_w);
        let content_x = x + bw[3] + pad[3];
        let content_y = y + bw[0] + pad[0];
        let items_start = self.items.len();
        let links_start = self.links.len();
        let controls_start = self.controls.len();

        let pushed_opacity = !b.is_anonymous() && s.opacity < 1.0;
        if pushed_opacity {
            self.items
                .push(PaintItem::PushOpacity(s.opacity.clamp(0.0, 1.0)));
        }

        let paints_box = !b.is_anonymous() && s.visible;
        let bg_slot =
            (paints_box && !s.background_color.is_transparent()).then(|| self.placeholder());
        let border_slot = (paints_box && bw.iter().any(|&w| w > 0.0)).then(|| self.placeholder());
        let needs_clip = !b.is_anonymous() && clip_rect_for(&s, Rect::default()).is_some();
        let clip_slot = needs_clip.then(|| self.placeholder());

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

        let pad_v = pad[0] + pad[2] + bw[0] + bw[2];
        let h = resolve_box_height(&s, content_h, pad_v, None);
        let border_box = Rect::new(
            x,
            y,
            content_w + pad[1] + pad[3] + bw[1] + bw[3],
            h + pad_v,
        );
        if let Some(i) = bg_slot {
            self.items[i] = PaintItem::Rect {
                rect: border_box,
                color: s.background_color,
                radius: resolve_border_radius(&s, border_box),
            };
        }
        if let Some(i) = border_slot {
            self.items[i] = PaintItem::Border {
                rect: border_box,
                widths: bw,
                colors: s.border_color,
            };
        }
        if let Some(ci) = clip_slot {
            let padding_box = Rect::new(
                x + bw[3],
                y + bw[0],
                (border_box.w - bw[1] - bw[3]).max(0.0),
                (border_box.h - bw[0] - bw[2]).max(0.0),
            );
            self.items[ci] =
                PaintItem::PushClip(clip_rect_for(&s, padding_box).unwrap_or(padding_box));
            self.items.push(PaintItem::PopClip);
        }
        if pushed_opacity {
            self.items.push(PaintItem::PopOpacity);
        }

        if !b.is_anonymous() {
            let (dx, dy) = Self::relative_offset(&s, cb_w, None);
            translate_outputs(
                &mut self.items[items_start..],
                &mut self.links[links_start..],
                &mut self.controls[controls_start..],
                dx,
                dy,
            );
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
            if !child.is_anonymous()
                && matches!(child.style.position, Position::Absolute | Position::Fixed)
            {
                let (ref_x, ref_y, ref_w) = if child.style.position == Position::Fixed {
                    (0.0, 0.0, self.viewport_width)
                } else {
                    (x, y, w)
                };
                let items_start = self.items.len();
                let links_start = self.links.len();
                let controls_start = self.controls.len();
                let ch = self.layout_block(child, ref_x, ref_w, ref_y);
                let cw = self
                    .items
                    .get(items_start..)
                    .and_then(|sl| {
                        sl.iter().find_map(|i| match i {
                            PaintItem::Rect { rect, .. } | PaintItem::Border { rect, .. } => {
                                Some(rect.w)
                            }
                            _ => None,
                        })
                    })
                    .unwrap_or(0.0);
                let dx = if !child.style.inset[3].is_auto() {
                    child.style.inset[3].resolve_or_zero(ref_w)
                } else if !child.style.inset[1].is_auto() {
                    ref_w - cw - child.style.inset[1].resolve_or_zero(ref_w)
                } else {
                    0.0
                };
                let dy = if !child.style.inset[0].is_auto() {
                    child.style.inset[0].resolve_or_zero(0.0)
                } else {
                    cursor - ref_y
                };
                let _ = ch;
                translate_outputs(
                    &mut self.items[items_start..],
                    &mut self.links[links_start..],
                    &mut self.controls[controls_start..],
                    dx,
                    dy,
                );
                continue;
            }
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

    /// Monta um layout do Parley com o estilo base do bloco, faixas de estilo e caixas inline.
    pub(crate) fn build_text_layout(
        &mut self,
        base: &ComputedStyle,
        text: &str,
        spans: &[inline::Span],
        replacements: &[InlineReplacement],
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
        for (idx, rep) in replacements.iter().enumerate() {
            if rep.index < text.len() {
                builder.push_inline_box(InlineBox {
                    id: idx as u64,
                    kind: InlineBoxKind::InFlow,
                    index: rep.index,
                    width: rep.width.max(0.0),
                    height: rep.height.max(0.0),
                    baseline: rep.baseline,
                    vertical_align: VerticalAlign::BASELINE,
                });
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
        let mut collector = InlineCollector::new(
            self.builder,
            self.engine,
            self.images,
            &mut self.link_urls,
            w,
            true,
        );
        for &n in nodes {
            collector.collect(n, 0);
        }
        let (text, spans, mut replacements) = collector.finish();
        if text.is_empty() {
            return 0.0;
        }
        let layout =
            self.build_text_layout(style, &text, &spans, &replacements, Some(w.max(1.0)));
        let h = layout.height();
        let mut placed_boxes = Vec::new();
        for line in layout.lines() {
            let m = line.metrics();
            for item in line.items() {
                match item {
                    PositionedLayoutItem::GlyphRun(run) => {
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
                    PositionedLayoutItem::InlineBox(ib) => {
                        placed_boxes.push(ib);
                    }
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

        for ib in placed_boxes {
            let Some(rep) = replacements.get_mut(ib.id as usize) else {
                continue;
            };
            let box_x = x + ib.x;
            let box_y = y + ib.y;
            let kind = std::mem::replace(&mut rep.kind, InlineReplacementKind::MeasureOnly);
            match kind {
                InlineReplacementKind::Img {
                    node,
                    style: ist,
                    pad,
                    bw,
                    ml,
                    mt,
                } => {
                    let (dx, dy) = Self::relative_offset(&ist, w, None);
                    let bx = box_x + ml + dx;
                    let by = box_y + mt + dy;
                    let inner_w = (ib.width - ml - ist.margin[1].resolve_or_zero(w).max(0.0)).max(0.0);
                    let inner_h = (ib.height - mt - ist.margin[2].resolve_or_zero(w).max(0.0)).max(0.0);
                    let border_box = Rect::new(bx, by, inner_w, inner_h);
                    let content_box = Rect::new(
                        bx + bw[3] + pad[3],
                        by + bw[0] + pad[0],
                        (inner_w - pad[1] - pad[3] - bw[1] - bw[3]).max(0.0),
                        (inner_h - pad[0] - pad[2] - bw[0] - bw[2]).max(0.0),
                    );
                    let pushed_op = ist.opacity < 1.0;
                    if pushed_op {
                        self.items
                            .push(PaintItem::PushOpacity(ist.opacity.clamp(0.0, 1.0)));
                    }
                    if ist.visible && !ist.background_color.is_transparent() {
                        self.items.push(PaintItem::Rect {
                            rect: border_box,
                            color: ist.background_color,
                            radius: resolve_border_radius(&ist, border_box),
                        });
                    }
                    if ist.visible && bw.iter().any(|&w| w > 0.0) {
                        self.items.push(PaintItem::Border {
                            rect: border_box,
                            widths: bw,
                            colors: ist.border_color,
                        });
                    }
                    paint_img_content(self, node, &ist, content_box, border_box);
                    if pushed_op {
                        self.items.push(PaintItem::PopOpacity);
                    }
                    if rep.link != 0 {
                        self.links.push(LinkArea {
                            rect: border_box,
                            link: rep.link,
                        });
                    }
                }
                InlineReplacementKind::Control {
                    node,
                    style: cst,
                    info,
                    pad,
                    bw,
                    ml,
                    mt,
                } => {
                    let (dx, dy) = Self::relative_offset(&cst, w, None);
                    let bx = box_x + ml + dx;
                    let by = box_y + mt + dy;
                    let inner_w = (ib.width - ml - cst.margin[1].resolve_or_zero(w).max(0.0)).max(0.0);
                    let inner_h = (ib.height - mt - cst.margin[2].resolve_or_zero(w).max(0.0)).max(0.0);
                    let border_box = Rect::new(bx, by, inner_w, inner_h);
                    paint_control_full(self, node, &info, &cst, border_box, pad, bw);
                    if rep.link != 0 {
                        self.links.push(LinkArea {
                            rect: border_box,
                            link: rep.link,
                        });
                    }
                }
                InlineReplacementKind::Subtree {
                    mut items,
                    mut links,
                    mut controls,
                    ml,
                    mt,
                } => {
                    translate_outputs(&mut items, &mut links, &mut controls, box_x + ml, box_y + mt);
                    self.items.extend(items);
                    self.links.extend(links);
                    self.controls.extend(controls);
                }
                InlineReplacementKind::MeasureOnly => {}
            }
        }
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
        let layout = self.build_text_layout(&base, &marker, &[], &[], None);
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

fn resolve_box_height(
    s: &ComputedStyle,
    content_h: f32,
    pad_v: f32,
    cb_h: Option<f32>,
) -> f32 {
    let to_ch = |spec_h: f32| {
        if s.box_sizing == BoxSizing::BorderBox {
            (spec_h - pad_v).max(0.0)
        } else {
            spec_h.max(0.0)
        }
    };
    let resolve_len = |l: Length| match l {
        Length::Px(px) => Some(to_ch(px)),
        Length::Percent(p) => cb_h.map(|h| to_ch(h * p / 100.0)),
        Length::Auto => None,
    };
    let mut h = resolve_len(s.height).unwrap_or(content_h);
    if let Some(max_h) = resolve_len(s.max_height) {
        h = h.min(max_h);
    }
    if let Some(min_h) = resolve_len(s.min_height) {
        h = h.max(min_h);
    }
    h.max(0.0)
}

#[cfg(test)]
mod tests;
