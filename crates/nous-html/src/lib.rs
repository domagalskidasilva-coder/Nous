//! Parser HTML do Nous: o html5ever (o mesmo parser do Servo, que segue a especificação
//! WHATWG) entrega eventos de construção da árvore e este `TreeSink` grava cada nó
//! direto no DOM em arena do `nous-dom`, sem árvore intermediária.

use std::borrow::Cow;
use std::cell::{Ref, RefCell};
use std::collections::HashSet;

use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeBuilderOpts, TreeSink};
use html5ever::{Attribute, ParseOpts, QualName, parse_document};
use nous_dom::{Document, ElementData, NodeData, NodeId};

/// Analisa um documento HTML completo.
///
/// O parser roda com "scripting" ligado (como um navegador com JavaScript): o conteúdo
/// de `<noscript>` vira texto bruto e a folha padrão o esconde.
pub fn parse_html(html: &str) -> Document {
    parse_html_with_scripting(html, true)
}

/// Igual a [`parse_html`], escolhendo se o parser age como se houvesse JavaScript.
/// Com `false`, o conteúdo de `<noscript>` é analisado como HTML normal.
pub fn parse_html_with_scripting(html: &str, scripting_enabled: bool) -> Document {
    let opts = ParseOpts {
        tree_builder: TreeBuilderOpts {
            scripting_enabled,
            ..Default::default()
        },
        ..Default::default()
    };
    parse_document(Sink::default(), opts).one(html)
}

#[derive(Default)]
struct Sink {
    doc: RefCell<Document>,
    /// Elementos `annotation-xml` do MathML que são pontos de integração HTML.
    mathml_integration_points: RefCell<HashSet<NodeId>>,
}

impl TreeSink for Sink {
    type Handle = NodeId;
    type Output = Document;
    type ElemName<'a> = Ref<'a, QualName>;

    fn finish(self) -> Document {
        self.doc.into_inner()
    }

    fn parse_error(&self, _msg: Cow<'static, str>) {
        // Erros de parse são normais na web real; o algoritmo já se recupera deles.
    }

    fn get_document(&self) -> NodeId {
        NodeId::DOCUMENT
    }

