//! DOM do Nous: todos os nós ficam num único `Vec` (arena), ligados por índices `u32`.
//!
//! Por que arena: cada nó custa um bloco contíguo de memória, sem `Rc`/`RefCell` por nó,
//! sem contagem de referência e com ótima localidade de cache. Nomes de tags e atributos
//! são átomos internados (`markup5ever`), então `div` existe uma vez só na memória.

pub use markup5ever::{LocalName, Namespace, QualName, local_name, ns};

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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u32);

impl NodeId {
    /// O nó raiz (o próprio documento) é sempre o primeiro da arena.
    pub const DOCUMENT: NodeId = NodeId(0);

    #[inline]
    pub fn index(self) -> usize {
        self.0 as usize
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
}

#[derive(Debug)]
pub enum NodeData {
    Document,
    Doctype {
        name: String,
    },
    Element(ElementData),
    Text(String),
    Comment(String),
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

    pub fn as_text(&self) -> Option<&str> {
        match &self.data {
            NodeData::Text(t) => Some(t),
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
        let id = NodeId(u32::try_from(self.nodes.len()).expect("documento grande demais"));
        self.nodes.push(Node::new(data));
        id
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

    /// Estimativa de memória usada pelo DOM (para `nous://memoria`).
    pub fn approx_heap_bytes(&self) -> usize {
        let mut total = self.nodes.capacity() * std::mem::size_of::<Node>();
        for n in &self.nodes {
            total += match &n.data {
                NodeData::Text(t) | NodeData::Comment(t) => t.capacity(),
                NodeData::Element(e) => {
                    e.attrs.capacity() * std::mem::size_of::<Attribute>()
                        + e.attrs.iter().map(|a| a.value.len()).sum::<usize>()
                }
                NodeData::Doctype { name } => name.capacity(),
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
}
