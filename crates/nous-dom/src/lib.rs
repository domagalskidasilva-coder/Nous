//! DOM do Nous: todos os nós ficam num único `Vec` (arena), ligados por índices `u32`.
//!
//! Por que arena: cada nó custa um bloco contíguo de memória, sem `Rc`/`RefCell` por nó,
//! sem contagem de referência e com ótima localidade de cache. Nomes de tags e atributos
//! são átomos internados (`markup5ever`), então `div` existe uma vez só na memória.

pub use markup5ever::{LocalName, Namespace, QualName, local_name, ns};
use std::num::NonZeroU32;

/// Atributo de elemento. O valor é um `Box<str>` (e não o `StrTendril` do html5ever, que
/// tem contagem de referência não atômica): assim o documento pode ser montado numa thread
/// de rede e entregue à thread da interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attribute {
    pub name: QualName,
    pub value: Box<str>,
}

impl From<markup5ever::Attribute> for Attribute {
    fn from(a: markup5ever::Attribute) -> Self {
        Attribute {
            name: a.name,
            value: Box::from(&*a.value),
        }
    }
}

/// Índice de um nó dentro do [`Document`].
///
/// Armazenado como [`NonZeroU32`] (`índice + 1`) para que `Option<NodeId>` ocupe
/// apenas 4 bytes via niche optimization.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(NonZeroU32);

impl NodeId {
    /// O nó raiz (o próprio documento) é sempre o primeiro da arena.
    pub const DOCUMENT: NodeId = NodeId(NonZeroU32::new(1).unwrap());

    #[inline]
    pub fn index(self) -> usize {
        self.0.get() as usize - 1
    }
}

#[derive(Debug)]
pub struct ElementData {
    pub name: QualName,
    pub attrs: Vec<Attribute>,
    /// Conteúdo de `<template>` (um fragmento separado), como manda a especificação.
    pub template_contents: Option<NodeId>,
}

impl ElementData {
    pub fn local_name(&self) -> &LocalName {
        &self.name.local
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|a| &*a.name.local == name)
            .map(|a| &*a.value)
    }

    pub fn id(&self) -> Option<&str> {
        self.attr("id")
    }

    pub fn has_class(&self, class: &str) -> bool {
        self.attr("class")
            .is_some_and(|c| c.split_ascii_whitespace().any(|c| c == class))
    }

    pub fn is(&self, local: &LocalName) -> bool {
        self.name.ns == ns!(html) && &self.name.local == local
    }

    /// Define ou atualiza o valor de um atributo (em `ns!()`).
    pub fn set_attr(&mut self, name: &str, value: &str) {
        if let Some(existing) = self.attrs.iter_mut().find(|a| &*a.name.local == name) {
            existing.value = Box::from(value);
        } else {
            self.attrs.push(Attribute {
                name: QualName::new(None, ns!(), LocalName::from(name)),
                value: Box::from(value),
            });
        }
    }

    /// Remove todos os atributos com esse nome local; retorna `true` se algo foi removido.
    pub fn remove_attr(&mut self, name: &str) -> bool {
        let before = self.attrs.len();
        self.attrs.retain(|a| &*a.name.local != name);
        self.attrs.len() != before
    }
}

#[derive(Debug)]
pub struct DoctypeData {
    pub name: Box<str>,
}

#[derive(Debug)]
pub enum NodeData {
    Document,
    Doctype(Box<DoctypeData>),
    Element(ElementData),
    Text(String),
    Comment(Box<str>),
    /// Fragmento sem pai (usado por `<template>`).
    Fragment,
}

#[derive(Debug)]
pub struct Node {
    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub last_child: Option<NodeId>,
    pub prev_sibling: Option<NodeId>,
    pub next_sibling: Option<NodeId>,
    pub data: NodeData,
}

