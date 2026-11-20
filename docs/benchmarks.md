# Medições de memória e desempenho

Cada marco acrescenta uma linha às tabelas abaixo.

## Como medir

- Build release (`cargo build --release`), máquina sem outros programas pesados abertos.
- RAM = soma do **conjunto de trabalho privado** (o número da coluna "Memória" do
  Gerenciador de Tarefas) de todos os processos do navegador:
  `Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object Name -like 'nous*' | Measure-Object WorkingSetPrivate -Sum`.
- Esperar 30 s depois de abrir as abas antes de medir.
- Mesmo conjunto de páginas nos dois navegadores, com o Chrome sem extensões e num perfil novo.

## Páginas de teste

1. `https://example.com/`
2. `https://pt.wikipedia.org/wiki/Rust_(linguagem_de_programação)`
3. `https://doc.rust-lang.org/book/`
4. … (completar até 10 páginas para o teste com 10 abas)

## Resultados

Máquina da primeira medição: Windows 11, AMD Radeon RX 580 + Vega 11, Chrome em perfil novo.
Medido 8–10 s depois de abrir.

| Data | Versão do Nous | Cenário | Nous (MB) | Chrome (MB) | Observações |
|---|---|---|---|---|---|
| 2026-11-20 | 0.1.0 | 1 aba (example.com) | 54 (1 processo) | 165 (10 processos) | Nous via Vulkan |
| 2026-11-20 | 0.1.0 | 1 aba (Wikipédia: Rust) | 83 (1 processo) | 226 (8 processos) | Nous sem JS e sem imagens ainda |
| 2026-11-20 | 0.1.0 | 1 aba (example.com), backend DX12 | 182 | — | por isso o Vulkan é o padrão no Windows |
| — | — | 10 abas | — | — | |

A comparação ainda favorece o Nous injustamente: ele não executa JavaScript nas páginas
nem decodifica imagens. Os números servem para acompanhar a evolução, não como veredito.

## Outros números

| Data | Versão | Binário release | CPU parado | Wikipédia (parse / estilo / layout) | test262 |
|---|---|---|---|---|---|
| 2026-11-20 | 0.1.0 | 14 MB | 0 ms em 3 s | 37 ms / 95 ms / 310 ms (build dev) | só fixtures (5/5); oficial ainda não rodado |
