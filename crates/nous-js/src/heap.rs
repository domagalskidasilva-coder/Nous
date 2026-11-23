//! Heap do JavaScript com coleta de lixo mark-sweep.
//!
//! Esqueleto: um vetor de células com lista de livres e células de upvalue para closures.

use std::collections::HashMap;
use std::mem::size_of;
use std::rc::Rc;

use crate::bytecode::{FunctionCode, Op};
use crate::host::HostFn;
use crate::value::{Kind, Value};
use crate::vm::NativeFn;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct GcRef(pub(crate) u32);

pub const PROP_WRITABLE: u8 = 1 << 0;
pub const PROP_ENUMERABLE: u8 = 1 << 1;
pub const PROP_CONFIGURABLE: u8 = 1 << 2;
pub const PROP_DEFAULT: u8 = PROP_WRITABLE | PROP_ENUMERABLE | PROP_CONFIGURABLE;
pub const PROP_METHOD: u8 = PROP_WRITABLE | PROP_CONFIGURABLE;

#[derive(Clone, Copy, Debug)]
pub struct Prop {
    pub key: GcRef,
    pub flags: u8,
    pub value: Value,
}

pub struct FunctionObj {
    pub code: Rc<FunctionCode>,
    pub upvalues: Rc<[GcRef]>,
    pub bound_this: Value,
    pub home_object: Option<GcRef>,
    pub super_ctor: Option<GcRef>,
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum MapKey {
    Undefined,
    Null,
    Bool(bool),
    Number(u64),
    String(Rc<str>),
    Object(GcRef),
}

impl MapKey {
    pub fn from_value(v: Value, heap: &Heap) -> Self {
        match v.kind() {
            Kind::Undefined => MapKey::Undefined,
            Kind::Null => MapKey::Null,
            Kind::Bool(b) => MapKey::Bool(b),
            Kind::Number(n) => {
                let bits = if n.is_nan() {
                    0x7FF8_0000_0000_0000
                } else if n == 0.0 {
                    0.0f64.to_bits()
                } else {
                    n.to_bits()
                };
                MapKey::Number(bits)
            }
            Kind::String(r) => MapKey::String(heap.string_rc(r)),
            Kind::Object(r) => MapKey::Object(r),
        }
    }
}

#[derive(Default)]
pub struct JsMap {
    pub entries: Vec<Option<(Value, Value)>>,
    pub index: HashMap<MapKey, usize>,
    pub len: usize,
}

impl JsMap {
    pub fn get(&self, key: &MapKey) -> Option<Value> {
        self.index
            .get(key)
            .and_then(|&i| self.entries[i].map(|(_, v)| v))
    }

    pub fn has(&self, key: &MapKey) -> bool {
        self.index.contains_key(key)
    }

    pub fn set(&mut self, key: MapKey, k_val: Value, v_val: Value) {
        if let Some(&idx) = self.index.get(&key) {
            self.entries[idx] = Some((k_val, v_val));
        } else {
            let idx = self.entries.len();
            self.entries.push(Some((k_val, v_val)));
            self.index.insert(key, idx);
            self.len += 1;
        }
    }

    pub fn delete(&mut self, key: &MapKey) -> bool {
        if let Some(idx) = self.index.remove(key) {
            self.entries[idx] = None;
            self.len -= 1;
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
        self.len = 0;
    }

    pub fn shrink_to_fit(&mut self, heap: &Heap) {
        if self.entries.len() > self.len {
            let mut new_entries = Vec::with_capacity(self.len);
            self.index.clear();
            for pair in self.entries.drain(..).flatten() {
                let k = MapKey::from_value(pair.0, heap);
                let idx = new_entries.len();
                new_entries.push(Some(pair));
                self.index.insert(k, idx);
            }
            self.entries = new_entries;
        }
        self.entries.shrink_to_fit();
        self.index.shrink_to_fit();
    }
}

#[derive(Default)]
pub struct JsSet {
    pub entries: Vec<Option<Value>>,
    pub index: HashMap<MapKey, usize>,
    pub len: usize,
}

impl JsSet {
    pub fn has(&self, key: &MapKey) -> bool {
        self.index.contains_key(key)
    }

