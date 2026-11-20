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
    New {
        dst: Reg,
        callee: Reg,
        args: Reg,
        argc: u16,
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
    pub params: u16,
    pub registers: u16,
    pub code: Vec<Op>,
    /// Números e strings internadas usados pela função.
    pub constants: Vec<Value>,
    /// Funções aninhadas criadas por `Closure`.
    pub functions: Vec<Rc<FunctionCode>>,
    pub is_arrow: bool,
}
