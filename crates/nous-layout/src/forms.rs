//! Controles de formulário HTML (`<input>`, `<button>`, `<textarea>`, `<select>`):
//! extração de estado do DOM, dimensionamento, pintura limpa e registro de áreas interativas.

use std::borrow::Cow;

use nous_dom::{Document, ElementData, NodeId, local_name};
use nous_style::{BoxSizing, ComputedStyle, Length, Rgba, TextAlign};
use parley::{Alignment, AlignmentOptions, FontFamily, FontStyle, FontWeight, Layout, StyleProperty, TextWrapMode};

use crate::{LayoutCx, LayoutEngine, PaintItem, Rect, TextBrush, brush_color, family_source, line_height};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormControlKind {
    TextInput,
    Password,
    Checkbox { checked: bool },
    Radio { checked: bool },
    Button,
    Textarea,
    Select,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FormControlArea {
    pub node: NodeId,
    pub rect: Rect,
    pub kind: FormControlKind,
    pub value: String,
    pub placeholder: String,
    pub disabled: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ControlInfo {
    pub kind: FormControlKind,
    pub value: String,
    pub placeholder: String,
    pub disabled: bool,
    pub cols: Option<u32>,
    pub rows: Option<u32>,
}

pub(crate) fn is_form_control(el: &ElementData) -> bool {
    matches!(
        &**el.local_name(),
        "input" | "button" | "textarea" | "select"
    )
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut last_space = true;
    for ch in s.chars() {
        if ch.is_ascii_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

fn selected_option_text(doc: &Document, select_node: NodeId) -> String {
    let mut first: Option<String> = None;
    for n in doc.descendants(select_node) {
        if n == select_node {
            continue;
        }
        let Some(el) = doc.element(n) else { continue };
        if el.is(&local_name!("option")) {
            let txt = collapse_ws(&doc.text_content(n));
            if el.attr("selected").is_some() {
                return txt;
            }
            if first.is_none() {
                first = Some(txt);
            }
        }
    }
    first.unwrap_or_default()
}

pub(crate) fn extract_control_info(doc: &Document, node: NodeId) -> Option<ControlInfo> {
    let el = doc.element(node)?;
    if !is_form_control(el) {
        return None;
    }
    let disabled = el.attr("disabled").is_some();
    let placeholder = el.attr("placeholder").unwrap_or("").to_owned();
    let tag = &**el.local_name();
    match tag {
        "input" => {
            let ty = el.attr("type").unwrap_or("text").trim().to_ascii_lowercase();
            let checked = el.attr("checked").is_some();
            let raw_val = el.attr("value").unwrap_or("");
            let cols = el.attr("size").and_then(|s| s.trim().parse::<u32>().ok()).map(|n| n.clamp(1, 120));
            match ty.as_str() {
                "checkbox" => Some(ControlInfo {
                    kind: FormControlKind::Checkbox { checked },
                    value: raw_val.to_owned(),
                    placeholder: String::new(),
                    disabled,
                    cols: None,
                    rows: None,
                }),
                "radio" => Some(ControlInfo {
                    kind: FormControlKind::Radio { checked },
                    value: raw_val.to_owned(),
                    placeholder: String::new(),
                    disabled,
                    cols: None,
                    rows: None,
                }),
                "submit" => Some(ControlInfo {
                    kind: FormControlKind::Button,
                    value: if raw_val.is_empty() { "Submit".to_owned() } else { raw_val.to_owned() },
                    placeholder: String::new(),
                    disabled,
                    cols: None,
                    rows: None,
                }),
                "reset" => Some(ControlInfo {
                    kind: FormControlKind::Button,
                    value: if raw_val.is_empty() { "Reset".to_owned() } else { raw_val.to_owned() },
                    placeholder: String::new(),
                    disabled,
                    cols: None,
                    rows: None,
                }),
                "button" => Some(ControlInfo {
                    kind: FormControlKind::Button,
                    value: raw_val.to_owned(),
                    placeholder: String::new(),
                    disabled,
                    cols: None,
                    rows: None,
                }),
                "password" => Some(ControlInfo {
                    kind: FormControlKind::Password,
                    value: raw_val.to_owned(),
                    placeholder,
                    disabled,
                    cols,
                    rows: None,
                }),
                _ => Some(ControlInfo {
                    kind: FormControlKind::TextInput,
                    value: raw_val.to_owned(),
                    placeholder,
                    disabled,
                    cols,
                    rows: None,
                }),
            }
        }
        "button" => {
            let content = collapse_ws(&doc.text_content(node));
            let value = if content.is_empty() {
                el.attr("value").unwrap_or("").to_owned()
            } else {
                content
            };
            Some(ControlInfo {
                kind: FormControlKind::Button,
                value,
                placeholder: String::new(),
                disabled,
                cols: None,
                rows: None,
            })
        }
        "textarea" => {
            let cols = el.attr("cols").and_then(|s| s.trim().parse::<u32>().ok()).map(|n| n.clamp(1, 200));
            let rows = el.attr("rows").and_then(|s| s.trim().parse::<u32>().ok()).map(|n| n.clamp(1, 100));
            Some(ControlInfo {
                kind: FormControlKind::Textarea,
                value: doc.text_content(node),
                placeholder,
                disabled,
                cols,
                rows,
            })
        }
        "select" => Some(ControlInfo {
            kind: FormControlKind::Select,
            value: selected_option_text(doc, node),
            placeholder: String::new(),
            disabled,
            cols: None,
            rows: None,
        }),
        _ => None,
    }
}

fn build_control_text_layout(
    engine: &mut LayoutEngine,
    style: &ComputedStyle,
    text: &str,
    color: Rgba,
    align: Alignment,
    max_width: Option<f32>,
    wrap: bool,
) -> Layout<TextBrush> {
    let mut b = engine
        .layout_cx
        .ranged_builder(&mut engine.font_cx, text, 1.0, true);
    b.push_default(StyleProperty::FontSize(style.font_size));
    b.push_default(StyleProperty::FontFamily(FontFamily::Source(Cow::Owned(
        family_source(style),
    ))));
    b.push_default(StyleProperty::FontWeight(FontWeight::new(
        style.font_weight as f32,
    )));
    b.push_default(StyleProperty::FontStyle(if style.italic {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    }));
    b.push_default(StyleProperty::LineHeight(line_height(style)));
    b.push_default(StyleProperty::Brush(TextBrush {
        color: brush_color(color, style.visible),
        link: 0,
    }));
    if !wrap {
        b.push_default(StyleProperty::TextWrapMode(TextWrapMode::NoWrap));
    }
    let mut layout = b.build(text);
    layout.set_alignment(align, AlignmentOptions::default());
    layout.break_all_lines(max_width);
    layout
}

/// Calcula o tamanho intrínseco da caixa de conteúdo (sem padding/border).
pub(crate) fn measure_control_content(
    engine: &mut LayoutEngine,
    info: &ControlInfo,
    style: &ComputedStyle,
    avail_w: Option<f32>,
) -> (f32, f32) {
    let lh = style.line_height_px();
    match info.kind {
        FormControlKind::Checkbox { .. } | FormControlKind::Radio { .. } => (13.0, 13.0),
        FormControlKind::Button => {
            if info.value.is_empty() {
                (28.0, lh)
            } else {
                let layout = build_control_text_layout(
                    engine,
                    style,
                    &info.value,
                    style.color,
                    Alignment::Left,
                    avail_w.map(|w| w.max(1.0)),
                    avail_w.is_some(),
                );
                (layout.full_width().max(16.0), layout.height().max(lh))
            }
        }
        FormControlKind::TextInput | FormControlKind::Password => {
            let w = info
                .cols
                .map(|c| c as f32 * style.font_size * 0.6)
                .unwrap_or(160.0);
            (w, lh)
        }
        FormControlKind::Textarea => {
            let w = info.cols.unwrap_or(28) as f32 * style.font_size * 0.55;
            let h = info.rows.unwrap_or(3) as f32 * lh;
            (w, h)
        }
        FormControlKind::Select => {
            let text_w = if info.value.is_empty() {
                60.0
            } else {
                let layout = build_control_text_layout(
                    engine,
                    style,
                    &info.value,
                    style.color,
                    Alignment::Left,
                    None,
                    false,
                );
                layout.full_width()
            };
            ((text_w + 22.0).max(80.0), lh)
        }
    }
}

/// Resolve dimensões finais `(border_box_w, border_box_h, pad, bw)` para controle fora do Taffy.
pub(crate) fn resolve_control_box_size(
    engine: &mut LayoutEngine,
    info: &ControlInfo,
    style: &ComputedStyle,
    cb_w: f32,
    cb_h: Option<f32>,
) -> (f32, f32, [f32; 4], [f32; 4]) {
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
    let max_w = style.max_width.resolve(cb_w).map(to_cw);
    let min_w = style.min_width.resolve(cb_w).map(to_cw);

    let (intr_w, intr_h) = measure_control_content(
        engine,
        info,
        style,
        spec_w.or_else(|| Some((cb_w - extra_w).max(1.0))),
    );
    let mut cw = spec_w.unwrap_or(intr_w);
    if let Some(mx) = max_w {
        cw = cw.min(mx);
    }
    if let Some(mn) = min_w {
        cw = cw.max(mn);
    }
    cw = cw.max(0.0);

    let resolve_len_h = |l: Length| match l {
        Length::Px(px) => Some(to_ch(px)),
        Length::Percent(p) => cb_h.map(|h| to_ch(h * p / 100.0)),
        Length::Auto => None,
    };
    let mut ch = resolve_len_h(style.height).unwrap_or(intr_h);
    if let Some(mx) = resolve_len_h(style.max_height) {
        ch = ch.min(mx);
    }
    if let Some(mn) = resolve_len_h(style.min_height) {
        ch = ch.max(mn);
    }
    ch = ch.max(0.0);

    (cw + extra_w, ch + extra_h, pad, bw)
}

/// Pinta fundo/borda + interior de um controle de formulário (quando usado como `InlineBox` ou bloco).
pub(crate) fn paint_control_full(
    cx: &mut LayoutCx<'_, '_>,
    node: NodeId,
    info: &ControlInfo,
    style: &ComputedStyle,
    border_box: Rect,
    pad: [f32; 4],
    bw: [f32; 4],
) {
    let pushed_opacity = style.opacity < 1.0;
    if pushed_opacity {
        cx.items
            .push(PaintItem::PushOpacity(style.opacity.clamp(0.0, 1.0)));
    }
    let radius = crate::resolve_border_radius(style, border_box);
    if style.visible && !style.background_color.is_transparent() {
        cx.items.push(PaintItem::Rect {
            rect: border_box,
            color: style.background_color,
            radius,
        });
    }
    if style.visible && bw.iter().any(|&w| w > 0.0) {
        cx.items.push(PaintItem::Border {
            rect: border_box,
            widths: bw,
            colors: style.border_color,
        });
    }
    let content_box = Rect::new(
        border_box.x + bw[3] + pad[3],
        border_box.y + bw[0] + pad[0],
        (border_box.w - pad[1] - pad[3] - bw[1] - bw[3]).max(0.0),
        (border_box.h - pad[0] - pad[2] - bw[0] - bw[2]).max(0.0),
    );
    paint_control_inner(cx, node, info, style, border_box, content_box);
    if pushed_opacity {
        cx.items.push(PaintItem::PopOpacity);
    }
}

/// Pinta o conteúdo visual interno do controle e registra `FormControlArea`.
pub(crate) fn paint_control_inner(
    cx: &mut LayoutCx<'_, '_>,
    node: NodeId,
    info: &ControlInfo,
    style: &ComputedStyle,
    border_box: Rect,
    content_box: Rect,
) {
    if style.visible {
        match info.kind {
            FormControlKind::Checkbox { checked } => {
                if checked {
                    let accent = if info.disabled {
                        Rgba::new(140, 148, 170, 255)
                    } else {
                        Rgba::new(59, 130, 246, 255)
                    };
                    let radius = crate::resolve_border_radius(style, border_box).min(3.0);
                    cx.items.push(PaintItem::Rect {
                        rect: border_box,
                        color: accent,
                        radius,
                    });
                    let mut check_style = style.clone();
                    check_style.font_size = (border_box.h * 0.78).max(9.0);
                    check_style.font_weight = 700;
                    check_style.text_align = TextAlign::Center;
                    let layout = build_control_text_layout(
                        cx.engine,
                        &check_style,
                        "✓",
                        Rgba::WHITE,
                        Alignment::Center,
                        Some(border_box.w.max(1.0)),
                        false,
                    );
                    let ty = border_box.y + ((border_box.h - layout.height()) * 0.5).max(0.0);
                    cx.items.push(PaintItem::Text {
                        x: border_box.x,
                        y: ty,
                        bounds: border_box,
                        layout: Box::new(layout),
                    });
                }
            }
            FormControlKind::Radio { checked } => {
                if checked {
                    let accent = if info.disabled {
                        Rgba::new(140, 148, 170, 255)
                    } else {
                        Rgba::new(59, 130, 246, 255)
                    };
                    let dot_w = (border_box.w * 0.48).max(4.0);
                    let dot_h = (border_box.h * 0.48).max(4.0);
                    let dot = Rect::new(
                        border_box.x + (border_box.w - dot_w) * 0.5,
                        border_box.y + (border_box.h - dot_h) * 0.5,
                        dot_w,
                        dot_h,
                    );
                    cx.items.push(PaintItem::Rect {
                        rect: dot,
                        color: accent,
                        radius: dot_w.min(dot_h) * 0.5,
                    });
                }
            }
            FormControlKind::Button => {
                if !info.value.is_empty() && content_box.w > 0.0 && content_box.h > 0.0 {
                    cx.items.push(PaintItem::PushClip(content_box));
                    let layout = build_control_text_layout(
                        cx.engine,
                        style,
                        &info.value,
                        style.color,
                        Alignment::Center,
                        Some(content_box.w.max(1.0)),
                        true,
                    );
                    let ty = content_box.y + ((content_box.h - layout.height()) * 0.5).max(0.0);
                    cx.items.push(PaintItem::Text {
                        x: content_box.x,
                        y: ty,
                        bounds: content_box,
                        layout: Box::new(layout),
                    });
                    cx.items.push(PaintItem::PopClip);
                }
            }
            FormControlKind::TextInput | FormControlKind::Password | FormControlKind::Textarea => {
                let is_textarea = matches!(info.kind, FormControlKind::Textarea);
                let (shown, text_color): (Cow<'_, str>, Rgba) = if !info.value.is_empty() {
                    if matches!(info.kind, FormControlKind::Password) {
                        (
                            Cow::Owned("•".repeat(info.value.chars().count())),
                            style.color,
                        )
                    } else {
                        (Cow::Borrowed(&info.value), style.color)
                    }
                } else if !info.placeholder.is_empty() {
                    (
                        Cow::Borrowed(&info.placeholder),
                        Rgba::new(130, 132, 145, 255),
                    )
                } else {
                    (Cow::Borrowed(""), style.color)
                };
                if !shown.is_empty() && content_box.w > 0.0 && content_box.h > 0.0 {
                    cx.items.push(PaintItem::PushClip(content_box));
                    let layout = build_control_text_layout(
                        cx.engine,
                        style,
                        &shown,
                        text_color,
                        Alignment::Left,
                        is_textarea.then(|| content_box.w.max(1.0)),
                        is_textarea,
                    );
                    let ty = if is_textarea {
                        content_box.y
                    } else {
                        content_box.y + ((content_box.h - layout.height()) * 0.5).max(0.0)
                    };
                    cx.items.push(PaintItem::Text {
                        x: content_box.x,
                        y: ty,
                        bounds: content_box,
                        layout: Box::new(layout),
                    });
                    cx.items.push(PaintItem::PopClip);
                }
            }
            FormControlKind::Select => {
                if content_box.w > 0.0 && content_box.h > 0.0 {
                    cx.items.push(PaintItem::PushClip(content_box));
                    let arrow_w = 16.0f32.min(content_box.w * 0.4);
                    let label_w = (content_box.w - arrow_w).max(0.0);
                    if !info.value.is_empty() && label_w > 1.0 {
                        let layout = build_control_text_layout(
                            cx.engine,
                            style,
                            &info.value,
                            style.color,
                            Alignment::Left,
                            None,
                            false,
                        );
                        let ty = content_box.y + ((content_box.h - layout.height()) * 0.5).max(0.0);
                        cx.items.push(PaintItem::Text {
                            x: content_box.x,
                            y: ty,
                            bounds: Rect::new(content_box.x, content_box.y, label_w, content_box.h),
                            layout: Box::new(layout),
                        });
                    }
                    let arrow_layout = build_control_text_layout(
                        cx.engine,
                        style,
                        "▾",
                        style.color,
                        Alignment::Right,
                        Some(arrow_w.max(1.0)),
                        false,
                    );
                    let ay =
                        content_box.y + ((content_box.h - arrow_layout.height()) * 0.5).max(0.0);
                    let ax = content_box.x + (content_box.w - arrow_w).max(0.0);
                    cx.items.push(PaintItem::Text {
                        x: ax,
                        y: ay,
                        bounds: content_box,
                        layout: Box::new(arrow_layout),
                    });
                    cx.items.push(PaintItem::PopClip);
                }
            }
        }
    }

    cx.controls.push(FormControlArea {
        node,
        rect: border_box,
        kind: info.kind.clone(),
        value: info.value.clone(),
        placeholder: info.placeholder.clone(),
        disabled: info.disabled,
    });
}
