//! Árvore de caixas: decide o que é bloco, o que é conteúdo inline e onde nascem
//! blocos anônimos (quando um bloco tem filhos de bloco e de linha misturados).

use std::sync::Arc;

use nous_dom::{Document, NodeData, NodeId, local_name};
use nous_style::{ComputedStyle, Display, StyleMap};

pub(crate) struct BlockBox {
    /// `None` = bloco anônimo (só agrupa conteúdo inline).
    pub node: Option<NodeId>,
    pub style: Arc<ComputedStyle>,
    pub content: BlockContent,
}

pub(crate) enum BlockContent {
    Blocks(Vec<BlockBox>),
    /// Nós de nível de linha (texto e elementos inline), na ordem do documento.
    Inline(Vec<NodeId>),
    Empty,
}

impl BlockBox {
    pub fn is_anonymous(&self) -> bool {
        self.node.is_none()
    }
}

pub(crate) struct BoxBuilder<'a> {
    pub doc: &'a Document,
    pub styles: &'a StyleMap,
}

impl<'a> BoxBuilder<'a> {
    pub fn style_of(&self, node: NodeId) -> Option<&'a Arc<ComputedStyle>> {
        self.styles.get(node)
    }

    fn is_hidden(&self, node: NodeId) -> bool {
        match &self.doc.node(node).data {
            NodeData::Element(_) => self
                .style_of(node)
                .is_none_or(|s| s.display == Display::None),
            NodeData::Text(_) => false,
            _ => true,
        }
    }

    /// Elemento inline que contém um bloco (ex.: `<a><div>…</div></a>`): tratamos o
    /// próprio elemento como bloco, uma simplificação do "block-in-inline" da especificação.
    fn contains_block(&self, node: NodeId) -> bool {
        self.doc.children(node).any(|c| {
            if self.is_hidden(c) || self.doc.element(c).is_none() {
                return false;
            }
            let s = self.style_of(c).expect("elemento sem estilo");
            s.is_block_level() || (s.display == Display::Inline && self.contains_block(c))
        })
    }

    fn is_block_level(&self, node: NodeId) -> bool {
        let Some(el) = self.doc.element(node) else {
            return false;
        };
        if el.is(&local_name!("br")) {
            return false;
        }
        let s = self.style_of(node).expect("elemento sem estilo");
        s.is_block_level() || (s.display == Display::Inline && self.contains_block(node))
    }

    /// Texto que some por inteiro com o colapso de espaços.
    fn is_collapsible_whitespace(&self, node: NodeId) -> bool {
        match &self.doc.node(node).data {
            NodeData::Text(t) => {
                let ws = self
                    .style_of(node)
                    .map(|s| s.white_space.collapses_spaces() && !s.white_space.keeps_newlines())
                    .unwrap_or(true);
                ws && t.chars().all(|c| c.is_ascii_whitespace())
            }
            _ => false,
        }
    }

    pub fn build_block(&self, node: NodeId) -> BlockBox {
        let style = self.style_of(node).expect("bloco sem estilo").clone();
        let children: Vec<NodeId> = self
            .doc
            .children(node)
            .filter(|&c| !self.is_hidden(c))
            .collect();
        let content = self.build_content(&style, &children);
        BlockBox {
            node: Some(node),
            style,
            content,
        }
    }

    fn build_content(
        &self,
        parent_style: &Arc<ComputedStyle>,
        children: &[NodeId],
    ) -> BlockContent {
        if children.is_empty() {
            return BlockContent::Empty;
        }
        if !children.iter().any(|&c| self.is_block_level(c)) {
            if children.iter().all(|&c| self.is_collapsible_whitespace(c)) {
                return BlockContent::Empty;
            }
            return BlockContent::Inline(children.to_vec());
        }
        let mut blocks = Vec::new();
        let mut run: Vec<NodeId> = Vec::new();
        let flush = |run: &mut Vec<NodeId>, blocks: &mut Vec<BlockBox>| {
            if run.iter().all(|&c| self.is_collapsible_whitespace(c)) {
                run.clear();
                return;
            }
            blocks.push(BlockBox {
                node: None,
                style: Arc::new(ComputedStyle::inherit_from(parent_style)),
                content: BlockContent::Inline(std::mem::take(run)),
            });
        };
        for &c in children {
            if self.is_block_level(c) {
                flush(&mut run, &mut blocks);
                blocks.push(self.build_block(c));
            } else {
                run.push(c);
            }
        }
        flush(&mut run, &mut blocks);
        if blocks.is_empty() {
            BlockContent::Empty
        } else {
            BlockContent::Blocks(blocks)
        }
    }
}
