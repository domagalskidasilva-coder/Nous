//! Bytecode baseado em registradores (como o Ignition do V8 e o LLInt do JavaScriptCore).
//!
//! Cada função tem uma janela de registradores: os parâmetros ficam nos primeiros,
//! depois as variáveis locais e por fim os temporários das expressões.

use crate::value::Value;
use std::rc::Rc;

pub type Reg = u16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Exp,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    StrictEq,
    StrictNe,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
    In,
    InstanceOf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Plus,
    Not,
    BitNot,
    TypeOf,
    Void,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpvalueDesc {
    /// `true` = captura registrador local do pai imediato;
    /// `false` = captura upvalue do pai imediato.
    pub is_local: bool,
    /// Índice do registrador no pai (se `is_local`) ou índice na tabela de upvalues do pai.
    pub index: u16,
}

#[derive(Clone, Copy, Debug)]
pub enum Op {
    LoadConst {
        dst: Reg,
        idx: u32,
    },
    LoadUndefined {
        dst: Reg,
    },
    LoadNull {
        dst: Reg,
    },
    LoadBool {
        dst: Reg,
        value: bool,
    },
    LoadThis {
        dst: Reg,
    },
    Move {
        dst: Reg,
        src: Reg,
    },
    Binary {
        op: BinOp,
        dst: Reg,
        lhs: Reg,
        rhs: Reg,
    },
    Unary {
        op: UnOp,
        dst: Reg,
        src: Reg,
    },
    Jump {
        target: u32,
    },
    JumpIfFalse {
        cond: Reg,
        target: u32,
    },
    JumpIfTrue {
        cond: Reg,
        target: u32,
    },
    JumpIfNotNullish {
        cond: Reg,
        target: u32,
    },
    JumpIfNullish {
        cond: Reg,
        target: u32,
    },
    JumpIfNotUndefined {
        cond: Reg,
        target: u32,
    },
    /// Lê variável global; lança ReferenceError se não existir.
    GetGlobal {
        dst: Reg,
        name: u32,
    },
    /// `typeof x` com `x` global: não lança se não existir.
    TypeOfGlobal {
        dst: Reg,
        name: u32,
    },
    SetGlobal {
        name: u32,
        src: Reg,
    },
    /// `var x` no topo do script: cria a propriedade global se ainda não existir.
    DeclareGlobal {
        name: u32,
    },
    DeleteGlobal {
        dst: Reg,
        name: u32,
    },
    GetUpvalue {
        dst: Reg,
        upvalue: u16,
    },
    SetUpvalue {
        upvalue: u16,
        src: Reg,
    },
    CloseUpvalues {
        from_reg: Reg,
    },
    GetNamed {
        dst: Reg,
        obj: Reg,
        name: u32,
    },
    SetNamed {
        obj: Reg,
        name: u32,
        src: Reg,
    },
    DeleteNamed {
        dst: Reg,
        obj: Reg,
        name: u32,
    },
    GetKeyed {
        dst: Reg,
        obj: Reg,
        key: Reg,
    },
    SetKeyed {
        obj: Reg,
        key: Reg,
        src: Reg,
    },
    DeleteKeyed {
        dst: Reg,
        obj: Reg,
        key: Reg,
    },
    CheckObjectNotNullish {
        src: Reg,
    },
    NewObject {
        dst: Reg,
    },
    NewArray {
        dst: Reg,
    },
    ArrayPush {
        array: Reg,
        src: Reg,
    },
    ArrayPushSpread {
        array: Reg,
        iterable: Reg,
    },
    ObjectAssignSpread {
        obj: Reg,
        src: Reg,
    },
    ObjectRest {
        dst: Reg,
        obj: Reg,
        excl_start: Reg,
        excl_count: u16,
    },
    DefineMethod {
        obj: Reg,
        name: u32,
        func: Reg,
        enumerable: bool,
    },
    DefineMethodKeyed {
        obj: Reg,
        key: Reg,
        func: Reg,
        enumerable: bool,
    },
    SetupClass {
        ctor: Reg,
        super_ctor: Reg,
    },
    ForInInit {
        dst: Reg,
        obj: Reg,
    },
    ForOfInit {
        dst: Reg,
        src: Reg,
    },
    IterNext {
        iter: Reg,
        dst: Reg,
        done_target: u32,
    },
    IterRest {
        iter: Reg,
        dst: Reg,
    },
    Closure {
        dst: Reg,
        func: u32,
    },
    /// Argumentos ficam em registradores consecutivos a partir de `args`.
    Call {
        dst: Reg,
        callee: Reg,
        this: Reg,
        args: Reg,
        argc: u16,
    },
    CallSpread {
        dst: Reg,
        callee: Reg,
        this: Reg,
        args_array: Reg,
    },
    New {
        dst: Reg,
        callee: Reg,
        args: Reg,
        argc: u16,
    },
    NewSpread {
        dst: Reg,
        callee: Reg,
        args_array: Reg,
    },
    SuperCall {
        dst: Reg,
        args: Reg,
        argc: u16,
    },
    SuperCallSpread {
        dst: Reg,
        args_array: Reg,
    },
    FinishSuper {
        src: Reg,
    },
    GetSuperNamed {
        dst: Reg,
        name: u32,
    },
    GetSuperKeyed {
        dst: Reg,
        key: Reg,
    },
    Return {
        src: Reg,
    },
    Throw {
        src: Reg,
    },
    EnterTry {
        catch_target: u32,
        exc: Reg,
    },
    ExitTry,
}

#[derive(Debug)]
pub struct FunctionCode {
    pub name: Box<str>,
    /// Quantidade de parâmetros posicionais antes do `...rest`.
    pub params: u16,
    /// `Function.length` segundo a especificação (antes de parâmetro padrão ou rest).
    pub length: u16,
    pub has_rest: bool,
    pub registers: u16,
    pub code: Vec<Op>,
    /// Números e strings internadas usados pela função.
    pub constants: Vec<Value>,
    /// Funções aninhadas criadas por `Closure`.
    pub functions: Vec<Rc<FunctionCode>>,
    pub upvalues: Vec<UpvalueDesc>,
    pub is_arrow: bool,
    pub is_class_ctor: bool,
    pub is_derived_ctor: bool,
}
