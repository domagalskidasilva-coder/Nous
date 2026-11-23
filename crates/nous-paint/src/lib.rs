//! Pintura do Nous: converte o [`PageLayout`] em comandos do Vello (GPU) e decodifica imagens.
//!
//! Só o que aparece na tela é desenhado: itens fora do retângulo visível nem entram
//! na cena, mantendo `PushClip`/`PopClip` e `PushOpacity`/`PopOpacity` estritamente balanceados.

pub use nous_layout::{
    DecodedImage, FormControlArea, FormControlKind, ImageMap, LayoutEngine, PageLayout, Rect,
    TextBrush,
};
pub use parley;
pub use vello;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use nous_layout::PaintItem;
use nous_style::Rgba;
use parley::{Layout, PositionedLayoutItem};
use vello::kurbo::{Affine, Rect as KRect, RoundedRect};
use vello::peniko::{
    BlendMode, Blob, Color, Fill, ImageAlphaType, ImageBrush, ImageData, ImageFormat,
    ImageQuality,
};
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

/// Decodifica bytes de imagem (PNG, JPEG, WebP, GIF) para RGBA8 e reduz proporcionalmente
/// caso `width > max_dim || height > max_dim` para economizar RAM.
pub fn decode_image_downscaled(bytes: &[u8], max_dim: u32) -> Result<DecodedImage, String> {
    if bytes.is_empty() {
        return Err("dados de imagem vazios".into());
    }
    let img = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return Err("imagem com dimensão zero".into());
    }
    let resized = if max_dim > 0 && (w > max_dim || h > max_dim) {
        img.resize(max_dim, max_dim, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgba = resized.into_rgba8();
    let (width, height) = rgba.dimensions();
    let raw = rgba.into_raw().into_boxed_slice();
    Ok(DecodedImage {
        width,
        height,
        rgba: Arc::from(raw),
    })
}

struct ArcBytes(Arc<[u8]>);

impl AsRef<[u8]> for ArcBytes {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

type ImageIdTable = Mutex<HashMap<usize, (Weak<[u8]>, u64)>>;

/// Mantém IDs estáveis no cache de atlas da Vello para cada `Arc<[u8]>` vivo.
fn stable_image_blob(rgba: &Arc<[u8]>) -> Blob<u8> {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1u64 << 63);
    static TABLE: OnceLock<ImageIdTable> = OnceLock::new();

    let ptr_key = Arc::as_ptr(rgba) as *const u8 as usize;
    let table = TABLE.get_or_init(|| Mutex::new(HashMap::new()));
    let id = if let Ok(mut map) = table.lock() {
        if let Some((weak, id)) = map.get(&ptr_key)
            && weak.strong_count() > 0
            && Weak::ptr_eq(weak, &Arc::downgrade(rgba))
        {
            *id
        } else {
            if map.len() > 512 {
                map.retain(|_, (w, _)| w.strong_count() > 0);
            }
            let new_id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            map.insert(ptr_key, (Arc::downgrade(rgba), new_id));
            new_id
        }
    } else {
        NEXT_ID.fetch_add(1, Ordering::Relaxed)
    };
    let data: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(ArcBytes(Arc::clone(rgba)));
    Blob::from_raw_parts(data, id)
}

#[derive(Clone, Copy)]
enum LayerEntry {
    Clip { rect: Rect, pushed: bool },
    Opacity { alpha: f32, pushed: bool },
}

impl LayerEntry {
    fn is_pushed(self) -> bool {
        match self {
            LayerEntry::Clip { pushed, .. } | LayerEntry::Opacity { pushed, .. } => pushed,
        }
    }
}

fn flush_layers(
    scene: &mut Scene,
    layers: &mut [LayerEntry],
    transform: Affine,
    visible: Rect,
) -> (usize, usize) {
    let mut pushed_clips = 0;
    let mut pushed_opacities = 0;
    for layer in layers.iter_mut() {
        match layer {
            LayerEntry::Clip { rect, pushed } if !*pushed => {
                scene.push_clip_layer(Fill::NonZero, transform, &krect(rect));
                *pushed = true;
                pushed_clips += 1;
            }
            LayerEntry::Opacity { alpha, pushed } if !*pushed => {
                scene.push_layer(
                    Fill::NonZero,
                    BlendMode::default(),
                    *alpha,
                    transform,
                    &krect(&visible),
                );
                *pushed = true;
                pushed_opacities += 1;
            }
            _ => {}
        }
    }
    (pushed_clips, pushed_opacities)
}

fn can_draw_inside_layers(layers: &[LayerEntry], item_bounds: &Rect, visible: &Rect) -> bool {
    for layer in layers {
        match layer {
            LayerEntry::Clip { rect, .. } => {
                if !rect.intersects(visible) || !rect.intersects(item_bounds) {
                    return false;
                }
            }
            LayerEntry::Opacity { alpha, .. } => {
                if *alpha <= 0.0 {
                    return false;
                }
            }
        }
    }
    true
}

/// Desenha a página. `visible` é a área visível em coordenadas da página e
/// `transform` leva da página para a tela (rolagem, posição da aba, escala do monitor).
pub fn paint_page(scene: &mut Scene, page: &PageLayout, transform: Affine, visible: Rect) {
    paint_page_inner(scene, page, transform, visible);
}

/// Implementação interna que também retorna `(pushes, pops)` para testes de balanceamento.
fn paint_page_inner(
    scene: &mut Scene,
    page: &PageLayout,
    transform: Affine,
    visible: Rect,
) -> (usize, usize) {
    if !page.canvas_color.is_transparent() {
        scene.fill(
            Fill::NonZero,
            transform,
            rgba(page.canvas_color),
            None,
            &krect(&visible),
        );
    }

    let mut layers: Vec<LayerEntry> = Vec::new();
    let mut pushes = 0usize;
    let mut pops = 0usize;

    for item in &page.items {
        match item {
            PaintItem::PushClip(rect) => {
                layers.push(LayerEntry::Clip {
                    rect: *rect,
                    pushed: false,
                });
                continue;
            }
            PaintItem::PopClip => {
                while let Some(top) = layers.pop() {
                    if top.is_pushed() {
                        scene.pop_layer();
                        pops += 1;
                    }
                    if matches!(top, LayerEntry::Clip { .. }) {
                        break;
                    }
                }
                continue;
            }
            PaintItem::PushOpacity(alpha) => {
                layers.push(LayerEntry::Opacity {
                    alpha: alpha.clamp(0.0, 1.0),
                    pushed: false,
                });
                continue;
            }
            PaintItem::PopOpacity => {
                while let Some(top) = layers.pop() {
                    if top.is_pushed() {
                        scene.pop_layer();
                        pops += 1;
                    }
                    if matches!(top, LayerEntry::Opacity { .. }) {
                        break;
                    }
                }
                continue;
            }
            _ => {}
        }

        let bounds = item.bounds();
        if !bounds.intersects(&visible) || !can_draw_inside_layers(&layers, &bounds, &visible) {
            continue;
        }

        match item {
            PaintItem::Rect {
                rect,
                color,
                radius,
            } => {
                if color.is_transparent() || rect.w <= 0.0 || rect.h <= 0.0 {
                    continue;
                }
                let (pc, po) = flush_layers(scene, &mut layers, transform, visible);
                pushes += pc + po;
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
                let any_visible = sides
                    .iter()
                    .any(|(s, c)| s.w > 0.0 && s.h > 0.0 && !c.is_transparent());
                if !any_visible {
                    continue;
                }
                let (pc, po) = flush_layers(scene, &mut layers, transform, visible);
                pushes += pc + po;
                for (side, color) in sides {
                    if side.w > 0.0 && side.h > 0.0 && !color.is_transparent() {
                        scene.fill(Fill::NonZero, transform, rgba(color), None, &krect(&side));
                    }
                }
            }
            PaintItem::Text { x, y, layout, .. } => {
                let (pc, po) = flush_layers(scene, &mut layers, transform, visible);
                pushes += pc + po;
                let t = transform * Affine::translate((*x as f64, *y as f64));
                draw_text(scene, layout, t, None);
            }
            PaintItem::Image {
                rect,
                image,
                radius,
            } => {
                let expected = (image.width as usize)
                    .saturating_mul(image.height as usize)
                    .saturating_mul(4);
                if image.width == 0
                    || image.height == 0
                    || rect.w <= 0.0
                    || rect.h <= 0.0
                    || image.rgba.len() < expected
                {
                    continue;
                }
                let (pc, po) = flush_layers(scene, &mut layers, transform, visible);
                pushes += pc + po;
                let data = ImageData {
                    data: stable_image_blob(&image.rgba),
                    format: ImageFormat::Rgba8,
                    alpha_type: ImageAlphaType::Alpha,
                    width: image.width,
                    height: image.height,
                };
                let brush = ImageBrush::new(data).with_quality(ImageQuality::Medium);
                let brush_xform = Affine::translate((rect.x as f64, rect.y as f64))
                    * Affine::scale_non_uniform(
                        rect.w as f64 / image.width as f64,
                        rect.h as f64 / image.height as f64,
                    );
                if *radius > 0.0 {
                    let rr = RoundedRect::from_rect(krect(rect), *radius as f64);
                    scene.fill(Fill::NonZero, transform, &brush, Some(brush_xform), &rr);
                } else {
                    scene.draw_image(&brush, transform * brush_xform);
                }
            }
            PaintItem::PushClip(_)
            | PaintItem::PopClip
            | PaintItem::PushOpacity(_)
            | PaintItem::PopOpacity => {}
        }
    }

    while let Some(layer) = layers.pop() {
        if layer.is_pushed() {
            scene.pop_layer();
            pops += 1;
        }
    }

    (pushes, pops)
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
    use image::{ImageBuffer, ImageFormat as ImgFmt, Rgba as ImgRgba};
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
            controls: vec![],
        };
        let mut scene = Scene::new();
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

    #[test]
    fn decode_image_downscaled_preserva_proporcao() {
        let buf = ImageBuffer::from_pixel(400, 200, ImgRgba([10u8, 20, 30, 255]));
        let mut png_bytes = Vec::new();
        buf.write_to(&mut std::io::Cursor::new(&mut png_bytes), ImgFmt::Png)
            .expect("encode png");

        let decoded = decode_image_downscaled(&png_bytes, 100).expect("decode");
        assert_eq!(decoded.width, 100);
        assert_eq!(decoded.height, 50);
        assert_eq!(decoded.rgba.len(), 100 * 50 * 4);

        let full = decode_image_downscaled(&png_bytes, 1000).expect("decode full");
        assert_eq!(full.width, 400);
        assert_eq!(full.height, 200);
    }

    #[test]
    fn pinta_imagem_e_camadas_balanceadas_no_culling() {
        let img = DecodedImage {
            width: 2,
            height: 2,
            rgba: Arc::from([255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 0, 255]),
        };
        let page = PageLayout {
            width: 800.0,
            height: 2000.0,
            canvas_color: Rgba::WHITE,
            items: vec![
                // Camada visível com filho visível e filho fora da tela
                PaintItem::PushOpacity(0.5),
                PaintItem::PushClip(Rect::new(0.0, 0.0, 300.0, 300.0)),
                PaintItem::Image {
                    rect: Rect::new(10.0, 10.0, 100.0, 80.0),
                    image: img.clone(),
                    radius: 8.0,
                },
                PaintItem::Rect {
                    rect: Rect::new(10.0, 1400.0, 50.0, 50.0),
                    color: Rgba::new(0, 0, 255, 255),
                    radius: 0.0,
                },
                PaintItem::PopClip,
                PaintItem::PopOpacity,
                // Camada inteira fora da tela (todos os filhos culled)
                PaintItem::PushOpacity(0.8),
                PaintItem::PushClip(Rect::new(0.0, 1200.0, 200.0, 200.0)),
                PaintItem::Image {
                    rect: Rect::new(10.0, 1250.0, 50.0, 50.0),
                    image: img,
                    radius: 0.0,
                },
                PaintItem::PopClip,
                PaintItem::PopOpacity,
                // Clip na tela mas sem filhos visíveis
                PaintItem::PushClip(Rect::new(0.0, 0.0, 200.0, 200.0)),
                PaintItem::Rect {
                    rect: Rect::new(0.0, 1500.0, 100.0, 50.0),
                    color: Rgba::BLACK,
                    radius: 0.0,
                },
                PaintItem::PopClip,
            ],
            links: vec![],
            link_urls: vec![],
            controls: vec![],
        };
        let mut scene = Scene::new();
        let (pushes, pops) = paint_page_inner(
            &mut scene,
            &page,
            Affine::IDENTITY,
            Rect::new(0.0, 0.0, 800.0, 600.0),
        );
        assert_eq!(pushes, pops);
        assert_eq!(pushes, 2); // só PushOpacity(0.5) + PushClip(0..300) foram enviados
    }
}
