use super::*;
use nous_dom::{Attribute, ElementData, LocalName, NodeData, QualName, ns};
use nous_style::{Stylist, Viewport, compute_styles};

fn el(doc: &mut Document, parent: NodeId, name: &str, attrs: &[(&str, &str)]) -> NodeId {
    let id = doc.create(NodeData::Element(ElementData {
        name: QualName::new(None, ns!(html), LocalName::from(name)),
        attrs: attrs
            .iter()
            .map(|(k, v)| Attribute {
                name: QualName::new(None, ns!(), LocalName::from(*k)),
                value: (*v).into(),
            })
            .collect(),
        template_contents: None,
    }));
    doc.append(parent, id);
    id
}

fn text(doc: &mut Document, parent: NodeId, s: &str) {
    doc.append_text(parent, s);
}

/// Documento `<html><body>…</body></html>`; devolve (doc, body).
fn page() -> (Document, NodeId) {
    let mut doc = Document::new();
    let html = el(&mut doc, NodeId::DOCUMENT, "html", &[]);
    let body = el(&mut doc, html, "body", &[]);
    (doc, body)
}

fn run(doc: &Document, css: &str, width: f32) -> PageLayout {
    let mut stylist = Stylist::with_user_agent_sheet();
    stylist.add_author_sheet(css, None);
    let styles = compute_styles(
        doc,
        &stylist,
        Viewport {
            width,
            height: 800.0,
        },
    );
    let mut engine = LayoutEngine::new();
    layout_document(doc, &styles, &mut engine, width)
}

#[test]
fn altura_cresce_com_paragrafos() {
    let (mut d1, b1) = page();
    let p = el(&mut d1, b1, "p", &[]);
    text(&mut d1, p, "Olá, Nous.");
    let h1 = run(&d1, "", 800.0).height;

    let (mut d3, b3) = page();
    for _ in 0..3 {
        let p = el(&mut d3, b3, "p", &[]);
        text(&mut d3, p, "Olá, Nous.");
    }
    let h3 = run(&d3, "", 800.0).height;
    assert!(h1 > 0.0, "altura {h1}");
    assert!(h3 > h1 + 20.0, "1 parágrafo {h1}, 3 parágrafos {h3}");
}

#[test]
fn margem_auto_centraliza() {
    let (mut doc, body) = page();
    let div = el(&mut doc, body, "div", &[("id", "caixa")]);
    text(&mut doc, div, "centro");
    let layout = run(
        &doc,
        "#caixa { width: 600px; margin: 0 auto; background: red }",
        1000.0,
    );
    let rect = layout
        .items
        .iter()
        .find_map(|i| match i {
            PaintItem::Rect { rect, color, .. } if *color == Rgba::new(255, 0, 0, 255) => {
                Some(*rect)
            }
            _ => None,
        })
        .expect("fundo vermelho");
    // body tem margem de 8px: (984 - 600) / 2 + 8 = 200
    assert!((rect.x - 200.0).abs() < 0.01, "x = {}", rect.x);
    assert!((rect.w - 600.0).abs() < 0.01, "w = {}", rect.w);
}

#[test]
fn acha_link() {
    let (mut doc, body) = page();
    let p = el(&mut doc, body, "p", &[]);
    text(&mut doc, p, "Veja ");
    let a = el(&mut doc, p, "a", &[("href", "/destino")]);
    text(&mut doc, a, "este link");
    text(&mut doc, p, " agora.");
    let layout = run(&doc, "", 800.0);
    assert_eq!(layout.link_urls, vec!["/destino".to_owned()]);
    let area = layout.links.first().expect("área de link").rect;
    let (cx, cy) = (area.x + area.w / 2.0, area.y + area.h / 2.0);
    assert_eq!(layout.link_at(cx, cy), Some("/destino"));
    assert_eq!(layout.link_at(1.0, 1.0), None);
}

