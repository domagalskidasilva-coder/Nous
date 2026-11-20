# Arquitetura do Nous

Objetivo: um navegador com motor próprio, em Rust, que use o mínimo de RAM e CPU, com
interface no estilo dos navegadores modernos (abas no topo, omnibox) e identidade própria.

## Pilha

Uma única pilha gráfica serve a interface e as páginas: o mesmo dispositivo de GPU, o
mesmo renderizador e o mesmo sistema de fontes. Nada é carregado em dobro.

| Função | Peça |
|---|---|
| Janela e eventos | `winit` |
| GPU | `wgpu` (DirectX 12 / Vulkan / Metal), via `vello` |
| Desenho 2D (interface e páginas) | `vello` (com `vello_cpu` como reserva para GPUs antigas) |
| Texto | `parley` + `fontique` + `skrifa` |
| Parser HTML | `html5ever` |
| CSS | cascata provisória própria → **Stylo** (o motor de CSS do Firefox) |
| Layout | blocos e texto próprios com `parley`; `taffy` para flex/grid |
| Rede | `reqwest`/`hyper` + `h2` + `rustls` (provedor `ring`); HTTP/3 com `h3`/`quinn` depois |
| Bloqueador | `adblock` (adblock-rust) — Fase 4 |
| Imagens | `image`, `zune-jpeg`, `png`, `image-webp`, `rav1d` |
| Internacionalização | `icu4x` |
| Acessibilidade | `accesskit` |
| Armazenamento | `redb` |
| JavaScript | `nous-js` (próprio), parser do `oxc` |

### Partes que não são Rust

- A criptografia por baixo do `rustls` (`ring`) tem trechos em C e assembly auditados.
  Trocar por criptografia 100% Rust seria um risco de segurança num produto publicado.
- Drivers de GPU do sistema.
- Decodificadores de vídeo do sistema (Fase 5).
- DRM Widevine (exige licença do Google).

### CSS: cascata provisória

A cascata atual em `nous-style` (seletores, especificidade, `!important`, `@media` por
largura, herança e um subconjunto de propriedades) existe para o motor desenhar páginas
desde já. O próximo passo é colocar o **Stylo** atrás da **mesma API** `ComputedStyle`:
layout e pintura não mudam quando a troca acontecer.

## Modelo de processos

- **Navegadores com Chromium**: processo do navegador + GPU + rede + utilitários + um
  renderizador por site. Dezenas de processos, cada um com seu custo fixo de memória.
- **Nous**:
  - Um processo principal: interface, rede e GPU/compositor rodando como threads.
  - Processos de conteúdo isolados (sandbox), agrupados por site, com limite
    (por padrão, perto do número de núcleos).
  - Modo opcional **"Mínimo de RAM"**: mais sites por processo, trocando um pouco de
    isolamento por memória.
- Hoje tudo roda num processo só, mas as fronteiras já são as mesmas (mensagens com a
  mesma API). Na Fase 4 isso vira multiprocesso sem reescrever o motor.

A rede usa um único runtime `tokio` com 2 threads, compartilhado por todas as abas, e um
pool pequeno de conexões ociosas (4 por servidor, 60 s).

## Técnicas de economia de RAM e CPU

- **Redesenho sob demanda**: sem animação nem carregamento, a janela não gasta CPU.
- **Pintar só o visível**: só o que cruza a área da janela entra na cena da GPU.
- **Abas em segundo plano**: liberam o layout; depois de um tempo o JavaScript e os
  timers congelam; sob pressão de memória a aba é descartada (guardando URL, rolagem e
  formulários).
- **Sessão restaurada sem carregar**: cada aba só carrega quando é aberta.
- **Bloqueador nativo** antes da rede (Fase 4).
- **Imagens** decodificadas no tamanho exibido; as que saem da tela são liberadas.
- **DOM em arena** com nomes de tags e atributos internados.
- **JavaScript**: valores de 8 bytes (NaN-boxing), compilação só quando a função roda,
  descarte de bytecode frio, shapes e GC geracional.
- **`nous://memoria`** mostra a RAM do processo e de cada aba.

## Motor JavaScript (`nous-js`)

1. **Parser**: `oxc_parser`. Daqui em diante, tudo é nosso.
2. **Compilador**: bytecode baseado em registradores (como Ignition/V8 e LLInt/JSC).
3. **Interpretador**: valores NaN-boxed; depois hidden classes (shapes) e inline caches.
4. **GC**: mark-sweep hoje; depois geracional (berçário + geração velha com mark-compact)
   e incremental.
5. **Funções nativas** guiadas pelo test262; regex via `regress`, Intl via ICU4X,
   Temporal via `temporal_rs`.
6. **Fase 5**: JIT baseline com **Cranelift** (Rust).

## Medição

- `cargo test --workspace` e CI nos três sistemas.
- test262 (`tools/test262-runner`) e, depois, web-platform-tests: porcentagem registrada a
  cada marco.
- RAM com 1 e 10 abas comparada a outros navegadores: [benchmarks.md](benchmarks.md).
