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
fn closures_basicas_e_aninhadas() {
    assert_eq!(
        run("function f() { var x = 1; return function () { return x; }; } f()()"),
        "1"
    );
    // Contador com mutação interna
    assert_eq!(
        run(
            "function make() { let n = 0; return () => ++n; } \
             const a = make(), b = make(); \
             [a(), a(), b(), a()].join(',')"
        ),
        "1,2,1,3"
    );
    // Closures aninhadas em múltiplos níveis sem referência intermediária
    assert_eq!(
        run(
            "function l1(a) { \
               return function l2(b) { \
                 return function l3(c) { return a + b + c; }; \
               }; \
             } \
             l1(10)(20)(12)"
        ),
        "42"
    );
    // Mutação compartilhada entre pai e filhos e entre irmãos
    assert_eq!(
        run(
            "function pair() { \
               let v = 5; \
               const inc = () => { v += 2; return v; }; \
               const get = () => v; \
               v = 10; \
               return [get(), inc(), get()]; \
             } \
             pair().join(',')"
        ),
        "10,12,12"
    );
    // Arrow function preserva `this` léxico mesmo com `.call`
    assert_eq!(
        run(
            "const obj = { v: 99, maker() { return () => () => this.v; } }; \
             const fn = obj.maker(); \
             [fn()(), fn.call({ v: 1 })()].join(',')"
        ),
        "99,99"
    );
}

#[test]
fn closures_em_lacos_let() {
    assert_eq!(
        run(
            "const fns = []; \
             for (let i = 0; i < 5; i++) { fns.push(() => i); } \
             fns.map(f => f()).join(',')"
        ),
        "0,1,2,3,4"
    );
    // `continue` ainda fecha a iteração corretamente
    assert_eq!(
        run(
            "const fns = []; \
             for (let i = 0; i < 5; i++) { \
               if (i === 2) { fns.push(() => i * 10); continue; } \
               fns.push(() => i); \
             } \
             fns.map(f => f()).join(',')"
        ),
        "0,1,20,3,4"
    );
    // `var` compartilha o mesmo slot
    assert_eq!(
        run(
            "var fns = []; \
             for (var i = 0; i < 3; i++) { fns.push(() => i); } \
             fns.map(f => f()).join(',')"
        ),
        "3,3,3"
    );
    // `for..of` e `for..in` com `const`/`let`
    assert_eq!(
        run(
            "const fns = []; \
             for (const x of ['a', 'b', 'c']) { fns.push(() => x); } \
             fns.map(f => f()).join('')"
        ),
        "abc"
    );
    assert_eq!(
        run(
            "const fns = []; \
             for (const k in { x: 1, y: 2 }) { fns.push(() => k); } \
             fns.map(f => f()).join(',')"
        ),
        "x,y"
    );
}