#[test]
fn display_none_nao_ocupa_espaco() {
    let (empty, _) = page();
    let base = run(&empty, "", 800.0).height;

    let (mut doc, body) = page();
    let div = el(&mut doc, body, "div", &[("style", "display: none")]);
    for _ in 0..5 {
        let p = el(&mut doc, div, "p", &[]);
        text(&mut doc, p, "escondido");
    }
    let layout = run(&doc, "", 800.0);
    assert_eq!(layout.height, base);
    assert!(
        layout
            .items
            .iter()
            .all(|i| !matches!(i, PaintItem::Text { .. }))
    );
}

#[test]
fn texto_quebra_em_linhas() {
    let (mut doc, body) = page();
    let p = el(&mut doc, body, "p", &[]);
    text(
        &mut doc,
        p,
        "Um navegador leve precisa quebrar linhas longas em várias linhas quando a largura é pequena.",
    );
    let layout = run(&doc, "body { margin: 0 }", 120.0);
    let lines = layout
        .items
        .iter()
        .find_map(|i| match i {
            PaintItem::Text { layout, .. } => Some(layout.len()),
            _ => None,
        })
        .expect("texto");
    assert!(lines > 2, "linhas = {lines}");
}

#[test]
fn colapso_de_espacos() {
    let (mut doc, body) = page();
    let p = el(&mut doc, body, "p", &[]);
    text(&mut doc, p, "  a  \n\t b ");
    let b = el(&mut doc, p, "b", &[]);
    text(&mut doc, b, " c ");
    let layout = run(&doc, "", 800.0);
    let t = layout
        .items
        .iter()
        .find_map(|i| match i {
            PaintItem::Text { layout, .. } => Some(layout.text_len()),
            _ => None,
        })
        .expect("texto");
    // "a b c" = 5 bytes
    assert_eq!(t, 5);
}

#[test]
fn lista_tem_marcador() {
    let (mut doc, body) = page();
    let ol = el(&mut doc, body, "ol", &[]);
    for _ in 0..2 {
        let li = el(&mut doc, ol, "li", &[]);
        text(&mut doc, li, "item");
    }
    let layout = run(&doc, "", 800.0);
    let texts = layout
        .items
        .iter()
        .filter(|i| matches!(i, PaintItem::Text { .. }))
        .count();
    assert_eq!(texts, 4, "2 conteúdos + 2 marcadores");
}

fn run_with_images(doc: &Document, css: &str, width: f32, images: &ImageMap) -> PageLayout {
    let mut stylist = Stylist::with_user_agent_sheet();
    stylist.add_author_sheet(css, None);
    let styles = compute_styles(
        doc,
        &stylist,
        Viewport {
            width,
            height: 800.0,
        },
    );
    let mut engine = LayoutEngine::new();
    layout_document_with_images(doc, &styles, &mut engine, width, images)
}

#[test]
fn flexbox_lado_a_lado_e_gap() {
    let (mut doc, body) = page();
    let row = el(&mut doc, body, "div", &[("id", "row")]);
    let a = el(&mut doc, row, "div", &[("id", "a")]);
    text(&mut doc, a, "Coluna A");
    let b = el(&mut doc, row, "div", &[("id", "b")]);
    text(&mut doc, b, "Coluna B");

    let layout = run(
        &doc,
        "body { margin: 0 } \
         #row { display: flex; gap: 20px; width: 420px; } \
         #a { flex: 1; background: red; } \
         #b { flex: 1; background: blue; }",
        800.0,
    );
    let ra = layout
        .items
        .iter()
        .find_map(|i| match i {
            PaintItem::Rect { rect, color, .. } if *color == Rgba::new(255, 0, 0, 255) => {
                Some(*rect)
            }
            _ => None,
        })
        .expect("a");
    let rb = layout
        .items
        .iter()
        .find_map(|i| match i {
            PaintItem::Rect { rect, color, .. } if *color == Rgba::new(0, 0, 255, 255) => {
                Some(*rect)
            }
            _ => None,
        })
        .expect("b");
    assert!((ra.w - 200.0).abs() < 1.0, "ra.w = {}", ra.w);
    assert!((rb.w - 200.0).abs() < 1.0, "rb.w = {}", rb.w);
    assert!((rb.x - 220.0).abs() < 1.0, "rb.x = {}", rb.x);
    assert!((ra.y - rb.y).abs() < 0.5);
}

