//! Pintura do Nous: converte o [`PageLayout`] em comandos do Vello (GPU).
//!
//! Só o que aparece na tela é desenhado: itens fora do retângulo visível nem entram
//! na cena, o que poupa CPU para montar a cena e memória de GPU para renderizá-la.

pub use nous_layout::{LayoutEngine, PageLayout, Rect, TextBrush};
pub use parley;
pub use vello;

use nous_layout::PaintItem;
use nous_style::Rgba;
use parley::{Layout, PositionedLayoutItem};
use vello::kurbo::{Affine, Rect as KRect, RoundedRect};
use vello::peniko::{Color, Fill};
use vello::{FontEmbolden, Scene};

pub fn to_color(c: [u8; 4]) -> Color {
    Color::from_rgba8(c[0], c[1], c[2], c[3])
}

pub fn rgba(c: Rgba) -> Color {
    Color::from_rgba8(c.r, c.g, c.b, c.a)
}

fn krect(r: &Rect) -> KRect {
    KRect::new(
        r.x as f64,
        r.y as f64,
        (r.x + r.w) as f64,
        (r.y + r.h) as f64,
    )
}

/// Desenha a página. `visible` é a área visível em coordenadas da página e
/// `transform` leva da página para a tela (rolagem, posição da aba, escala do monitor).
pub fn paint_page(scene: &mut Scene, page: &PageLayout, transform: Affine, visible: Rect) {
    if !page.canvas_color.is_transparent() {
        scene.fill(
            Fill::NonZero,
            transform,
            rgba(page.canvas_color),
            None,
            &krect(&visible),
        );
    }
    for item in &page.items {
        if !item.bounds().intersects(&visible) {
            continue;
        }
        match item {
            PaintItem::Rect {
                rect,
                color,
                radius,
            } => {
                if color.is_transparent() {
                    continue;
                }
                if *radius > 0.0 {
                    let rr = RoundedRect::from_rect(krect(rect), *radius as f64);
                    scene.fill(Fill::NonZero, transform, rgba(*color), None, &rr);
                } else {
                    scene.fill(Fill::NonZero, transform, rgba(*color), None, &krect(rect));
                }
            }
            PaintItem::Border {
                rect,
                widths,
                colors,
            } => {
                let [t, r, b, l] = *widths;
                let sides = [
                    (Rect::new(rect.x, rect.y, rect.w, t), colors[0]),
                    (Rect::new(rect.x + rect.w - r, rect.y, r, rect.h), colors[1]),
                    (Rect::new(rect.x, rect.y + rect.h - b, rect.w, b), colors[2]),
                    (Rect::new(rect.x, rect.y, l, rect.h), colors[3]),
                ];
                for (side, color) in sides {
                    if side.w > 0.0 && side.h > 0.0 && !color.is_transparent() {
                        scene.fill(Fill::NonZero, transform, rgba(color), None, &krect(&side));
                    }
                }
            }
            PaintItem::Text { x, y, layout, .. } => {
                let t = transform * Affine::translate((*x as f64, *y as f64));
                draw_text(scene, layout, t, None);
            }
        }
    }
}

