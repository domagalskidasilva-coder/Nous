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
