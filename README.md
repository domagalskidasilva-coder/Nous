# Nous

Navegador web feito em Rust, com motor próprio, pensado para gastar o mínimo de RAM e CPU.
A interface lembra a de navegadores conhecidos (abas no topo, barra de endereço), mas a
identidade visual e o motor são do Nous.

## Princípios

- **Leve primeiro.** Cada decisão de arquitetura é medida em memória e processamento.
  Nada roda sem necessidade: a janela só redesenha quando algo muda.
- **Rust de ponta a ponta.** Interface, rede, HTML, CSS, layout, pintura e JavaScript em Rust.
- **Motor próprio.** Montado com peças Rust testadas (html5ever, Vello, Parley…) e um motor
  JavaScript escrito aqui. Cada peça fica atrás de uma API nossa, então pode ser trocada.
- **Padrões da web.** O alvo de compatibilidade são as especificações WHATWG/W3C, medidas
  pelos mesmos testes que os grandes navegadores usam: web-platform-tests e test262.

## Arquitetura

| Crate | Função |
|---|---|
| `nous-app` | Binário `nous`: janela, eventos, abas e sessão |
| `nous-ui` | Interface (abas, omnibox, botões, temas) desenhada com Vello |
| `nous-engine` | API do motor: WebView, navegação, histórico, páginas internas |
| `nous-net` | Rede: HTTP/1.1 e HTTP/2, TLS (rustls + ring), cookies, `file:` e `data:` |
| `nous-html` | Parser HTML (html5ever) gravando direto no DOM do Nous |
| `nous-dom` | DOM em arena: nós compactos ligados por índices, nomes internados |
| `nous-style` | Cascata CSS (provisória até o Stylo) que gera o `ComputedStyle` |
| `nous-layout` | Layout de blocos e texto (Parley) |
| `nous-paint` | Lista de desenho para o Vello (GPU via wgpu) |
| `nous-js` | Motor JavaScript próprio: parser oxc, bytecode de registradores, GC |
| `tools/test262-runner` | Roda a suíte oficial test262 no `nous-js` |

Detalhes em [docs/arquitetura.md](docs/arquitetura.md).

## Compilar e rodar

Pré-requisitos: Rust estável (via [rustup](https://rustup.rs)). No Windows, também o
Visual Studio Build Tools com a carga de trabalho de C++. No Linux, as bibliotecas de
janela: `libxkbcommon-dev libwayland-dev libx11-dev libxcursor-dev libxrandr-dev libxi-dev libvulkan-dev`.

```sh
cargo run --release
```

Testes de todo o workspace:

```sh
cargo test --workspace
```

No Windows, se o `cargo` não achar o linker (terminal comum, ou Build Tools sem as
ferramentas `Hostx64`), use o script que carrega o ambiente do Visual Studio:

```powershell
.\scripts\cargo.ps1 run --release
```

Captura sem janela (testes visuais): `nous --captura saida.png https://example.com`
(acrescente `--claro` para o tema claro). Medição de RAM: `.\scripts\medir-ram.ps1`.

## Atalhos de teclado

| Atalho | Ação |
|---|---|
| `Ctrl+T` | Nova aba |
| `Ctrl+W` | Fechar aba |
| `Ctrl+L` | Ir para a barra de endereço |
| `Ctrl+Tab` | Próxima aba |
| `F5` | Recarregar |
| `Alt+←` / `Alt+→` | Voltar / avançar |

## Páginas internas

- `nous://nova-aba` — página de nova aba.
- `nous://memoria` — memória usada pelo processo e por aba.

## Status

| Fase | O que entrega | Situação |
|---|---|---|
| 0. Fundação | Workspace, CI nos 3 sistemas, janela com interface estilo navegador | em andamento |
| 1. Motor estático | Rede, HTML→DOM, CSS, layout, texto, imagens, rolagem, links | começou: páginas simples sem JavaScript |
| 2. JavaScript v1 | Interpretador, GC, funções nativas principais | esqueleto |
| 3. JavaScript ↔ DOM | Web IDL, event loop, eventos, fetch, Canvas 2D | a fazer |
| 4. Beta do produto | Multiprocesso com sandbox, abas dormindo, bloqueador, instaladores, atualização | a fazer |
| 5. Desempenho e web pesada | JIT (Cranelift), GC incremental, WebGL/WebGPU, mídia | a fazer |

## test262 (JavaScript)

```sh
git clone --depth 1 https://github.com/tc39/test262
cargo run -p test262-runner --release -- test262
```

A pasta `test262/` está no `.gitignore`. O runner mostra quantos testes passam; o número
é registrado a cada marco.
