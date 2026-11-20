//! Motor JavaScript próprio do Nous.
//!
//! Caminho do código: texto → `oxc_parser` (AST) → [`compiler`] (bytecode de
//! registradores) → [`vm`] (interpretador) com valores NaN-boxed ([`value`]) num heap
//! com coleta de lixo ([`heap`]).
//!
//! Estado: esqueleto. Funções, objetos, protótipos, exceções e laços funcionam; closures
//! que capturam variáveis locais de outra função ainda não (dão `JsError::Unsupported`).

pub mod builtins;
pub mod bytecode;
pub mod compiler;
pub mod heap;
pub mod value;
pub mod vm;

pub use value::{Kind, Value};
pub use vm::{JsError, Vm};

#[cfg(test)]
mod tests;