    fn elem_name<'a>(&'a self, target: &'a NodeId) -> Ref<'a, QualName> {
        Ref::map(self.doc.borrow(), |d| {
            &d.element(*target)
                .expect("html5ever pediu o nome de um nó que não é elemento")
                .name
        })
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> NodeId {
        let mut doc = self.doc.borrow_mut();
        let template_contents = flags.template.then(|| doc.create(NodeData::Fragment));
        let id = doc.create(NodeData::Element(ElementData {
            name,
            // valores viram `Box<str>`: o documento pode ir para outra thread
            attrs: attrs.into_iter().map(Into::into).collect(),
            template_contents,
        }));
        if flags.mathml_annotation_xml_integration_point {
            self.mathml_integration_points.borrow_mut().insert(id);
        }
        id
    }

    fn create_comment(&self, text: StrTendril) -> NodeId {
        self.doc
            .borrow_mut()
            .create(NodeData::Comment(text.to_string()))
    }

    fn create_pi(&self, _target: StrTendril, data: StrTendril) -> NodeId {
        // Instruções de processamento só existem em XML; no HTML viram comentários.
        self.doc
            .borrow_mut()
            .create(NodeData::Comment(data.to_string()))
    }

    fn append(&self, parent: &NodeId, child: NodeOrText<NodeId>) {
        let mut doc = self.doc.borrow_mut();
        match child {
            NodeOrText::AppendNode(node) => doc.append(*parent, node),
            NodeOrText::AppendText(text) => doc.append_text(*parent, &text),
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &NodeId,
        prev_element: &NodeId,
        child: NodeOrText<NodeId>,
    ) {
        let has_parent = self.doc.borrow().node(*element).parent.is_some();
        if has_parent {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(
        &self,
        name: StrTendril,
        _public_id: StrTendril,
        _system_id: StrTendril,
    ) {
        let mut doc = self.doc.borrow_mut();
        let id = doc.create(NodeData::Doctype {
            name: name.to_string(),
        });
        doc.append(NodeId::DOCUMENT, id);
    }

    fn get_template_contents(&self, target: &NodeId) -> NodeId {
        self.doc
            .borrow()
            .element(*target)
            .and_then(|e| e.template_contents)
            .expect("html5ever pediu o conteúdo de algo que não é <template>")
    }

    fn same_node(&self, x: &NodeId, y: &NodeId) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, mode: QuirksMode) {
        self.doc.borrow_mut().quirks_mode = mode == QuirksMode::Quirks;
    }

    fn append_before_sibling(&self, sibling: &NodeId, new_node: NodeOrText<NodeId>) {
        let mut doc = self.doc.borrow_mut();
        match new_node {
            NodeOrText::AppendNode(node) => doc.insert_before(*sibling, node),
            NodeOrText::AppendText(text) => doc.insert_text_before(*sibling, &text),
        }
    }

    fn add_attrs_if_missing(&self, target: &NodeId, attrs: Vec<Attribute>) {
        let mut doc = self.doc.borrow_mut();
        if let NodeData::Element(el) = &mut doc.node_mut(*target).data {
            for attr in attrs {
                if !el.attrs.iter().any(|a| a.name == attr.name) {
                    el.attrs.push(attr.into());
                }
            }
        }
    }

    fn remove_from_parent(&self, target: &NodeId) {
        self.doc.borrow_mut().detach(*target);
    }

    fn reparent_children(&self, node: &NodeId, new_parent: &NodeId) {
        let mut doc = self.doc.borrow_mut();
        let children: Vec<NodeId> = doc.children(*node).collect();
        for child in children {
            doc.append(*new_parent, child);
        }
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &NodeId) -> bool {
        self.mathml_integration_points.borrow().contains(handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nous_dom::local_name;

    fn element_names(doc: &Document, parent: NodeId) -> Vec<String> {
        doc.children(parent)
            .filter_map(|c| doc.element(c).map(|e| e.local_name().to_string()))
            .collect()
    }

    #[test]
    fn documento_simples() {
        let doc = parse_html("<!DOCTYPE html><html><head></head><body><p>Olá</p></body></html>");
        assert!(!doc.quirks_mode);
        let html = doc.document_element().expect("html");
        assert_eq!(element_names(&doc, html), vec!["head", "body"]);
        let body = doc.body().expect("body");
        assert_eq!(doc.text_content(body), "Olá");
        assert!(matches!(
            doc.node(doc.children(NodeId::DOCUMENT).next().unwrap())
                .data,
            NodeData::Doctype { .. }
        ));
    }

    #[test]
    fn titulo_e_modo_quirks() {
        let doc = parse_html("<title>  Meu   site </title><p>x");
        assert_eq!(doc.title().as_deref(), Some("Meu site"));
        // sem DOCTYPE o documento fica em modo quirks
        assert!(doc.quirks_mode);
    }

    #[test]
    fn aninhamento_e_atributos() {
        let doc =
            parse_html(r#"<div id="a" class="x y"><ul><li>1<li>2</ul><a href="/b">link</a></div>"#);
        let div = doc.find_element(&local_name!("div")).expect("div");
        let el = doc.element(div).unwrap();
        assert_eq!(el.id(), Some("a"));
        assert!(el.has_class("y"));
        assert_eq!(element_names(&doc, div), vec!["ul", "a"]);
        let ul = doc.find_element(&local_name!("ul")).unwrap();
        // <li> sem fechar: o parser fecha sozinho, então são dois irmãos
        assert_eq!(element_names(&doc, ul), vec!["li", "li"]);
        let a = doc.find_element(&local_name!("a")).unwrap();
        assert_eq!(doc.element(a).unwrap().attr("href"), Some("/b"));
    }

    #[test]
    fn textos_vizinhos_sao_mesclados() {
        let doc = parse_html("<p>um &amp; dois <!-- c --> três</p>");
        let p = doc.find_element(&local_name!("p")).unwrap();
        let kids: Vec<NodeId> = doc.children(p).collect();
        // texto, comentário, texto (o comentário separa os dois textos)
        assert_eq!(kids.len(), 3);
        assert_eq!(doc.node(kids[0]).as_text(), Some("um & dois "));
        let doc = parse_html("<p>a&lt;b&gt;c</p>");
        let p = doc.find_element(&local_name!("p")).unwrap();
        assert_eq!(doc.children(p).count(), 1);
        assert_eq!(doc.text_content(p), "a<b>c");
    }

    #[test]
    fn tabela_mal_formada_nao_quebra() {
        // texto solto dentro de <table> é "adotado" para antes da tabela (foster parenting)
        let doc = parse_html("<table>texto<tr><td>1<td>2</table><b>x<i>y</b>z</i>");
        let body = doc.body().unwrap();
        assert!(doc.text_content(body).contains("texto"));
        let table = doc.find_element(&local_name!("table")).unwrap();
        assert!(
            doc.descendants(table)
                .any(|n| doc.element(n).is_some_and(|e| e.is(&local_name!("td"))))
        );
        // o texto adotado fica antes da tabela, não dentro dela
        assert!(!doc.text_content(table).contains("texto"));
        assert_eq!(doc.text_content(body), "texto12xyz");
    }

    #[test]
    fn template_guarda_conteudo_separado() {
        let doc = parse_html("<template id=t><p>dentro</p></template><p>fora</p>");
        let t = doc.find_element(&local_name!("template")).unwrap();
        let contents = doc
            .element(t)
            .unwrap()
            .template_contents
            .expect("conteúdo do template");
        assert!(matches!(doc.node(contents).data, NodeData::Fragment));
        assert_eq!(doc.children(t).count(), 0);
        assert_eq!(doc.text_content(contents), "dentro");
        let body = doc.body().unwrap();
        assert_eq!(doc.text_content(body), "fora");
    }

    #[test]
    fn documento_pode_ir_para_outra_thread() {
        fn assert_send<T: Send>() {}
        assert_send::<nous_dom::Document>();
        let doc = parse_html("<p class=x>oi</p>");
        let doc = std::thread::spawn(move || doc).join().unwrap();
        let p = doc.find_element(&local_name!("p")).unwrap();
        assert_eq!(doc.element(p).unwrap().attr("class"), Some("x"));
    }

    #[test]
    fn noscript_depende_do_scripting() {
        let com = parse_html_with_scripting("<body><noscript><p>sem js</p></noscript>", true);
        let ns = com.find_element(&local_name!("noscript")).unwrap();
        assert!(com.children(ns).all(|c| com.element(c).is_none()));
        let sem = parse_html_with_scripting("<body><noscript><p>sem js</p></noscript>", false);
        let ns = sem.find_element(&local_name!("noscript")).unwrap();
        assert_eq!(element_names(&sem, ns), vec!["p"]);
    }
}
