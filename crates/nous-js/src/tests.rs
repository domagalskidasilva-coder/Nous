//! Testes do motor completo (parser → compilador → interpretador).

use crate::{JsError, Vm};

/// Roda o código e devolve o valor final como o console mostraria.
fn run(src: &str) -> String {
    let mut vm = Vm::new();
    match vm.eval(src) {
        Ok(v) => vm.display(v),
        Err(e) => panic!("erro inesperado em `{src}`: {e}"),
    }
}

fn run_err(src: &str) -> JsError {
    let mut vm = Vm::new();
    match vm.eval(src) {
        Ok(v) => panic!("esperava erro em `{src}`, veio {}", vm.display(v)),
        Err(e) => e,
    }
}

#[test]
fn aritmetica_e_precedencia() {
    assert_eq!(run("1 + 2 * 3"), "7");
    assert_eq!(run("(1 + 2) * 3"), "9");
    assert_eq!(run("2 ** 10"), "1024");
    assert_eq!(run("7 % 3"), "1");
    assert_eq!(run("10 / 4"), "2.5");
    assert_eq!(run("1 / 0"), "Infinity");
    assert_eq!(run("0 / 0"), "NaN");
    assert_eq!(run("-5 >> 1"), "-3");
    assert_eq!(run("-1 >>> 28"), "15");
    assert_eq!(run("5 & 3 | 8"), "9");
    assert_eq!(run("'4' * '2'"), "8");
    assert_eq!(run("1 < 2 && 2 <= 2 && 3 > 2 && !(1 >= 2)"), "true");
    assert_eq!(run("1 == '1'"), "true");
    assert_eq!(run("1 === '1'"), "false");
    assert_eq!(run("null == undefined"), "true");
}

#[test]
fn strings_e_concatenacao() {
    assert_eq!(run("'a' + 'b' + 1"), "ab1");
    assert_eq!(run("1 + 2 + '3'"), "33");
    assert_eq!(run("`x=${1 + 1}!`"), "x=2!");
    assert_eq!(run("'abc'.length"), "3");
    assert_eq!(run("'abc'[1]"), "b");
    assert_eq!(run("'b' > 'a'"), "true");
    assert_eq!(run("'' + null + undefined + true"), "nullundefinedtrue");
}

#[test]
fn typeof_de_tudo() {
    assert_eq!(run("typeof 1"), "number");
    assert_eq!(run("typeof 'x'"), "string");
    assert_eq!(run("typeof undefined"), "undefined");
    assert_eq!(run("typeof null"), "object");
    assert_eq!(run("typeof {}"), "object");
    assert_eq!(run("typeof function () {}"), "function");
    assert_eq!(run("typeof naoExiste"), "undefined");
    assert_eq!(run("typeof Math.floor"), "function");
}

#[test]
fn funcoes_e_recursao() {
    assert_eq!(
        run("function fat(n) { return n <= 1 ? 1 : n * fat(n - 1); } fat(10)"),
        "3628800"
    );
    assert_eq!(
        run("function fib(n) { if (n < 2) return n; return fib(n - 1) + fib(n - 2); } fib(20)"),
        "6765"
    );
    assert_eq!(
        run("function soma(a, b) { var t = a + b; return t; } soma(2, 3) + soma(1)"),
        "NaN"
    );
    assert_eq!(run("var dobro = (x) => x * 2; dobro(21)"), "42");
    assert_eq!(run("var f = function (a) { return a; }; f(7, 8, 9)"), "7");
    // hoisting: a função existe antes da declaração
    assert_eq!(run("antes(); function antes() { return 1; }"), "1");
}

#[test]
fn objetos_e_prototipos() {
    let src = "
        function Ponto(x, y) { this.x = x; this.y = y; }
        Ponto.prototype.soma = function () { return this.x + this.y; };
        var p = new Ponto(2, 3);
        [p.soma(), p instanceof Ponto, Object.getPrototypeOf(p) === Ponto.prototype, 'x' in p].join(',')
    ";
    assert_eq!(run(src), "5,true,true,true");
    assert_eq!(
        run("var o = { a: 1, 'b c': 2, 3: 'tres' }; o.a + o['b c'] + o[3]"),
        "3tres"
    );
    assert_eq!(run("var o = {}; o.n = 1; o.n += 2; o.n"), "3");
    assert_eq!(run("var o = { f() { return this.v; }, v: 9 }; o.f()"), "9");
    assert_eq!(run("String({})"), "[object Object]");
    assert_eq!(run("({}).hasOwnProperty.call({ a: 1 }, 'a')"), "true");
}

#[test]
fn arrays() {
    assert_eq!(run("var a = [1, 2]; a.push(3); a.length"), "3");
    assert_eq!(run("[1, 2, 3].join('-')"), "1-2-3");
    assert_eq!(run("var a = []; a[5] = 1; a.length"), "6");
    assert_eq!(run("String([1, [2, 3]])"), "1,2,3");
    assert_eq!(run("var a = [10, 20]; a[1]"), "20");
    assert_eq!(run("Array.isArray([]) && !Array.isArray({})"), "true");
}

