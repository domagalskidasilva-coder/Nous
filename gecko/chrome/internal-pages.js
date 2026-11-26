"use strict";

// Nous Internal Pages — Kanso Zen Edition (nous://nova-aba, nous://memoria, nous://config)
// Rendered directly inside the chrome window with ZERO content-process RAM.

(() => {
  const HTML_NS = "http://www.w3.org/1999/xhtml";
  const SVG_NS = "http://www.w3.org/2000/svg";

  function el(tag, props = {}, ...children) {
    const node = document.createElementNS(HTML_NS, tag);
    for (const [k, v] of Object.entries(props)) {
      if (v === null || v === undefined || v === false) continue;
      if (k === "className") {
        node.className = v;
      } else if (k === "textContent") {
        node.textContent = v;
      } else if (k === "style" && typeof v === "object") {
        Object.assign(node.style, v);
      } else if (k.startsWith("on") && typeof v === "function") {
        node.addEventListener(k.slice(2).toLowerCase(), v);
      } else {
        node.setAttribute(k, String(v));
      }
    }
    for (const child of children.flat()) {
      if (child === null || child === undefined || child === false) continue;
      node.append(typeof child === "string" ? document.createTextNode(child) : child);
    }
    return node;
  }

  function searchIconSvg() {
    const svg = document.createElementNS(SVG_NS, "svg");
    svg.setAttribute("viewBox", "0 0 18 18");
    svg.setAttribute("fill", "none");
    svg.setAttribute("stroke", "currentColor");
    svg.setAttribute("stroke-width", "1.35");
    svg.setAttribute("stroke-linecap", "round");
    svg.setAttribute("aria-hidden", "true");
    const c = document.createElementNS(SVG_NS, "circle");
    c.setAttribute("cx", "8");
    c.setAttribute("cy", "8");
    c.setAttribute("r", "5");
    const l = document.createElementNS(SVG_NS, "line");
    l.setAttribute("x1", "11.8");
    l.setAttribute("y1", "11.8");
    l.setAttribute("x2", "15.2");
    l.setAttribute("y2", "15.2");
    svg.append(c, l);
    return svg;
  }

  function hostOf(url) {
    try {
      return new URL(url).hostname.replace(/^www\./, "");
    } catch {
      return url;
    }
  }

  function formatKansoClock() {
    const now = new Date();
    const time = now.toLocaleTimeString("pt-BR", { hour: "2-digit", minute: "2-digit" });
    const weekday = now.toLocaleDateString("pt-BR", { weekday: "short" }).replace(/\./g, "");
    const day = now.toLocaleDateString("pt-BR", { day: "2-digit", month: "long" });
    return `${time} · ${weekday}, ${day}`;
  }

  // -------------------------------------------------------------------------
  // 1. New Tab Page (nous://nova-aba) — Kanso Zen Ma (Negative Space)
  // -------------------------------------------------------------------------
  function renderNewTab(container, api) {
    const stats = api.getStats();
    const cfg = api.getConfig();
    let addOpen = false;

    const input = el("input", {
      className: "kanso-search-input",
      type: "text",
      placeholder: "Para onde você quer ir?",
      autocomplete: "off",
      spellcheck: "false",
    });

    const form = el(
      "form",
      {
        className: "kanso-search-form",
        onSubmit: event => {
          event.preventDefault();
          const val = input.value.trim();
          if (val) {
            api.navigateActive(val);
          }
        },
      },
      searchIconSvg(),
      input,
      el("button", {
        className: "kanso-search-submit",
        type: "submit",
        title: "Abrir ou pesquisar (Enter)",
        textContent: "↵",
      })
    );

    const bookmarks = api.getSpeedDial();
    const addFormHost = el("div", { style: { width: "100%" } });

    function renderAddForm() {
      if (!addOpen) {
        addFormHost.replaceChildren();
        return;
      }
      const titleInput = el("input", {
        className: "kanso-add-input",
        type: "text",
        placeholder: "Nome (ex: Linear)",
      });
      const urlInput = el("input", {
        className: "kanso-add-input",
        type: "text",
        placeholder: "Endereço (ex: linear.app)",
      });
      const addForm = el(
        "form",
        {
          className: "kanso-add-form",
          onSubmit: e => {
            e.preventDefault();
            const rawUrl = urlInput.value.trim();
            if (!rawUrl) return;
            const normalized = /^https?:\/\//i.test(rawUrl) ? rawUrl : `https://${rawUrl}`;
            api.addBookmark(titleInput.value.trim() || hostOf(normalized), normalized);
            renderNewTab(container, api);
          },
        },
        titleInput,
        urlInput,
        el("button", { className: "btn-primary", type: "submit", textContent: "Salvar" }),
        el("button", {
          className: "btn-ghost",
          type: "button",
          textContent: "Cancelar",
          onClick: () => {
            addOpen = false;
            renderAddForm();
          },
        })
      );
      addFormHost.replaceChildren(addForm);
      titleInput.focus();
    }

    const linkNodes = bookmarks.map(item => {
      const host = hostOf(item.url);
      const removeBtn = el("button", {
        className: "kanso-link-remove",
        type: "button",
        title: "Remover atalho",
        textContent: "×",
        onClick: e => {
          e.stopPropagation();
          api.removeBookmark(item.url);
          renderNewTab(container, api);
        },
      });
      return el(
        "button",
        {
          className: "kanso-link-item",
          type: "button",
          title: host,
          onClick: () => api.navigateActive(item.url),
        },
        el("span", { textContent: item.title || host }),
        el("span", { className: "kanso-link-arrow", textContent: "↗" }),
        removeBtn
      );
    });

    linkNodes.push(
      el("button", {
        className: "kanso-add-trigger",
        type: "button",
        title: "Adicionar novo atalho",
        textContent: "+",
        onClick: () => {
          addOpen = !addOpen;
          renderAddForm();
        },
      })
    );

    const googleSigned = Boolean(api.isGoogleSignedIn && api.isGoogleSignedIn());

    const topBar = el(
      "div",
      { className: "kanso-topbar" },
      el(
        "button",
        {
          className: `kanso-account-pill${googleSigned ? " connected" : ""}`,
          type: "button",
          title: googleSigned
            ? "Sua Conta Google está conectada — clique para gerenciar"
            : "Entrar na sua Conta Google diretamente sem precisar abrir outro site",
          onClick: () => api.openGoogleAccount(),
        },
        el("span", { className: "kanso-account-dot" }),
        el("span", { textContent: googleSigned ? "Conta Google" : "Entrar · Google ↗" })
      )
    );

    const footer = el(
      "footer",
      { className: "kanso-footer" },
      el("span", { textContent: `${stats.engineLabel} · SINGLE-BAR CHROME` }),
      el(
        "div",
        { className: "kanso-footer-right" },
        el("button", {
          className: "kanso-foot-link",
          type: "button",
          textContent: cfg.theme === "washi" ? "Kanso" : "Washi",
          title: "Alternar entre superfície escura e clara",
          onClick: () => {
            api.setTheme(cfg.theme === "washi" ? "kanso" : "washi");
            renderNewTab(container, api);
          },
        }),
        el("span", { className: "kanso-foot-sep", textContent: "·" }),
        el("button", {
          className: "kanso-foot-link",
          type: "button",
          textContent: "Histórico",
          onClick: () => api.navigateActive("nous://historico"),
        }),
        el("span", { className: "kanso-foot-sep", textContent: "·" }),
        el("button", {
          className: "kanso-foot-link",
          type: "button",
          textContent: "Downloads",
          onClick: () => api.navigateActive("nous://downloads"),
        }),
        el("span", { className: "kanso-foot-sep", textContent: "·" }),
        el("button", {
          className: "kanso-foot-link",
          type: "button",
          textContent: "Memória",
          onClick: () => api.navigateActive("nous://memoria"),
        }),
        el("span", { className: "kanso-foot-sep", textContent: "·" }),
        el("button", {
          className: "kanso-foot-link",
          type: "button",
          textContent: "Preferências",
          onClick: () => api.navigateActive("nous://config"),
        })
      )
    );

    const shell = el(
      "div",
      { className: "kanso-shell" },
      topBar,
      el(
        "div",
        { className: "kanso-center" },
        el(
          "div",
          { className: "kanso-mark" },
          el("h1", { className: "kanso-title", textContent: "nous" }),
          el("div", { className: "kanso-sub", "data-live-clock": "true", textContent: formatKansoClock() })
        ),
        form,
        el(
          "div",
          { className: "kanso-links-wrap" },
          el("nav", { className: "kanso-links", "aria-label": "Acesso rápido" }, linkNodes),
          addFormHost
        )
      ),
      footer
    );

    container.replaceChildren(shell);
  }

  // -------------------------------------------------------------------------
  // 2. Memory & Efficiency Monitor (nous://memoria)
  // -------------------------------------------------------------------------
  function renderMemory(container, api) {
    const stats = api.getStats();
    const tabs = api.getTabList();
    const perfNames = { eco: "Eco", balanced: "Equilibrado", turbo: "Turbo" };

    const header = el(
      "div",
      { className: "dash-header" },
      el(
        "div",
        {},
        el("div", { className: "dash-kicker", textContent: `Telemetria Local · ${stats.engineLabel}` }),
        el("h1", { className: "dash-title", textContent: "Memória" }),
        el("p", {
          className: "dash-subtitle",
          textContent: "Consumo residente do processo Gecko, hibernação sob demanda e arquitetura enxuta.",
        })
      ),
      el(
        "div",
        { className: "dash-actions" },
        el("button", {
          className: "btn-primary",
          type: "button",
          textContent: "Otimizar RAM",
          onClick: () => api.optimizeMemory().then(() => renderMemory(container, api)),
        }),
        el("button", {
          className: "btn-ghost",
          type: "button",
          textContent: "Hibernar inativas",
          onClick: () => {
            api.hibernateInactiveTabs();
            renderMemory(container, api);
          },
        }),
        el("button", {
          className: "btn-ghost",
          type: "button",
          textContent: "Atualizar",
          onClick: () => {
            api.refreshMemoryNow();
            renderMemory(container, api);
          },
        })
      )
    );

    const metricStrip = el(
      "div",
      { className: "metric-strip" },
      el(
        "div",
        { className: "metric-cell" },
        el("div", { className: "kpi-label", textContent: "01 · RAM Residente" }),
        el("div", { className: "kpi-value", "data-live-ram": "true", textContent: stats.ramLabel }),
        el("div", { className: "kpi-sub", textContent: "Processo principal via Gecko MemoryReporter" })
      ),
      el(
        "div",
        { className: "metric-cell" },
        el("div", { className: "kpi-label", textContent: "02 · Abas Web" }),
        el("div", { className: "kpi-value", textContent: `${stats.liveTabs} / ${stats.totalTabs}` }),
        el("div", {
          className: "kpi-sub",
          textContent: `${stats.sleepingTabs} hibernando · ${stats.internalTabs} internas (0 MB)`,
        })
      ),
      el(
        "div",
        { className: "metric-cell" },
        el("div", { className: "kpi-label", textContent: "03 · Rastreadores" }),
        el("div", { className: "kpi-value", textContent: String(stats.blockedTotal) }),
        el("div", { className: "kpi-sub", textContent: `${stats.blockerDomains} domínios + heurísticas O(1)` })
      ),
      el(
        "div",
        { className: "metric-cell" },
        el("div", { className: "kpi-label", textContent: "04 · Perfil" }),
        el("div", { className: "kpi-value", textContent: perfNames[stats.perfProfile] || stats.perfProfile }),
        el("div", {
          className: "kpi-sub",
          textContent: stats.hibernateMinutes > 0
            ? `Auto-hiberna após ${stats.hibernateMinutes} min`
            : "Auto-hibernação desativada",
        })
      )
    );

    const tabRows = tabs.map(tab => {
      let stateClass = "live";
      let stateLabel = "Em memória";
      if (tab.isActive) {
        stateClass = "active";
        stateLabel = tab.isInternal ? "Ativa · 0 MB" : "Ativa";
      } else if (tab.isInternal) {
        stateClass = "internal";
        stateLabel = "Interna · 0 MB";
      } else if (tab.sleeping) {
        stateClass = "sleeping";
        stateLabel = "Hibernando · 0 MB";
      }

      const btns = el("div", { className: "tab-mgr-btns" });
      if (!tab.isActive) {
        btns.append(
          el("button", {
            className: "mini-action",
            type: "button",
            textContent: "Abrir",
            onClick: () => api.selectTabById(tab.id),
          })
        );
      }
      if (!tab.isActive && !tab.isInternal) {
        btns.append(
          el("button", {
            className: "mini-action",
            type: "button",
            textContent: tab.sleeping ? "Acordar" : "Hibernar",
            onClick: () => {
              if (tab.sleeping) api.wakeTabById(tab.id);
              else api.hibernateTabById(tab.id);
              renderMemory(container, api);
            },
          })
        );
      }
      btns.append(
        el("button", {
          className: "mini-action danger",
          type: "button",
          textContent: "Fechar",
          onClick: () => {
            api.closeTabById(tab.id);
            renderMemory(container, api);
          },
        })
      );

      return el(
        "div",
        { className: "tab-mgr-row" },
        el(
          "div",
          { className: "tab-mgr-info" },
          el("div", { className: "tab-mgr-title", textContent: tab.title }),
          el("div", {
            className: "tab-mgr-url mono",
            textContent: tab.blocked > 0 ? `${tab.url} · ${tab.blocked} bloqueados` : tab.url,
          })
        ),
        el("span", { className: `state-tag ${stateClass}`, textContent: stateLabel }),
        btns
      );
    });

    const specs = [
      ["Processos auxiliares (GPU / Socket / Utility)", "DESATIVADOS"],
      ["Pré-alocação de processos ociosos", "0 PROCESSOS"],
      ["Páginas internas (nous://)", "0 MB CONTEÚDO"],
      ["Cache de memória HTTP", "12 MB MÁX."],
      ["Cache de superfícies de imagem", "16 MB DESCARTÁVEL"],
      ["BFCache de histórico por aba", "1 PÁGINA"],
      ["Descarte de camadas GPU em 2º plano", "IMEDIATO"],
      ["Telemetria, ML/ONNX e SafeBrowsing DB", "REMOVIDOS"],
      ["Places SQLite / FormHistory / Signons", "DESATIVADOS"],
      ["Bloqueio nativo de rastreadores", "ATIVO · O(1)"],
    ];

    const wrap = el(
      "div",
      { className: "page-wrap" },
      header,
      metricStrip,
      el(
        "section",
        { className: "editorial-section" },
        el(
          "div",
          { className: "editorial-section-head" },
          el("h2", { textContent: "01 · Gerenciador de Abas" }),
          el("span", { className: "editorial-section-count", textContent: `${tabs.length} ${tabs.length === 1 ? "ABA" : "ABAS"}` })
        ),
        el("div", { className: "tab-mgr-list" }, tabRows)
      ),
      el(
        "section",
        { className: "editorial-section" },
        el(
          "div",
          { className: "editorial-section-head" },
          el("h2", { textContent: "02 · Especificações do Motor" }),
          el("span", { className: "editorial-section-count", textContent: stats.engineLabel })
        ),
        el(
          "div",
          { className: "spec-grid" },
          specs.map(([k, v]) =>
            el("div", { className: "spec-row" }, el("span", { className: "spec-key", textContent: k }), el("span", { className: "spec-val", textContent: v }))
          )
        )
      )
    );

    container.replaceChildren(wrap);
  }

  // -------------------------------------------------------------------------
  // 3. Preferences (nous://config)
  // -------------------------------------------------------------------------
  function renderConfig(container, api) {
    const cfg = api.getConfig();
    const stats = api.getStats();

    const makeSegment = (items, currentValue, onSelect) =>
      el(
        "div",
        { className: "segmented" },
        items.map(item =>
          el(
            "button",
            {
              className: `seg-btn${item.value === currentValue ? " active" : ""}`,
              type: "button",
              onClick: () => {
                onSelect(item.value);
                renderConfig(container, api);
              },
            },
            item.swatch ? el("span", { className: "seg-swatch", style: { background: item.swatch } }) : null,
            item.label
          )
        )
      );

    const makeRow = (name, desc, control) =>
      el(
        "div",
        { className: "setting-row" },
        el(
          "div",
          { className: "setting-meta" },
          el("div", { className: "setting-name", textContent: name }),
          el("div", { className: "setting-desc", textContent: desc })
        ),
        control
      );

    const shortcuts = [
      ["Nova aba", "Ctrl+T"],
      ["Fechar aba", "Ctrl+W"],
      ["Reabrir aba fechada", "Ctrl+Shift+T"],
      ["Nova janela", "Ctrl+N"],
      ["Janela privada", "Ctrl+Shift+P"],
      ["Barra de endereço", "Ctrl+L"],
      ["Paleta de abas", "Ctrl+Shift+A"],
      ["Localizar na página", "Ctrl+F"],
      ["Favoritar página", "Ctrl+D"],
      ["Favoritos", "Ctrl+Shift+O"],
      ["Histórico", "Ctrl+H"],
      ["Downloads", "Ctrl+J"],
      ["Alternar barra lateral", "Ctrl+B"],
      ["Modo Zen", "F11"],
      ["Modo Leitura", "Ctrl+Alt+R"],
      ["Imprimir / PDF", "Ctrl+P"],
      ["Salvar página", "Ctrl+S"],
      ["Ver código-fonte", "Ctrl+U"],
      ["Limpar dados", "Ctrl+Shift+Del"],
      ["Recarregar / Forçar", "F5 / Ctrl+Shift+R"],
      ["Voltar / Avançar", "Alt+← / Alt+→"],
      ["Zoom + / − / 100%", "Ctrl++ / − / 0"],
      ["Monitor de Memória", "Ctrl+Shift+M"],
      ["Preferências", "Ctrl+,"],
    ];

    const wrap = el(
      "div",
      { className: "page-wrap" },
      el(
        "div",
        { className: "dash-header" },
        el(
          "div",
          {},
          el("div", { className: "dash-kicker", textContent: `Nous Desktop · ${stats.engineLabel}` }),
          el("h1", { className: "dash-title", textContent: "Preferências" }),
          el("p", {
            className: "dash-subtitle",
            textContent: "Superfície visual, economia de memória, privacidade e comportamento de navegação.",
          })
        ),
        el(
          "div",
          { className: "dash-actions" },
          el("button", {
            className: "btn-ghost",
            type: "button",
            textContent: "Monitor de Memória ↗",
            onClick: () => api.navigateActive("nous://memoria"),
          })
        )
      ),
      el(
        "section",
        { className: "editorial-section" },
        el(
          "div",
          { className: "editorial-section-head" },
          el("h2", { textContent: "01 · Superfície & Layout" }),
          el("span", { className: "editorial-section-count", textContent: "INTERFACE" })
        ),
        makeRow(
          "Superfície",
          "Paleta fosca silenciosa, sincronizada com o tema escuro/claro dos sites.",
          makeSegment(
            [
              { value: "kanso", label: "Kanso", swatch: "#141413" },
              { value: "washi", label: "Washi", swatch: "#f5f2ec" },
              { value: "sumi", label: "Sumi", swatch: "#000000" },
              { value: "stone", label: "Stone", swatch: "#2c2c29" },
            ],
            cfg.theme,
            v => api.setTheme(v)
          )
        ),
        makeRow(
          "Modo escuro em sites",
          "Mantém todos os sites escuros automaticamente quando você usa um tema escuro.",
          makeSegment(
            [
              { value: true, label: "Auto" },
              { value: false, label: "Desligado" },
            ],
            cfg.forceDarkWeb,
            v => api.setForceDarkWeb(v)
          )
        ),
        makeRow(
          "Layout das abas",
          "Barra única minimalista de 44px ou barra lateral recolhível (Ctrl+B).",
          makeSegment(
            [
              { value: "top", label: "Barra Única" },
              { value: "sidebar", label: "Lateral" },
            ],
            cfg.layout,
            v => api.setLayout(v)
          )
        ),
        makeRow(
          "Densidade",
          "Ajusta a altura da barra superior para ganhar mais área útil.",
          makeSegment(
            [
              { value: "compact", label: "Compacta" },
              { value: "normal", label: "Padrão" },
            ],
            cfg.density,
            v => api.setDensity(v)
          )
        ),
        makeRow(
          "Transições sutis",
          "Desative para resposta instantânea e menor trabalho de composição.",
          makeSegment(
            [
              { value: true, label: "Ativas" },
              { value: false, label: "Desligadas" },
            ],
            cfg.animations,
            v => api.setAnimations(v)
          )
        )
      ),
      el(
        "section",
        { className: "editorial-section" },
        el(
          "div",
          { className: "editorial-section-head" },
          el("h2", { textContent: "02 · Leveza, Privacidade & Busca" }),
          el("span", { className: "editorial-section-count", textContent: "MOTOR" })
        ),
        makeRow(
          "Perfil de desempenho",
          "Controla limites de cache, processos de conteúdo e hibernação automática.",
          makeSegment(
            [
              { value: "eco", label: "Eco" },
              { value: "balanced", label: "Equilibrado" },
              { value: "turbo", label: "Turbo" },
            ],
            cfg.perfProfile,
            v => api.setPerfProfile(v)
          )
        ),
        makeRow(
          "Hibernar abas inativas",
          "Descarrega abas em segundo plano para zerar o consumo de RAM.",
          makeSegment(
            [
              { value: 1, label: "1m" },
              { value: 2, label: "2m" },
              { value: 5, label: "5m" },
              { value: 15, label: "15m" },
              { value: 0, label: "Nunca" },
            ],
            cfg.hibernateMinutes,
            v => api.setHibernateMinutes(v)
          )
        ),
        makeRow(
          "Bloqueador de rastreadores",
          "Cancela scripts de anúncios e telemetria antes da requisição de rede.",
          makeSegment(
            [
              { value: true, label: "Ativo" },
              { value: false, label: "Pausado" },
            ],
            cfg.blockerEnabled,
            v => api.setBlockerEnabled(v)
          )
        ),
        makeRow(
          "Buscador padrão",
          "Mecanismo utilizado ao digitar termos diretamente na busca.",
          makeSegment(
            [
              { value: "duckduckgo", label: "DuckDuckGo" },
              { value: "google", label: "Google" },
              { value: "brave", label: "Brave" },
              { value: "startpage", label: "Startpage" },
            ],
            cfg.searchEngine,
            v => api.setSearchEngine(v)
          )
        )
      ),
      el(
        "section",
        { className: "editorial-section" },
        el(
          "div",
          { className: "editorial-section-head" },
          el("h2", { textContent: "03 · Atalhos de Teclado" }),
          el("span", { className: "editorial-section-count", textContent: `${shortcuts.length} ATALHOS` })
        ),
        el(
          "div",
          { className: "shortcut-grid" },
          shortcuts.map(([name, keys]) =>
            el("div", { className: "shortcut-item" }, el("span", { textContent: name }), el("kbd", { textContent: keys }))
          )
        )
      )
    );

    container.replaceChildren(wrap);
  }

  // -------------------------------------------------------------------------
  // 4. Downloads (nous://downloads)
  // -------------------------------------------------------------------------
  function renderDownloads(container, api) {
    const snap = api.getDownloads();
    const stats = api.getStats();

    const rows = snap.items.length === 0
      ? [el("div", { className: "empty-state", textContent: "Nenhum download nesta sessão." })]
      : snap.items.map(dl => {
          const btns = el("div", { className: "tab-mgr-btns" });
          if (dl.state === "downloading" || dl.state === "paused") {
            btns.append(
              el("button", {
                className: "mini-action",
                type: "button",
                textContent: dl.state === "paused" ? "Retomar" : "Pausar",
                onClick: () => api.pauseOrResumeDownload(dl.id).then(() => renderDownloads(container, api)),
              }),
              el("button", {
                className: "mini-action danger",
                type: "button",
                textContent: "Cancelar",
                onClick: () => api.cancelDownload(dl.id).then(() => renderDownloads(container, api)),
              })
            );
          } else if (dl.state === "done") {
            btns.append(
              el("button", {
                className: "mini-action",
                type: "button",
                textContent: "Abrir",
                onClick: () => api.openDownloadFile(dl.id),
              }),
              el("button", {
                className: "mini-action",
                type: "button",
                textContent: "Pasta",
                onClick: () => api.showDownloadInFolder(dl.id),
              })
            );
          }

          const metaText = [
            dl.sizeLabel,
            dl.speedLabel,
            dl.state === "done" ? "Concluído" : dl.state === "paused" ? "Pausado" : dl.state === "error" ? "Erro" : `${dl.percent}%`,
          ].filter(Boolean).join(" · ");

          return el(
            "div",
            { className: "tab-mgr-row" },
            el(
              "div",
              { className: "tab-mgr-info" },
              el("div", { className: "tab-mgr-title", textContent: dl.name }),
              el("div", { className: "tab-mgr-url mono", textContent: metaText }),
              dl.state === "downloading"
                ? el("div", { className: "dl-bar", style: { marginTop: "6px" } }, el("div", { className: "dl-fill", style: { width: `${dl.percent}%` } }))
                : null
            ),
            btns
          );
        });

    const wrap = el(
      "div",
      { className: "page-wrap" },
      el(
        "div",
        { className: "dash-header" },
        el(
          "div",
          {},
          el("div", { className: "dash-kicker", textContent: `Arquivos · ${stats.engineLabel}` }),
          el("h1", { className: "dash-title", textContent: "Downloads" }),
          el("p", { className: "dash-subtitle", textContent: "Gerencie transferências ativas e arquivos salvos localmente." })
        ),
        el(
          "div",
          { className: "dash-actions" },
          el("button", {
            className: "btn-ghost",
            type: "button",
            textContent: "Limpar concluídos",
            onClick: () => api.clearFinishedDownloads().then(() => renderDownloads(container, api)),
          })
        )
      ),
      el(
        "section",
        { className: "editorial-section" },
        el(
          "div",
          { className: "editorial-section-head" },
          el("h2", { textContent: "01 · Transferências" }),
          el("span", { className: "editorial-section-count", textContent: `${snap.items.length} ITENS` })
        ),
        el("div", { className: "tab-mgr-list" }, rows)
      )
    );
    container.replaceChildren(wrap);
  }

  // -------------------------------------------------------------------------
  // 5. History (nous://historico)
  // -------------------------------------------------------------------------
  function renderHistory(container, api, filterQuery = "") {
    const all = api.getHistory();
    const q = filterQuery.trim().toLowerCase();
    const filtered = q
      ? all.filter(h => (h.title || "").toLowerCase().includes(q) || (h.url || "").toLowerCase().includes(q))
      : all;

    const searchInput = el("input", {
      className: "filter-input",
      type: "text",
      placeholder: "Filtrar histórico por título ou endereço…",
      value: filterQuery,
      onInput: e => {
        const pos = e.target.selectionStart;
        renderHistory(container, api, e.target.value);
        const next = container.querySelector(".filter-input");
        if (next) {
          next.focus();
          try { next.setSelectionRange(pos, pos); } catch {}
        }
      },
    });

    const rows = filtered.length === 0
      ? [el("div", { className: "empty-state", textContent: q ? "Nenhum resultado encontrado." : "Histórico vazio." })]
      : filtered.map(h => {
          const dt = h.ts ? new Date(h.ts).toLocaleString("pt-BR", { day: "2-digit", month: "2-digit", hour: "2-digit", minute: "2-digit" }) : "";
          return el(
            "div",
            { className: "tab-mgr-row" },
            el(
              "div",
              { className: "tab-mgr-info", style: { cursor: "pointer" }, onClick: () => api.navigateActive(h.url) },
              el("div", { className: "tab-mgr-title", textContent: h.title || hostOf(h.url) }),
              el("div", { className: "tab-mgr-url mono", textContent: h.url })
            ),
            el("span", { className: "editorial-section-count", textContent: dt }),
            el(
              "div",
              { className: "tab-mgr-btns" },
              el("button", { className: "mini-action", type: "button", textContent: "Abrir", onClick: () => api.navigateActive(h.url) }),
              el("button", {
                className: "mini-action danger",
                type: "button",
                textContent: "Remover",
                onClick: () => {
                  api.removeHistoryUrl(h.url);
                  renderHistory(container, api, filterQuery);
                },
              })
            )
          );
        });

    const wrap = el(
      "div",
      { className: "page-wrap" },
      el(
        "div",
        { className: "dash-header" },
        el(
          "div",
          {},
          el("div", { className: "dash-kicker", textContent: "Navegação Local · JSON Enxuto" }),
          el("h1", { className: "dash-title", textContent: "Histórico" }),
          el("p", { className: "dash-subtitle", textContent: "Páginas visitadas recentemente, armazenadas localmente sem SQLite pesado." })
        ),
        el(
          "div",
          { className: "dash-actions" },
          el("button", {
            className: "btn-ghost",
            type: "button",
            textContent: "Limpar tudo",
            onClick: () => {
              api.clearHistory();
              renderHistory(container, api, "");
            },
          })
        )
      ),
      el("div", { className: "filter-bar" }, searchInput),
      el(
        "section",
        { className: "editorial-section" },
        el(
          "div",
          { className: "editorial-section-head" },
          el("h2", { textContent: "01 · Visitas Recentes" }),
          el("span", { className: "editorial-section-count", textContent: `${filtered.length} ITENS` })
        ),
        el("div", { className: "tab-mgr-list" }, rows)
      )
    );
    container.replaceChildren(wrap);
  }

  // -------------------------------------------------------------------------
  // 6. Bookmarks (nous://favoritos) — with Chrome/Firefox HTML Import & Export
  // -------------------------------------------------------------------------
  function renderBookmarks(container, api, filterQuery = "") {
    const all = api.getAllBookmarks();
    const q = filterQuery.trim().toLowerCase();
    const filtered = q
      ? all.filter(b => (b.title || "").toLowerCase().includes(q) || (b.url || "").toLowerCase().includes(q))
      : all;

    const searchInput = el("input", {
      className: "filter-input",
      type: "text",
      placeholder: "Buscar favorito…",
      value: filterQuery,
      onInput: e => {
        const pos = e.target.selectionStart;
        renderBookmarks(container, api, e.target.value);
        const next = container.querySelector(".filter-input");
        if (next) {
          next.focus();
          try { next.setSelectionRange(pos, pos); } catch {}
        }
      },
    });

    const rows = filtered.length === 0
      ? [el("div", { className: "empty-state", textContent: "Nenhum favorito encontrado." })]
      : filtered.map(b =>
          el(
            "div",
            { className: "tab-mgr-row" },
            el(
              "div",
              { className: "tab-mgr-info", style: { cursor: "pointer" }, onClick: () => api.navigateActive(b.url) },
              el("div", { className: "tab-mgr-title", textContent: b.title || hostOf(b.url) }),
              el("div", { className: "tab-mgr-url mono", textContent: b.url })
            ),
            el(
              "div",
              { className: "tab-mgr-btns" },
              el("button", { className: "mini-action", type: "button", textContent: "Abrir", onClick: () => api.navigateActive(b.url) }),
              el("button", {
                className: "mini-action danger",
                type: "button",
                textContent: "Remover",
                onClick: () => {
                  api.removeBookmark(b.url);
                  renderBookmarks(container, api, filterQuery);
                },
              })
            )
          )
        );

    const wrap = el(
      "div",
      { className: "page-wrap" },
      el(
        "div",
        { className: "dash-header" },
        el(
          "div",
          {},
          el("div", { className: "dash-kicker", textContent: "Coleção Pessoal" }),
          el("h1", { className: "dash-title", textContent: "Favoritos" }),
          el("p", { className: "dash-subtitle", textContent: "Importe ou exporte no formato HTML compatível com Chrome, Edge e Firefox." })
        ),
        el(
          "div",
          { className: "dash-actions" },
          el("button", {
            className: "btn-ghost",
            type: "button",
            textContent: "Importar HTML",
            onClick: () => api.importBookmarksHtml().then(() => renderBookmarks(container, api, filterQuery)),
          }),
          el("button", {
            className: "btn-ghost",
            type: "button",
            textContent: "Exportar HTML",
            onClick: () => api.exportBookmarksHtml(),
          })
        )
      ),
      el("div", { className: "filter-bar" }, searchInput),
      el(
        "section",
        { className: "editorial-section" },
        el(
          "div",
          { className: "editorial-section-head" },
          el("h2", { textContent: "01 · Salvos" }),
          el("span", { className: "editorial-section-count", textContent: `${filtered.length} ITENS` })
        ),
        el("div", { className: "tab-mgr-list" }, rows)
      )
    );
    container.replaceChildren(wrap);
  }

  window.NousInternalPages = {
    formatKansoClock,
    render(route, container, api) {
      if (route === "nous://memoria") {
        renderMemory(container, api);
      } else if (route === "nous://config") {
        renderConfig(container, api);
      } else if (route === "nous://downloads") {
        renderDownloads(container, api);
      } else if (route === "nous://historico") {
        renderHistory(container, api);
      } else if (route === "nous://favoritos") {
        renderBookmarks(container, api);
      } else {
        renderNewTab(container, api);
      }
    },
  };
})();