#[test]
fn closures_sobrevivem_ao_gc() {
    let mut vm = Vm::new();
    let v = vm
        .eval(
            "function outer(seed) { \
               let s = { val: seed }; \
               return () => s.val; \
             } \
             const keep = outer(777); \
             for (let i = 0; i < 30000; i++) { \
               let tmp = outer(i); \
               tmp(); \
             } \
             keep()",
        )
        .expect("roda");
    assert_eq!(vm.display(v), "777");
    assert!(vm.heap.collections > 0, "GC deve rodar sob alocação pesada");
    assert!(
        vm.heap.live_cells() < 10_000,
        "células mortas devem ser coletadas: {}",
        vm.heap.live_cells()
    );
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

#[test]
fn for_in_e_for_of() {
    assert_eq!(
        run("const out = []; for (const k in { a: 1, b: 2 }) out.push(k); out.join(',')"),
        "a,b"
    );
    // Não enumera propriedades nativas de Object.prototype
    assert_eq!(
        run("let c = 0; for (const k in {}) c++; c"),
        "0"
    );
    assert_eq!(
        run("const out = []; for (const x of [10, 20, 30]) { if (x === 20) continue; out.push(x); } out.join(',')"),
        "10,30"
    );
    assert_eq!(
        run("const out = []; for (const ch of 'a😀b') out.push(ch); out.join('-')"),
        "a-😀-b"
    );
    assert_eq!(
        run("let c = 0; for (const k in null) c++; for (const k in undefined) c++; c"),
        "0"
    );
}

#[test]
fn destructuring_defaults_e_rest_spread() {
    assert_eq!(
        run("const [a, , b = 99, ...rest] = [1, 2, undefined, 4, 5]; [a, b, rest.join('-')].join(':')"),
        "1:99:4-5"
    );
    assert_eq!(
        run("const [x = 10] = [null]; x === null"),
        "true"
    );
    assert_eq!(
        run("const key = 'dyn'; const { a, b: [c, d = 7], [key]: e, ...r } = { a: 1, b: [2], dyn: 3, z: 4 }; [a, c, d, e, r.z, r.a].join(',')"),
        "1,2,7,3,4,"
    );
    assert_eq!(
        run("let p = 1, q = 2; [p, q] = [q, p]; p + ',' + q"),
        "2,1"
    );
    assert_eq!(
        run("let u; ({ u = 42 } = {}); u"),
        "42"
    );
    assert_eq!(
        run("function f(a = 2, b = a * 3, ...more) { return [a, b, more.length, f.length].join(','); } f(undefined, undefined, 9, 8)"),
        "2,6,2,0"
    );
    assert_eq!(
        run("function g({ x, y = 5 }, [z]) { return x + y + z; } g({ x: 10 }, [3])"),
        "18"
    );
    assert_eq!(
        run("const arr = [2, 3]; const full = [1, ...arr, ...'hi', 4]; full.join(',')"),
        "1,2,3,h,i,4"
    );
    assert_eq!(
        run("const o1 = { a: 1, b: 2 }; const o2 = { ...o1, b: 20, [ 'c' + 1 ]: 30, ...null }; [o2.a, o2.b, o2.c1].join(',')"),
        "1,20,30"
    );
    assert_eq!(
        run("function sum(a, b, c, d) { return a + b + c + d; } sum(1, ...[2, 3], 4)"),
        "10"
    );
}

#[test]
fn classes_heranca_e_super() {
    assert_eq!(
        run(
            "class Animal { \
               kind = 'beast'; \
               constructor(name) { this.name = name; } \
               speak() { return this.name + '!'; } \
               static id() { return 100; } \
             } \
             class Dog extends Animal { \
               legs = 4; \
               constructor(name, breed) { \
                 super(name); \
                 this.breed = breed; \
               } \
               speak() { return super.speak() + ' (' + this.breed + ',' + this.legs + ',' + this.kind + ')'; } \
               static id() { return super.id() + 1; } \
             } \
             const d = new Dog('Rex', 'Lab'); \
             [d.speak(), d instanceof Dog, d instanceof Animal, Dog.id()].join('|')"
        ),
        "Rex! (Lab,4,beast)|true|true|101"
    );
    // Construtor padrão em subclasse encaminha argumentos para super(...args)
    assert_eq!(
        run(
            "class Base { constructor(x, y) { this.sum = x + y; } } \
             class Sub extends Base {} \
             new Sub(20, 22).sum"
        ),
        "42"
    );
    // Arrow function dentro de método herda `this` e `super`
    assert_eq!(
        run(
            "class B { greet() { return 'hi'; } } \
             class C extends B { \
               name = 'world'; \
               make() { return () => super.greet() + ' ' + this.name; } \
             } \
             new C().make()()"
        ),
        "hi world"
    );
    // Expressão de classe nomeada captura nome interno
    assert_eq!(
        run(
            "const Maker = class Hidden { \
               constructor(v) { this.v = v; } \
               static build(v) { return new Hidden(v); } \
             }; \
             Maker.build(55).v"
        ),
        "55"
    );
    // Class sem `new` lança TypeError
    assert!(matches!(
        run_err("class C {} C()"),
        JsError::Thrown(ref m) if m.starts_with("TypeError")
    ));
}

#[test]
fn optional_chaining_e_delete() {
    assert_eq!(
        run(
            "let hits = 0; \
             const obj = { a: { b: (x) => x * 2 } }; \
             const n = null; \
             [ \
               obj?.a?.b(5), \
               n?.a.b(hits++), \
               n?.[hits++], \
               obj.missing?.(), \
               hits \
             ].map(String).join(',')"
        ),
        "10,undefined,undefined,undefined,0"
    );
    // Parênteses delimitam o curto-circuito
    assert!(matches!(
        run_err("const n = null; (n?.a).b"),
        JsError::Thrown(ref m) if m.starts_with("TypeError")
    ));
    assert_eq!(
        run("const o = { a: 1, b: 2 }; [delete o.a, 'a' in o, o.b].join(',')"),
        "true,false,2"
    );
}

#[test]
fn builtins_array_completos() {
    assert_eq!(
        run(
            "const a = [1, 2, 3]; \
             a.push(4); \
             const p = a.pop(); \
             const s = a.shift(); \
             a.unshift(9, 8); \
             [p, s, a.join(',')].join('|')"
        ),
        "4|1|9,8,2,3"
    );
    assert_eq!(
        run("[1, 2, 3, 4, 5].slice(1, -1).join(',')"),
        "2,3,4"
    );
    assert_eq!(
        run("const a = [1, 2, 3, 4]; const rem = a.splice(1, 2, 99, 100); [rem.join(','), a.join(',')].join('|')"),
        "2,3|1,99,100,4"
    );
    assert_eq!(
        run("[1, 2].concat([3, 4], 5).join(',')"),
        "1,2,3,4,5"
    );
    assert_eq!(
        run("[1, 2, NaN, 2].indexOf(2) + ',' + [1, 2, 3, 2].lastIndexOf(2) + ',' + [1, NaN].includes(NaN)"),
        "1,3,true"
    );
    assert_eq!(
        run("[1, 2, 3].reverse().join(',') + '|' + [1, 2, 3, 4].fill(0, 1, 3).join(',')"),
        "3,2,1|1,0,0,4"
    );
    assert_eq!(
        run(
            "const a = [1, 2, 3, 4]; \
             [ \
               a.map(x => x * 2).join(','), \
               a.filter(x => x % 2 === 0).join(','), \
               a.reduce((acc, x) => acc + x, 0), \
               ['a', 'b', 'c'].reduceRight((acc, x) => acc + x), \
               a.find(x => x > 2), \
               a.findIndex(x => x > 2), \
               a.some(x => x > 3), \
               a.every(x => x > 0) \
             ].join('|')"
        ),
        "2,4,6,8|2,4|10|cba|3|2|true|true"
    );
    assert_eq!(
        run("const fl = [1, [2, [3, [4]]]].flat(2); fl.length + ':' + Array.isArray(fl[3]) + ':' + fl[3][0]"),
        "4:true:4"
    );
    assert_eq!(
        run("[10, 2, 1, undefined, 5].sort().join(',') + '|' + [10, 2, 1, 5].sort((a, b) => a - b).join(',')"),
        "1,10,2,5,|1,2,5,10"
    );
    assert_eq!(
        run("Array.from('abc', (c, i) => c + i).join(',') + '|' + Array.from({ length: 3, 0: 'x', 2: 'z' }).map(String).join(',') + '|' + Array.of(3).join(',')"),
        "a0,b1,c2|x,undefined,z|3"
    );
}

#[test]
fn builtins_string_completos() {
    assert_eq!(
        run("'hello'.slice(1, -1) + '|' + 'hello'.substring(4, 1)"),
        "ell|ell"
    );
    assert_eq!(
        run("'banana'.indexOf('na') + ',' + 'banana'.lastIndexOf('na') + ',' + 'banana'.includes('nan') + ',' + 'banana'.startsWith('ba') + ',' + 'banana'.endsWith('na')"),
        "2,4,true,true,true"
    );
    assert_eq!(
        run("'a,b,c'.split(',', 2).join('-') + '|' + 'ab'.split('').join('-')"),
        "a-b|a-b"
    );
    assert_eq!(
        run("'  hi  '.trim() + '|' + '  hi  '.trimStart() + '|' + '  hi  '.trimEnd()"),
        "hi|hi  |  hi"
    );
    assert_eq!(
        run("'AbC'.toLowerCase() + ',' + 'AbC'.toUpperCase()"),
        "abc,ABC"
    );
    assert_eq!(
        run("'foo bar foo'.replace('foo', 'baz') + '|' + 'foo bar foo'.replaceAll('foo', m => m.toUpperCase())"),
        "baz bar foo|FOO bar FOO"
    );
    assert_eq!(
        run("'ab'.repeat(3) + '|' + '5'.padStart(3, '0') + '|' + '5'.padEnd(3, '.')"),
        "ababab|005|5.."
    );
    assert_eq!(
        run("'ABC'.charAt(1) + ',' + 'ABC'.charAt(9) + ',' + 'ABC'.charCodeAt(0) + ',' + String.fromCharCode(65, 66) + ',' + 'a'.concat('b', 'c')"),
        "B,,65,AB,abc"
    );
}

#[test]
fn builtins_object_completos() {
    assert_eq!(
        run(
            "const o = { a: 1, b: 2 }; \
             [Object.keys(o).join(','), Object.values(o).join(','), Object.entries(o).map(e => e.join(':')).join(',')].join('|')"
        ),
        "a,b|1,2|a:1,b:2"
    );
    assert_eq!(
        run("const o = Object.fromEntries([['x', 10], ['y', 20]]); o.x + o.y"),
        "30"
    );
    assert_eq!(
        run("const t = { a: 1 }; Object.assign(t, { b: 2 }, { a: 3 }); t.a + ',' + t.b"),
        "3,2"
    );
    assert_eq!(
        run("const proto = { greet() { return 'hi ' + this.n; } }; const o = Object.create(proto); o.n = 'Ada'; [o.greet(), Object.hasOwn(o, 'n'), Object.hasOwn(o, 'greet')].join(',')"),
        "hi Ada,true,false"
    );
    assert_eq!(
        run("const f = Object.freeze({ x: 1 }); f.x = 2; f.y = 3; [f.x, f.y, Object.isFrozen(f)].map(String).join(',')"),
        "1,undefined,true"
    );
    assert_eq!(
        run(
            "const o = {}; \
             Object.defineProperty(o, 'hidden', { value: 42, enumerable: false, writable: false }); \
             o.hidden = 99; \
             [o.hidden, Object.keys(o).length, Object.is(NaN, NaN), !Object.is(0, -0)].join(',')"
        ),
        "42,0,true,true"
    );
}

#[test]
fn json_parse_e_stringify() {
    assert_eq!(
        run("const v = JSON.parse('{\"a\":[1,true,null,\"😀\"],\"b\":\"x\\\\ny\"}'); [v.a[0], v.a[1], v.a[2], v.a[3], v.b.length].join(',')"),
        "1,true,,😀,3"
    );
    assert_eq!(
        run("JSON.parse('{\"a\":1,\"b\":2}', (k, v) => k === 'a' ? v * 10 : v).a"),
        "10"
    );
    assert_eq!(
        run("JSON.stringify({ a: 1, b: undefined, c: () => {}, d: [1, undefined, NaN] })"),
        "{\"a\":1,\"d\":[1,null,null]}"
    );
    assert_eq!(
        run("JSON.stringify({ a: 1, b: 2 }, ['b'], 2)"),
        "{\n  \"b\": 2\n}"
    );
    assert_eq!(
        run("JSON.stringify({ toJSON() { return { ok: true }; } })"),
        "{\"ok\":true}"
    );
    assert!(matches!(
        run_err("const a = []; a.push(a); JSON.stringify(a)"),
        JsError::Thrown(ref m) if m.starts_with("TypeError")
    ));
    assert!(matches!(
        run_err("JSON.parse('{bad}')"),
        JsError::Thrown(ref m) if m.starts_with("SyntaxError")
    ));
    assert!(matches!(
        run_err("JSON.parse('[1,]')"),
        JsError::Thrown(ref m) if m.starts_with("SyntaxError")
    ));
}

#[test]
fn builtins_math_e_number() {
    assert_eq!(
        run("[Number.isNaN(NaN), Number.isNaN('NaN'), Number.isFinite(10), Number.isFinite(Infinity), Number.isInteger(4.0), Number.isInteger(4.5), Number.MAX_SAFE_INTEGER].join(',')"),
        "true,false,true,false,true,false,9007199254740991"
    );
    assert_eq!(
        run("(3.14159).toFixed(2) + '|' + (-0).toFixed(2) + '|' + (255).toString(16) + '|' + (-10).toString(2)"),
        "3.14|0.00|ff|-1010"
    );
    assert_eq!(
        run("[Math.clz32(1), Math.imul(0xffffffff, 5), Math.hypot(3, 4), Math.round(0.49999999999999994), Math.cbrt(27), Math.sin(0), Math.cos(0), Math.log2(8), Math.log10(1000)].join(',')"),
        "31,-5,5,0,3,0,1,3,3"
    );
}

#[test]
fn builtins_map_e_set() {
    assert_eq!(
        run(
            "const m = new Map([['a', 1]]); \
             const obj = {}; \
             m.set(obj, 2).set(NaN, 3).set(-0, 4); \
             const seen = []; \
             m.forEach((v, k) => seen.push(String(v))); \
             [m.size, m.get('a'), m.get(obj), m.get(NaN), m.get(0), m.has(NaN), m.delete('a'), m.size, seen.join(',')].join('|')"
        ),
        "4|1|2|3|4|true|true|3|1,2,3,4"
    );
    assert_eq!(
        run("const m = new Map([['x', 1], ['y', 2]]); const res = []; for (const [k, v] of m) res.push(k + '=' + v); m.clear(); [res.join(','), m.size].join('|')"),
        "x=1,y=2|0"
    );
    assert_eq!(
        run(
            "const s = new Set([1, 2, 2, NaN, NaN, -0, 0]); \
             s.add(3); \
             const items = []; \
             for (const v of s) items.push(String(v)); \
             [s.size, s.has(NaN), s.has(0), s.delete(2), s.size, items.join(',')].join('|')"
        ),
        "5|true|true|true|4|1,2,NaN,0,3"
    );
}

#[test]
fn host_bridge_timers_e_memoria() {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    let log = Rc::new(RefCell::new(Vec::<String>::new()));
    let log_cb = log.clone();

    let mut vm = Vm::new();
    let proto = vm.new_object();
    vm.define_host_method(
        proto,
        "setText",
        Rc::new(move |vm, this, args| {
            let (tag, id) = vm
                .host_object_id(this)
                .ok_or_else(|| JsError::Thrown("TypeError: expected host node".into()))?;
            let text = vm.value_to_string(args.first().copied().unwrap_or(crate::Value::UNDEFINED))?;
            log_cb.borrow_mut().push(format!("{tag}:{id}:{text}"));
            let sv = vm.new_string(&text);
            vm.set_prop(this, "textContent", sv)?;
            Ok(crate::Value::UNDEFINED)
        }),
    )
    .unwrap();

    let node = vm.new_host_object(7, 42, Some(proto));
    assert_eq!(vm.host_object_id(node), Some((7, 42)));
    assert_eq!(vm.host_object_id(crate::Value::UNDEFINED), None);
    vm.set_global("el", node);

    vm.define_global_fn(
        "hostAdd",
        Rc::new(|vm, _, args| {
            let a = vm.value_to_number(args[0])?;
            let b = vm.value_to_number(args[1])?;
            Ok(crate::Value::number(a + b))
        }),
    );

    let r = vm
        .eval(
            "el.setText('hello'); \
             let fired = 0; \
             const id1 = setTimeout((x) => { fired += x; }, 50, 10); \
             const id2 = setTimeout(() => { fired += 999; }, 10); \
             clearTimeout(id2); \
             [el.textContent, hostAdd(20, 22), fired].join(',')",
        )
        .unwrap();
    assert_eq!(vm.display(r), "hello,42,0");
    assert_eq!(*log.borrow(), vec!["7:42:hello".to_owned()]);
    assert_eq!(vm.pending_timers(), 1);

    // Antes de vencer não roda
    assert_eq!(vm.run_due_timers(10_000).unwrap(), 0);
    vm.advance_timers(Duration::from_millis(60));
    assert_eq!(vm.run_due_timers(10_000).unwrap(), 1);
    let fired = vm.eval("fired").unwrap();
    assert_eq!(vm.display(fired), "10");

    // Limite estrito de passos em timer infinito
    vm.eval("setTimeout(() => { while (true) {} }, 0);").unwrap();
    assert!(matches!(
        vm.run_due_timers(500),
        Err(JsError::Thrown(_))
    ));
    vm.clear_all_timers();

    // Contabilidade e compactação de memória
    vm.eval(
        "for (let i = 0; i < 20000; i++) { const o = { i, a: [i, i + 1] }; }",
    )
    .unwrap();
    let before = vm.approx_heap_bytes();
    vm.gc_collect();
    let after = vm.approx_heap_bytes();
    assert!(
        after < before,
        "gc_collect deve reduzir bytes: antes={before}, depois={after}"
    );
}