impl Node {
    fn new(data: NodeData) -> Self {
        Node {
            parent: None,
            first_child: None,
            last_child: None,
            prev_sibling: None,
            next_sibling: None,
            data,
        }
    }

    pub fn as_element(&self) -> Option<&ElementData> {
        match &self.data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn as_element_mut(&mut self) -> Option<&mut ElementData> {
        match &mut self.data {
            NodeData::Element(e) => Some(e),
            _ => None,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match &self.data {
            NodeData::Text(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_comment(&self) -> Option<&str> {
        match &self.data {
            NodeData::Comment(c) => Some(c),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct Document {
    nodes: Vec<Node>,
    pub quirks_mode: bool,
}

impl Default for Document {
    fn default() -> Self {
        Self::new()
    }
}

impl Document {
    pub fn new() -> Self {
        Document {
            nodes: vec![Node::new(NodeData::Document)],
            quirks_mode: false,
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len() <= 1
    }

    pub fn create(&mut self, data: NodeData) -> NodeId {
        let raw = self
            .nodes
            .len()
            .checked_add(1)
            .and_then(|n| u32::try_from(n).ok())
            .and_then(NonZeroU32::new)
            .expect("documento grande demais");
        let id = NodeId(raw);
        self.nodes.push(Node::new(data));
        id
    }

    /// Cria um novo nó de elemento HTML (`tag` normalizada para minúsculas ASCII).
    pub fn create_element(&mut self, tag: &str) -> NodeId {
        let local = if tag.bytes().any(|b| b.is_ascii_uppercase()) {
            LocalName::from(tag.to_ascii_lowercase())
        } else {
            LocalName::from(tag)
        };
        self.create(NodeData::Element(ElementData {
            name: QualName::new(None, ns!(html), local),
            attrs: Vec::new(),
            template_contents: None,
        }))
    }

    #[inline]
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.index()]
    }

    #[inline]
    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.index()]
    }

    pub fn element(&self, id: NodeId) -> Option<&ElementData> {
        self.node(id).as_element()
    }

    pub fn element_mut(&mut self, id: NodeId) -> Option<&mut ElementData> {
        self.node_mut(id).as_element_mut()
    }

    /// Remove `child` de onde estiver (o nó continua na arena, só fica solto).
    pub fn detach(&mut self, child: NodeId) {
        let (parent, prev, next) = {
            let n = self.node(child);
            (n.parent, n.prev_sibling, n.next_sibling)
        };
        let Some(parent) = parent else { return };
        match prev {
            Some(p) => self.node_mut(p).next_sibling = next,
            None => self.node_mut(parent).first_child = next,
        }
        match next {
            Some(n) => self.node_mut(n).prev_sibling = prev,
            None => self.node_mut(parent).last_child = prev,
        }
        let n = self.node_mut(child);
        n.parent = None;
        n.prev_sibling = None;
        n.next_sibling = None;
    }

    pub fn append(&mut self, parent: NodeId, child: NodeId) {
        self.detach(child);
        let last = self.node(parent).last_child;
        {
            let c = self.node_mut(child);
            c.parent = Some(parent);
            c.prev_sibling = last;
        }
        match last {
            Some(l) => self.node_mut(l).next_sibling = Some(child),
            None => self.node_mut(parent).first_child = Some(child),
        }
        self.node_mut(parent).last_child = Some(child);
    }

    pub fn insert_before(&mut self, sibling: NodeId, child: NodeId) {
        self.detach(child);
        let (parent, prev) = {
            let s = self.node(sibling);
            (s.parent.expect("irmão sem pai"), s.prev_sibling)
        };
        {
            let c = self.node_mut(child);
            c.parent = Some(parent);
            c.prev_sibling = prev;
            c.next_sibling = Some(sibling);
        }
        self.node_mut(sibling).prev_sibling = Some(child);
        match prev {
            Some(p) => self.node_mut(p).next_sibling = Some(child),
            None => self.node_mut(parent).first_child = Some(child),
        }
    }

    /// Junta texto a um nó de texto vizinho em vez de criar outro (como o parser HTML pede).
    pub fn append_text(&mut self, parent: NodeId, text: &str) {
        if let Some(last) = self.node(parent).last_child
            && let NodeData::Text(t) = &mut self.node_mut(last).data
        {
            t.push_str(text);
            return;
        }
        let t = self.create(NodeData::Text(text.to_owned()));
        self.append(parent, t);
    }

    pub fn insert_text_before(&mut self, sibling: NodeId, text: &str) {
        if let Some(prev) = self.node(sibling).prev_sibling
            && let NodeData::Text(t) = &mut self.node_mut(prev).data
        {
            t.push_str(text);
            return;
        }
        let t = self.create(NodeData::Text(text.to_owned()));
        self.insert_before(sibling, t);
    }

    pub fn children(&self, id: NodeId) -> Children<'_> {
        Children {
            doc: self,
            next: self.node(id).first_child,
        }
    }

    /// Percorre a subárvore em pré-ordem (o próprio `root` incluído).
    pub fn descendants(&self, root: NodeId) -> Descendants<'_> {
        Descendants {
            doc: self,
            root,
            next: Some(root),
        }
    }

    pub fn document_element(&self) -> Option<NodeId> {
        self.children(NodeId::DOCUMENT)
            .find(|&c| self.node(c).as_element().is_some())
    }

    pub fn find_element(&self, local: &LocalName) -> Option<NodeId> {
        self.descendants(NodeId::DOCUMENT)
            .find(|&n| self.element(n).is_some_and(|e| e.is(local)))
    }

    pub fn body(&self) -> Option<NodeId> {
        self.find_element(&local_name!("body"))
    }

    pub fn text_content(&self, id: NodeId) -> String {
        let mut out = String::new();
        for n in self.descendants(id) {
            if let Some(t) = self.node(n).as_text() {
                out.push_str(t);
            }
        }
        out
    }

    /// Substitui o conteúdo textual do nó: em `Text`/`Comment` atualiza o texto; em
    /// `Element`/`Fragment` remove os filhos e cria (ou reaproveita) um único nó de texto.
    pub fn set_text_content(&mut self, id: NodeId, text: &str) {
        match &mut self.node_mut(id).data {
            NodeData::Text(t) => {
                t.clear();
                t.push_str(text);
                return;
            }
            NodeData::Comment(c) => {
                *c = Box::from(text);
                return;
            }
            NodeData::Document | NodeData::Doctype(_) => return,
            NodeData::Element(_) | NodeData::Fragment => {}
        }

        // Reaproveita um único filho de texto existente para evitar crescer a arena em atualizações repetidas.
        let (first, last) = {
            let n = self.node(id);
            (n.first_child, n.last_child)
        };
        if let (Some(f), Some(l)) = (first, last)
            && f == l
            && let NodeData::Text(t) = &mut self.node_mut(f).data
        {
            if text.is_empty() {
                self.detach(f);
            } else {
                t.clear();
                t.push_str(text);
            }
            return;
        }

        while let Some(child) = self.node(id).first_child {
            self.detach(child);
        }
        if !text.is_empty() {
            self.append_text(id, text);
        }
    }

    pub fn get_element_by_id(&self, id: &str) -> Option<NodeId> {
        if id.is_empty() {
            return None;
        }
        self.descendants(NodeId::DOCUMENT)
            .find(|&n| self.element(n).is_some_and(|e| e.id() == Some(id)))
    }

    pub fn elements_by_tag(&self, tag: &str) -> Vec<NodeId> {
        let all = tag == "*";
        self.descendants(NodeId::DOCUMENT)
            .filter(|&n| {
                self.element(n)
                    .is_some_and(|e| all || str::eq_ignore_ascii_case(e.local_name(), tag))
            })
            .collect()
    }

    pub fn elements_by_class(&self, class: &str) -> Vec<NodeId> {
        let classes: Vec<&str> = class.split_ascii_whitespace().collect();
        if classes.is_empty() {
            return Vec::new();
        }
        self.descendants(NodeId::DOCUMENT)
            .filter(|&n| {
                self.element(n)
                    .is_some_and(|e| classes.iter().all(|c| e.has_class(c)))
            })
            .collect()
    }

    pub fn title(&self) -> Option<String> {
        let t = self.find_element(&local_name!("title"))?;
        let text = self.text_content(t);
        let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
        (!collapsed.is_empty()).then_some(collapsed)
    }

    /// Valor de `<base href>` se existir.
    pub fn base_href(&self) -> Option<&str> {
        let b = self.find_element(&local_name!("base"))?;
        self.element(b)?.attr("href")
    }

    /// Libera a capacidade sobressalente da arena, atributos e nós de texto.
    pub fn compact(&mut self) {
        self.nodes.shrink_to_fit();
        for node in &mut self.nodes {
            match &mut node.data {
                NodeData::Element(el) => {
                    el.attrs.shrink_to_fit();
                }
                NodeData::Text(t) => {
                    t.shrink_to_fit();
                }
                _ => {}
            }
        }
    }

    /// Estimativa de memória usada pelo DOM (para `nous://memoria`).
    pub fn approx_heap_bytes(&self) -> usize {
        let mut total = self.nodes.capacity() * std::mem::size_of::<Node>();
        for n in &self.nodes {
            total += match &n.data {
                NodeData::Text(t) => t.capacity(),
                NodeData::Comment(t) => t.len(),
                NodeData::Element(e) => {
                    e.attrs.capacity() * std::mem::size_of::<Attribute>()
                        + e.attrs.iter().map(|a| a.value.len()).sum::<usize>()
                }
                NodeData::Doctype(d) => std::mem::size_of::<DoctypeData>() + d.name.len(),
                _ => 0,
            };
        }
        total
    }
}

pub struct Children<'a> {
    doc: &'a Document,
    next: Option<NodeId>,
}

impl Iterator for Children<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let cur = self.next?;
        self.next = self.doc.node(cur).next_sibling;
        Some(cur)
    }
}

