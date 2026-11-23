//! Ponte Host/DOM e contabilidade de memória para embutir o `nous-js`.
//!
//! Mantém o motor desacoplado de `nous-dom`: o `nous-engine` cria objetos host opacos
//! com `(tag, id)` e registra funções globais/métodos via [`HostFn`].

use std::mem::size_of;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::heap::{ObjectKind, PROP_METHOD};
use crate::value::{Kind, Value};
use crate::vm::{Frame, JsError, Vm};

/// Limite máximo de timers pendentes por VM (proteção contra exaustão de memória).
pub const MAX_TIMERS: usize = 1024;

/// Callback registrada pelo host (ex.: `nous-engine` / DOM).
pub type HostFn = Rc<dyn Fn(&mut Vm, Value, &[Value]) -> Result<Value, JsError>>;

pub(crate) struct TimerEntry {
    pub id: u64,
    pub due_at: Instant,
    pub callback: Value,
    pub args: Vec<Value>,
}

pub(crate) fn install_timers(vm: &mut Vm) {
    let k_set = vm.heap.intern("setTimeout");
    let f_set = vm.alloc_native_fn("setTimeout", native_set_timeout, 1, false);
    vm.heap
        .object_mut(vm.global)
        .define_own(k_set, Value::object(f_set), PROP_METHOD);

    let k_clear = vm.heap.intern("clearTimeout");
    let f_clear = vm.alloc_native_fn("clearTimeout", native_clear_timeout, 1, false);
    vm.heap
        .object_mut(vm.global)
        .define_own(k_clear, Value::object(f_clear), PROP_METHOD);
}

fn native_set_timeout(vm: &mut Vm, _this: Value, args: &[Value]) -> Result<Value, Value> {
    let cb = args.first().copied().unwrap_or(Value::UNDEFINED);
    if !vm.is_callable(cb) {
        return Err(vm.type_error("setTimeout callback must be a function"));
    }
    if vm.timers.len() >= MAX_TIMERS {
        return Err(vm.range_error("Timer queue limit exceeded"));
    }
    let delay_ms = match args.get(1).copied() {
        Some(v) if !v.is_undefined() => {
            let n = vm.to_number(v)?;
            if n.is_finite() && n > 0.0 {
                n.min(u32::MAX as f64) as u64
            } else {
                0
            }
        }
        _ => 0,
    };
    let extra_args = if args.len() > 2 {
        args[2..].to_vec()
    } else {
        Vec::new()
    };
    let id = vm.next_timer_id;
    vm.next_timer_id = vm.next_timer_id.wrapping_add(1).max(1);
    let due_at = Instant::now() + Duration::from_millis(delay_ms);
    vm.timers.push(TimerEntry {
        id,
        due_at,
        callback: cb,
        args: extra_args,
    });
    Ok(Value::number(id as f64))
}

fn native_clear_timeout(vm: &mut Vm, _this: Value, args: &[Value]) -> Result<Value, Value> {
    let arg = args.first().copied().unwrap_or(Value::UNDEFINED);
    if arg.is_nullish() {
        return Ok(Value::UNDEFINED);
    }
    let n = vm.to_number(arg)?;
    if n.is_finite() && n >= 1.0 && n <= u64::MAX as f64 {
        let id = n as u64;
        if let Some(pos) = vm.timers.iter().position(|t| t.id == id) {
            vm.timers.swap_remove(pos);
        }
    }
    Ok(Value::UNDEFINED)
}

struct StepLimitGuard<'a> {
    vm: &'a mut Vm,
    prev: Option<u64>,
}

impl Drop for StepLimitGuard<'_> {
    fn drop(&mut self) {
        self.vm.step_limit = self.prev;
    }
}

impl Vm {
    // --------------------------------------------------------- objetos e funções host

    /// Aloca uma função de host (`ObjectKind::HostFunction`) sem anexá-la a nenhum objeto.
    pub fn new_host_fn(&mut self, name: &str, f: HostFn) -> Value {
        let fn_ref = self.heap.alloc_object(
            Some(self.protos.function),
            ObjectKind::HostFunction {
                func: f,
                constructor: false,
            },
        );
        let n = self.heap.intern(name);
        let (k_name, k_len) = (self.names.name, self.names.length);
        let obj = self.heap.object_mut(fn_ref);
        obj.define_own(k_name, Value::string(n), PROP_METHOD);
        obj.define_own(k_len, Value::number(0.0), PROP_METHOD);
        Value::object(fn_ref)
    }

