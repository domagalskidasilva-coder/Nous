"use strict";

// Nous Desktop Browser — Main Chrome Controller (Gecko 153 ESR)
// Ultra-lightweight architecture:
// - Internal nous:// views render natively in chrome (0 content-process RAM)
// - Web pages mount lazily in sandboxed remote <browser> elements
// - Background tabs drop GPU layers immediately and hibernate automatically
// - Native nsIBrowserDOMWindow, Finder, Zoom, Audio Mute, Tracker Blocker & Live RAM

(() => {
  const HTML_NS = "http://www.w3.org/1999/xhtml";
  const SVG_NS = "http://www.w3.org/2000/svg";
  const WEB_PROGRESS = Ci.nsIWebProgress;
  const WPL = Ci.nsIWebProgressListener;
  const SECURITY = Services.scriptSecurityManager;

  const INTERNAL_PAGES = {
    "nous://nova-aba": "Nova aba",
    "nous://memoria": "Memória & Leveza",
    "nous://config": "Preferências",
    "nous://downloads": "Downloads",
    "nous://historico": "Histórico",
    "nous://favoritos": "Favoritos",
  };

  let isPrivateWindow = false;
  try {
    const { PrivateBrowsingUtils } = ChromeUtils.importESModule("resource://gre/modules/PrivateBrowsingUtils.sys.mjs");
    isPrivateWindow = Boolean(PrivateBrowsingUtils.isWindowPrivate(window));
  } catch {}
  if (!isPrivateWindow) {
    try {
      isPrivateWindow = Services.env.get("NOUS_PRIVATE_WINDOW") === "1";
    } catch {}
  }

  const THEME_MIGRATION = {
    kanso: "kanso",
    washi: "washi",
    sumi: "sumi",
    stone: "stone",
    noir: "kanso",
    obsidian: "kanso",
    papel: "washi",
    porcelain: "washi",
    void: "sumi",
    oled: "sumi",
    sand: "stone",
    nordic: "stone",
  };

  function getGeckoEngineLabel() {
    try {
      const raw = Services.appinfo.platformVersion || "153";
      const parts = raw.split(".");
      const shortVer = parts.length >= 2 ? `${parts[0]}.${parts[1]}` : parts[0];
      return `Gecko ${shortVer} ESR`;
    } catch {
      return "Gecko 153 ESR";
    }
  }

  const SEARCH_ENGINES = {
    duckduckgo: { label: "DuckDuckGo", url: q => `https://duckduckgo.com/?q=${encodeURIComponent(q)}` },
    google: { label: "Google", url: q => `https://www.google.com/search?q=${encodeURIComponent(q)}` },
    brave: { label: "Brave Search", url: q => `https://search.brave.com/search?q=${encodeURIComponent(q)}` },
    startpage: { label: "Startpage", url: q => `https://www.startpage.com/do/search?q=${encodeURIComponent(q)}` },
  };

  const DEFAULT_BOOKMARKS = [
    { title: "Google", url: "https://www.google.com" },
    { title: "YouTube", url: "https://www.youtube.com" },
    { title: "GitHub", url: "https://github.com" },
    { title: "Wikipédia", url: "https://pt.wikipedia.org" },
    { title: "MDN Web Docs", url: "https://developer.mozilla.org" },
    { title: "Hacker News", url: "https://news.ycombinator.com" },
  ];

  const byId = id => document.getElementById(id);

  // -------------------------------------------------------------------------
  // Preferences & Lightweight JSON State Persistence (bookmarks & history)
  // -------------------------------------------------------------------------
  function getStrPref(name, fallback) {
    try {
      return Services.prefs.getStringPref(name, fallback);
    } catch {
      return fallback;
    }
  }
  function getBoolPref(name, fallback) {
    try {
      return Services.prefs.getBoolPref(name, fallback);
    } catch {
      return fallback;
    }
  }
  function getIntPref(name, fallback) {
    try {
      return Services.prefs.getIntPref(name, fallback);
    } catch {
      return fallback;
    }
  }
  function setPref(name, value) {
    try {
      if (typeof value === "boolean") Services.prefs.setBoolPref(name, value);
      else if (typeof value === "number") Services.prefs.setIntPref(name, value);
      else Services.prefs.setStringPref(name, String(value));
    } catch {}
  }

  let stateFilePath = null;
  function getStateFile() {
    if (stateFilePath) return stateFilePath;
    try {
      const dir = Services.dirsvc.get("ProfD", Ci.nsIFile);
      dir.append("nous-state.json");
      stateFilePath = dir;
      return dir;
    } catch {
      return null;
    }
  }

  const persisted = {
    bookmarks: DEFAULT_BOOKMARKS.slice(),
    history: [],
    allowlist: [],
  };

  function getProfileFile(name) {
    try {
      const dir = Services.dirsvc.get("ProfD", Ci.nsIFile);
      dir.append(name);
      return dir;
    } catch {
      return null;
    }
  }

  function readUtf8File(file) {
    if (!file || !file.exists()) return null;
    const stream = Cc["@mozilla.org/network/file-input-stream;1"].createInstance(Ci.nsIFileInputStream);
    stream.init(file, 0x01, 0o444, 0);
    const conv = Cc["@mozilla.org/intl/converter-input-stream;1"].createInstance(Ci.nsIConverterInputStream);
    conv.init(stream, "UTF-8", 8192, 0);
    let raw = "";
    const out = {};
    while (conv.readString(8192, out) !== 0) {
      raw += out.value;
    }
    conv.close();
    stream.close();
    return raw;
  }

  function writeUtf8File(file, content) {
    if (!file) return;
    const stream = Cc["@mozilla.org/network/file-output-stream;1"].createInstance(Ci.nsIFileOutputStream);
    stream.init(file, 0x02 | 0x08 | 0x20, 0o600, 0);
    const conv = Cc["@mozilla.org/intl/converter-output-stream;1"].createInstance(Ci.nsIConverterOutputStream);
    conv.init(stream, "UTF-8");
    conv.writeString(content);
    conv.close();
    stream.close();
  }

  function loadPersistedState() {
    try {
      const raw = readUtf8File(getStateFile());
      if (!raw) return;
      const parsed = JSON.parse(raw);
      if (Array.isArray(parsed.bookmarks)) {
        persisted.bookmarks = parsed.bookmarks.filter(b => b && typeof b.url === "string").slice(0, 256);
      }
      if (Array.isArray(parsed.history)) {
        persisted.history = parsed.history.filter(h => h && typeof h.url === "string").slice(0, 500);
      }
      if (Array.isArray(parsed.allowlist)) {
        persisted.allowlist = parsed.allowlist.filter(h => typeof h === "string").slice(0, 256);
        window.NousBlocker?.setAllowedHosts(persisted.allowlist);
      }
    } catch {}
  }

  let saveTimer = null;
  function scheduleSaveState() {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      saveTimer = null;
      try {
        writeUtf8File(getStateFile(), JSON.stringify(persisted));
      } catch {}
    }, 500);
  }

  function recordHistory(url, title) {
    if (isPrivateWindow || !url || isInternalUrl(url) || url === "about:blank" || url.startsWith("view-source:")) return;
    persisted.history = persisted.history.filter(h => h.url !== url);
    persisted.history.unshift({ url, title: title || hostOf(url), ts: Date.now() });
    if (persisted.history.length > 500) persisted.history.length = 500;
    scheduleSaveState();
  }

  let sessionSaveTimer = null;
  function saveSessionNow() {
    if (isPrivateWindow || Services.env.get("NOUS_CAPTURE_PNG")) return;
    try {
      const activeIdx = Math.max(0, tabs.indexOf(activeTab));
      const payload = {
        activeIndex: activeIdx,
        tabs: tabs.slice(0, 40).map(t => ({
          url: t.url || "nous://nova-aba",
          title: t.title || "",
          pinned: Boolean(t.pinned),
        })),
      };
      writeUtf8File(getProfileFile("nous-session.json"), JSON.stringify(payload));
    } catch {}
  }

  function scheduleSaveSession() {
    if (isPrivateWindow) return;
    if (sessionSaveTimer) clearTimeout(sessionSaveTimer);
    sessionSaveTimer = setTimeout(() => {
      sessionSaveTimer = null;
      saveSessionNow();
    }, 1200);
  }

  function restoreLazySession() {
    if (isPrivateWindow || Services.env.get("NOUS_CAPTURE_PNG")) return false;
    try {
      const raw = readUtf8File(getProfileFile("nous-session.json"));
      if (!raw) return false;
      const parsed = JSON.parse(raw);
      if (!Array.isArray(parsed.tabs) || parsed.tabs.length === 0) return false;
      const activeIdx = Math.min(Math.max(0, Number(parsed.activeIndex) || 0), parsed.tabs.length - 1);
      const valid = parsed.tabs.filter(t => t && typeof t.url === "string");
      if (valid.length === 0) return false;

      valid.forEach((entry, i) => {
        const isTarget = i === activeIdx;
        const isInt = isInternalUrl(entry.url);
        const tab = createTab(entry.url, {
          activate: isTarget,
          pinned: Boolean(entry.pinned),
          skipLoad: !isTarget && !isInt,
        });
        if (!isTarget && !isInt) {
          tab.title = entry.title || hostOf(entry.url);
          tab.sleeping = true;
        }
      });
      renderTabs(true);
      return true;
    } catch {
      return false;
    }
  }

  function consumePendingExternalUrls() {
    try {
      const qFile = getProfileFile("open-url.queue");
      if (!qFile || !qFile.exists()) return;
      const raw = readUtf8File(qFile);
      try { qFile.remove(false); } catch {}
      if (!raw) return;
      const lines = raw.split(/\r?\n/).map(s => s.trim()).filter(Boolean);
      for (const line of lines) {
        try {
          const target = resolveInput(line);
          createTab(target, { activate: true });
        } catch {}
      }
    } catch {}
  }

  // -------------------------------------------------------------------------
  // Runtime State
  // -------------------------------------------------------------------------
  const config = {
    theme: THEME_MIGRATION[getStrPref("nous.theme", "kanso")] || "kanso",
    layout: getStrPref("nous.layout", "top"),
    density: getStrPref("nous.density", "normal"),
    perfProfile: getStrPref("nous.perfProfile", "eco"),
    searchEngine: getStrPref("nous.searchEngine", "duckduckgo"),
    blockerEnabled: getBoolPref("nous.blocker.enabled", true),
    forceDarkWeb: getBoolPref("nous.forceDarkWeb", true),
    hibernateMinutes: getIntPref("nous.hibernation.minutes", 2),
    animations: getBoolPref("nous.animations.enabled", true),
    sidebarCollapsed: false,
    zen: false,
  };

  let nextTabId = 1;
  const tabs = [];
  const closedTabs = [];
  let activeTab = null;
  let contextTargetTab = null;
  let latestRamMb = null;
  let toastTimer = null;
  let omniResults = [];
  let omniSelectedIndex = -1;
  const faviconCache = new Map();

  let memoryManager = null;
  try {
    memoryManager = Cc["@mozilla.org/memory-reporter-manager;1"].getService(Ci.nsIMemoryReporterManager);
  } catch {
    memoryManager = null;
  }

  // -------------------------------------------------------------------------
  // Helpers
  // -------------------------------------------------------------------------
  function isInternalUrl(url) {
    return typeof url === "string" && Object.prototype.hasOwnProperty.call(INTERNAL_PAGES, url);
  }

  function normalizeInternalUrl(raw) {
    const lower = raw.trim().toLowerCase().replace(/\/+$/, "");
    if (lower === "nous://nova-aba" || lower === "nous://home" || lower === "about:home" || lower === "about:newtab") {
      return "nous://nova-aba";
    }
    if (lower === "nous://memoria" || lower === "nous://memory" || lower === "about:memory") {
      return "nous://memoria";
    }
    if (lower === "nous://config" || lower === "nous://settings" || lower === "nous://ajustes") {
      return "nous://config";
    }
    if (lower === "nous://downloads" || lower === "about:downloads") {
      return "nous://downloads";
    }
    if (lower === "nous://historico" || lower === "nous://history") {
      return "nous://historico";
    }
    if (lower === "nous://favoritos" || lower === "nous://bookmarks") {
      return "nous://favoritos";
    }
    return null;
  }

  function hostOf(url) {
    try {
      const u = new URL(url);
      return u.hostname.replace(/^www\./, "") || url;
    } catch {
      return url || "";
    }
  }

  function resolveInput(rawInput) {
    const value = (rawInput || "").trim();
    if (!value) {
      return "nous://nova-aba";
    }
    const internal = normalizeInternalUrl(value);
    if (internal) {
      return internal;
    }
    if (/^(?:https?|file):\/\//i.test(value)) {
      return new URL(value).href;
    }
    if (/^(?:about:|view-source:)/i.test(value)) {
      return value;
    }
    if (/^(?:localhost|\d{1,3}(?:\.\d{1,3}){3}|[\p{L}\d-]+(?:\.[\p{L}\d-]+)+)(?::\d+)?(?:[/?#].*)?$/iu.test(value)) {
      const isLocal = /^(?:localhost|127\.0\.0\.1|192\.168\.|10\.|0\.0\.0\.0)(?::|[/?#]|$)/i.test(value);
      return new URL(`${isLocal ? "http" : "https"}://${value}`).href;
    }
    if (/^[a-z][a-z\d+.-]*:/i.test(value)) {
      throw new Error("Protocolo não suportado. Use HTTP, HTTPS ou nous://.");
    }
    const engine = SEARCH_ENGINES[config.searchEngine] || SEARCH_ENGINES.duckduckgo;
    return engine.url(value);
  }

  function showToast(text, duration = 2400) {
    const toast = byId("toast");
    toast.textContent = text;
    toast.hidden = false;
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => {
      toast.hidden = true;
      toastTimer = null;
    }, duration);
  }

  function svgIcon(paths, viewBox = "0 0 14 14") {
    const svg = document.createElementNS(SVG_NS, "svg");
    svg.setAttribute("viewBox", viewBox);
    svg.setAttribute("fill", "none");
    svg.setAttribute("stroke", "currentColor");
    svg.setAttribute("stroke-width", "1.5");
    svg.setAttribute("stroke-linecap", "round");
    svg.setAttribute("stroke-linejoin", "round");
    for (const d of paths) {
      const p = document.createElementNS(SVG_NS, "path");
      p.setAttribute("d", d);
      svg.append(p);
    }
    return svg;
  }

  // -------------------------------------------------------------------------
  // Memory Telemetry & Minimization
  // -------------------------------------------------------------------------
  function readRamMb() {
    if (!memoryManager) return null;
    try {
      const bytes = memoryManager.residentUnique || memoryManager.residentFast || memoryManager.resident;
      if (bytes > 0) {
        return Math.round(bytes / (1024 * 1024));
      }
    } catch {}
    return null;
  }

  function updateRamIndicator() {
    const mb = readRamMb();
    if (mb !== null) {
      latestRamMb = mb;
      const label = `${mb} MB`;
      byId("ram-value").textContent = label;
      const stage = byId("internal-stage");
      if (stage && !stage.hidden) {
        stage.querySelectorAll("[data-live-ram]").forEach(node => {
          node.textContent = label;
        });
      }
    }
  }

  function optimizeMemoryNow() {
    const before = readRamMb();
    return new Promise(resolve => {
      try {
        Services.obs.notifyObservers(null, "memory-pressure", "heap-minimize");
      } catch {}
      if (memoryManager && typeof memoryManager.minimizeMemoryUsage === "function") {
        try {
          memoryManager.minimizeMemoryUsage(() => {
            setTimeout(() => {
              updateRamIndicator();
              const after = readRamMb();
              if (before !== null && after !== null && before > after) {
                showToast(`Memória otimizada · ${before - after} MB liberados (${after} MB)`);
              } else {
                showToast(`Memória compactada (${after ?? "—"} MB)`);
              }
              resolve();
            }, 250);
          });
          return;
        } catch {}
      }
      updateRamIndicator();
      showToast("Coleta de memória solicitada");
      resolve();
    });
  }

  // -------------------------------------------------------------------------
  // Lazy Sandboxed <browser> Lifecycle & Hibernation
  // -------------------------------------------------------------------------
  function attachBrowser(tab, openWindowInfo = null) {
    if (tab.browser) return tab.browser;

    // Lazily warm up Gecko Downloads listener only on first web tab (0 MB on nous://nova-aba startup)
    window.NousDownloads?.ensureInitialized();

    const browser = document.createXULElement("browser");
    browser.setAttribute("type", "content");
    browser.setAttribute("remote", "true");
    browser.setAttribute("maychangeremoteness", "true");
    browser.setAttribute("messagemanagergroup", "browsers");
    browser.setAttribute("disableglobalhistory", "true");
    if (isPrivateWindow) {
      browser.setAttribute("usercontextid", "0");
    }
    if (openWindowInfo) {
      browser.openWindowInfo = openWindowInfo;
    }
    browser.hidden = tab !== activeTab;
    byId("pages").append(browser);

    tab.browser = browser;
    tab.sleeping = false;

    tab.progress = {
      QueryInterface: ChromeUtils.generateQI(["nsIWebProgressListener", "nsISupportsWeakReference"]),
      onLocationChange(progress, _req, location) {
        if (!progress.isTopLevel) return;
        const spec = location?.spec || browser.currentURI?.spec || "";
        if (spec && spec !== "about:blank") {
          tab.url = spec;
          tab.blockedCount = 0;
          resolveFaviconForTab(tab, spec);
        }
        syncTabMetadata(tab);
      },
      onStateChange(progress, _req, flags) {
        if (!progress.isTopLevel) return;
        if (flags & WPL.STATE_START) {
          tab.loading = true;
          tab.loadFraction = 0.16;
          syncTabMetadata(tab);
        } else if (flags & WPL.STATE_STOP) {
          tab.loading = false;
          tab.loadFraction = 1;
          syncTabMetadata(tab);
          if (tab.url && !isInternalUrl(tab.url)) {
            recordHistory(tab.url, tab.title);
          }
        }
      },
      onProgressChange(progress, _req, curSelf, maxSelf, curTotal, maxTotal) {
        if (!progress.isTopLevel || !tab.loading) return;
        const max = maxTotal > 0 ? maxTotal : maxSelf;
        const cur = maxTotal > 0 ? curTotal : curSelf;
        if (max > 0) {
          tab.loadFraction = Math.min(0.95, Math.max(0.16, cur / max));
          if (tab === activeTab) updateProgressBar(tab);
        }
      },
      onSecurityChange(progress, _req, state) {
        if (!progress.isTopLevel) return;
        tab.isSecure = Boolean(state & WPL.STATE_IS_SECURE);
        if (tab === activeTab) updateSecurityBadge(tab);
      },
    };

    browser.addProgressListener(
      tab.progress,
      WEB_PROGRESS.NOTIFY_LOCATION |
        WEB_PROGRESS.NOTIFY_STATE_NETWORK |
        WEB_PROGRESS.NOTIFY_PROGRESS |
        WEB_PROGRESS.NOTIFY_SECURITY
    );

    browser.addEventListener("DOMTitleChanged", () => syncTabMetadata(tab));
    browser.addEventListener("DOMWindowClose", () => closeTab(tab));
    browser.addEventListener("DOMAudioPlaybackStarted", () => {
      tab.audioPlaying = true;
      renderTabs();
    });
    browser.addEventListener("DOMAudioPlaybackStopped", () => {
      tab.audioPlaying = false;
      renderTabs();
    });

    if (tab !== activeTab) {
      try {
        browser.docShellIsActive = false;
        browser.preserveLayers(false);
      } catch {}
    }

    return browser;
  }

  function detachBrowser(tab) {
    if (!tab.browser) return;
    try {
      if (tab.progress) {
        tab.browser.removeProgressListener(tab.progress);
      }
    } catch {}
    try {
      if (typeof tab.browser.destroy === "function") {
        tab.browser.destroy();
      }
    } catch {}
    try {
      tab.browser.remove();
    } catch {}
    tab.browser = null;
    tab.progress = null;
    tab.loading = false;
    tab.audioPlaying = false;
  }

  function hibernateTab(tab, silent = false) {
    if (!tab || tab === activeTab || isInternalUrl(tab.url) || !tab.browser) {
      return false;
    }
    if (tab.audioPlaying && !tab.muted) {
      return false;
    }
    detachBrowser(tab);
    tab.sleeping = true;
    renderTabs();
    if (!silent) {
      showToast(`Aba hibernada · RAM liberada`);
      setTimeout(updateRamIndicator, 300);
    }
    return true;
  }

  function wakeTab(tab) {
    if (!tab || !tab.sleeping || isInternalUrl(tab.url)) return;
    tab.sleeping = false;
    loadTabUrl(tab, tab.url, { recordNav: false });
    renderTabs();
  }

  function hibernateInactiveTabs() {
    let count = 0;
    for (const tab of tabs) {
      if (hibernateTab(tab, true)) {
        count += 1;
      }
    }
    if (count > 0) {
      try {
        Services.obs.notifyObservers(null, "memory-pressure", "heap-minimize");
      } catch {}
      setTimeout(updateRamIndicator, 350);
      showToast(`${count} ${count === 1 ? "aba hibernada" : "abas hibernadas"} · RAM liberada`);
    } else {
      showToast("Nenhuma aba inativa para hibernar");
    }
    return count;
  }

  function resolveFaviconForTab(tab, url) {
    try {
      const parsed = new URL(url);
      if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
        tab.favicon = null;
        return;
      }
      const origin = parsed.origin;
      if (faviconCache.has(origin)) {
        tab.favicon = faviconCache.get(origin);
        return;
      }
      const iconUrl = `${origin}/favicon.ico`;
      if (faviconCache.size > 64) {
        const firstKey = faviconCache.keys().next().value;
        faviconCache.delete(firstKey);
      }
      faviconCache.set(origin, iconUrl);
      tab.favicon = iconUrl;
    } catch {
      tab.favicon = null;
    }
  }

  function loadTabUrl(tab, url, { bypassCache = false, recordNav = true } = {}) {
    tab.url = url;
    tab.lastActiveAt = Date.now();
    tab.blockedCount = 0;

    if (isInternalUrl(url)) {
      // Zero content process RAM for internal pages!
      detachBrowser(tab);
      tab.sleeping = false;
      tab.title = INTERNAL_PAGES[url];
      tab.favicon = null;
      tab.isSecure = false;
      tab.loading = false;
      tab.loadFraction = 0;
      if (tab === activeTab) {
        renderActiveStage();
        syncToolbarForActive();
      }
      renderTabs();
      scheduleSaveSession();
      return;
    }

    const browser = attachBrowser(tab);
    if (!browser.isRemoteBrowser) {
      showToast("Aviso: processo de conteúdo Gecko indisponível.");
      return;
    }

    tab.loading = true;
    tab.loadFraction = 0.14;
    resolveFaviconForTab(tab, url);
    if (tab === activeTab) {
      renderActiveStage();
      syncToolbarForActive();
    }

    const uri = Services.io.newURI(url);
    const loadFlags = bypassCache ? Ci.nsIWebNavigation.LOAD_FLAGS_BYPASS_CACHE : 0;
    browser.loadURI(uri, {
      triggeringPrincipal: SECURITY.getSystemPrincipal(),
      loadFlags,
    });
    if (recordNav) {
      tab.title = hostOf(url) || "Carregando…";
    }
    renderTabs();
    scheduleSaveSession();
  }

  function syncTabMetadata(tab) {
    if (isInternalUrl(tab.url)) {
      tab.title = INTERNAL_PAGES[tab.url];
    } else if (tab.browser) {
      const uri = tab.browser.currentURI;
      const spec = uri?.spec || tab.url;
      if (spec && spec !== "about:blank") {
        tab.url = spec;
      }
      const host = uri && (uri.schemeIs("http") || uri.schemeIs("https")) ? uri.displayHost : hostOf(tab.url);
      tab.title = tab.browser.contentTitle || host || "Nova aba";
      try {
        tab.isSecure = tab.url.startsWith("https://");
      } catch {}
    }
    renderTabs();
    if (tab === activeTab) {
      syncToolbarForActive();
    }
  }

  // -------------------------------------------------------------------------
  // Tab Operations
  // -------------------------------------------------------------------------
  function createTab(address = "nous://nova-aba", { activate = true, pinned = false, openWindowInfo = null, skipLoad = false } = {}) {
    const tab = {
      id: nextTabId++,
      url: address,
      title: isInternalUrl(address) ? INTERNAL_PAGES[address] : hostOf(address) || "Nova aba",
      favicon: null,
      browser: null,
      progress: null,
      pinned,
      muted: false,
      audioPlaying: false,
      sleeping: false,
      loading: false,
      loadFraction: 0,
      isSecure: address.startsWith("https://"),
      blockedCount: 0,
      lastActiveAt: Date.now(),
    };

    if (pinned) {
      const insertAt = tabs.findIndex(t => !t.pinned);
      if (insertAt === -1) tabs.push(tab);
      else tabs.splice(insertAt, 0, tab);
    } else {
      tabs.push(tab);
    }

    if (openWindowInfo) {
      attachBrowser(tab, openWindowInfo);
    }

    if (activate) {
      selectTab(tab);
    } else {
      renderTabs();
    }

    if (!skipLoad) {
      loadTabUrl(tab, address);
    }
    return tab;
  }

  function selectTab(tab) {
    if (!tab) return;
    if (activeTab && activeTab !== tab) {
      activeTab.lastActiveAt = Date.now();
      if (activeTab.browser) {
        activeTab.browser.hidden = true;
        try {
          activeTab.browser.docShellIsActive = false;
          // Immediately free off-screen GPU tile layers to keep RAM low.
          activeTab.browser.preserveLayers(false);
        } catch {}
      }
    }

    activeTab = tab;
    tab.lastActiveAt = Date.now();

    if (tab.sleeping) {
      wakeTab(tab);
    }

    if (tab.browser) {
      tab.browser.hidden = false;
      try {
        tab.browser.docShellIsActive = true;
      } catch {}
    }

    closeFindBar();
    hideOmniboxDropdown();
    renderActiveStage();
    syncToolbarForActive();
    renderTabs();
  }

  function closeTab(tab) {
    if (!tab) return;
    const index = tabs.indexOf(tab);
    if (index === -1) return;

    if (tab.url && tab.url !== "nous://nova-aba") {
      closedTabs.unshift({ url: tab.url, title: tab.title });
      if (closedTabs.length > 20) closedTabs.length = 20;
    }

    if (tabs.length === 1) {
      window.close();
      return;
    }

    const wasActive = tab === activeTab;
    tabs.splice(index, 1);
    detachBrowser(tab);

    if (wasActive) {
      const next = tabs[Math.min(index, tabs.length - 1)];
      activeTab = null;
      selectTab(next);
    } else {
      renderTabs();
      if (activeTab && activeTab.url === "nous://memoria") {
        renderActiveStage();
      }
    }
    scheduleSaveSession();
  }

  function reopenClosedTab() {
    const last = closedTabs.shift();
    if (!last) {
      showToast("Nenhuma aba fechada recentemente");
      return;
    }
    createTab(last.url, { activate: true });
    showToast("Aba reaberta");
  }

  function togglePinTab(tab) {
    if (!tab) return;
    tab.pinned = !tab.pinned;
    // Keep pinned tabs grouped first
    tabs.sort((a, b) => Number(b.pinned) - Number(a.pinned));
    renderTabs();
  }

  function toggleMuteTab(tab) {
    if (!tab || !tab.browser) return;
    try {
      if (tab.muted || tab.browser.audioMuted) {
        tab.browser.unmute();
        tab.muted = false;
      } else {
        tab.browser.mute();
        tab.muted = true;
      }
    } catch {}
    renderTabs();
  }

  // -------------------------------------------------------------------------
  // Rendering: Tabs, Stage, Toolbar, Security, Progress
  // -------------------------------------------------------------------------
  function buildTabElement(tab, isSidebar) {
    const item = document.createElementNS(HTML_NS, "div");
    item.className = "tab-item";
    if (tab.pinned) item.classList.add("pinned");
    if (tab.sleeping) item.classList.add("sleeping");
    item.setAttribute("role", "tab");
    item.setAttribute("aria-selected", tab === activeTab ? "true" : "false");
    item.setAttribute("title", tab.sleeping ? `${tab.title} (Hibernando · 0 MB)` : tab.title);

    // Icon / Spinner / Kanso Sage Dot
    if (tab.loading) {
      const iconSlot = document.createElementNS(HTML_NS, "span");
      iconSlot.className = "tab-icon-slot";
      const spinner = document.createElementNS(HTML_NS, "span");
      spinner.className = "tab-spinner";
      iconSlot.append(spinner);
      item.append(iconSlot);
    } else if (!isSidebar && !tab.pinned && tab === activeTab && !tab.favicon) {
      const iconSlot = document.createElementNS(HTML_NS, "span");
      iconSlot.className = "tab-icon-slot";
      const dot = document.createElementNS(HTML_NS, "span");
      dot.className = "tab-sage-dot";
      iconSlot.append(dot);
      item.append(iconSlot);
    } else if (tab.favicon) {
      const iconSlot = document.createElementNS(HTML_NS, "span");
      iconSlot.className = "tab-icon-slot";
      const img = document.createElementNS(HTML_NS, "img");
      img.className = "tab-favicon";
      img.src = tab.favicon;
      img.alt = "";
      img.addEventListener("error", () => {
        tab.favicon = null;
        try {
          faviconCache.set(new URL(tab.url).origin, null);
        } catch {}
        if (!isSidebar && tab === activeTab && !tab.pinned) {
          const dot = document.createElementNS(HTML_NS, "span");
          dot.className = "tab-sage-dot";
          iconSlot.replaceChildren(dot);
        } else {
          iconSlot.replaceChildren(createMonogram(tab));
        }
      }, { once: true });
      iconSlot.append(img);
      item.append(iconSlot);
    } else if (isSidebar || tab.pinned) {
      const iconSlot = document.createElementNS(HTML_NS, "span");
      iconSlot.className = "tab-icon-slot";
      if (tab === activeTab && !tab.pinned) {
        const dot = document.createElementNS(HTML_NS, "span");
        dot.className = "tab-sage-dot";
        iconSlot.append(dot);
      } else {
        iconSlot.append(createMonogram(tab));
      }
      item.append(iconSlot);
    }

    if (!tab.pinned) {
      const label = document.createElementNS(HTML_NS, "span");
      label.className = "tab-title";
      label.textContent = tab.title || "Nova aba";
      item.append(label);
    }

    // Sleeping or Audio badge
    if (tab.sleeping) {
      const badge = document.createElementNS(HTML_NS, "span");
      badge.className = "tab-badge sleep-badge";
      badge.title = "Aba hibernando (0 MB RAM)";
      badge.append(svgIcon(["M10.8 8.6A4.4 4.4 0 0 1 5.4 3.2A4.6 4.6 0 1 0 10.8 8.6Z"]));
      item.append(badge);
    } else if (tab.audioPlaying || tab.muted) {
      const muteBtn = document.createElementNS(HTML_NS, "button");
      muteBtn.type = "button";
      muteBtn.className = "tab-badge";
      muteBtn.title = tab.muted ? "Ativar som" : "Silenciar aba";
      muteBtn.append(
        tab.muted
          ? svgIcon(["M2.5 5.2H4.8L7.6 2.8V11.2L4.8 8.8H2.5Z", "M9.6 5.2L12.4 8.8", "M12.4 5.2L9.6 8.8"])
          : svgIcon(["M2.5 5.2H4.8L7.6 2.8V11.2L4.8 8.8H2.5Z", "M9.6 4.8A3 3 0 0 1 9.6 9.2", "M11.2 3.3A5 5 0 0 1 11.2 10.7"])
      );
      muteBtn.addEventListener("click", e => {
        e.stopPropagation();
        toggleMuteTab(tab);
      });
      item.append(muteBtn);
    }

    if (!tab.pinned) {
      const closeBtn = document.createElementNS(HTML_NS, "button");
      closeBtn.type = "button";
      closeBtn.className = "tab-close";
      closeBtn.setAttribute("aria-label", "Fechar aba");
      closeBtn.append(svgIcon(["M3.5 3.5L10.5 10.5", "M10.5 3.5L3.5 10.5"]));
      closeBtn.addEventListener("click", e => {
        e.stopPropagation();
        closeTab(tab);
      });
      item.append(closeBtn);
    }

    item.addEventListener("click", () => selectTab(tab));
    item.addEventListener("auxclick", e => {
      if (e.button === 1) {
        e.preventDefault();
        closeTab(tab);
      }
    });
    item.addEventListener("contextmenu", e => {
      e.preventDefault();
      openTabContextMenu(tab, e.clientX, e.clientY);
    });

    // Drag-to-reorder tabs
    item.draggable = true;
    item.addEventListener("dragstart", e => {
      e.dataTransfer.effectAllowed = "move";
      e.dataTransfer.setData("text/x-nous-tab", String(tab.id));
      item.classList.add("dragging");
    });
    item.addEventListener("dragend", () => {
      item.classList.remove("dragging");
      document.querySelectorAll(".tab-item.drag-over").forEach(n => n.classList.remove("drag-over"));
    });
    item.addEventListener("dragover", e => {
      if (e.dataTransfer.types.includes("text/x-nous-tab")) {
        e.preventDefault();
        item.classList.add("drag-over");
      }
    });
    item.addEventListener("dragleave", () => item.classList.remove("drag-over"));
    item.addEventListener("drop", e => {
      e.preventDefault();
      item.classList.remove("drag-over");
      const draggedId = Number(e.dataTransfer.getData("text/x-nous-tab"));
      if (!draggedId || draggedId === tab.id) return;
      const fromIdx = tabs.findIndex(t => t.id === draggedId);
      const toIdx = tabs.indexOf(tab);
      if (fromIdx === -1 || toIdx === -1) return;
      const [moved] = tabs.splice(fromIdx, 1);
      moved.pinned = tab.pinned;
      tabs.splice(toIdx, 0, moved);
      renderTabs(true);
      scheduleSaveSession();
    });

    return item;
  }

  function createMonogram(tab) {
    const span = document.createElementNS(HTML_NS, "span");
    span.className = "tab-monogram";
    if (isInternalUrl(tab.url)) {
      span.textContent = "N";
    } else {
      const ch = (tab.title || hostOf(tab.url) || "W").trim().charAt(0).toUpperCase();
      span.textContent = ch || "W";
    }
    return span;
  }

  let lastTabsSignature = "";

  function renderTabs(force = false) {
    const sig =
      config.layout +
      "|" +
      (activeTab?.id ?? 0) +
      "|" +
      tabs
        .map(t => `${t.id}:${t.title}:${t.loading ? 1 : 0}:${t.pinned ? 1 : 0}:${t.sleeping ? 1 : 0}:${t.audioPlaying ? 1 : 0}:${t.muted ? 1 : 0}:${t.favicon || ""}`)
        .join(";");
    if (!force && sig === lastTabsSignature) {
      return;
    }
    lastTabsSignature = sig;

    const topHost = byId("tabs-top");
    const sideHost = byId("tabs-side");
    const sidePinnedHost = byId("tabs-pinned-side");

    if (config.layout === "sidebar") {
      topHost.replaceChildren();
      const pinnedFrag = document.createDocumentFragment();
      const regularFrag = document.createDocumentFragment();
      let pinnedCount = 0;
      for (const tab of tabs) {
        if (tab.pinned) {
          pinnedFrag.append(buildTabElement(tab, true));
          pinnedCount += 1;
        } else {
          regularFrag.append(buildTabElement(tab, true));
        }
      }
      sidePinnedHost.replaceChildren(pinnedFrag);
      sidePinnedHost.hidden = pinnedCount === 0;
      sideHost.replaceChildren(regularFrag);
    } else {
      sideHost.replaceChildren();
      sidePinnedHost.replaceChildren();
      sidePinnedHost.hidden = true;
      const frag = document.createDocumentFragment();
      for (const tab of tabs) {
        frag.append(buildTabElement(tab, false));
      }
      topHost.replaceChildren(frag);
    }
  }

  function renderActiveStage() {
    const internalStage = byId("internal-stage");
    const pages = byId("pages");

    if (!activeTab) return;

    if (isInternalUrl(activeTab.url)) {
      pages.hidden = true;
      internalStage.hidden = false;
      window.NousInternalPages.render(activeTab.url, internalStage, internalApi);
    } else {
      internalStage.hidden = true;
      internalStage.replaceChildren();
      pages.hidden = false;
      if (activeTab.browser) {
        activeTab.browser.hidden = false;
      }
    }
  }

  function updateSecurityBadge(tab) {
    const badge = byId("security-badge");
    if (!tab || isInternalUrl(tab.url)) {
      badge.dataset.state = "internal";
      badge.title = "Página interna Nous";
    } else if (tab.isSecure || tab.url.startsWith("https://")) {
      badge.dataset.state = "secure";
      badge.title = "Conexão segura (HTTPS)";
    } else {
      badge.dataset.state = "insecure";
      badge.title = "Conexão sem criptografia (HTTP)";
    }
  }

  function updateProgressBar(tab) {
    const wrap = byId("load-progress");
    const bar = byId("load-progress-bar");
    if (tab && tab.loading) {
      wrap.classList.add("active");
      bar.style.width = `${Math.round((tab.loadFraction || 0.2) * 100)}%`;
    } else {
      bar.style.width = "100%";
      wrap.classList.remove("active");
      setTimeout(() => {
        if (!activeTab?.loading) bar.style.width = "0%";
      }, 200);
    }
  }

  function updateZoomIndicator() {
    const pill = byId("btn-zoom-pill");
    if (!activeTab?.browser) {
      pill.hidden = true;
      return;
    }
    const pct = Math.round((activeTab.browser.fullZoom || 1) * 100);
    if (pct === 100) {
      pill.hidden = true;
    } else {
      pill.textContent = `${pct}%`;
      pill.hidden = false;
    }
  }

  function isBookmarked(url) {
    return persisted.bookmarks.some(b => b.url === url);
  }

  function syncToolbarForActive() {
    if (!activeTab) return;
    const isInt = isInternalUrl(activeTab.url);
    const addr = byId("address");

    if (document.activeElement !== addr) {
      addr.value = activeTab.url === "nous://nova-aba" ? "" : activeTab.url;
    }

    byId("btn-back").disabled = isInt || !activeTab.browser?.canGoBack;
    byId("btn-forward").disabled = isInt || !activeTab.browser?.canGoForward;
    byId("btn-reload").classList.toggle("loading", Boolean(activeTab.loading));
    byId("btn-reload").title = activeTab.loading ? "Parar carregamento (Esc)" : "Recarregar (F5)";

    const star = byId("btn-bookmark");
    star.hidden = isInt;
    star.classList.toggle("bookmarked", !isInt && isBookmarked(activeTab.url));

    const readerBtn = byId("btn-reader");
    if (readerBtn) {
      const canReader = !isInt && (activeTab.url.startsWith("http://") || activeTab.url.startsWith("https://") || activeTab.url.startsWith("about:reader"));
      readerBtn.hidden = !canReader;
      readerBtn.classList.toggle("bookmarked", activeTab.url.startsWith("about:reader"));
    }

    const shield = byId("btn-shield");
    shield.classList.toggle("off", !config.blockerEnabled);
    shield.classList.toggle("has-blocks", (activeTab.blockedCount || 0) > 0);
    byId("shield-count").textContent = String(activeTab.blockedCount || 0);
    shield.title = config.blockerEnabled
      ? `Proteção ativa · ${activeTab.blockedCount} nesta aba (${window.NousBlocker.totalBlocked} na sessão)`
      : "Bloqueador pausado (clique para reativar)";

    updateSecurityBadge(activeTab);
    updateProgressBar(activeTab);
    updateZoomIndicator();

    document.title = `${activeTab.title || "Nova aba"} — Nous`;
  }

  // -------------------------------------------------------------------------
  // Omnibox Command Palette Suggestions
  // -------------------------------------------------------------------------
  function buildOmniboxSuggestions(query) {
    const q = query.trim().toLowerCase();
    const items = [];

    if (!q) {
      // Quick internal routes & top bookmarks
      items.push(
        { kind: "Nous", title: "Monitor de Memória & Leveza", sub: "nous://memoria", action: () => navigateActive("nous://memoria") },
        { kind: "Nous", title: "Configurações", sub: "nous://config", action: () => navigateActive("nous://config") }
      );
      for (const bm of persisted.bookmarks.slice(0, 4)) {
        items.push({ kind: "Favorito", title: bm.title, sub: bm.url, action: () => navigateActive(bm.url) });
      }
      return items;
    }

    // 1. Open tabs jump
    for (const t of tabs) {
      if (t !== activeTab && (t.title.toLowerCase().includes(q) || t.url.toLowerCase().includes(q))) {
        items.push({
          kind: "Aba",
          title: t.title,
          sub: t.url,
          action: () => selectTab(t),
        });
        if (items.length >= 2) break;
      }
    }

    // 2. Internal pages match
    for (const [route, title] of Object.entries(INTERNAL_PAGES)) {
      if (route.includes(q) || title.toLowerCase().includes(q)) {
        items.push({ kind: "Nous", title, sub: route, action: () => navigateActive(route) });
      }
    }

    // 3. Bookmarks match
    for (const bm of persisted.bookmarks) {
      if (bm.title.toLowerCase().includes(q) || bm.url.toLowerCase().includes(q)) {
        items.push({ kind: "Favorito", title: bm.title, sub: bm.url, action: () => navigateActive(bm.url) });
        if (items.length >= 6) break;
      }
    }

    // 4. Recent history match
    for (const h of persisted.history) {
      if (items.some(i => i.sub === h.url)) continue;
      if (h.title.toLowerCase().includes(q) || h.url.toLowerCase().includes(q)) {
        items.push({ kind: "Histórico", title: h.title, sub: h.url, action: () => navigateActive(h.url) });
        if (items.length >= 8) break;
      }
    }

    // 5. Primary direct navigation / search
    const engine = SEARCH_ENGINES[config.searchEngine] || SEARCH_ENGINES.duckduckgo;
    const looksLikeUrl = /^(https?:\/\/|nous:\/\/|localhost\b|[\p{L}\d-]+\.[\p{L}\d-]+)/iu.test(query.trim());
    items.unshift({
      kind: looksLikeUrl ? "Ir" : engine.label,
      title: query.trim(),
      sub: looksLikeUrl ? "Abrir endereço" : `Pesquisar com ${engine.label}`,
      action: () => navigateActive(query.trim()),
    });

    return items.slice(0, 8);
  }

  function renderOmniboxDropdown(query) {
    const drop = byId("omnibox-dropdown");
    omniResults = buildOmniboxSuggestions(query);
    omniSelectedIndex = omniResults.length > 0 ? 0 : -1;
    if (omniResults.length === 0) {
      drop.hidden = true;
      return;
    }

    const frag = document.createDocumentFragment();
    omniResults.forEach((item, idx) => {
      const btn = document.createElementNS(HTML_NS, "button");
      btn.type = "button";
      btn.className = `omni-item${idx === omniSelectedIndex ? " selected" : ""}`;
      btn.setAttribute("role", "option");

      const kind = document.createElementNS(HTML_NS, "span");
      kind.className = "omni-kind";
      kind.textContent = item.kind;

      const main = document.createElementNS(HTML_NS, "span");
      main.className = "omni-main";
      const t = document.createElementNS(HTML_NS, "span");
      t.className = "omni-title";
      t.textContent = item.title;
      const s = document.createElementNS(HTML_NS, "span");
      s.className = "omni-sub";
      s.textContent = item.sub;
      main.append(t, s);

      btn.append(kind, main);
      btn.addEventListener("mousedown", e => {
        e.preventDefault();
        hideOmniboxDropdown();
        item.action();
      });
      frag.append(btn);
    });
    drop.replaceChildren(frag);
    drop.hidden = false;
  }

  function hideOmniboxDropdown() {
    byId("omnibox-dropdown").hidden = true;
    omniResults = [];
    omniSelectedIndex = -1;
  }

  function moveOmniboxSelection(delta) {
    if (omniResults.length === 0) return;
    omniSelectedIndex = (omniSelectedIndex + delta + omniResults.length) % omniResults.length;
    const nodes = byId("omnibox-dropdown").children;
    for (let i = 0; i < nodes.length; i++) {
      nodes[i].classList.toggle("selected", i === omniSelectedIndex);
    }
  }

  // -------------------------------------------------------------------------
  // Navigation, Bookmarks, Zoom, Native Find
  // -------------------------------------------------------------------------
  function navigateActive(rawInput) {
    if (!activeTab) return;
    hideOmniboxDropdown();
    try {
      const target = resolveInput(rawInput);
      loadTabUrl(activeTab, target);
      byId("address").blur();
      if (activeTab.browser) {
        activeTab.browser.focus();
      }
    } catch (err) {
      showToast(err.message || "Endereço inválido");
    }
  }

  function toggleBookmarkCurrent() {
    if (!activeTab || isInternalUrl(activeTab.url)) return;
    const exists = isBookmarked(activeTab.url);
    if (exists) {
      persisted.bookmarks = persisted.bookmarks.filter(b => b.url !== activeTab.url);
      showToast("Removido dos favoritos");
    } else {
      persisted.bookmarks.unshift({ title: activeTab.title || hostOf(activeTab.url), url: activeTab.url });
      if (persisted.bookmarks.length > 64) persisted.bookmarks.length = 64;
      showToast("Salvo nos favoritos");
    }
    scheduleSaveState();
    syncToolbarForActive();
  }

  function adjustZoom(delta) {
    if (!activeTab?.browser) return;
    const next = delta === 0
      ? 1.0
      : Math.min(2.5, Math.max(0.5, Number(((activeTab.browser.fullZoom || 1) + delta).toFixed(2))));
    activeTab.browser.fullZoom = next;
    updateZoomIndicator();
    showToast(`Zoom ${Math.round(next * 100)}%`, 1100);
  }

  // Inline Finder (Ctrl+F)
  let finderListenerRegisteredFor = null;
  function openFindBar() {
    if (!activeTab?.browser) {
      showToast("Busca na página disponível em páginas web");
      return;
    }
    const bar = byId("find-bar");
    bar.hidden = false;
    const input = byId("find-input");
    input.focus();
    input.select();

    if (finderListenerRegisteredFor !== activeTab.browser && activeTab.browser.finder) {
      try {
        activeTab.browser.finder.addResultListener({
          onFindResult(data) {
            const st = byId("find-status");
            if (!byId("find-input").value) {
              st.textContent = "";
            } else if (data.result === Ci.nsITypeAheadFind.FIND_NOTFOUND) {
              st.textContent = "0 / 0";
            } else {
              st.textContent = "OK";
            }
          },
          onMatchesCountResult(res) {
            if (typeof res.total === "number") {
              byId("find-status").textContent = res.total > 0 ? `${res.current}/${res.total}` : "0/0";
            }
          },
        });
        finderListenerRegisteredFor = activeTab.browser;
      } catch {}
    }
  }

  function runFind(backwards = false, again = false) {
    const term = byId("find-input").value;
    const finder = activeTab?.browser?.finder;
    if (!finder) return;
    if (!term) {
      byId("find-status").textContent = "";
      try {
        finder.removeSelection();
      } catch {}
      return;
    }
    try {
      if (again) {
        finder.findAgain(term, backwards, false, false);
      } else {
        finder.fastFind(term, false, false);
      }
      if (typeof finder.requestMatchesCount === "function") {
        finder.requestMatchesCount(term, { linksOnly: false });
      }
    } catch {}
  }

  function closeFindBar() {
    const bar = byId("find-bar");
    if (bar.hidden) return;
    bar.hidden = true;
    byId("find-status").textContent = "";
    try {
      activeTab?.browser?.finder?.removeSelection();
    } catch {}
    activeTab?.browser?.focus();
  }

  // -------------------------------------------------------------------------
  // Theme, Layout, Density, Performance Profile & Zen Mode
  // -------------------------------------------------------------------------
  function isGoogleSignedIn() {
    try {
      const cookies = Services.cookies.getCookiesFromHost("google.com", {});
      for (const c of cookies) {
        if (c.name === "SID" || c.name === "SAPISID" || c.name === "__Secure-1PSID" || c.name === "__Secure-3PSID") {
          return true;
        }
      }
    } catch {}
    return false;
  }

  function openGoogleAccount() {
    const target = isGoogleSignedIn()
      ? "https://myaccount.google.com/"
      : "https://accounts.google.com/ServiceLogin";
    if (activeTab && isInternalUrl(activeTab.url)) {
      navigateActive(target);
    } else {
      createTab(target, { activate: true });
    }
  }

  function getDarkWebExclusions() {
    const raw = getStrPref("nous.darkWebExclude", "");
    return new Set(raw.split(",").map(s => s.trim().toLowerCase()).filter(Boolean));
  }

  function isDarkWebExcluded(host) {
    const h = String(host || "").toLowerCase().replace(/^www\./, "");
    return Boolean(h && getDarkWebExclusions().has(h));
  }

  function toggleDarkWebForHost(host) {
    const h = String(host || "").toLowerCase().replace(/^www\./, "");
    if (!h) return;
    const set = getDarkWebExclusions();
    if (set.has(h)) set.delete(h);
    else set.add(h);
    setPref("nous.darkWebExclude", Array.from(set).join(","));
    broadcastDarkWebUpdate();
  }

  function broadcastDarkWebUpdate() {
    for (const tab of tabs) {
      try {
        const actor = tab.browser?.browsingContext?.currentWindowGlobal?.getActor("NousContent");
        actor?.sendAsyncMessage("Nous:UpdateDarkWeb", {});
      } catch {}
    }
  }

  function setForceDarkWeb(enabled) {
    config.forceDarkWeb = Boolean(enabled);
    setPref("nous.forceDarkWeb", config.forceDarkWeb);
    applyVisualConfig();
    broadcastDarkWebUpdate();
    showToast(config.forceDarkWeb ? "Modo escuro adaptativo em sites ativado" : "Modo escuro forçado em sites desativado");
  }

  function applyVisualConfig() {
    const root = document.documentElement;
    root.dataset.theme = config.theme;
    root.dataset.layout = config.layout;
    root.dataset.density = config.density;
    root.dataset.perf = config.perfProfile;
    root.dataset.animations = config.animations ? "on" : "off";
    root.dataset.sidebarCollapsed = config.sidebarCollapsed ? "true" : "false";
    root.dataset.zen = config.zen ? "true" : "false";

    // Sync Gecko native dark/light content preferences (0 = Force Dark, 1 = Force Light)
    const isLight = config.theme === "washi";
    setPref("ui.systemUsesDarkTheme", isLight ? 0 : 1);
    setPref("browser.in-content.dark-mode", !isLight);
    setPref("layout.css.prefers-color-scheme.content-override", isLight ? 1 : 0);
    setPref("browser.display.background_color", isLight ? "#f5f2ec" : "#141413");
    setPref("browser.display.foreground_color", isLight ? "#1c1b18" : "#e8e5de");

    // Sync popover active states
    document.querySelectorAll("[data-set-theme]").forEach(btn => {
      btn.classList.toggle("active", btn.dataset.setTheme === config.theme);
    });
    document.querySelectorAll("[data-set-perf]").forEach(btn => {
      btn.classList.toggle("active", btn.dataset.setPerf === config.perfProfile);
    });
    document.querySelectorAll("[data-set-layout]").forEach(btn => {
      btn.classList.toggle("active", btn.dataset.setLayout === config.layout);
    });

    const blockerState = byId("menu-blocker-state");
    blockerState.textContent = config.blockerEnabled ? "Ativo" : "Pausado";
    blockerState.classList.toggle("off", !config.blockerEnabled);

    const darkWebState = byId("menu-darkweb-state");
    if (darkWebState) {
      const activeDark = !isLight && config.forceDarkWeb;
      darkWebState.textContent = isLight ? "Claro" : activeDark ? "Auto" : "Off";
      darkWebState.classList.toggle("off", !activeDark);
    }

    const googleState = byId("menu-google-state");
    if (googleState) {
      const signed = isGoogleSignedIn();
      googleState.textContent = signed ? "Conectado" : "Entrar";
      googleState.classList.toggle("off", !signed);
    }

    byId("btn-sidebar-nav")?.classList.toggle("active", config.layout === "sidebar");
    byId("btn-zen")?.classList.toggle("active", config.zen);

    broadcastDarkWebUpdate();
    renderTabs(true);
    syncToolbarForActive();
  }

  function setTheme(theme) {
    const resolved = THEME_MIGRATION[theme] || "kanso";
    config.theme = resolved;
    setPref("nous.theme", resolved);
    applyVisualConfig();
  }

  function setLayout(layout) {
    config.layout = layout;
    setPref("nous.layout", layout);
    applyVisualConfig();
  }

  function setDensity(density) {
    config.density = density;
    setPref("nous.density", density);
    applyVisualConfig();
  }

  function setAnimations(enabled) {
    config.animations = Boolean(enabled);
    setPref("nous.animations.enabled", config.animations);
    applyVisualConfig();
  }

  function setHibernateMinutes(mins) {
    config.hibernateMinutes = Number(mins) || 0;
    setPref("nous.hibernation.minutes", config.hibernateMinutes);
  }

  function setSearchEngine(engine) {
    if (!SEARCH_ENGINES[engine]) return;
    config.searchEngine = engine;
    setPref("nous.searchEngine", engine);
    if (activeTab?.url === "nous://nova-aba") {
      renderActiveStage();
    }
  }

  function setBlockerEnabled(enabled) {
    config.blockerEnabled = Boolean(enabled);
    setPref("nous.blocker.enabled", config.blockerEnabled);
    window.NousBlocker.setEnabled(config.blockerEnabled);
    applyVisualConfig();
    showToast(config.blockerEnabled ? "Bloqueador de rastreadores ativado" : "Bloqueador pausado");
  }

  function setPerfProfile(profile) {
    config.perfProfile = profile;
    setPref("nous.perfProfile", profile);
    if (profile === "eco") {
      config.hibernateMinutes = 2;
      setPref("nous.hibernation.minutes", 2);
      setPref("dom.ipc.processCount", 1);
      setPref("browser.sessionhistory.max_total_viewers", 1);
      setPref("image.mem.surfacecache.max_size_kb", 16384);
      showToast("Perfil Eco ativado · economia máxima de RAM");
    } else if (profile === "balanced") {
      config.hibernateMinutes = 5;
      setPref("nous.hibernation.minutes", 5);
      setPref("dom.ipc.processCount", 2);
      setPref("browser.sessionhistory.max_total_viewers", 2);
      setPref("image.mem.surfacecache.max_size_kb", 32768);
      showToast("Perfil Equilibrado ativado");
    } else {
      config.hibernateMinutes = 15;
      setPref("nous.hibernation.minutes", 15);
      setPref("dom.ipc.processCount", 4);
      setPref("browser.sessionhistory.max_total_viewers", 4);
      setPref("image.mem.surfacecache.max_size_kb", 65536);
      showToast("Perfil Turbo ativado");
    }
    applyVisualConfig();
  }

  function toggleZenMode() {
    config.zen = !config.zen;
    document.documentElement.dataset.zenReveal = "false";
    applyVisualConfig();
    if (config.zen) {
      showToast("Modo Zen · mova o mouse ao topo ou pressione F11 para sair");
    }
  }

  // -------------------------------------------------------------------------
  // Menus (Popover & Tab Context Menu)
  // -------------------------------------------------------------------------
  function togglePopover(forceState) {
    const pop = byId("menu-popover");
    const open = typeof forceState === "boolean" ? forceState : pop.hidden;
    pop.hidden = !open;
    byId("btn-menu").setAttribute("aria-expanded", open ? "true" : "false");
  }

  function openTabContextMenu(tab, x, y) {
    contextTargetTab = tab;
    const menu = byId("tab-context-menu");
    byId("ctx-pin-label").textContent = tab.pinned ? "Desafixar aba" : "Fixar aba";
    byId("ctx-mute-label").textContent = tab.muted ? "Ativar som da aba" : "Silenciar aba";
    const canHibernate = tab !== activeTab && !isInternalUrl(tab.url);
    const hBtn = byId("ctx-hibernate-label");
    hBtn.hidden = !canHibernate;
    hBtn.textContent = tab.sleeping ? "Acordar aba" : "Hibernar aba (liberar RAM)";

    menu.hidden = false;
    const maxX = window.innerWidth - menu.offsetWidth - 8;
    const maxY = window.innerHeight - menu.offsetHeight - 8;
    menu.style.left = `${Math.max(8, Math.min(x, maxX))}px`;
    menu.style.top = `${Math.max(8, Math.min(y, maxY))}px`;
  }

  function closeTabContextMenu() {
    byId("tab-context-menu").hidden = true;
    contextTargetTab = null;
  }

  // -------------------------------------------------------------------------
  // Content Actor Registration, Chrome Bridge, Web Context Menu & Fullscreen
  // -------------------------------------------------------------------------
  let activeFullscreenActor = null;

  function registerNousContentActor() {
    try {
      ChromeUtils.registerWindowActor("NousContent", {
        parent: { esModuleURI: "resource://nous/actors/NousContentParent.sys.mjs" },
        child: {
          esModuleURI: "resource://nous/actors/NousContentChild.sys.mjs",
          observers: ["getUserMedia:request"],
          events: {
            DOMContentLoaded: { capture: true },
            pageshow: { capture: true },
            load: { capture: true },
            DOMLinkAdded: {},
            mouseover: {},
            mouseout: {},
            focusin: {},
            focusout: {},
            click: { capture: true },
            auxclick: { capture: true },
            contextmenu: {},
            "MozDOMFullscreen:Request": {},
            "MozDOMFullscreen:Exit": {},
            "MozDOMFullscreen:Entered": {},
            "MozDOMFullscreen:Exited": {},
            MozAfterPaint: {},
          },
        },
        allFrames: true,
      });
    } catch {
      // Already registered by another Nous window.
    }
  }

  function copyTextToClipboard(text) {
    if (!text) return;
    try {
      const helper = Cc["@mozilla.org/widget/clipboardhelper;1"].getService(Ci.nsIClipboardHelper);
      helper.copyString(text);
    } catch {}
  }

  function closePageContextMenu() {
    const el = byId("page-context-menu");
    if (el) el.hidden = true;
  }

  function openPageContextMenu(browser, data, actor) {
    const menu = byId("page-context-menu");
    if (!menu) return;
    const tab = tabs.find(t => t.browser === browser) || activeTab;
    const frag = document.createDocumentFragment();

    const addItem = (label, kbd, onClick, disabled = false) => {
      const btn = document.createElementNS(HTML_NS, "button");
      btn.type = "button";
      btn.className = "ctx-item";
      if (disabled) btn.disabled = true;
      const lbl = document.createElementNS(HTML_NS, "span");
      lbl.textContent = label;
      btn.append(lbl);
      if (kbd) {
        const k = document.createElementNS(HTML_NS, "span");
        k.className = "ctx-kbd";
        k.textContent = kbd;
        btn.append(k);
      }
      btn.addEventListener("click", () => {
        closePageContextMenu();
        onClick();
      });
      frag.append(btn);
    };
    const addSep = () => {
      const sep = document.createElementNS(HTML_NS, "div");
      sep.className = "popover-divider";
      frag.append(sep);
    };

    if (data.linkUrl) {
      addItem("Abrir link em nova aba", "Ctrl+Clique", () => createTab(data.linkUrl, { activate: false }));
      addItem("Abrir link em primeiro plano", "", () => createTab(data.linkUrl, { activate: true }));
      addItem("Copiar endereço do link", "", () => {
        copyTextToClipboard(data.linkUrl);
        showToast("Link copiado");
      });
      addItem("Salvar link como…", "", () => window.NousDownloads.saveUrlWithPicker(window, data.linkUrl, "", isPrivateWindow));
      addSep();
    }

    if (data.mediaType === "image" && data.mediaUrl) {
      addItem("Abrir imagem em nova aba", "", () => createTab(data.mediaUrl, { activate: true }));
      addItem("Salvar imagem como…", "", () => window.NousDownloads.saveUrlWithPicker(window, data.mediaUrl, "", isPrivateWindow));
      addItem("Copiar endereço da imagem", "", () => {
        copyTextToClipboard(data.mediaUrl);
        showToast("Endereço da imagem copiado");
      });
      addSep();
    }

    if (data.selectionText) {
      addItem("Copiar", "Ctrl+C", () => {
        copyTextToClipboard(data.selectionText);
        showToast("Texto copiado");
      });
      const eng = SEARCH_ENGINES[config.searchEngine] || SEARCH_ENGINES.duckduckgo;
      addItem(`Pesquisar "${data.selectionText.slice(0, 22)}${data.selectionText.length > 22 ? "…" : ""}"`, "", () => {
        createTab(eng.url(data.selectionText), { activate: true });
      });
      addSep();
    }

    if (data.isEditable) {
      addItem("Recortar", "Ctrl+X", () => actor?.sendAsyncMessage("Nous:ExecCommand", { command: "cut" }));
      addItem("Copiar", "Ctrl+C", () => actor?.sendAsyncMessage("Nous:ExecCommand", { command: "copy" }));
      addItem("Colar", "Ctrl+V", () => actor?.sendAsyncMessage("Nous:ExecCommand", { command: "paste" }));
      addItem("Selecionar tudo", "Ctrl+A", () => actor?.sendAsyncMessage("Nous:ExecCommand", { command: "selectAll" }));
      addSep();
    }

    addItem("Voltar", "Alt+←", () => browser?.goBack(), !browser?.canGoBack);
    addItem("Avançar", "Alt+→", () => browser?.goForward(), !browser?.canGoForward);
    addItem("Recarregar", "F5", () => browser?.reload());
    addSep();
    addItem("Modo Leitura", "Ctrl+Alt+R", () => toggleReaderMode());
    addItem("Salvar página como…", "Ctrl+S", () => saveActivePage());
    addItem("Imprimir / Salvar PDF", "Ctrl+P", () => printActivePage());
    addItem("Ver código-fonte", "Ctrl+U", () => viewActiveSource());
    addItem("Inspecionar elemento", "F12", () => openDevTools());

    menu.replaceChildren(frag);
    menu.hidden = false;

    const localX = Math.round((data.screenX || 0) - window.mozInnerScreenX);
    const localY = Math.round((data.screenY || 0) - window.mozInnerScreenY);
    const maxX = window.innerWidth - menu.offsetWidth - 8;
    const maxY = window.innerHeight - menu.offsetHeight - 8;
    menu.style.left = `${Math.max(8, Math.min(localX, maxX))}px`;
    menu.style.top = `${Math.max(8, Math.min(localY, maxY))}px`;
    if (tab) tab.lastActiveAt = Date.now();
  }

  function exitDOMFullscreen() {
    document.documentElement.dataset.domFullscreen = "false";
    if (window.fullScreen) {
      window.fullScreen = false;
    }
    if (activeFullscreenActor) {
      try {
        activeFullscreenActor.sendAsyncMessage("Nous:DOMFullscreen:CleanUp", {});
      } catch {}
      activeFullscreenActor = null;
    }
  }

  const permissionQueue = [];
  let activePermissionPrompt = null;

  function renderPermissionPrompt() {
    const bar = byId("permission-bar");
    if (!bar) return;
    if (!activePermissionPrompt) {
      activePermissionPrompt = permissionQueue.shift() || null;
    }
    if (!activePermissionPrompt) {
      bar.hidden = true;
      return;
    }
    byId("perm-host").textContent = activePermissionPrompt.host || "SITE";
    byId("perm-msg").textContent = activePermissionPrompt.message || "solicita permissão.";
    bar.hidden = false;
  }

  function enqueuePermissionPrompt(promptObj) {
    permissionQueue.push(promptObj);
    if (!activePermissionPrompt) {
      renderPermissionPrompt();
    }
  }

  function resolvePermissionPrompt(allow) {
    const current = activePermissionPrompt;
    activePermissionPrompt = null;
    if (current) {
      try {
        if (allow) current.onAllow?.();
        else current.onDeny?.();
      } catch {}
    }
    renderPermissionPrompt();
  }

  window.NousChromeBridge = {
    onFavicon(browser, data) {
      if (!data?.iconUrl) return;
      const tab = tabs.find(t => t.browser === browser);
      if (!tab) return;
      try {
        const origin = new URL(data.pageUrl || tab.url).origin;
        faviconCache.set(origin, data.iconUrl);
      } catch {}
      if (tab.favicon !== data.iconUrl) {
        tab.favicon = data.iconUrl;
        renderTabs();
      }
    },
    onLinkHover(browser, url) {
      if (browser !== activeTab?.browser) return;
      const pill = byId("link-hover-pill");
      if (!pill) return;
      if (url) {
        pill.textContent = url;
        pill.hidden = false;
      } else {
        pill.hidden = true;
      }
    },
    onOpenLink(_browser, data) {
      if (!data?.url) return;
      createTab(data.url, { activate: !data.inBackground });
    },
    onContextMenu(browser, data, actor) {
      openPageContextMenu(browser, data, actor);
    },
    onDOMFullscreenRequest(browser, actor) {
      if (browser !== activeTab?.browser) return;
      activeFullscreenActor = actor;
      document.documentElement.dataset.domFullscreen = "true";
      window.fullScreen = true;
      try {
        actor.sendAsyncMessage("Nous:DOMFullscreen:Entered", {});
      } catch {}
    },
    onDOMFullscreenExit() {
      exitDOMFullscreen();
    },
    onDOMFullscreenPainted() {},
    onMediaRequest(_browser, data, actor) {
      if (!data?.callID) return;
      const parts = [];
      if (data.screen) parts.push("Compartilhamento de tela");
      if (data.video) parts.push("Câmera");
      if (data.audio) parts.push("Microfone");
      const what = parts.length ? parts.join(" e ") : "mídia";
      enqueuePermissionPrompt({
        host: data.host || "Este site",
        message: `solicita acesso a ${what}.`,
        onAllow: () => actor?.sendAsyncMessage("Nous:MediaResponse", { callID: data.callID, allow: true }),
        onDeny: () => actor?.sendAsyncMessage("Nous:MediaResponse", { callID: data.callID, allow: false }),
      });
    },
  };

  // -------------------------------------------------------------------------
  // Site Security Popover, Downloads Popover, Clear Data, Tab Palette, Tools
  // -------------------------------------------------------------------------
  function toggleSitePopover(forceState) {
    const pop = byId("site-popover");
    const open = typeof forceState === "boolean" ? forceState : pop.hidden;
    if (!open) {
      pop.hidden = true;
      return;
    }
    togglePopover(false);
    toggleDownloadsPopover(false);

    const url = activeTab?.url || "nous://nova-aba";
    const isInt = isInternalUrl(url);
    const host = isInt ? url : hostOf(url);
    byId("site-pop-kicker").textContent = isInt
      ? "PÁGINA INTERNA NOUS"
      : url.startsWith("https://")
      ? "CONEXÃO SEGURA · HTTPS"
      : "CONEXÃO SEM CRIPTOGRAFIA · HTTP";
    byId("site-pop-host").textContent = host;

    const body = byId("site-pop-body");
    body.replaceChildren();

    if (isInt) {
      const note = document.createElementNS(HTML_NS, "div");
      note.className = "menu-row";
      note.textContent = "Renderizada localmente com 0 MB de conteúdo.";
      body.append(note);
    } else {
      const allowed = window.NousBlocker.isHostAllowed(host);
      const blockerRow = document.createElementNS(HTML_NS, "button");
      blockerRow.type = "button";
      blockerRow.className = "menu-row";
      blockerRow.innerHTML = "";
      const bLbl = document.createElementNS(HTML_NS, "span");
      bLbl.textContent = "Bloqueador neste site";
      const bPill = document.createElementNS(HTML_NS, "span");
      bPill.className = `menu-pill${allowed || !config.blockerEnabled ? " off" : ""}`;
      bPill.textContent = allowed ? "Pausado" : config.blockerEnabled ? "Ativo" : "Global off";
      blockerRow.append(bLbl, bPill);
      blockerRow.addEventListener("click", () => {
        window.NousBlocker.setHostAllowed(host, !allowed);
        persisted.allowlist = window.NousBlocker.getAllowedHosts();
        scheduleSaveState();
        showToast(!allowed ? `Bloqueador pausado em ${host}` : `Bloqueador reativado em ${host}`);
        activeTab?.browser?.reload();
        toggleSitePopover(false);
      });
      body.append(blockerRow);

      const countRow = document.createElementNS(HTML_NS, "div");
      countRow.className = "menu-row";
      const cLbl = document.createElementNS(HTML_NS, "span");
      cLbl.textContent = "Rastreadores barrados na aba";
      const cVal = document.createElementNS(HTML_NS, "span");
      cVal.className = "mono";
      cVal.textContent = String(activeTab?.blockedCount || 0);
      countRow.append(cLbl, cVal);
      body.append(countRow);

      const perms = window.NousPermissions?.getSitePermissions(url) || [];
      for (const p of perms) {
        const pr = document.createElementNS(HTML_NS, "button");
        pr.type = "button";
        pr.className = "menu-row";
        const pl = document.createElementNS(HTML_NS, "span");
        pl.textContent = p.label;
        const pv = document.createElementNS(HTML_NS, "span");
        pv.className = `menu-pill${p.allowed ? "" : " off"}`;
        pv.textContent = p.allowed ? "Permitido · Limpar" : "Bloqueado · Limpar";
        pr.append(pl, pv);
        pr.addEventListener("click", () => {
          window.NousPermissions.setSitePermission(url, p.type, "reset");
          showToast(`Permissão de ${p.label} redefinida`);
          toggleSitePopover(true);
        });
        body.append(pr);
      }

      const darkEx = isDarkWebExcluded(host);
      const darkRow = document.createElementNS(HTML_NS, "button");
      darkRow.type = "button";
      darkRow.className = "menu-row";
      const dLbl = document.createElementNS(HTML_NS, "span");
      dLbl.textContent = "Escuro adaptativo neste site";
      const dPill = document.createElementNS(HTML_NS, "span");
      dPill.className = `menu-pill${darkEx || !config.forceDarkWeb ? " off" : ""}`;
      dPill.textContent = darkEx ? "Desativado" : config.forceDarkWeb ? "Auto" : "Global off";
      darkRow.append(dLbl, dPill);
      darkRow.addEventListener("click", () => {
        toggleDarkWebForHost(host);
        showToast(!darkEx ? `Escuro adaptativo desativado em ${host}` : `Escuro adaptativo reativado em ${host}`);
        toggleSitePopover(false);
      });
      body.append(darkRow);

      const clrRow = document.createElementNS(HTML_NS, "button");
      clrRow.type = "button";
      clrRow.className = "menu-row";
      const cl = document.createElementNS(HTML_NS, "span");
      cl.textContent = "Limpar cookies e dados deste site";
      const ca = document.createElementNS(HTML_NS, "span");
      ca.className = "mono";
      ca.textContent = "Limpar →";
      clrRow.append(cl, ca);
      clrRow.addEventListener("click", async () => {
        await window.NousPermissions.clearSiteData(url);
        showToast(`Cookies e dados de ${host} removidos`);
        toggleSitePopover(false);
      });
      body.append(clrRow);
    }

    pop.hidden = false;
  }

  function syncDownloadsUi(snap) {
    const btn = byId("btn-downloads");
    const lbl = byId("dl-pill-label");
    if (!btn || !lbl) return;
    if (snap.activeCount > 0) {
      btn.hidden = false;
      lbl.textContent = `↓ ${snap.activePercent}%`;
    } else if (snap.items.length > 0) {
      btn.hidden = false;
      lbl.textContent = "↓";
    } else {
      btn.hidden = true;
    }

    const pop = byId("downloads-popover");
    if (pop && !pop.hidden) {
      renderDownloadsPopover(snap);
    }
    if (activeTab?.url === "nous://downloads") {
      renderActiveStage();
    }
  }

  function renderDownloadsPopover(snap) {
    const list = byId("downloads-popover-list");
    if (!list) return;
    list.replaceChildren();
    if (!snap.items.length) {
      const empty = document.createElementNS(HTML_NS, "div");
      empty.className = "dl-pop-meta";
      empty.textContent = "Nenhum download recente.";
      list.append(empty);
      return;
    }
    for (const dl of snap.items.slice(0, 5)) {
      const card = document.createElementNS(HTML_NS, "div");
      card.className = "dl-pop-item";
      const top = document.createElementNS(HTML_NS, "div");
      top.className = "dl-pop-row";
      const name = document.createElementNS(HTML_NS, "span");
      name.className = "dl-pop-name";
      name.textContent = dl.name;
      const act = document.createElementNS(HTML_NS, "button");
      act.type = "button";
      act.className = "dl-pop-link mono";
      if (dl.state === "done") {
        act.textContent = "Abrir ↗";
        act.addEventListener("click", () => window.NousDownloads.openFile(dl.id));
      } else {
        act.textContent = `${dl.percent}%`;
        act.addEventListener("click", () => window.NousDownloads.pauseOrResume(dl.id));
      }
      top.append(name, act);
      const meta = document.createElementNS(HTML_NS, "div");
      meta.className = "dl-pop-meta mono";
      meta.textContent = [dl.sizeLabel, dl.speedLabel, dl.state === "done" ? "Concluído" : ""].filter(Boolean).join(" · ");
      card.append(top, meta);
      if (dl.state === "downloading") {
        const bar = document.createElementNS(HTML_NS, "div");
        bar.className = "dl-bar";
        const fill = document.createElementNS(HTML_NS, "div");
        fill.className = "dl-fill";
        fill.style.width = `${dl.percent}%`;
        bar.append(fill);
        card.append(bar);
      }
      list.append(card);
    }
  }

  function toggleDownloadsPopover(forceState) {
    const pop = byId("downloads-popover");
    const open = typeof forceState === "boolean" ? forceState : pop.hidden;
    if (!open) {
      pop.hidden = true;
      return;
    }
    togglePopover(false);
    toggleSitePopover(false);
    window.NousDownloads.ensureInitialized().then(() => {
      renderDownloadsPopover(window.NousDownloads.getSnapshot());
    });
    pop.hidden = false;
  }

  let selectedClearRangeMs = 3600000;
  function openClearDataModal() {
    togglePopover(false);
    byId("clear-data-backdrop").hidden = false;
  }
  function closeClearDataModal() {
    byId("clear-data-backdrop").hidden = true;
  }

  function openTabPalette() {
    togglePopover(false);
    byId("palette-backdrop").hidden = false;
    const inp = byId("palette-input");
    inp.value = "";
    renderTabPalette("");
    inp.focus();
  }
  function closeTabPalette() {
    byId("palette-backdrop").hidden = true;
  }
  function renderTabPalette(query) {
    const q = query.trim().toLowerCase();
    const list = byId("palette-list");
    list.replaceChildren();
    const entries = [];
    for (const t of tabs) {
      if (!q || t.title.toLowerCase().includes(q) || t.url.toLowerCase().includes(q)) {
        entries.push({
          kind: t.sleeping ? "Hibernando" : "Aba",
          title: t.title,
          sub: t.url,
          run: () => selectTab(t),
        });
      }
    }
    for (const c of closedTabs.slice(0, 6)) {
      if (!q || c.title.toLowerCase().includes(q) || c.url.toLowerCase().includes(q)) {
        entries.push({
          kind: "Fechada",
          title: c.title,
          sub: c.url,
          run: () => createTab(c.url, { activate: true }),
        });
      }
    }
    const quickCmds = [
      { kind: "Ação", title: "Nova janela privada", sub: "Ctrl+Shift+P", run: () => openNewWindow(true) },
      { kind: "Ação", title: "Downloads", sub: "nous://downloads", run: () => navigateActive("nous://downloads") },
      { kind: "Ação", title: "Histórico", sub: "nous://historico", run: () => navigateActive("nous://historico") },
      { kind: "Ação", title: "Favoritos", sub: "nous://favoritos", run: () => navigateActive("nous://favoritos") },
      { kind: "Ação", title: "Monitor de Memória", sub: "nous://memoria", run: () => navigateActive("nous://memoria") },
      { kind: "Ação", title: "Hibernar abas inativas", sub: "Liberar RAM", run: hibernateInactiveTabs },
      { kind: "Ação", title: "Otimizar RAM agora", sub: "GC Gecko", run: optimizeMemoryNow },
      { kind: "Ação", title: "Limpar dados de navegação", sub: "Ctrl+Shift+Del", run: openClearDataModal },
    ];
    for (const cmd of quickCmds) {
      if (!q || cmd.title.toLowerCase().includes(q) || cmd.sub.toLowerCase().includes(q)) {
        entries.push(cmd);
      }
    }
    entries.slice(0, 12).forEach((item, idx) => {
      const btn = document.createElementNS(HTML_NS, "button");
      btn.type = "button";
      btn.className = `omni-item${idx === 0 ? " selected" : ""}`;
      const k = document.createElementNS(HTML_NS, "span");
      k.className = "omni-kind";
      k.textContent = item.kind;
      const m = document.createElementNS(HTML_NS, "span");
      m.className = "omni-main";
      const tt = document.createElementNS(HTML_NS, "span");
      tt.className = "omni-title";
      tt.textContent = item.title;
      const ss = document.createElementNS(HTML_NS, "span");
      ss.className = "omni-sub";
      ss.textContent = item.sub;
      m.append(tt, ss);
      btn.append(k, m);
      btn.addEventListener("click", () => {
        closeTabPalette();
        item.run();
      });
      list.append(btn);
    });
  }

  function openNewWindow(privateMode = false) {
    try {
      const features = privateMode
        ? "chrome,dialog=no,resizable,all,private"
        : "chrome,dialog=no,resizable,all,non-private";
      Services.ww.openWindow(null, "chrome://nous/content/window.xhtml", "_blank", features, null);
    } catch {
      showToast("Não foi possível abrir nova janela");
    }
  }

  function printActivePage() {
    if (!activeTab?.browser) {
      showToast("Abra uma página web para imprimir");
      return;
    }
    try {
      const bc = activeTab.browser.browsingContext;
      const ps = Cc["@mozilla.org/gfx/printsettings-service;1"].getService(Ci.nsIPrintSettingsService);
      const settings = ps.createNewPrintSettings();
      bc.print(settings);
    } catch {
      showToast("Impressão indisponível nesta página");
    }
  }

  function saveActivePage() {
    if (!activeTab || isInternalUrl(activeTab.url)) {
      showToast("Abra uma página web para salvar");
      return;
    }
    const name = `${(activeTab.title || hostOf(activeTab.url) || "pagina").replace(/[\\/:*?"<>|]+/g, "_")}.html`;
    window.NousDownloads.saveUrlWithPicker(window, activeTab.url, name, isPrivateWindow);
  }

  function viewActiveSource() {
    if (!activeTab || isInternalUrl(activeTab.url) || activeTab.url.startsWith("view-source:")) return;
    createTab(`view-source:${activeTab.url}`, { activate: true });
  }

  function toggleReaderMode() {
    if (!activeTab || isInternalUrl(activeTab.url)) return;
    if (activeTab.url.startsWith("about:reader?url=")) {
      try {
        const orig = decodeURIComponent(activeTab.url.slice("about:reader?url=".length));
        loadTabUrl(activeTab, orig);
      } catch {}
    } else if (activeTab.url.startsWith("http://") || activeTab.url.startsWith("https://")) {
      loadTabUrl(activeTab, `about:reader?url=${encodeURIComponent(activeTab.url)}`);
    }
  }

  function openDevTools() {
    try {
      const { BrowserConsoleManager } = ChromeUtils.importESModule("resource://devtools/client/webconsole/browser-console-manager.sys.mjs");
      BrowserConsoleManager.openBrowserConsoleOrFocus();
    } catch {
      viewActiveSource();
      showToast("Código-fonte aberto em nova aba");
    }
  }

  async function importBookmarksHtml() {
    try {
      const fp = Cc["@mozilla.org/filepicker;1"].createInstance(Ci.nsIFilePicker);
      fp.init(window.browsingContext, "Importar favoritos (HTML)", Ci.nsIFilePicker.modeOpen);
      fp.appendFilters(Ci.nsIFilePicker.filterHTML | Ci.nsIFilePicker.filterAll);
      const res = await new Promise(resolve => fp.open(resolve));
      if (res !== Ci.nsIFilePicker.returnOK) return;
      const html = readUtf8File(fp.file) || "";
      const re = /<a\b[^>]*href=["']([^"']+)["'][^>]*>([^<]+)<\/a>/gi;
      let added = 0;
      let m;
      while ((m = re.exec(html)) !== null) {
        const url = m[1].trim();
        const title = m[2].trim();
        if (!/^https?:\/\//i.test(url)) continue;
        if (!persisted.bookmarks.some(b => b.url === url)) {
          persisted.bookmarks.push({ title: title || hostOf(url), url });
          added += 1;
        }
      }
      if (persisted.bookmarks.length > 256) persisted.bookmarks.length = 256;
      scheduleSaveState();
      showToast(added > 0 ? `${added} favoritos importados` : "Nenhum novo favorito encontrado");
    } catch {
      showToast("Erro ao importar favoritos");
    }
  }

  async function exportBookmarksHtml() {
    try {
      const fp = Cc["@mozilla.org/filepicker;1"].createInstance(Ci.nsIFilePicker);
      fp.init(window.browsingContext, "Exportar favoritos (HTML)", Ci.nsIFilePicker.modeSave);
      fp.defaultString = "nous-favoritos.html";
      fp.appendFilters(Ci.nsIFilePicker.filterHTML);
      const res = await new Promise(resolve => fp.open(resolve));
      if (res !== Ci.nsIFilePicker.returnOK && res !== Ci.nsIFilePicker.returnReplace) return;
      const esc = s => String(s || "").replace(/[&<>"]/g, c => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
      const lines = [
        "<!DOCTYPE NETSCAPE-Bookmark-file-1>",
        '<META HTTP-EQUIV="Content-Type" CONTENT="text/html; charset=UTF-8">',
        "<TITLE>Favoritos do Nous</TITLE>",
        "<H1>Favoritos do Nous</H1>",
        "<DL><p>",
        ...persisted.bookmarks.map(b => `    <DT><A HREF="${esc(b.url)}">${esc(b.title)}</A>`),
        "</DL><p>",
      ];
      writeUtf8File(fp.file, lines.join("\n"));
      showToast("Favoritos exportados em HTML");
    } catch {
      showToast("Erro ao exportar favoritos");
    }
  }

  // -------------------------------------------------------------------------
  // Native Gecko nsIBrowserDOMWindow (handles target="_blank" & window.open)
  // -------------------------------------------------------------------------
  function installBrowserDOMWindow() {
    const openInTab = (uri, openWindowInfo, where, skipLoad) => {
      const bg = where === Ci.nsIBrowserDOMWindow.OPEN_NEWTAB_BACKGROUND;
      const spec = uri?.spec || "about:blank";
      const tab = createTab(spec, {
        activate: !bg,
        openWindowInfo: openWindowInfo || null,
        skipLoad: Boolean(skipLoad) || !uri,
      });
      if (!tab.browser) {
        attachBrowser(tab, openWindowInfo || null);
      }
      return tab;
    };

    window.browserDOMWindow = {
      QueryInterface: ChromeUtils.generateQI(["nsIBrowserDOMWindow"]),
      createContentWindow(_uri, openWindowInfo, where) {
        return openInTab(null, openWindowInfo, where, true).browser?.browsingContext ?? null;
      },
      openURI(uri, openWindowInfo, where) {
        return openInTab(uri, openWindowInfo, where, false).browser?.browsingContext ?? null;
      },
      createContentWindowInFrame(_uri, params, where) {
        return openInTab(null, params?.openWindowInfo, where, true).browser ?? null;
      },
      openURIInFrame(uri, params, where) {
        return openInTab(uri, params?.openWindowInfo, where, false).browser ?? null;
      },
      canClose() {
        return true;
      },
      get tabCount() {
        return tabs.length;
      },
    };
  }

  // -------------------------------------------------------------------------
  // Internal Pages API Bridge
  // -------------------------------------------------------------------------
  const internalApi = {
    navigateActive,
    getConfig: () => ({ ...config }),
    getSearchEngineLabel: () => (SEARCH_ENGINES[config.searchEngine] || SEARCH_ENGINES.duckduckgo).label,
    getSpeedDial: () => persisted.bookmarks.slice(0, 12),
    getAllBookmarks: () => persisted.bookmarks.slice(),
    importBookmarksHtml,
    exportBookmarksHtml,
    getHistory: () => persisted.history.slice(),
    removeHistoryUrl(url) {
      persisted.history = persisted.history.filter(h => h.url !== url);
      scheduleSaveState();
    },
    clearHistory() {
      persisted.history = [];
      scheduleSaveState();
      showToast("Histórico limpo");
    },
    getDownloads: () => window.NousDownloads.getSnapshot(),
    pauseOrResumeDownload: id => window.NousDownloads.pauseOrResume(id),
    cancelDownload: id => window.NousDownloads.cancelDownload(id),
    openDownloadFile: id => window.NousDownloads.openFile(id),
    showDownloadInFolder: id => window.NousDownloads.showInFolder(id),
    clearFinishedDownloads: () => window.NousDownloads.clearFinished(),
    addBookmark(title, url) {
      if (!url) return;
      persisted.bookmarks = persisted.bookmarks.filter(b => b.url !== url);
      persisted.bookmarks.push({ title: title || hostOf(url), url });
      if (persisted.bookmarks.length > 64) persisted.bookmarks.length = 64;
      scheduleSaveState();
      showToast("Atalho adicionado");
    },
    removeBookmark(url) {
      persisted.bookmarks = persisted.bookmarks.filter(b => b.url !== url);
      scheduleSaveState();
    },
    getStats() {
      let liveTabs = 0;
      let sleepingTabs = 0;
      let internalTabs = 0;
      for (const t of tabs) {
        if (isInternalUrl(t.url)) internalTabs += 1;
        else if (t.sleeping) sleepingTabs += 1;
        else liveTabs += 1;
      }
      if (latestRamMb === null) {
        const current = readRamMb();
        if (current !== null) latestRamMb = current;
      }
      return {
        ramLabel: latestRamMb !== null ? `${latestRamMb} MB` : "— MB",
        engineLabel: getGeckoEngineLabel(),
        totalTabs: tabs.length,
        liveTabs,
        sleepingTabs,
        internalTabs,
        blockedTotal: window.NousBlocker.totalBlocked,
        blockerDomains: window.NousBlocker.domainCount,
        perfProfile: config.perfProfile,
        hibernateMinutes: config.hibernateMinutes,
      };
    },
    getTabList: () =>
      tabs.map(t => ({
        id: t.id,
        title: t.title,
        url: t.url,
        isActive: t === activeTab,
        isInternal: isInternalUrl(t.url),
        sleeping: t.sleeping,
        blocked: t.blockedCount,
      })),
    selectTabById: id => selectTab(tabs.find(t => t.id === id)),
    hibernateTabById: id => hibernateTab(tabs.find(t => t.id === id)),
    wakeTabById: id => wakeTab(tabs.find(t => t.id === id)),
    closeTabById: id => closeTab(tabs.find(t => t.id === id)),
    hibernateInactiveTabs,
    optimizeMemory: optimizeMemoryNow,
    refreshMemoryNow: updateRamIndicator,
    setTheme,
    setLayout,
    setDensity,
    setAnimations,
    setPerfProfile,
    setHibernateMinutes,
    setBlockerEnabled,
    setForceDarkWeb,
    isGoogleSignedIn,
    openGoogleAccount,
    setSearchEngine,
  };

  // -------------------------------------------------------------------------
  // Custom Frameless Window Controls
  // -------------------------------------------------------------------------
  function syncWindowStateAttribute() {
    const maximized = window.windowState === window.STATE_MAXIMIZED;
    document.documentElement.setAttribute("sizemode", maximized ? "maximized" : "normal");
  }

  function handleWindowAction(action) {
    if (action === "minimize") {
      window.minimize();
    } else if (action === "maximize") {
      if (window.windowState === window.STATE_MAXIMIZED) {
        window.restore();
      } else {
        window.maximize();
      }
      syncWindowStateAttribute();
    } else if (action === "close") {
      window.close();
    }
  }

  // -------------------------------------------------------------------------
  // Background Schedulers (Ultra-Low Idle CPU — paused when window unfocused)
  // -------------------------------------------------------------------------
  function startLightweightSchedulers() {
    // Poll RAM gently every 8s and refresh NTP clock when visible.
    updateRamIndicator();
    setInterval(() => {
      if (document.hidden) return;
      updateRamIndicator();
      const stage = byId("internal-stage");
      if (stage && !stage.hidden && window.NousInternalPages?.formatKansoClock) {
        const clk = stage.querySelector("[data-live-clock]");
        if (clk) clk.textContent = window.NousInternalPages.formatKansoClock();
      }
    }, 8000);

    // Auto-hibernate check once every 30s.
    setInterval(() => {
      if (config.hibernateMinutes <= 0) return;
      const thresholdMs = config.hibernateMinutes * 60 * 1000;
      const now = Date.now();
      for (const tab of tabs) {
        if (
          tab !== activeTab &&
          !tab.pinned &&
          !tab.sleeping &&
          tab.browser &&
          !isInternalUrl(tab.url) &&
          now - tab.lastActiveAt >= thresholdMs
        ) {
          hibernateTab(tab, true);
        }
      }
    }, 30000);
  }

  // -------------------------------------------------------------------------
  // Initialization & Event Wiring
  // -------------------------------------------------------------------------
  function focusAddress() {
    const addr = byId("address");
    if (config.zen) {
      document.documentElement.dataset.zenReveal = "true";
    }
    addr.focus();
    addr.select();
    renderOmniboxDropdown(addr.value);
  }

  window.addEventListener("load", () => {
    registerNousContentActor();
    loadPersistedState();
    installBrowserDOMWindow();

    if (isPrivateWindow) {
      document.documentElement.dataset.private = "true";
      const pb = byId("private-badge");
      if (pb) pb.hidden = false;
    }

    // Subscribe to native Gecko Downloads lazily & install Kanso permission prompt handler
    window.NousDownloads.subscribe(syncDownloadsUi);
    window.NousPermissions?.installPromptHandler(enqueuePermissionPrompt);
    byId("btn-perm-allow")?.addEventListener("click", () => resolvePermissionPrompt(true));
    byId("btn-perm-deny")?.addEventListener("click", () => resolvePermissionPrompt(false));

    // Install lightweight tracker blocker
    window.NousBlocker.install(config.blockerEnabled);
    window.NousBlocker.onBlocked = ({ browsingContextId }) => {
      let target = null;
      if (browsingContextId !== null) {
        target = tabs.find(t => t.browser?.browsingContext?.id === browsingContextId) || null;
      }
      if (!target && activeTab && !isInternalUrl(activeTab.url)) {
        target = activeTab;
      }
      if (target) {
        target.blockedCount += 1;
        if (target === activeTab) {
          byId("shield-count").textContent = String(target.blockedCount);
          byId("btn-shield")?.classList.add("has-blocks");
        }
      }
    };

    applyVisualConfig();
    syncWindowStateAttribute();

    // Window controls
    document.querySelectorAll("[data-win-action]").forEach(btn => {
      btn.addEventListener("click", () => handleWindowAction(btn.dataset.winAction));
    });
    byId("titlebar-drag-spacer")?.addEventListener("dblclick", e => {
      if (e.target === e.currentTarget) handleWindowAction("maximize");
    });
    window.addEventListener("sizemodechange", syncWindowStateAttribute);
    window.addEventListener("resize", syncWindowStateAttribute);

    // Navigation & toolbar buttons
    byId("btn-back").addEventListener("click", () => activeTab?.browser?.goBack());
    byId("btn-forward").addEventListener("click", () => activeTab?.browser?.goForward());
    byId("btn-reload").addEventListener("click", () => {
      if (!activeTab) return;
      if (isInternalUrl(activeTab.url)) {
        renderActiveStage();
      } else if (activeTab.loading) {
        activeTab.browser?.stop();
      } else {
        activeTab.browser?.reload();
      }
    });
    byId("btn-brand-home")?.addEventListener("click", () => navigateActive("nous://nova-aba"));
    byId("btn-new-tab-top").addEventListener("click", () => {
      createTab("nous://nova-aba");
      focusAddress();
    });
    byId("btn-new-tab-side").addEventListener("click", () => {
      createTab("nous://nova-aba");
      focusAddress();
    });
    byId("btn-sidebar-nav")?.addEventListener("click", () => setLayout(config.layout === "top" ? "sidebar" : "top"));
    byId("btn-sidebar-collapse").addEventListener("click", () => {
      config.sidebarCollapsed = !config.sidebarCollapsed;
      applyVisualConfig();
    });
    byId("btn-side-sleep").addEventListener("click", hibernateInactiveTabs);
    byId("btn-side-memory").addEventListener("click", () => navigateActive("nous://memoria"));
    byId("btn-side-config").addEventListener("click", () => navigateActive("nous://config"));
    byId("btn-ram-pill").addEventListener("click", () => navigateActive("nous://memoria"));
    byId("btn-zen").addEventListener("click", toggleZenMode);
    byId("btn-reader")?.addEventListener("click", toggleReaderMode);
    byId("btn-bookmark").addEventListener("click", toggleBookmarkCurrent);
    byId("btn-zoom-pill").addEventListener("click", () => adjustZoom(0));
    byId("btn-shield").addEventListener("click", () => setBlockerEnabled(!config.blockerEnabled));
    byId("security-badge")?.addEventListener("click", e => {
      e.stopPropagation();
      toggleSitePopover();
    });
    byId("btn-downloads")?.addEventListener("click", e => {
      e.stopPropagation();
      toggleDownloadsPopover();
    });
    byId("btn-dl-open-page")?.addEventListener("click", () => {
      toggleDownloadsPopover(false);
      navigateActive("nous://downloads");
    });
    byId("btn-menu").addEventListener("click", e => {
      e.stopPropagation();
      toggleSitePopover(false);
      toggleDownloadsPopover(false);
      const g = byId("menu-google-state");
      if (g) {
        const signed = isGoogleSignedIn();
        g.textContent = signed ? "Conectado" : "Entrar";
        g.classList.toggle("off", !signed);
      }
      togglePopover();
    });

    // Horizontal wheel scrolling on single-bar tabstrip
    byId("tabs-top")?.addEventListener("wheel", e => {
      if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
        e.currentTarget.scrollLeft += e.deltaY;
      }
    }, { passive: true });

    // Drag & drop URLs or local files (.html / .pdf / images) onto window
    byId("app-shell")?.addEventListener("dragover", e => {
      if (e.dataTransfer?.types && !e.dataTransfer.types.includes("text/x-nous-tab")) {
        e.preventDefault();
      }
    });
    byId("app-shell")?.addEventListener("drop", e => {
      if (e.dataTransfer?.types?.includes("text/x-nous-tab")) return;
      const files = e.dataTransfer?.files;
      if (files && files.length > 0 && files[0].mozFullPath) {
        e.preventDefault();
        const p = files[0].mozFullPath.replace(/\\/g, "/");
        createTab(`file:///${p}`, { activate: true });
        return;
      }
      const droppedUrl = e.dataTransfer?.getData("text/uri-list") || e.dataTransfer?.getData("text/plain") || "";
      if (droppedUrl.trim()) {
        e.preventDefault();
        try {
          createTab(resolveInput(droppedUrl.split(/\r?\n/)[0]), { activate: true });
        } catch {}
      }
    });

    // Clear browsing data modal
    document.querySelectorAll("[data-clr-range]").forEach(btn => {
      btn.addEventListener("click", () => {
        selectedClearRangeMs = Number(btn.dataset.clrRange) || 0;
        document.querySelectorAll("[data-clr-range]").forEach(b => b.classList.toggle("active", b === btn));
      });
    });
    byId("btn-clr-cancel")?.addEventListener("click", closeClearDataModal);
    byId("clear-data-backdrop")?.addEventListener("click", e => {
      if (e.target === e.currentTarget) closeClearDataModal();
    });
    byId("btn-clr-confirm")?.addEventListener("click", async () => {
      const clearHist = byId("clr-history")?.checked;
      const clearCookies = byId("clr-cookies")?.checked;
      const clearCache = byId("clr-cache")?.checked;
      const clearStorage = byId("clr-storage")?.checked;
      if (clearHist) {
        if (selectedClearRangeMs > 0) {
          const cutoff = Date.now() - selectedClearRangeMs;
          persisted.history = persisted.history.filter(h => (h.ts || 0) < cutoff);
        } else {
          persisted.history = [];
        }
        scheduleSaveState();
      }
      await window.NousPermissions.clearBrowsingData({
        rangeMs: selectedClearRangeMs,
        cookies: Boolean(clearCookies),
        cache: Boolean(clearCache),
        storage: Boolean(clearStorage),
      });
      closeClearDataModal();
      showToast("Dados de navegação limpos");
      if (activeTab?.url === "nous://historico") renderActiveStage();
    });

    // Quick Tab & Command Palette (Ctrl+Shift+A)
    byId("palette-backdrop")?.addEventListener("click", e => {
      if (e.target === e.currentTarget) closeTabPalette();
    });
    byId("palette-input")?.addEventListener("input", e => renderTabPalette(e.target.value));
    byId("palette-input")?.addEventListener("keydown", e => {
      if (e.key === "Escape") {
        closeTabPalette();
      } else if (e.key === "Enter") {
        e.preventDefault();
        byId("palette-list")?.querySelector(".omni-item")?.click();
      }
    });

    // Omnibox form & palette keyboard handling
    const addr = byId("address");
    byId("navigation-form").addEventListener("submit", e => {
      e.preventDefault();
      if (omniSelectedIndex >= 0 && omniResults[omniSelectedIndex]) {
        const chosen = omniResults[omniSelectedIndex];
        hideOmniboxDropdown();
        chosen.action();
      } else {
        navigateActive(addr.value);
      }
    });
    addr.addEventListener("focus", () => {
      addr.select();
      renderOmniboxDropdown(addr.value);
    });
    addr.addEventListener("input", () => renderOmniboxDropdown(addr.value));
    addr.addEventListener("blur", () => {
      setTimeout(hideOmniboxDropdown, 140);
    });
    addr.addEventListener("keydown", e => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        moveOmniboxSelection(1);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        moveOmniboxSelection(-1);
      } else if (e.key === "Enter" && e.altKey) {
        e.preventDefault();
        hideOmniboxDropdown();
        try {
          createTab(resolveInput(addr.value), { activate: true });
        } catch {}
      } else if (e.key === "Enter" && e.ctrlKey) {
        e.preventDefault();
        hideOmniboxDropdown();
        const raw = addr.value.trim();
        if (raw && !raw.includes("://") && !raw.includes(" ")) {
          navigateActive(`https://www.${raw.replace(/\.com$/i, "")}.com`);
        } else {
          navigateActive(raw);
        }
      } else if (e.key === "Escape") {
        hideOmniboxDropdown();
        addr.value = activeTab?.url === "nous://nova-aba" ? "" : (activeTab?.url || "");
        addr.blur();
      }
    });

    // Inline find bar
    const findInput = byId("find-input");
    findInput.addEventListener("input", () => runFind(false, false));
    findInput.addEventListener("keydown", e => {
      if (e.key === "Enter") {
        e.preventDefault();
        runFind(e.shiftKey, true);
      } else if (e.key === "Escape") {
        e.preventDefault();
        closeFindBar();
      }
    });
    byId("find-prev").addEventListener("click", () => runFind(true, true));
    byId("find-next").addEventListener("click", () => runFind(false, true));
    byId("find-close").addEventListener("click", closeFindBar);

    // Popover menu handlers
    document.querySelectorAll("[data-set-theme]").forEach(btn => {
      btn.addEventListener("click", () => setTheme(btn.dataset.setTheme));
    });
    document.querySelectorAll("[data-set-perf]").forEach(btn => {
      btn.addEventListener("click", () => setPerfProfile(btn.dataset.setPerf));
    });
    document.querySelectorAll("[data-set-layout]").forEach(btn => {
      btn.addEventListener("click", () => setLayout(btn.dataset.setLayout));
    });
    document.querySelectorAll("[data-menu-action]").forEach(btn => {
      btn.addEventListener("click", () => {
        const act = btn.dataset.menuAction;
        togglePopover(false);
        if (act === "toggle-blocker") setBlockerEnabled(!config.blockerEnabled);
        else if (act === "google-account") openGoogleAccount();
        else if (act === "toggle-dark-web") setForceDarkWeb(!config.forceDarkWeb);
        else if (act === "new-window") openNewWindow(false);
        else if (act === "private-window") openNewWindow(true);
        else if (act === "open-downloads") {
          window.NousDownloads.ensureInitialized().then(() => navigateActive("nous://downloads"));
        }
        else if (act === "open-history") navigateActive("nous://historico");
        else if (act === "open-bookmarks") navigateActive("nous://favoritos");
        else if (act === "print") printActivePage();
        else if (act === "clear-data") openClearDataModal();
        else if (act === "hibernate-others") hibernateInactiveTabs();
        else if (act === "optimize-ram") optimizeMemoryNow();
        else if (act === "find") openFindBar();
        else if (act === "toggle-zen") toggleZenMode();
        else if (act === "open-memory") navigateActive("nous://memoria");
        else if (act === "open-config") navigateActive("nous://config");
      });
    });

    // Tab context menu actions
    document.querySelectorAll("[data-ctx]").forEach(btn => {
      btn.addEventListener("click", () => {
        const tab = contextTargetTab;
        const act = btn.dataset.ctx;
        closeTabContextMenu();
        if (!tab) return;
        if (act === "new") createTab("nous://nova-aba");
        else if (act === "duplicate") createTab(tab.url);
        else if (act === "pin") togglePinTab(tab);
        else if (act === "mute") toggleMuteTab(tab);
        else if (act === "hibernate") {
          if (tab.sleeping) wakeTab(tab);
          else hibernateTab(tab);
        } else if (act === "close-others") {
          for (const other of tabs.slice()) {
            if (other !== tab && !other.pinned) closeTab(other);
          }
        } else if (act === "close") {
          closeTab(tab);
        }
      });
    });

    // Global click-outside dismissal
    window.addEventListener("click", e => {
      if (!byId("menu-popover").hidden && !byId("menu-popover").contains(e.target) && e.target !== byId("btn-menu")) {
        togglePopover(false);
      }
      if (!byId("site-popover").hidden && !byId("site-popover").contains(e.target) && !byId("security-badge").contains(e.target)) {
        toggleSitePopover(false);
      }
      if (!byId("downloads-popover").hidden && !byId("downloads-popover").contains(e.target) && !byId("btn-downloads").contains(e.target)) {
        toggleDownloadsPopover(false);
      }
      if (!byId("tab-context-menu").hidden && !byId("tab-context-menu").contains(e.target)) {
        closeTabContextMenu();
      }
      if (!byId("page-context-menu").hidden && !byId("page-context-menu").contains(e.target)) {
        closePageContextMenu();
      }
    });

    // Check external single-instance URL queue whenever window receives focus (0 idle CPU!)
    window.addEventListener("focus", consumePendingExternalUrls);

    // Zen mode hover reveal
    byId("zen-hotspot").addEventListener("mouseenter", () => {
      if (config.zen) document.documentElement.dataset.zenReveal = "true";
    });
    byId("chrome-top").addEventListener("mouseleave", () => {
      if (config.zen && document.activeElement !== byId("address")) {
        document.documentElement.dataset.zenReveal = "false";
      }
    });

    // Global keyboard shortcuts
    window.addEventListener("keydown", e => {
      const key = e.key.toLowerCase();
      if (key === "f5") {
        e.preventDefault();
        if (activeTab && !isInternalUrl(activeTab.url)) activeTab.browser?.reload();
        else renderActiveStage();
        return;
      }
      if (key === "f6") {
        e.preventDefault();
        focusAddress();
        return;
      }
      if (key === "f11") {
        e.preventDefault();
        toggleZenMode();
        return;
      }
      if (key === "f12") {
        e.preventDefault();
        openDevTools();
        return;
      }
      if (key === "escape") {
        if (document.documentElement.dataset.domFullscreen === "true") {
          exitDOMFullscreen();
        } else if (activePermissionPrompt) {
          resolvePermissionPrompt(false);
        } else if (!byId("palette-backdrop").hidden) {
          closeTabPalette();
        } else if (!byId("clear-data-backdrop").hidden) {
          closeClearDataModal();
        } else if (!byId("page-context-menu").hidden) {
          closePageContextMenu();
        } else if (!byId("site-popover").hidden) {
          toggleSitePopover(false);
        } else if (!byId("downloads-popover").hidden) {
          toggleDownloadsPopover(false);
        } else if (!byId("find-bar").hidden) {
          closeFindBar();
        } else if (!byId("menu-popover").hidden) {
          togglePopover(false);
        } else if (activeTab?.loading) {
          activeTab.browser?.stop();
        } else if (config.zen) {
          toggleZenMode();
        }
        return;
      }
      if (e.altKey && key === "arrowleft") {
        e.preventDefault();
        activeTab?.browser?.goBack();
        return;
      }
      if (e.altKey && key === "arrowright") {
        e.preventDefault();
        activeTab?.browser?.goForward();
        return;
      }
      if (e.altKey && key === "d") {
        e.preventDefault();
        focusAddress();
        return;
      }
      if (!e.ctrlKey) return;

      if (key === "t" && e.shiftKey) {
        e.preventDefault();
        reopenClosedTab();
      } else if (key === "t") {
        e.preventDefault();
        createTab("nous://nova-aba");
        focusAddress();
      } else if (key === "n" && e.shiftKey) {
        e.preventDefault();
        openNewWindow(true);
      } else if (key === "n") {
        e.preventDefault();
        openNewWindow(false);
      } else if (key === "p" && e.shiftKey) {
        e.preventDefault();
        openNewWindow(true);
      } else if (key === "p") {
        e.preventDefault();
        printActivePage();
      } else if (key === "s") {
        e.preventDefault();
        saveActivePage();
      } else if (key === "u") {
        e.preventDefault();
        viewActiveSource();
      } else if (key === "j") {
        e.preventDefault();
        window.NousDownloads.ensureInitialized().then(() => navigateActive("nous://downloads"));
      } else if (key === "h") {
        e.preventDefault();
        navigateActive("nous://historico");
      } else if (key === "o" && e.shiftKey) {
        e.preventDefault();
        navigateActive("nous://favoritos");
      } else if (key === "a" && e.shiftKey) {
        e.preventDefault();
        openTabPalette();
      } else if (key === "delete" && e.shiftKey) {
        e.preventDefault();
        openClearDataModal();
      } else if (key === "i" && e.shiftKey) {
        e.preventDefault();
        openDevTools();
      } else if (key === "r" && e.altKey) {
        e.preventDefault();
        toggleReaderMode();
      } else if (key === "w") {
        e.preventDefault();
        closeTab(activeTab);
      } else if (key === "l" || key === "k") {
        e.preventDefault();
        focusAddress();
      } else if (key === "f") {
        e.preventDefault();
        openFindBar();
      } else if (key === "d") {
        e.preventDefault();
        toggleBookmarkCurrent();
      } else if (key === "b") {
        e.preventDefault();
        setLayout(config.layout === "top" ? "sidebar" : "top");
      } else if (key === ",") {
        e.preventDefault();
        navigateActive("nous://config");
      } else if (key === "m" && e.shiftKey) {
        e.preventDefault();
        navigateActive("nous://memoria");
      } else if (key === "r") {
        e.preventDefault();
        if (activeTab && !isInternalUrl(activeTab.url)) {
          if (e.shiftKey) loadTabUrl(activeTab, activeTab.url, { bypassCache: true });
          else activeTab.browser?.reload();
        } else {
          renderActiveStage();
        }
      } else if (key === "tab") {
        e.preventDefault();
        const next = (tabs.indexOf(activeTab) + (e.shiftKey ? -1 : 1) + tabs.length) % tabs.length;
        selectTab(tabs[next]);
      } else if (key === "=" || key === "+") {
        e.preventDefault();
        adjustZoom(0.1);
      } else if (key === "-") {
        e.preventDefault();
        adjustZoom(-0.1);
      } else if (key === "0") {
        e.preventDefault();
        adjustZoom(0);
      } else if (/^[1-9]$/.test(key)) {
        e.preventDefault();
        const idx = key === "9" ? tabs.length - 1 : Number(key) - 1;
        if (tabs[idx]) selectTab(tabs[idx]);
      }
    });

    window.addEventListener("unload", () => {
      saveSessionNow();
      window.NousBlocker.uninstall();
      window.NousPermissions?.uninstallPromptHandler();
    });

    // Read RAM before first internal page render so initial paint never displays "— MB".
    updateRamIndicator();

    // Open initial URL from launcher, restore hibernated session, or default nous://nova-aba
    const startRaw = Services.env.get("NOUS_GECKO_START_URL") || "";
    if (startRaw.trim()) {
      try {
        createTab(resolveInput(startRaw), { activate: true });
      } catch {
        createTab("nous://nova-aba", { activate: true });
      }
    } else if (!restoreLazySession()) {
      createTab("nous://nova-aba", { activate: true });
    }
    consumePendingExternalUrls();

    startLightweightSchedulers();

    // Optional deterministic capture hook (only active when NOUS_CAPTURE_PNG env var is set).
    const captureOut = Services.env.get("NOUS_CAPTURE_PNG") || "";
    if (captureOut) {
      const waitMs = Math.max(800, Number(Services.env.get("NOUS_CAPTURE_WAIT_MS") || 2200));
      const openMenu = Services.env.get("NOUS_CAPTURE_MENU") === "1";
      setTimeout(async () => {
        try {
          updateRamIndicator();
          if (openMenu) {
            togglePopover(true);
            await new Promise(r => setTimeout(r, 120));
          }
          const w = Math.max(1, window.innerWidth);
          const h = Math.max(1, window.innerHeight);
          const canvas = document.createElementNS(HTML_NS, "canvas");
          canvas.width = w;
          canvas.height = h;
          const ctx = canvas.getContext("2d");
          const bg = getComputedStyle(document.documentElement).getPropertyValue("--bg-frame").trim() || "#090a10";
          const snap = await window.browsingContext.currentWindowGlobal.drawSnapshot(
            new DOMRect(0, 0, w, h),
            1,
            bg
          );
          ctx.drawImage(snap, 0, 0);
          snap.close();

          if (activeTab?.browser && !activeTab.browser.hidden && activeTab.browser.browsingContext?.currentWindowGlobal) {
            const r = activeTab.browser.getBoundingClientRect();
            if (r.width > 0 && r.height > 0) {
              try {
                const contentSnap = await activeTab.browser.browsingContext.currentWindowGlobal.drawSnapshot(
                  new DOMRect(0, 0, r.width, r.height),
                  1,
                  bg
                );
                ctx.drawImage(contentSnap, r.left, r.top, r.width, r.height);
                contentSnap.close();
              } catch {}
            }
          }

          const blob = await new Promise(res => canvas.toBlob(res, "image/png"));
          const buf = new Uint8Array(await blob.arrayBuffer());
          await IOUtils.write(captureOut, buf);
        } catch (err) {
          try {
            await IOUtils.writeUTF8(captureOut + ".err.txt", String(err && err.stack ? err.stack : err));
          } catch {}
        } finally {
          window.close();
        }
      }, waitMs);
    }
  }, { once: true });
})();
