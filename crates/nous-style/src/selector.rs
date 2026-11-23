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
    FirstOfType,
    LastOfType,
    NthChild(i32, i32),
    Root,
    Link,
    Empty,
    Checked,
    Disabled,
    Not(Box<Compound>),
    NotList(Vec<Selector>),
    Is(Vec<Selector>),
    Where(Vec<Selector>),
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
            Pseudo::NotList(sels) | Pseudo::Is(sels) => {
                let best = sels.iter().map(Selector::specificity).max().unwrap_or(0);
                *a += (best >> 16) & 0xff;
                *b += (best >> 8) & 0xff;
                *c += best & 0xff;
            }
            Pseudo::Where(_) => {}
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
        // lê o compound até espaço ou combinador (fora de colchetes/parênteses/aspas)
        let start = i;
        let mut end = input.len();
        let mut depth = 0i32;
        let mut quote: Option<char> = None;
        while let Some(&(j, c)) = chars.peek() {
            if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
                chars.next();
                continue;
            }
            if depth == 0 && (c.is_whitespace() || matches!(c, '>' | '+' | '~')) {
                end = j;
                break;
            }
            match c {
                '"' | '\'' => quote = Some(c),
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
                let close = find_closing_bracket(rest)?;
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
                    ("first-of-type", None) => comp.pseudos.push(Pseudo::FirstOfType),
                    ("last-of-type", None) => comp.pseudos.push(Pseudo::LastOfType),
                    ("nth-child", Some(arg)) => match parse_nth(arg) {
                        Some((a, b)) => comp.pseudos.push(Pseudo::NthChild(a, b)),
                        None => comp.never = true,
                    },
                    ("root", None) => comp.pseudos.push(Pseudo::Root),
                    ("link" | "any-link", None) => comp.pseudos.push(Pseudo::Link),
                    ("empty", None) => comp.pseudos.push(Pseudo::Empty),
                    ("checked", None) => comp.pseudos.push(Pseudo::Checked),
                    ("disabled", None) => comp.pseudos.push(Pseudo::Disabled),
                    // :visited nunca casa (privacidade, como nos navegadores modernos)
                    ("visited", None) => comp.never = true,
                    ("not", Some(arg)) => {
                        let trimmed = arg.trim();
                        if !trimmed.contains([',', ' ', '>', '+', '~'])
                            && let Some(inner) = parse_compound(trimmed)
                        {
                            comp.pseudos.push(Pseudo::Not(Box::new(inner)));
                        } else if let Some(list) = parse_selector_list(trimmed) {
                            comp.pseudos.push(Pseudo::NotList(list));
                        } else {
                            comp.never = true;
                        }
                    }
                    ("is", Some(arg)) => match parse_selector_list(arg.trim()) {
                        Some(list) => comp.pseudos.push(Pseudo::Is(list)),
                        None => comp.never = true,
                    },
                    ("where", Some(arg)) => match parse_selector_list(arg.trim()) {
                        Some(list) => comp.pseudos.push(Pseudo::Where(list)),
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

fn find_closing_bracket(s: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, ']') => return Some(i),
            _ => {}
        }
    }
    None
}

fn parse_nth(raw: &str) -> Option<(i32, i32)> {
    let s: String = raw
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    if s == "odd" {
        return Some((2, 1));
    }
    if s == "even" {
        return Some((2, 0));
    }
    if let Some((a_part, b_part)) = s.split_once('n') {
        let a = match a_part {
            "" | "+" => 1,
            "-" => -1,
            _ => a_part.parse::<i32>().ok()?,
        };
        let b = if b_part.is_empty() {
            0
        } else {
            b_part.parse::<i32>().ok()?
        };
        Some((a, b))
    } else {
        Some((0, s.parse::<i32>().ok()?))
    }
}

fn matching_paren(s: &str) -> Option<usize> {
    let mut depth = 0;
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '(') => depth += 1,
            (None, ')') if depth == 0 => return Some(i),
            (None, ')') => depth -= 1,
            _ => {}
        }
    }
    None
}