    /// Aloca um construtor de host (`new HostCtor(...)`) com protótipo opcional.
    pub fn new_host_constructor(&mut self, name: &str, f: HostFn, proto: Option<Value>) -> Value {
        let fn_ref = self.heap.alloc_object(
            Some(self.protos.function),
            ObjectKind::HostFunction {
                func: f,
                constructor: true,
            },
        );
        let proto_ref = proto.and_then(|v| v.as_object()).unwrap_or_else(|| {
            self.heap
                .alloc_object(Some(self.protos.object), ObjectKind::Ordinary)
        });
        let n = self.heap.intern(name);
        let (k_name, k_len, k_proto, k_ctor) = (
            self.names.name,
            self.names.length,
            self.names.prototype,
            self.names.constructor,
        );
        let obj = self.heap.object_mut(fn_ref);
        obj.define_own(k_name, Value::string(n), PROP_METHOD);
        obj.define_own(k_len, Value::number(0.0), PROP_METHOD);
        obj.define_own(k_proto, Value::object(proto_ref), PROP_METHOD);
        self.heap
            .object_mut(proto_ref)
            .define_own(k_ctor, Value::object(fn_ref), PROP_METHOD);
        Value::object(fn_ref)
    }

    /// Define uma função host no objeto global (`globalThis[name]`).
    pub fn define_global_fn(&mut self, name: &str, f: HostFn) {
        let fn_val = self.new_host_fn(name, f);
        let key = self.heap.intern(name);
        self.heap
            .object_mut(self.global)
            .define_own(key, fn_val, PROP_METHOD);
    }

    /// Cria um objeto host opaco identificável via `(tag, id)` (ex.: nós do DOM).
    pub fn new_host_object(&mut self, tag: u32, id: u64, proto: Option<Value>) -> Value {
        let proto_ref = proto.and_then(|v| v.as_object()).or(Some(self.protos.object));
        let obj = self
            .heap
            .alloc_object(proto_ref, ObjectKind::HostObject { tag, id });
        Value::object(obj)
    }

    /// Extrai `(tag, id)` se `val` for um objeto host criado por [`Vm::new_host_object`].
    pub fn host_object_id(&self, val: Value) -> Option<(u32, u64)> {
        let o = val.as_object()?;
        match self.heap.object(o).kind {
            ObjectKind::HostObject { tag, id } => Some((tag, id)),
            _ => None,
        }
    }

    // --------------------------------------------------------- alocação de valores

    pub fn new_object(&mut self) -> Value {
        let o = self
            .heap
            .alloc_object(Some(self.protos.object), ObjectKind::Ordinary);
        Value::object(o)
    }

    pub fn new_object_with_proto(&mut self, proto: Option<Value>) -> Value {
        let proto_ref = proto.and_then(|v| v.as_object());
        let o = self.heap.alloc_object(proto_ref, ObjectKind::Ordinary);
        Value::object(o)
    }

    pub fn new_array(&mut self, items: &[Value]) -> Value {
        let o = self
            .heap
            .alloc_object(Some(self.protos.array), ObjectKind::Array(items.to_vec()));
        Value::object(o)
    }

    // --------------------------------------------------------- propriedades e chamadas

    pub fn set_global(&mut self, name: &str, val: Value) {
        let key = self.heap.intern(name);
        self.heap.object_mut(self.global).set_own(key, val);
    }

    pub fn get_global(&mut self, name: &str) -> Option<Value> {
        let key = self.heap.intern(name);
        self.lookup(self.global, key)
    }

    pub fn get_prop(&mut self, obj: Value, name: &str) -> Result<Value, JsError> {
        let key = self.heap.intern(name);
        self.get_property(obj, key)
            .map_err(|e| self.value_to_js_error(e))
    }

    pub fn set_prop(&mut self, obj: Value, name: &str, val: Value) -> Result<(), JsError> {
        let key = self.heap.intern(name);
        self.set_property(obj, key, val)
            .map_err(|e| self.value_to_js_error(e))
    }

    pub fn define_host_method(&mut self, obj: Value, name: &str, f: HostFn) -> Result<(), JsError> {
        let Some(o) = obj.as_object() else {
            return Err(JsError::Thrown(
                "TypeError: Cannot define method on non-object".into(),
            ));
        };
        let fn_val = self.new_host_fn(name, f);
        let key = self.heap.intern(name);
        self.heap
            .object_mut(o)
            .define_own(key, fn_val, PROP_METHOD);
        Ok(())
    }

    pub fn get_index(&mut self, obj: Value, index: usize) -> Result<Value, JsError> {
        self.get_keyed(obj, Value::number(index as f64))
            .map_err(|e| self.value_to_js_error(e))
    }

    pub fn set_index(&mut self, obj: Value, index: usize, val: Value) -> Result<(), JsError> {
        self.set_keyed(obj, Value::number(index as f64), val)
            .map_err(|e| self.value_to_js_error(e))
    }