pub struct Descendants<'a> {
    doc: &'a Document,
    root: NodeId,
    next: Option<NodeId>,
}

impl Iterator for Descendants<'_> {
    type Item = NodeId;
    fn next(&mut self) -> Option<NodeId> {
        let cur = self.next?;
        let node = self.doc.node(cur);
        self.next = if let Some(c) = node.first_child {
            Some(c)
        } else {
            let mut n = cur;
            loop {
                if n == self.root {
                    break None;
                }
                let nn = self.doc.node(n);
                if let Some(s) = nn.next_sibling {
                    break Some(s);
                }
                match nn.parent {
                    Some(p) => n = p,
                    None => break None,
                }
            }
        };
        Some(cur)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(doc: &mut Document, name: &str) -> NodeId {
        doc.create(NodeData::Element(ElementData {
            name: QualName::new(None, ns!(html), LocalName::from(name)),
            attrs: vec![],
            template_contents: None,
        }))
    }

    #[test]
    fn append_insert_detach() {
        let mut d = Document::new();
        let html = el(&mut d, "html");
        d.append(NodeId::DOCUMENT, html);
        let a = el(&mut d, "a");
        let b = el(&mut d, "b");
        d.append(html, b);
        d.insert_before(b, a);
        assert_eq!(d.children(html).collect::<Vec<_>>(), vec![a, b]);
        d.detach(a);
        assert_eq!(d.children(html).collect::<Vec<_>>(), vec![b]);
        d.append_text(html, "oi ");
        d.append_text(html, "mundo");
        assert_eq!(d.text_content(html), "oi mundo");
        assert_eq!(d.descendants(NodeId::DOCUMENT).count(), 4);
    }

    #[test]
    fn node_id_niche_and_compact_sizes() {
        assert_eq!(std::mem::size_of::<NodeId>(), 4);
        assert_eq!(std::mem::size_of::<Option<NodeId>>(), 4);
        assert_eq!(NodeId::DOCUMENT.index(), 0);
        assert!(std::mem::size_of::<NodeData>() <= 56);
        assert!(std::mem::size_of::<Node>() <= 80);

        let mut d = Document::new();
        let first = d.create_element("div");
        assert_eq!(first.index(), 1);
        assert_eq!(d.len(), 2);
    }

    #[test]
    fn compact_shrinks_slack_capacity() {
        let mut d = Document::new();
        let div = d.create_element("div");
        d.append(NodeId::DOCUMENT, div);
        for i in 0..40 {
            d.element_mut(div)
                .unwrap()
                .set_attr(&format!("data-k{i}"), "valor");
        }
        for i in 10..40 {
            assert!(d.element_mut(div).unwrap().remove_attr(&format!("data-k{i}")));
        }
        assert!(!d.element_mut(div).unwrap().remove_attr("inexistente"));
        let txt = d.create(NodeData::Text(String::with_capacity(2048)));
        d.append(div, txt);
        d.set_text_content(txt, "curto");

        let before = d.approx_heap_bytes();
        d.compact();
        let after = d.approx_heap_bytes();
        assert!(after < before);
        assert_eq!(d.node(txt).as_text(), Some("curto"));
        assert_eq!(d.element(div).unwrap().attrs.len(), 10);
    }

    #[test]
    fn dom_query_and_mutation_helpers() {
        let mut d = Document::new();
        let html = d.create_element("HTML");
        let body = d.create_element("body");
        let main = d.create_element("main");
        let p1 = d.create_element("p");
        let p2 = d.create_element("p");

        d.append(NodeId::DOCUMENT, html);
        d.append(html, body);
        d.append(body, main);
        d.append(main, p1);
        d.append(main, p2);

        d.element_mut(main).unwrap().set_attr("id", "principal");
        d.element_mut(p1).unwrap().set_attr("class", "card destaque");
        d.element_mut(p2).unwrap().set_attr("class", "card");
        d.element_mut(p1).unwrap().set_attr("data-v", "1");
        d.element_mut(p1).unwrap().set_attr("data-v", "2");
        assert_eq!(d.element(p1).unwrap().attr("data-v"), Some("2"));

        assert_eq!(d.get_element_by_id("principal"), Some(main));
        assert_eq!(d.get_element_by_id(""), None);
        assert_eq!(d.get_element_by_id("ausente"), None);

        assert_eq!(d.elements_by_tag("P"), vec![p1, p2]);
        assert_eq!(d.elements_by_tag("p"), vec![p1, p2]);
        assert_eq!(d.elements_by_tag("*"), vec![html, body, main, p1, p2]);

        assert_eq!(d.elements_by_class("card"), vec![p1, p2]);
        assert_eq!(d.elements_by_class("card destaque"), vec![p1]);
        assert_eq!(d.elements_by_class("  "), Vec::<NodeId>::new());

        d.set_text_content(p1, "primeiro");
        assert_eq!(d.text_content(p1), "primeiro");
        // Atualização repetida reaproveita o único nó de texto
        let nodes_before = d.len();
        d.set_text_content(p1, "segundo");
        assert_eq!(d.text_content(p1), "segundo");
        assert_eq!(d.len(), nodes_before);

        d.set_text_content(main, "limpo");
        assert_eq!(d.text_content(main), "limpo");
        assert_eq!(d.children(main).count(), 1);
        assert_eq!(d.get_element_by_id("principal"), Some(main));

        d.set_text_content(main, "");
        assert_eq!(d.text_content(main), "");
        assert_eq!(d.children(main).count(), 0);
    }
}