/// Desenha um layout de texto do Parley (glifos, sublinhado e riscado).
/// `color_override` troca a cor de todos os glifos (útil na interface).
pub fn draw_text(
    scene: &mut Scene,
    layout: &Layout<TextBrush>,
    transform: Affine,
    color_override: Option<[u8; 4]>,
) {
    let mut coords: Vec<i16> = Vec::new();
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let style = glyph_run.style();
            let color = to_color(color_override.unwrap_or(style.brush.color));
            if color_override.is_none() && style.brush.color[3] == 0 {
                continue; // texto invisível (visibility: hidden)
            }
            let run = glyph_run.run();
            let font = run.font();
            let font_size = run.font_size();
            let synthesis = run.synthesis();
            let glyph_xform = synthesis
                .skew()
                .map(|angle| Affine::skew(angle.to_radians().tan() as f64, 0.0));
            coords.clear();
            coords.extend(run.normalized_coords().iter().map(|c| c.to_bits()));
            let mut draw = scene
                .draw_glyphs(font)
                .brush(color)
                .hint(true)
                .transform(transform)
                .glyph_transform(glyph_xform)
                .font_size(font_size)
                .normalized_coords(&coords);
            if synthesis.embolden() {
                let amount = (font_size as f64 / 48.0).max(0.25);
                draw = draw.font_embolden(FontEmbolden::new(vello::kurbo::Diagonal2::new(
                    amount, amount,
                )));
            }
            draw.draw(
                Fill::NonZero,
                glyph_run.positioned_glyphs().map(|g| vello::Glyph {
                    id: g.id,
                    x: g.x,
                    y: g.y,
                }),
            );

            let metrics = run.font_metrics();
            let x = glyph_run.offset();
            let w = glyph_run.advance();
            let baseline = glyph_run.baseline();
            if let Some(deco) = &style.underline {
                let offset = deco.offset.unwrap_or(metrics.underline_offset);
                let size = deco.size.unwrap_or(metrics.underline_size).max(1.0);
                let c = to_color(color_override.unwrap_or(deco.brush.color));
                let y = baseline - offset;
                let r = KRect::new(x as f64, y as f64, (x + w) as f64, (y + size) as f64);
                scene.fill(Fill::NonZero, transform, c, None, &r);
            }
            if let Some(deco) = &style.strikethrough {
                let offset = deco.offset.unwrap_or(metrics.strikethrough_offset);
                let size = deco.size.unwrap_or(metrics.strikethrough_size).max(1.0);
                let c = to_color(color_override.unwrap_or(deco.brush.color));
                let y = baseline - offset;
                let r = KRect::new(x as f64, y as f64, (x + w) as f64, (y + size) as f64);
                scene.fill(Fill::NonZero, transform, c, None, &r);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use parley::StyleProperty;

    fn layout_simples(engine: &mut LayoutEngine, text: &str) -> Layout<TextBrush> {
        let mut b = engine
            .layout_cx
            .ranged_builder(&mut engine.font_cx, text, 1.0, true);
        b.push_default(StyleProperty::FontSize(16.0));
        b.push_default(StyleProperty::Brush(TextBrush {
            color: [10, 20, 30, 255],
            link: 0,
        }));
        b.push(StyleProperty::Underline(true), 0..3);
        b.push(StyleProperty::Strikethrough(true), 4..6);
        let mut l = b.build(text);
        l.break_all_lines(Some(200.0));
        l
    }

    #[test]
    fn desenha_texto_sem_panico() {
        let mut engine = LayoutEngine::new();
        let layout = layout_simples(&mut engine, "Nous navegador leve");
        let mut scene = Scene::new();
        draw_text(&mut scene, &layout, Affine::IDENTITY, None);
        draw_text(
            &mut scene,
            &layout,
            Affine::translate((5.0, 5.0)),
            Some([255, 0, 0, 255]),
        );
    }

    #[test]
    fn pinta_pagina_sem_panico() {
        let mut engine = LayoutEngine::new();
        let layout = layout_simples(&mut engine, "texto da página");
        let page = PageLayout {
            width: 800.0,
            height: 2000.0,
            canvas_color: Rgba::WHITE,
            items: vec![
                PaintItem::Rect {
                    rect: Rect::new(0.0, 0.0, 100.0, 50.0),
                    color: Rgba::new(255, 0, 0, 255),
                    radius: 6.0,
                },
                PaintItem::Border {
                    rect: Rect::new(10.0, 10.0, 100.0, 50.0),
                    widths: [1.0, 2.0, 3.0, 4.0],
                    colors: [Rgba::BLACK; 4],
                },
                PaintItem::Text {
                    x: 0.0,
                    y: 1500.0,
                    bounds: Rect::new(0.0, 1500.0, 800.0, layout.height()),
                    layout: Box::new(layout),
                },
            ],
            links: vec![],
            link_urls: vec![],
        };
        let mut scene = Scene::new();
        // só o topo visível: o texto em y=1500 fica de fora
        paint_page(
            &mut scene,
            &page,
            Affine::IDENTITY,
            Rect::new(0.0, 0.0, 800.0, 600.0),
        );
        paint_page(
            &mut scene,
            &page,
            Affine::translate((0.0, -1200.0)),
            Rect::new(0.0, 1200.0, 800.0, 600.0),
        );
    }
}