    pub fn add(&mut self, key: MapKey, val: Value) {
        if !self.index.contains_key(&key) {
            let idx = self.entries.len();
            self.entries.push(Some(val));
            self.index.insert(key, idx);
            self.len += 1;
        }
    }

    pub fn delete(&mut self, key: &MapKey) -> bool {
        if let Some(idx) = self.index.remove(key) {
            self.entries[idx] = None;
            self.len -= 1;
            true
        } else {
            false
        }
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.index.clear();
        self.len = 0;
    }

    pub fn shrink_to_fit(&mut self, heap: &Heap) {
        if self.entries.len() > self.len {
            let mut new_entries = Vec::with_capacity(self.len);
            self.index.clear();
            for val in self.entries.drain(..).flatten() {
                let k = MapKey::from_value(val, heap);
                let idx = new_entries.len();
                new_entries.push(Some(val));
                self.index.insert(k, idx);
            }
            self.entries = new_entries;
        }
        self.entries.shrink_to_fit();
        self.index.shrink_to_fit();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapIterMode {
    Entries,
    Keys,
    Values,
}

pub enum IterKind {
    Empty,
    Array {
        arr: GcRef,
        idx: usize,
    },
    String {
        s: GcRef,
        byte_pos: usize,
    },
    Keys {
        keys: Vec<GcRef>,
        idx: usize,
    },
    Values {
        items: Vec<Value>,
        idx: usize,
    },
    Map {
        map: GcRef,
        idx: usize,
        mode: MapIterMode,
    },
    Set {
        set: GcRef,
        idx: usize,
        entries_mode: bool,
    },
    Custom {
        obj: Value,
        next_fn: Value,
    },
}

pub enum ObjectKind {
    Ordinary,
    Array(Vec<Value>),
    Function(FunctionObj),
    Native { func: NativeFn, constructor: bool },
    HostFunction { func: HostFn, constructor: bool },
    HostObject { tag: u32, id: u64 },
    Map(JsMap),
    Set(JsSet),
    Iterator(IterKind),
    Error,
}

pub struct Object {
    pub proto: Option<GcRef>,
    /// Propriedades próprias: chave (string internada), flags e valor.
    pub props: Vec<Prop>,
    pub frozen: bool,
    pub kind: ObjectKind,
}

impl Object {
    pub fn get_own(&self, key: GcRef) -> Option<Value> {
        self.props.iter().find(|p| p.key == key).map(|p| p.value)
    }

    pub fn get_own_prop(&self, key: GcRef) -> Option<&Prop> {
        self.props.iter().find(|p| p.key == key)
    }

    pub fn get_own_prop_mut(&mut self, key: GcRef) -> Option<&mut Prop> {
        self.props.iter_mut().find(|p| p.key == key)
    }

    /// Escrita de propriedade JS normal (respeita `writable` e `frozen`).
    pub fn set_own(&mut self, key: GcRef, value: Value) -> bool {
        if let Some(slot) = self.props.iter_mut().find(|p| p.key == key) {
            if self.frozen || (slot.flags & PROP_WRITABLE) == 0 {
                return false;
            }
            slot.value = value;
            return true;
        }
        if self.frozen {
            return false;
        }
        self.props.push(Prop {
            key,
            flags: PROP_DEFAULT,
            value,
        });
        true
    }

    /// Definição interna ou por descriptor com flags explícitas.
    pub fn define_own(&mut self, key: GcRef, value: Value, flags: u8) {
        match self.props.iter_mut().find(|p| p.key == key) {
            Some(slot) => {
                slot.value = value;
                slot.flags = flags;
            }
            None => self.props.push(Prop { key, flags, value }),
        }
    }

    pub fn delete_own(&mut self, key: GcRef) -> bool {
        let Some(pos) = self.props.iter().position(|p| p.key == key) else {
            return true;
        };
        if self.frozen || (self.props[pos].flags & PROP_CONFIGURABLE) == 0 {
            return false;
        }
        self.props.remove(pos);
        true
    }

    pub fn is_callable(&self) -> bool {
        matches!(
            self.kind,
            ObjectKind::Function(_)
                | ObjectKind::Native { .. }
                | ObjectKind::HostFunction { .. }
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub enum UpvalueState {
    Open(usize),
    Closed(Value),
}

pub enum Cell {
    String(Rc<str>),
    Object(Object),
    Upvalue(UpvalueState),
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
            frozen: false,
            kind,
        }))
    }

    pub fn alloc_upvalue(&mut self, abs_reg: usize) -> GcRef {
        self.alloc(Cell::Upvalue(UpvalueState::Open(abs_reg)))
    }

    pub fn upvalue(&self, r: GcRef) -> UpvalueState {
        match &self.cells[r.0 as usize] {
            Some(Cell::Upvalue(u)) => *u,
            _ => panic!("GcRef {r:?} não é upvalue"),
        }
    }

    pub fn close_upvalue(&mut self, r: GcRef, val: Value) {
        match &mut self.cells[r.0 as usize] {
            Some(Cell::Upvalue(state)) => *state = UpvalueState::Closed(val),
            _ => panic!("GcRef {r:?} não é upvalue"),
        }
    }

    pub fn set_closed_upvalue(&mut self, r: GcRef, val: Value) {
        match &mut self.cells[r.0 as usize] {
            Some(Cell::Upvalue(UpvalueState::Closed(slot))) => *slot = val,
            _ => panic!("GcRef {r:?} não é upvalue fechada"),
        }
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

    pub fn is_live(&self, r: GcRef) -> bool {
        self.cells.get(r.0 as usize).is_some_and(Option::is_some)
    }

    pub fn should_collect(&self) -> bool {
        self.allocated_since_gc >= self.gc_threshold
    }

    pub fn live_cells(&self) -> usize {
        self.cells.len() - self.free.len()
    }

    pub fn approx_bytes(&self) -> usize {
        let mut bytes = self.cells.capacity() * size_of::<Option<Cell>>()
            + self.marks.capacity() * size_of::<bool>()
            + self.free.capacity() * size_of::<u32>()
            + self.interned.capacity() * (size_of::<(Rc<str>, GcRef)>() + 16);

        for cell in self.cells.iter().flatten() {
            match cell {
                Cell::String(s) => bytes += s.len() + 16,
                Cell::Upvalue(_) => {}
                Cell::Object(o) => {
                    bytes += o.props.capacity() * size_of::<Prop>();
                    match &o.kind {
                        ObjectKind::Array(items) => {
                            bytes += items.capacity() * size_of::<Value>();
                        }
                        ObjectKind::Function(f) => {
                            bytes += f.upvalues.len() * size_of::<GcRef>();
                            bytes += approx_code_bytes(&f.code);
                        }
                        ObjectKind::Map(m) => {
                            bytes += m.entries.capacity() * size_of::<Option<(Value, Value)>>()
                                + m.index.capacity() * (size_of::<(MapKey, usize)>() + 16);
                        }
                        ObjectKind::Set(s) => {
                            bytes += s.entries.capacity() * size_of::<Option<Value>>()
                                + s.index.capacity() * (size_of::<(MapKey, usize)>() + 16);
                        }
                        ObjectKind::Iterator(it) => match it {
                            IterKind::Keys { keys, .. } => {
                                bytes += keys.capacity() * size_of::<GcRef>();
                            }
                            IterKind::Values { items, .. } => {
                                bytes += items.capacity() * size_of::<Value>();
                            }
                            _ => {}
                        },
                        _ => {}
                    }
                }
            }
        }
        bytes
    }

    /// Encolhe vetores internos, tabelas e células livres depois da coleta (ex.: aba dormindo).
    pub fn shrink_to_fit(&mut self) {
        while matches!(self.cells.last(), Some(None)) {
            self.cells.pop();
            self.marks.pop();
        }
        let len = self.cells.len() as u32;
        self.free.retain(|&i| i < len);
        self.cells.shrink_to_fit();
        self.marks.shrink_to_fit();
        self.free.shrink_to_fit();
        self.interned.shrink_to_fit();

        // Como shrink_to_fit de Map/Set lê strings do heap, separamos a verificação por índice.
        for i in 0..self.cells.len() {
            if let Some(Cell::Object(o)) = &mut self.cells[i] {
                o.props.shrink_to_fit();
                match &mut o.kind {
                    ObjectKind::Array(items) => items.shrink_to_fit(),
                    ObjectKind::Iterator(IterKind::Keys { keys, .. }) => keys.shrink_to_fit(),
                    ObjectKind::Iterator(IterKind::Values { items, .. }) => items.shrink_to_fit(),
                    _ => {}
                }
            }
        }
        // Compacta Maps e Sets que tinham entradas deletadas
        for i in 0..self.cells.len() {
            let needs_compact = matches!(
                &self.cells[i],
                Some(Cell::Object(Object {
                    kind: ObjectKind::Map(m),
                    ..
                })) if m.entries.capacity() > m.len
            ) || matches!(
                &self.cells[i],
                Some(Cell::Object(Object {
                    kind: ObjectKind::Set(s),
                    ..
                })) if s.entries.capacity() > s.len
            );
            if !needs_compact {
                continue;
            }
            if let Some(mut cell) = self.cells[i].take() {
                if let Cell::Object(ref mut o) = cell {
                    match &mut o.kind {
                        ObjectKind::Map(m) => m.shrink_to_fit(self),
                        ObjectKind::Set(s) => s.shrink_to_fit(self),
                        _ => {}
                    }
                }
                self.cells[i] = Some(cell);
            }
        }
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

        while let Some(r) = stack.pop() {
            let i = r.0 as usize;
            if self.marks[i] {
                continue;
            }
            self.marks[i] = true;
            match &self.cells[i] {
                Some(Cell::Object(o)) => {
                    if let Some(p) = o.proto {
                        stack.push(p);
                    }
                    for p in &o.props {
                        stack.push(p.key);
                        push_value(p.value, &mut stack);
                    }
                    match &o.kind {
                        ObjectKind::Array(items) => {
                            for v in items {
                                push_value(*v, &mut stack);
                            }
                        }
                        ObjectKind::Function(f) => {
                            mark_code(&f.code, &mut stack);
                            for &uv in f.upvalues.iter() {
                                stack.push(uv);
                            }
                            if f.code.is_arrow {
                                push_value(f.bound_this, &mut stack);
                            }
                            if let Some(h) = f.home_object {
                                stack.push(h);
                            }
                            if let Some(sc) = f.super_ctor {
                                stack.push(sc);
                            }
                        }
                        ObjectKind::Map(m) => {
                            for (k, v) in m.entries.iter().flatten() {
                                push_value(*k, &mut stack);
                                push_value(*v, &mut stack);
                            }
                        }
                        ObjectKind::Set(s) => {
                            for v in s.entries.iter().flatten() {
                                push_value(*v, &mut stack);
                            }
                        }
                        ObjectKind::Iterator(it) => match it {
                            IterKind::Empty => {}
                            IterKind::Array { arr, .. } => stack.push(*arr),
                            IterKind::String { s, .. } => stack.push(*s),
                            IterKind::Keys { keys, .. } => stack.extend(keys.iter().copied()),
                            IterKind::Values { items, .. } => {
                                for v in items {
                                    push_value(*v, &mut stack);
                                }
                            }
                            IterKind::Map { map, .. } => stack.push(*map),
                            IterKind::Set { set, .. } => stack.push(*set),
                            IterKind::Custom { obj, next_fn } => {
                                push_value(*obj, &mut stack);
                                push_value(*next_fn, &mut stack);
                            }
                        },
                        _ => {}
                    }
                }
                Some(Cell::Upvalue(UpvalueState::Closed(v))) => {
                    push_value(*v, &mut stack);
                }
                _ => {}
            }
        }

        self.interned.retain(|_, r| self.marks[r.0 as usize]);

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

fn approx_code_bytes(code: &FunctionCode) -> usize {
    let mut n = code.name.len()
        + code.code.capacity() * size_of::<Op>()
        + code.constants.capacity() * size_of::<Value>()
        + code.upvalues.capacity() * size_of::<crate::bytecode::UpvalueDesc>();
    for f in &code.functions {
        n += approx_code_bytes(f);
    }
    n
}
