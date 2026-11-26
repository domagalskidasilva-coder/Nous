# Nous

Navegador desktop minimalista de baixo consumo sobre o motor **Firefox Gecko 153 ESR**, com interface **Kanso Zen (Barra Única de 44px)**, páginas internas nativas de 0 MB de conteúdo, bloqueador de rastreadores O(1) e hibernação automática de abas.

> O repositório também mantém em `crates/` o experimento paralelo de motor próprio em Rust (`nous-app`, `nous-js`, `nous-html`, `nous-layout`, etc.) e o executável nativo fino `crates/nous-launcher` (`nous.exe`).

## Destaques do Nous Desktop (Gecko 153 ESR · Kanso Zen)

- **Barra única minimalista (44px)**: navegação, abas em pílula com ponto sálvia, omnibox central com paleta de comandos, leitura ao vivo de RAM, modo Zen (`F11`) e controles nativos de janela — com barra lateral vertical opcional (`Ctrl+B`).
- **4 superfícies curadas**: **Kanso** *(carvão fosco)*, **Washi** *(papel claro)*, **Sumi** *(OLED preto puro)* e **Stone** *(pedra quente)*, sincronizadas com o tema claro/escuro dos sites.
- **Leve por arquitetura**:
  - Páginas internas (`nous://nova-aba`, `nous://memoria`, `nous://config`, `nous://downloads`, `nous://historico`, `nous://favoritos`) renderizadas direto no chrome com **0 MB de processo de conteúdo**.
  - Hibernação automática de abas inativas e **restauração de sessão já hibernada (0 MB até o clique)**.
  - Bloqueador nativo de rastreadores O(1) com allowlist por domínio.
  - Zero telemetria, zero runtime ML/ONNX, zero SafeBrowsing DBs, zero SQLite Places/FormHistory/Signons e zero processos pré-alocados.
  - Consumo verificado: **~83 MB PrivWS** na nova aba e **~106 MB PrivWS** em `https://example.com`.
- **Navegador completo para uso diário**: menu de contexto web (botão direito), favicons reais, prévia de links no hover, `Ctrl+Clique` / clique do meio, `<select>` / seletores nativos, tela cheia HTML5, downloads nativos (`Downloads.sys.mjs`), PDF nativo, impressão/Salvar PDF (`Ctrl+P`), modo leitura (`Ctrl+Alt+R`), múltiplas janelas (`Ctrl+N`), janela privada (`Ctrl+Shift+P`), limpeza de dados (`Ctrl+Shift+Del`), arrastar abas para reordenar e importar/exportar favoritos HTML.

## Instalar o executável (`nous.exe`) no Windows

Requer Rust estável + Visual Studio Build Tools (usado automaticamente via `scripts/cargo.ps1`) e o runtime Firefox 153 ESR em `target/nous-gecko-runtime/core` (ou instalado no sistema).

```powershell
.\scripts\instalar-nous.ps1
```

Isso compila `nous.exe` com ícone Kanso, empacota o runtime Gecko enxuto (~80 MB menor em disco) em `%LOCALAPPDATA%\Programs\Nous\nous.exe`, cria atalhos no **Menu Iniciar** e na **Área de Trabalho** e registra o Nous em **Configurações → Aplicativos padrão**.

Para rodar direto do repositório sem instalar:

```powershell
.\scripts\nous-gecko.ps1
```

Para medir RAM e CPU ocioso:

```powershell
.\scripts\medir-gecko.ps1
.\scripts\medir-gecko.ps1 -Url https://example.com
```

## Atalhos principais

| Atalho | Ação |
|---|---|
| `Ctrl+T` / `Ctrl+W` / `Ctrl+Shift+T` | Nova aba / Fechar aba / Reabrir fechada |
| `Ctrl+N` / `Ctrl+Shift+P` | Nova janela / Nova janela privada |
| `Ctrl+L` / `Ctrl+Shift+A` | Barra de endereço / Paleta rápida de abas |
| `Ctrl+B` / `F11` | Barra lateral / Modo Zen |
| `Ctrl+J` / `Ctrl+H` / `Ctrl+Shift+O` | Downloads / Histórico / Favoritos |
| `Ctrl+D` / `Ctrl+F` / `Ctrl+P` / `Ctrl+S` | Favoritar / Buscar / Imprimir PDF / Salvar página |
| `Ctrl+Alt+R` / `Ctrl+U` / `F12` | Modo Leitura / Código-fonte / Console DevTools |
| `Ctrl+Shift+M` / `Ctrl+,` / `Ctrl+Shift+Del` | Monitor de Memória / Preferências / Limpar dados |

## Motor experimental em Rust (`crates/`)

```powershell
.\scripts\cargo.ps1 run --release
.\scripts\cargo.ps1 test --workspace
```

Detalhes da arquitetura Rust experimental em [docs/arquitetura.md](docs/arquitetura.md) e benchmarks em [docs/benchmarks.md](docs/benchmarks.md).