fn parse_attr(inner: &str) -> Option<AttrSel> {
    let inner = inner.trim();
    let (name_raw, rest_after_name) = read_ident(inner);
    if name_raw.is_empty() {
        return None;
    }
    let name = LocalName::from(name_raw.to_ascii_lowercase());
    let rest = rest_after_name.trim_start();
    if rest.is_empty() {
        return Some(AttrSel {
            name,
            op: AttrOp::Exists,
        });
    }
    let ops = ["~=", "|=", "^=", "$=", "*=", "="];
    for op in ops {
        if let Some(after_op) = rest.strip_prefix(op) {
            let mut value = after_op.trim();
            // ignora o flag " i" / " s" (case-insensitive/sensitive) no fim
            if let Some(v) = value
                .strip_suffix(" i")
                .or_else(|| value.strip_suffix(" I"))
                .or_else(|| value.strip_suffix(" s"))
                .or_else(|| value.strip_suffix(" S"))
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
            return Some(AttrSel { name, op });
        }
    }
    None
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
            Pseudo::FirstOfType => {
                let mut cur = prev_element(doc, node);
                let mut found = false;
                while let Some(sib) = cur {
                    if doc.element(sib).is_some_and(|e| e.local_name() == el.local_name()) {
                        found = true;
                        break;
                    }
                    cur = prev_element(doc, sib);
                }
                !found
            }
            Pseudo::LastOfType => {
                let mut cur = next_element(doc, node);
                let mut found = false;
                while let Some(sib) = cur {
                    if doc.element(sib).is_some_and(|e| e.local_name() == el.local_name()) {
                        found = true;
                        break;
                    }
                    cur = next_element(doc, sib);
                }
                !found
            }
            Pseudo::NthChild(a, b) => {
                let mut idx = 1i32;
                let mut cur = prev_element(doc, node);
                while let Some(sib) = cur {
                    idx += 1;
                    cur = prev_element(doc, sib);
                }
                let diff = idx - *b;
                if *a == 0 {
                    diff == 0
                } else {
                    diff % *a == 0 && (diff / *a) >= 0
                }
            }
            Pseudo::Root => parent_element(doc, node).is_none(),
            Pseudo::Link => matches!(&*el.name.local, "a" | "area") && el.attr("href").is_some(),
            Pseudo::Empty => doc.node(node).first_child.is_none(),
            Pseudo::Checked => el.attr("checked").is_some(),
            Pseudo::Disabled => el.attr("disabled").is_some(),
            Pseudo::Not(inner) => !matches_compound(doc, node, inner),
            Pseudo::NotList(sels) => !sels.iter().any(|s| matches(doc, node, s)),
            Pseudo::Is(sels) | Pseudo::Where(sels) => sels.iter().any(|s| matches(doc, node, s)),
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

    #[test]
    fn atributos_combinadores_e_pseudoclasses() {
        use nous_dom::{Attribute, ElementData, NodeData, QualName, ns};

        fn el(doc: &mut Document, parent: NodeId, tag: &str, attrs: &[(&str, &str)]) -> NodeId {
            let id = doc.create(NodeData::Element(ElementData {
                name: QualName::new(None, ns!(html), LocalName::from(tag)),
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

        let mut doc = Document::new();
        let ul = el(&mut doc, NodeId::DOCUMENT, "ul", &[("data-url", "x~=y")]);
        let li1 = el(&mut doc, ul, "li", &[("class", "a"), ("data-k", "v1")]);
        let li2 = el(&mut doc, ul, "li", &[("class", "b"), ("data-k", "v2")]);
        let li3 = el(&mut doc, ul, "li", &[("class", "c")]);

        let match_sel = |node: NodeId, s: &str| -> bool {
            let list = parse_selector_list(s).unwrap();
            list.iter().any(|sel| matches(&doc, node, sel))
        };

        assert!(match_sel(ul, r#"[data-url="x~=y"]"#));
        assert!(match_sel(li1, "ul > li:first-child"));
        assert!(!match_sel(li2, "ul > li:first-child"));
        assert!(match_sel(li3, "ul > li:last-child"));
        assert!(match_sel(li2, "li.a + li"));
        assert!(!match_sel(li3, "li.a + li"));
        assert!(match_sel(li3, "li.a ~ li"));
        assert!(match_sel(li2, "li:not(.a, .c)"));
        assert!(!match_sel(li1, "li:not(.a, .c)"));
        assert!(match_sel(li2, "li[data-k]"));
        assert!(match_sel(li2, "li[data-k='v2']"));
        assert!(!match_sel(li3, "li[data-k]"));
    }
}