    pub fn array_len(&self, val: Value) -> Option<usize> {
        let o = val.as_object()?;
        match &self.heap.object(o).kind {
            ObjectKind::Array(items) => Some(items.len()),
            _ => None,
        }
    }

    pub fn array_elements(&self, val: Value) -> Option<Vec<Value>> {
        let o = val.as_object()?;
        match &self.heap.object(o).kind {
            ObjectKind::Array(items) => Some(items.clone()),
            _ => None,
        }
    }

    /// Invoca uma função JavaScript ou nativa a partir do host.
    pub fn call(&mut self, callee: Value, this: Value, args: &[Value]) -> Result<Value, JsError> {
        if self.frames.is_empty() {
            self.steps = 0;
        }
        self.call_function(callee, this, args)
            .map_err(|e| self.value_to_js_error(e))
    }

    // --------------------------------------------------------- conversões de valor

    pub fn value_to_string(&mut self, val: Value) -> Result<String, JsError> {
        self.to_str(val)
            .map(|s| s.to_string())
            .map_err(|e| self.value_to_js_error(e))
    }

    pub fn value_to_number(&mut self, val: Value) -> Result<f64, JsError> {
        self.to_number(val).map_err(|e| self.value_to_js_error(e))
    }

    pub fn value_to_bool(&self, val: Value) -> bool {
        self.to_boolean(val)
    }

    pub fn as_str(&self, val: Value) -> Option<&str> {
        match val.kind() {
            Kind::String(r) => Some(self.heap.string(r)),
            _ => None,
        }
    }

    // --------------------------------------------------------- fila de timers

    /// Executa os timers vencidos (em ordem de vencimento) respeitando o limite estrito `max_steps`.
    pub fn run_due_timers(&mut self, max_steps: u64) -> Result<usize, JsError> {
        if max_steps == 0 || self.timers.is_empty() {
            return Ok(0);
        }
        let prev = self.step_limit;
        self.steps = 0;
        self.step_limit = Some(max_steps);
        let guard = StepLimitGuard { vm: self, prev };

        let mut ran = 0usize;
        loop {
            if guard.vm.steps >= max_steps {
                break;
            }
            let now = Instant::now();
            let next_idx = guard
                .vm
                .timers
                .iter()
                .enumerate()
                .filter(|(_, t)| t.due_at <= now)
                .min_by_key(|(_, t)| (t.due_at, t.id))
                .map(|(i, _)| i);
            let Some(idx) = next_idx else {
                break;
            };
            let entry = guard.vm.timers.swap_remove(idx);
            ran += 1;
            let global_this = Value::object(guard.vm.global);
            let res = guard
                .vm
                .call_function(entry.callback, global_this, &entry.args);
            if let Err(err_val) = res {
                let err = guard.vm.value_to_js_error(err_val);
                return Err(err);
            }
            guard.vm.maybe_gc();
        }
        Ok(ran)
    }

    pub fn pending_timers(&self) -> usize {
        self.timers.len()
    }

    pub fn next_timer_due(&self) -> Option<Duration> {
        let now = Instant::now();
        self.timers
            .iter()
            .map(|t| t.due_at.saturating_duration_since(now))
            .min()
    }

    pub fn clear_all_timers(&mut self) {
        self.timers.clear();
    }

    /// Avança o relógio lógico dos timers pendentes (útil em testes sem `sleep`).
    pub fn advance_timers(&mut self, delta: Duration) {
        for t in &mut self.timers {
            if let Some(earlier) = t.due_at.checked_sub(delta) {
                t.due_at = earlier;
            } else {
                t.due_at = Instant::now();
            }
        }
    }

    // --------------------------------------------------------- contabilidade e compactação de memória

    /// Estimativa em bytes de toda a memória mantida pela VM (células do heap, tabelas, registradores).
    pub fn approx_heap_bytes(&self) -> usize {
        self.heap.approx_bytes()
            + self.regs.capacity() * size_of::<Value>()
            + self.frames.capacity() * size_of::<Frame>()
            + self.open_upvalues.capacity() * size_of::<(usize, crate::heap::GcRef)>()
            + self.timers.capacity() * size_of::<TimerEntry>()
    }

    /// Coleta todo lixo alcançável e encolhe listas livres/capacidades sobressalentes (ideal quando uma aba dorme).
    pub fn gc_collect(&mut self) {
        self.gc();
        self.heap.shrink_to_fit();
        self.regs.shrink_to_fit();
        self.frames.shrink_to_fit();
        self.open_upvalues.shrink_to_fit();
        self.timers.shrink_to_fit();
    }
}
