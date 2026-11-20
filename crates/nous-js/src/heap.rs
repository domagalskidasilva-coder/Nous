//! Heap do JavaScript com coleta de lixo mark-sweep.
//!
//! Esqueleto: um vetor de células com lista de livres. Próximos passos do plano:
//! berçário (geração jovem) com cópia, geração velha com mark-compact e coleta incremental.

use std::collections::HashMap;
use std::rc::Rc;

use crate::bytecode::FunctionCode;
use crate::value::{Kind, Value};
use crate::vm::NativeFn;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct GcRef(pub(crate) u32);

pub enum ObjectKind {
    Ordinary,
    Array(Vec<Value>),
    Function(Rc<FunctionCode>),
    Native { func: NativeFn, constructor: bool },
    Error,
}

pub struct Object {
    pub proto: Option<GcRef>,
    /// Propriedades próprias: chave (string internada) e valor. Vetor pequeno e linear;
    /// shapes/hidden classes com inline caches substituem isto no próximo passo.
    pub props: Vec<(GcRef, Value)>,
    pub kind: ObjectKind,
}

impl Object {
    pub fn get_own(&self, key: GcRef) -> Option<Value> {
        self.props.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
    }

    pub fn set_own(&mut self, key: GcRef, value: Value) {
        match self.props.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 = value,
            None => self.props.push((key, value)),
        }
    }

    pub fn is_callable(&self) -> bool {
        matches!(
            self.kind,
            ObjectKind::Function(_) | ObjectKind::Native { .. }
        )
    }
}

pub enum Cell {
    String(Rc<str>),
    Object(Object),
}

pub struct Heap {
    cells: Vec<Option<Cell>>,
    marks: Vec<bool>,
    free: Vec<u32>,
    interned: HashMap<Rc<str>, GcRef>,
    allocated_since_gc: usize,
    pub gc_threshold: usize,
    pub collections: u64,
}

impl Default for Heap {
    fn default() -> Self {
        Heap {
            cells: Vec::new(),
            marks: Vec::new(),
            free: Vec::new(),
            interned: HashMap::new(),
            allocated_since_gc: 0,
            gc_threshold: 10_000,
            collections: 0,
        }
    }
}

impl Heap {
    pub fn alloc(&mut self, cell: Cell) -> GcRef {
        self.allocated_since_gc += 1;
        if let Some(i) = self.free.pop() {
            self.cells[i as usize] = Some(cell);
            return GcRef(i);
        }
        let i = u32::try_from(self.cells.len()).expect("heap JS cheio");
        self.cells.push(Some(cell));
        self.marks.push(false);
        GcRef(i)
    }

    pub fn alloc_object(&mut self, proto: Option<GcRef>, kind: ObjectKind) -> GcRef {
        self.alloc(Cell::Object(Object {
            proto,
            props: Vec::new(),
            kind,
        }))
    }

    /// String internada: a mesma sequência de caracteres sempre vira a mesma referência.
    /// Usada para nomes de propriedades (comparação vira comparação de inteiros).
    pub fn intern(&mut self, s: &str) -> GcRef {
        if let Some(&r) = self.interned.get(s) {
            return r;
        }
        let rc: Rc<str> = Rc::from(s);
        let r = self.alloc(Cell::String(rc.clone()));
        self.interned.insert(rc, r);
        r
    }

    pub fn alloc_string(&mut self, s: &str) -> GcRef {
        self.alloc(Cell::String(Rc::from(s)))
    }

    pub fn string(&self, r: GcRef) -> &str {
        match &self.cells[r.0 as usize] {
            Some(Cell::String(s)) => s,
            _ => panic!("GcRef {r:?} não é string"),
        }
    }

    pub fn string_rc(&self, r: GcRef) -> Rc<str> {
        match &self.cells[r.0 as usize] {
            Some(Cell::String(s)) => s.clone(),
            _ => panic!("GcRef {r:?} não é string"),
        }
    }

    pub fn object(&self, r: GcRef) -> &Object {
        match &self.cells[r.0 as usize] {
            Some(Cell::Object(o)) => o,
            _ => panic!("GcRef {r:?} não é objeto"),
        }
    }

    pub fn object_mut(&mut self, r: GcRef) -> &mut Object {
        match &mut self.cells[r.0 as usize] {
            Some(Cell::Object(o)) => o,
            _ => panic!("GcRef {r:?} não é objeto"),
        }
    }

    pub fn should_collect(&self) -> bool {
        self.allocated_since_gc >= self.gc_threshold
    }

    pub fn live_cells(&self) -> usize {
        self.cells.len() - self.free.len()
    }

    /// Marca tudo que é alcançável a partir das raízes e libera o resto.
    pub fn collect(&mut self, roots: impl IntoIterator<Item = Value>) {
        let mut stack: Vec<GcRef> = Vec::new();
        let push_value = |v: Value, stack: &mut Vec<GcRef>| match v.kind() {
            Kind::String(r) | Kind::Object(r) => stack.push(r),
            _ => {}
        };
        for v in roots {
            push_value(v, &mut stack);
        }
        stack.extend(self.interned.values().copied());

        while let Some(r) = stack.pop() {
            let i = r.0 as usize;
            if self.marks[i] {
                continue;
            }
            self.marks[i] = true;
            if let Some(Cell::Object(o)) = &self.cells[i] {
                if let Some(p) = o.proto {
                    stack.push(p);
                }
                for (k, v) in &o.props {
                    stack.push(*k);
                    push_value(*v, &mut stack);
                }
                match &o.kind {
                    ObjectKind::Array(items) => {
                        for v in items {
                            push_value(*v, &mut stack);
                        }
                    }
                    ObjectKind::Function(code) => mark_code(code, &mut stack),
                    _ => {}
                }
            }
        }

        for i in 0..self.cells.len() {
            if self.marks[i] {
                self.marks[i] = false;
            } else if self.cells[i].is_some() {
                self.cells[i] = None;
                self.free.push(i as u32);
            }
        }
        self.allocated_since_gc = 0;
        self.collections += 1;
        // cresce o limite junto com o heap vivo, para não coletar a toda hora
        self.gc_threshold = (self.live_cells() * 2).max(10_000);
    }
}

fn mark_code(code: &FunctionCode, stack: &mut Vec<GcRef>) {
    for c in &code.constants {
        if let Kind::String(r) | Kind::Object(r) = c.kind() {
            stack.push(r);
        }
    }
    for f in &code.functions {
        mark_code(f, stack);
    }
}