#[test]
fn excecoes() {
    assert_eq!(
        run("try { null.x } catch (e) { e instanceof TypeError }"),
        "true"
    );
    assert_eq!(run("try { throw 42 } catch (e) { e + 1 }"), "43");
    assert_eq!(
        run("try { naoExiste } catch (e) { e.name }"),
        "ReferenceError"
    );
    assert_eq!(
        run("var s = ''; try { s += 'a'; } finally { s += 'b'; } s"),
        "ab"
    );
    assert_eq!(
        run("var s = ''; try { try { throw 1 } finally { s += 'f' } } catch (e) { s += e } s"),
        "f1"
    );
    assert_eq!(
        run("function f() { throw new RangeError('fora') } try { f() } catch (e) { String(e) }"),
        "RangeError: fora"
    );
    match run_err("null.x") {
        JsError::Thrown(msg) => assert!(msg.starts_with("TypeError"), "{msg}"),
        other => panic!("esperava Thrown, veio {other:?}"),
    }
}

#[test]
fn lacos_break_continue() {
    assert_eq!(
        run(
            "var s = 0; for (var i = 0; i < 10; i++) { if (i == 5) continue; if (i == 8) break; s += i; } s"
        ),
        "23"
    );
    assert_eq!(run("var n = 0; while (n < 100) n += 7; n"), "105");
    assert_eq!(run("var n = 0; do { n++; } while (n < 3); n"), "3");
    assert_eq!(
        run(
            "var r = ''; for (let i = 0; i < 3; i++) { for (let j = 0; j < 3; j++) { if (j == 1) break; r += i; } } r"
        ),
        "012"
    );
    assert_eq!(
        run(
            "var r = ''; switch (2) { case 1: r += 'a'; case 2: r += 'b'; case 3: r += 'c'; break; default: r += 'd'; } r"
        ),
        "bc"
    );
    assert_eq!(
        run("var r; switch ('x') { case 'y': r = 1; break; default: r = 2; } r"),
        "2"
    );
}

#[test]
fn numero_para_string_como_js() {
    assert_eq!(run("1e21"), "1e+21");
    assert_eq!(run("0.1 + 0.2"), "0.30000000000000004");
    assert_eq!(run("String(-0)"), "0");
    assert_eq!(run("1 / 3"), "0.3333333333333333");
    assert_eq!(run("123456789012345680000"), "123456789012345680000");
    assert_eq!(run("(255).toString()"), "255");
    assert_eq!(run("1e-7"), "1e-7");
    assert_eq!(run("Number('0x10') + Number(' 12 ')"), "28");
    assert_eq!(run("parseInt('42px') + parseFloat('1.5e1x')"), "57");
}

#[test]
fn operadores_logicos_e_atualizacao() {
    assert_eq!(run("null ?? 5"), "5");
    assert_eq!(run("0 || 'a'"), "a");
    assert_eq!(run("1 && 2"), "2");
    assert_eq!(run("var x = null; x ??= 4; x"), "4");
    assert_eq!(run("var y = 1; y ||= 9; y"), "1");
    assert_eq!(run("var i = 5; var j = i++; i * 10 + j"), "65");
    assert_eq!(run("var o = { n: 1 }; ++o.n + o['n']++ + o.n"), "7");
    assert_eq!(run("var a = 1, b = (a = 2, a + 1); a + b"), "5");
}

#[test]
fn escopo_de_bloco() {
    assert_eq!(run("let x = 1; { let x = 2; } x"), "1");
    assert_eq!(
        run("function f() { let a = 1; { let a = 2; } return a; } f()"),
        "1"
    );
    assert_eq!(
        run("function g() { if (true) { var v = 3; } return v; } g()"),
        "3"
    );
}

#[test]
fn closure_ainda_nao_suportada() {
    let err = run_err("function f() { var x = 1; return function () { return x; }; } f()");
    assert!(matches!(err, JsError::Unsupported(_)), "{err:?}");
}

#[test]
fn erro_de_sintaxe() {
    assert!(matches!(run_err("var = ;"), JsError::Syntax(_)));
}

#[test]
fn coleta_de_lixo_libera_objetos() {
    let mut vm = Vm::new();
    let v = vm
        .eval("var ultimo; for (var i = 0; i < 50000; i++) { ultimo = { v: i, s: 'x' + i }; } ultimo.v")
        .expect("roda");
    assert_eq!(vm.display(v), "49999");
    assert!(vm.heap.collections > 0, "o GC deveria ter rodado");
    assert!(
        vm.heap.live_cells() < 30_000,
        "vivos: {}",
        vm.heap.live_cells()
    );
}

#[test]
fn console_log_usa_a_saida() {
    use std::cell::RefCell;
    use std::rc::Rc;
    let out = Rc::new(RefCell::new(Vec::new()));
    let sink = out.clone();
    let mut vm = Vm::new();
    vm.output = Box::new(move |s| sink.borrow_mut().push(s.to_owned()));
    vm.eval("console.log('oi', 1 + 1, [1, 2]); print(null)")
        .expect("roda");
    assert_eq!(
        *out.borrow(),
        vec!["oi 2 1,2".to_owned(), "null".to_owned()]
    );
}

#[test]
fn limite_de_instrucoes() {
    let mut vm = Vm::new();
    vm.step_limit = Some(10_000);
    assert!(matches!(
        vm.eval("while (true) {}"),
        Err(JsError::Thrown(_))
    ));
}