#[test]
fn grid_repeat_e_fr() {
    let (mut doc, body) = page();
    let grid = el(&mut doc, body, "div", &[("id", "grid")]);
    let c1 = el(&mut doc, grid, "div", &[("id", "c1")]);
    text(&mut doc, c1, "1");
    let c2 = el(&mut doc, grid, "div", &[("id", "c2")]);
    text(&mut doc, c2, "2");
    let c3 = el(&mut doc, grid, "div", &[("id", "c3")]);
    text(&mut doc, c3, "3");

    let layout = run(
        &doc,
        "body { margin: 0 } \
         #grid { display: grid; grid-template-columns: 100px repeat(2, 1fr); gap: 10px; width: 520px; } \
         #c1 { background: red; } #c2 { background: green; } #c3 { background: blue; }",
        800.0,
    );
    let rects: Vec<Rect> = layout
        .items
        .iter()
        .filter_map(|i| match i {
            PaintItem::Rect { rect, .. } => Some(*rect),
            _ => None,
        })
        .collect();
    assert_eq!(rects.len(), 3);
    assert!((rects[0].w - 100.0).abs() < 1.0);
    assert!((rects[1].x - 110.0).abs() < 1.0);
    assert!((rects[1].w - 200.0).abs() < 1.0);
    assert!((rects[2].x - 320.0).abs() < 1.0);
    assert!((rects[2].w - 200.0).abs() < 1.0);
}

#[test]
fn border_box_relative_clip_e_opacity() {
    let (mut doc, body) = page();
    let box_el = el(&mut doc, body, "div", &[("id", "bx")]);
    let inner = el(&mut doc, box_el, "p", &[]);
    text(&mut doc, inner, "conteúdo cortado");

    let layout = run(
        &doc,
        "body { margin: 0 } \
         #bx { box-sizing: border-box; width: 200px; padding: 20px; border: 5px solid black; \
               max-height: 80px; min-width: 100px; overflow: hidden; opacity: 0.5; \
               position: relative; left: 15px; top: 10px; background: red; }",
        800.0,
    );
    let bg = layout
        .items
        .iter()
        .find_map(|i| match i {
            PaintItem::Rect { rect, color, .. } if *color == Rgba::new(255, 0, 0, 255) => {
                Some(*rect)
            }
            _ => None,
        })
        .expect("rect");
    assert!((bg.w - 200.0).abs() < 0.5, "border-box w = {}", bg.w);
    assert!((bg.x - 15.0).abs() < 0.5, "relative x = {}", bg.x);
    assert!((bg.y - 10.0).abs() < 0.5, "relative y = {}", bg.y);

    let push_clips = layout
        .items
        .iter()
        .filter(|i| matches!(i, PaintItem::PushClip(_)))
        .count();
    let pop_clips = layout
        .items
        .iter()
        .filter(|i| matches!(i, PaintItem::PopClip))
        .count();
    let push_op = layout
        .items
        .iter()
        .filter(|i| matches!(i, PaintItem::PushOpacity(_)))
        .count();
    let pop_op = layout
        .items
        .iter()
        .filter(|i| matches!(i, PaintItem::PopOpacity))
        .count();
    assert_eq!(push_clips, 1);
    assert_eq!(pop_clips, 1);
    assert_eq!(push_op, 1);
    assert_eq!(pop_op, 1);
}

#[test]
fn imagem_aspect_ratio_e_alt_fallback() {
    let (mut doc, body) = page();
    let p = el(&mut doc, body, "p", &[]);
    let img_dec = el(&mut doc, p, "img", &[("style", "width: 100px")]);
    let img_alt = el(&mut doc, p, "img", &[("alt", "Logo")]);

    let mut images = ImageMap::new();
    images.insert(
        img_dec,
        DecodedImage {
            width: 400,
            height: 200,
            rgba: Arc::from(vec![255u8; 400 * 200 * 4]),
        },
    );
    let layout = run_with_images(&doc, "body { margin: 0 }", 800.0, &images);
    let img_rect = layout
        .items
        .iter()
        .find_map(|i| match i {
            PaintItem::Image { rect, .. } => Some(*rect),
            _ => None,
        })
        .expect("PaintItem::Image");
    assert!((img_rect.w - 100.0).abs() < 0.5);
    assert!((img_rect.h - 50.0).abs() < 0.5);
    assert!(img_alt.index() > 0);
    let has_alt_text = layout
        .items
        .iter()
        .any(|i| matches!(i, PaintItem::Text { layout, .. } if layout.text_len() >= 4));
    assert!(has_alt_text);
}

