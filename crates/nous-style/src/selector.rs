//! Seletores CSS: tipo, `*`, `#id`, `.classe`, `[atributo]`, algumas pseudo-classes
//! e os combinadores descendente, filho e irmãos. Seletores que ainda não entendemos
//! (`:hover`, `::before`…) nunca casam, o que é o comportamento seguro.

use nous_dom::{Document, LocalName, NodeId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttrOp {
    Exists,
    Equals(Box<str>),
    Includes(Box<str>),
    DashMatch(Box<str>),
    Prefix(Box<str>),
    Suffix(Box<str>),
    Substring(Box<str>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttrSel {
    pub name: LocalName,
    pub op: AttrOp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pseudo {
    FirstChild,
    LastChild,
    OnlyChild,
    Root,
    Link,
    Empty,
    Not(Box<Compound>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Compound {
    pub tag: Option<LocalName>,
    pub id: Option<Box<str>>,
    pub classes: Vec<Box<str>>,
    pub attrs: Vec<AttrSel>,
    pub pseudos: Vec<Pseudo>,
    /// Contém algo que não suportamos: nunca casa.
    pub never: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Combinator {
    Descendant,
    Child,
    NextSibling,
    SubsequentSibling,
}

/// Seletor complexo guardado da direita para a esquerda: `compounds[0]` é o alvo e
/// `combinators[i]` liga `compounds[i]` a `compounds[i + 1]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selector {
    pub compounds: Vec<Compound>,
    pub combinators: Vec<Combinator>,
}

impl Selector {
    pub fn rightmost(&self) -> &Compound {
        &self.compounds[0]
    }

    /// Especificidade empacotada em um `u32` (ids, classes, tipos), 8 bits cada.
    pub fn specificity(&self) -> u32 {
        let (mut a, mut b, mut c) = (0u32, 0u32, 0u32);
        for comp in &self.compounds {
            compound_specificity(comp, &mut a, &mut b, &mut c);
        }
        (a.min(255) << 16) | (b.min(255) << 8) | c.min(255)
    }
}

fn compound_specificity(comp: &Compound, a: &mut u32, b: &mut u32, c: &mut u32) {
    if comp.id.is_some() {
        *a += 1;
    }
    *b += (comp.classes.len() + comp.attrs.len()) as u32;
    for p in &comp.pseudos {
        match p {
            Pseudo::Not(inner) => compound_specificity(inner, a, b, c),
            _ => *b += 1,
        }
    }
    if comp.tag.is_some() {
        *c += 1;
    }
}

pub fn parse_selector_list(input: &str) -> Option<Vec<Selector>> {
    let mut out = Vec::new();
    for part in split_top_level(input, ',') {
        out.push(parse_selector(part.trim())?);
    }
    (!out.is_empty()).then_some(out)
}

fn split_top_level(input: &str, sep: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    let mut quote: Option<char> = None;
    for (i, ch) in input.char_indices() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(ch),
            (None, '(' | '[') => depth += 1,
            (None, ')' | ']') => depth -= 1,
            (None, c) if c == sep && depth == 0 => {
                parts.push(&input[start..i]);
                start = i + c.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(&input[start..]);
    parts
}

fn parse_selector(input: &str) -> Option<Selector> {
    if input.is_empty() {
        return None;
    }
    let mut compounds = Vec::new();
    let mut combinators = Vec::new();
    let mut chars = input.char_indices().peekable();
    let mut pending: Option<Combinator> = None;

    loop {
        // pula espaços e lê combinador explícito
        let mut saw_space = false;
        while let Some(&(_, c)) = chars.peek() {
            if c.is_whitespace() {
                saw_space = true;
                chars.next();
            } else {
                break;
            }
        }
        let Some(&(i, c)) = chars.peek() else { break };
        match c {
            '>' | '+' | '~' => {
                chars.next();
                pending = Some(match c {
                    '>' => Combinator::Child,
                    '+' => Combinator::NextSibling,
                    _ => Combinator::SubsequentSibling,
                });
                continue;
            }
            _ => {}
        }
        if !compounds.is_empty() {
            let comb = match pending.take() {
                Some(c) => c,
                None if saw_space => Combinator::Descendant,
                None => return None,
            };
            combinators.push(comb);
        } else if pending.is_some() {
            return None;
        }
        // lê o compound até espaço ou combinador (fora de colchetes/parênteses)
        let start = i;
        let mut end = input.len();
        let mut depth = 0;
        while let Some(&(j, c)) = chars.peek() {
            if depth == 0 && (c.is_whitespace() || matches!(c, '>' | '+' | '~')) {
                end = j;
                break;
            }
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => depth -= 1,
                _ => {}
            }
            chars.next();
        }
        compounds.push(parse_compound(&input[start..end])?);
    }
    if pending.is_some() || compounds.is_empty() {
        return None;
    }
    compounds.reverse();
    combinators.reverse();
    Some(Selector {
        compounds,
        combinators,
    })
}

fn is_ident_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_' || !c.is_ascii()
}

fn read_ident(s: &str) -> (&str, &str) {
    let end = s
        .char_indices()
        .find(|&(_, c)| !is_ident_char(c))
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    (&s[..end], &s[end..])
}

fn parse_compound(input: &str) -> Option<Compound> {
    let mut comp = Compound::default();
    let mut rest = input;
    if let Some(r) = rest.strip_prefix('*') {
        rest = r;
    } else if rest.starts_with(|c: char| c.is_alphabetic()) {
        let (name, r) = read_ident(rest);
        comp.tag = Some(LocalName::from(name.to_ascii_lowercase()));
        rest = r;
    }
    while !rest.is_empty() {
        let c = rest.chars().next()?;
        rest = &rest[c.len_utf8()..];
        match c {
            '#' => {
                let (id, r) = read_ident(rest);
                if id.is_empty() {
                    return None;
                }
                comp.id = Some(id.into());
                rest = r;
            }
            '.' => {
                let (class, r) = read_ident(rest);
                if class.is_empty() {
                    return None;
                }
                comp.classes.push(class.into());
                rest = r;
            }
            '[' => {
                let close = rest.find(']')?;
                comp.attrs.push(parse_attr(&rest[..close])?);
                rest = &rest[close + 1..];
            }
            ':' => {
                if let Some(r) = rest.strip_prefix(':') {
                    // pseudo-elemento (::before etc.): ainda não geramos caixas para eles
                    let (_, r) = read_ident(r);
                    rest = r;
                    comp.never = true;
                    continue;
                }
                let (name, r) = read_ident(rest);
                rest = r;
                let name = name.to_ascii_lowercase();
                let mut arg = None;
                if let Some(r) = rest.strip_prefix('(') {
                    let close = matching_paren(r)?;
                    arg = Some(&r[..close]);
                    rest = &r[close + 1..];
                }
                match (name.as_str(), arg) {
                    ("first-child", None) => comp.pseudos.push(Pseudo::FirstChild),
                    ("last-child", None) => comp.pseudos.push(Pseudo::LastChild),
                    ("only-child", None) => comp.pseudos.push(Pseudo::OnlyChild),
                    ("root", None) => comp.pseudos.push(Pseudo::Root),
                    ("link" | "any-link", None) => comp.pseudos.push(Pseudo::Link),
                    ("empty", None) => comp.pseudos.push(Pseudo::Empty),
                    // :visited nunca casa (privacidade, como nos navegadores modernos)
                    ("visited", None) => comp.never = true,
                    ("not", Some(arg)) => match parse_compound(arg.trim()) {
                        Some(inner) => comp.pseudos.push(Pseudo::Not(Box::new(inner))),
                        None => comp.never = true,
                    },
                    // estados dinâmicos (:hover, :focus…) e o resto: ainda não
                    _ => comp.never = true,
                }
            }
            _ => return None,
        }
    }
    Some(comp)
}

fn matching_paren(s: &str) -> Option<usize> {
    let mut depth = 0;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(i),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

fn parse_attr(inner: &str) -> Option<AttrSel> {
    let inner = inner.trim();
    let ops = ["~=", "|=", "^=", "$=", "*=", "="];
    for op in ops {
        if let Some(pos) = inner.find(op) {
            let name = inner[..pos].trim().to_ascii_lowercase();
            let mut value = inner[pos + op.len()..].trim();
            // ignora o flag " i" (case-insensitive) no fim
            if let Some(v) = value
                .strip_suffix(" i")
                .or_else(|| value.strip_suffix(" s"))
            {
                value = v.trim();
            }
            let value: Box<str> = value.trim_matches(['"', '\'']).into();
            let op = match op {
                "~=" => AttrOp::Includes(value),
                "|=" => AttrOp::DashMatch(value),
                "^=" => AttrOp::Prefix(value),
                "$=" => AttrOp::Suffix(value),
                "*=" => AttrOp::Substring(value),
                _ => AttrOp::Equals(value),
            };
            return Some(AttrSel {
                name: LocalName::from(name),
                op,
            });
        }
    }
    let (name, rest) = read_ident(inner);
    if name.is_empty() || !rest.trim().is_empty() {
        return None;
    }
    Some(AttrSel {
        name: LocalName::from(name.to_ascii_lowercase()),
        op: AttrOp::Exists,
    })
}

// ---------------------------------------------------------------- casamento

fn parent_element(doc: &Document, node: NodeId) -> Option<NodeId> {
    let p = doc.node(node).parent?;
    doc.element(p).map(|_| p)
}

fn prev_element(doc: &Document, node: NodeId) -> Option<NodeId> {
    let mut cur = doc.node(node).prev_sibling;
    while let Some(c) = cur {
        if doc.element(c).is_some() {
            return Some(c);
        }
        cur = doc.node(c).prev_sibling;
    }
    None
}

fn next_element(doc: &Document, node: NodeId) -> Option<NodeId> {
    let mut cur = doc.node(node).next_sibling;
    while let Some(c) = cur {
        if doc.element(c).is_some() {
            return Some(c);
        }
        cur = doc.node(c).next_sibling;
    }
    None
}

pub fn matches_compound(doc: &Document, node: NodeId, comp: &Compound) -> bool {
    if comp.never {
        return false;
    }
    let Some(el) = doc.element(node) else {
        return false;
    };
    if let Some(tag) = &comp.tag
        && el.local_name() != tag
    {
        return false;
    }
    if let Some(id) = &comp.id
        && el.id() != Some(&**id)
    {
        return false;
    }
    if !comp.classes.iter().all(|c| el.has_class(c)) {
        return false;
    }
    for a in &comp.attrs {
        let Some(v) = el.attr(&a.name) else {
            return false;
        };
        let ok = match &a.op {
            AttrOp::Exists => true,
            AttrOp::Equals(x) => v == &**x,
            AttrOp::Includes(x) => v.split_ascii_whitespace().any(|w| w == &**x),
            AttrOp::DashMatch(x) => v == &**x || v.starts_with(&format!("{x}-")),
            AttrOp::Prefix(x) => !x.is_empty() && v.starts_with(&**x),
            AttrOp::Suffix(x) => !x.is_empty() && v.ends_with(&**x),
            AttrOp::Substring(x) => !x.is_empty() && v.contains(&**x),
        };
        if !ok {
            return false;
        }
    }
    for p in &comp.pseudos {
        let ok = match p {
            Pseudo::FirstChild => prev_element(doc, node).is_none(),
            Pseudo::LastChild => next_element(doc, node).is_none(),
            Pseudo::OnlyChild => {
                prev_element(doc, node).is_none() && next_element(doc, node).is_none()
            }
            Pseudo::Root => parent_element(doc, node).is_none(),
            Pseudo::Link => matches!(&*el.name.local, "a" | "area") && el.attr("href").is_some(),
            Pseudo::Empty => doc.node(node).first_child.is_none(),
            Pseudo::Not(inner) => !matches_compound(doc, node, inner),
        };
        if !ok {
            return false;
        }
    }
    true
}

pub fn matches(doc: &Document, node: NodeId, sel: &Selector) -> bool {
    matches_from(doc, node, sel, 0)
}

fn matches_from(doc: &Document, node: NodeId, sel: &Selector, idx: usize) -> bool {
    if !matches_compound(doc, node, &sel.compounds[idx]) {
        return false;
    }
    let Some(&comb) = sel.combinators.get(idx) else {
        return true;
    };
    match comb {
        Combinator::Child => {
            parent_element(doc, node).is_some_and(|p| matches_from(doc, p, sel, idx + 1))
        }
        Combinator::Descendant => {
            let mut cur = parent_element(doc, node);
            while let Some(p) = cur {
                if matches_from(doc, p, sel, idx + 1) {
                    return true;
                }
                cur = parent_element(doc, p);
            }
            false
        }
        Combinator::NextSibling => {
            prev_element(doc, node).is_some_and(|p| matches_from(doc, p, sel, idx + 1))
        }
        Combinator::SubsequentSibling => {
            let mut cur = prev_element(doc, node);
            while let Some(p) = cur {
                if matches_from(doc, p, sel, idx + 1) {
                    return true;
                }
                cur = prev_element(doc, p);
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_e_especificidade() {
        let s = parse_selector_list("div > p.intro, #main a[href^=\"http\"]").unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].compounds.len(), 2);
        assert_eq!(s[0].combinators, vec![Combinator::Child]);
        assert_eq!(s[0].specificity(), (1 << 8) | 2);
        assert_eq!(s[1].specificity(), (1 << 16) | (1 << 8) | 1);
        assert!(parse_selector_list("a:hover").unwrap()[0].compounds[0].never);
        assert!(parse_selector_list("> a").is_none());
    }
}
