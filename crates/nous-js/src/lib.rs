//! Motor JavaScript próprio do Nous.
//!
//! Caminho do código: texto → `oxc_parser` (AST) → [`compiler`] (bytecode de
//! registradores) → [`vm`] (interpretador) com valores NaN-boxed ([`value`]) num heap
//! com coleta de lixo ([`heap`]).

pub mod builtins;
pub mod bytecode;
pub mod compiler;
pub mod heap;
pub mod host;
pub mod json;
pub mod value;
pub mod vm;

pub use host::HostFn;
pub use value::{Kind, Value};
pub use vm::{JsError, Vm};

#[cfg(test)]
mod tests;