#[test]
fn controles_de_formulario() {
    let (mut doc, body) = page();
    let form = el(&mut doc, body, "div", &[("style", "display: flex; gap: 8px")]);
    let _inp = el(
        &mut doc,
        form,
        "input",
        &[("type", "text"), ("placeholder", "Buscar")],
    );
    let _chk = el(
        &mut doc,
        form,
        "input",
        &[("type", "checkbox"), ("checked", "")],
    );
    let _rad = el(
        &mut doc,
        form,
        "input",
        &[("type", "radio"), ("checked", "")],
    );
    let btn = el(&mut doc, form, "button", &[]);
    text(&mut doc, btn, "Enviar");
    let ta = el(&mut doc, body, "textarea", &[("rows", "2")]);
    text(&mut doc, ta, "Oi");
    let sel = el(&mut doc, body, "select", &[]);
    let opt = el(&mut doc, sel, "option", &[("selected", "")]);
    text(&mut doc, opt, "Opção 1");

    let layout = run(&doc, "", 800.0);
    assert_eq!(layout.controls.len(), 6);
    assert_eq!(layout.controls[0].kind, FormControlKind::TextInput);
    assert_eq!(layout.controls[0].placeholder, "Buscar");
    assert_eq!(
        layout.controls[1].kind,
        FormControlKind::Checkbox { checked: true }
    );
    assert_eq!(
        layout.controls[2].kind,
        FormControlKind::Radio { checked: true }
    );
    assert_eq!(layout.controls[3].kind, FormControlKind::Button);
    assert_eq!(layout.controls[3].value, "Enviar");
    assert_eq!(layout.controls[4].kind, FormControlKind::Textarea);
    assert_eq!(layout.controls[5].kind, FormControlKind::Select);
    assert_eq!(layout.controls[5].value, "Opção 1");
}

#[test]
fn tabela_e_inline_flex_e_limites_extremos() {
    let (mut doc, body) = page();
    let table = el(&mut doc, body, "table", &[]);
    let tbody = el(&mut doc, table, "tbody", &[]);
    let r1 = el(&mut doc, tbody, "tr", &[]);
    let td1 = el(&mut doc, r1, "td", &[("style", "width: 120px; background: #111111")]);
    text(&mut doc, td1, "A");
    let td2 = el(&mut doc, r1, "td", &[("style", "width: 180px; background: #222222")]);
    text(&mut doc, td2, "B");

    let iflex = el(&mut doc, body, "div", &[("style", "display: inline-flex; gap: 10px")]);
    let s1 = el(&mut doc, iflex, "span", &[("style", "width: 50px; height: 20px; background: #333333")]);
    let s2 = el(&mut doc, iflex, "span", &[("style", "width: 60px; height: 20px; background: #444444")]);
    let _ = (s1, s2);

    let layout = run(&doc, "body { margin: 0 }", 800.0);
    let r_a = layout
        .items
        .iter()
        .find_map(|i| match i {
            PaintItem::Rect { rect, color, .. } if *color == Rgba::new(0x11, 0x11, 0x11, 255) => {
                Some(*rect)
            }
            _ => None,
        })
        .expect("td1");
    let r_b = layout
        .items
        .iter()
        .find_map(|i| match i {
            PaintItem::Rect { rect, color, .. } if *color == Rgba::new(0x22, 0x22, 0x22, 255) => {
                Some(*rect)
            }
            _ => None,
        })
        .expect("td2");
    assert!(r_b.x > r_a.x + r_a.w - 1.0);
    assert!((r_a.y - r_b.y).abs() < 0.5);
    assert!(layout.approx_heap_bytes() > 0);

    // Viewport largura zero não deve entrar em pânico
    let zero_layout = run(&doc, "", 0.0);
    assert_eq!(zero_layout.width, 0.0);
}
