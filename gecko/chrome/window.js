"use strict";

const HTML = "http://www.w3.org/1999/xhtml";
const WEB_PROGRESS = Ci.nsIWebProgress;
const SECURITY = Services.scriptSecurityManager;
const HOME_HTML = `<!doctype html><meta charset="utf-8"><title>Nova aba</title>
<style>
body { background: #121724; color: #edf1fa; font: 16px "Segoe UI", sans-serif; }
main { max-width: 640px; margin: 12vh auto 0; padding: 24px; }
h1 { font-size: 48px; margin: 0 0 16px; }
p { color: #b6c4dc; line-height: 1.5; }
</style>
<main lang="pt-BR"><h1>Nous</h1>
<p>Digite um endereço ou pesquise na barra acima.</p>
<p id="engine">Carregando a página local.</p></main>
<script>
document.querySelector("#engine").textContent = "Gecko renderizou esta página e executou JavaScript.";
</script>`;
const HOME = "data:text/html;charset=utf-8," + encodeURIComponent(HOME_HTML);
const tabs = [];
let active = null;

const byId = id => document.getElementById(id);

function showMessage(value = "") {
  const message = byId("message");
  message.textContent = value;
  message.hidden = !value;
}

function resolveAddress(input) {
  const value = input.trim();
  if (!value) {
    return HOME;
  }
  if (/^https?:\/\//i.test(value)) {
    return new URL(value).href;
  }
  if (/^(?:localhost|[\p{L}\d-]+(?:\.[\p{L}\d-]+)+)(?::\d+)?(?:[/?#].*)?$/iu.test(value)) {
    const scheme = /^(?:localhost|127\.0\.0\.1)(?::|[/?#]|$)/i.test(value) ? "http" : "https";
    return new URL(`${scheme}://${value}`).href;
  }
  if (/^[a-z][a-z\d+.-]*:/i.test(value)) {
    throw new Error("Use um endereço HTTP ou HTTPS.");
  }
  return "https://duckduckgo.com/?q=" + encodeURIComponent(value);
}

function load(tab, address) {
  if (!tab.browser.isRemoteBrowser) {
    throw new Error("O Gecko não inicializou o processo de conteúdo.");
  }
  const uri = Services.io.newURI(address);
  tab.browser.loadURI(uri, {
    triggeringPrincipal: SECURITY.createNullPrincipal({}),
  });
}

function updateTab(tab) {
  const uri = tab.browser.currentURI;
  const current = uri?.spec ?? "";
  const home = !current || current === HOME || current === "about:blank";
  const site = uri?.schemeIs("http") || uri?.schemeIs("https") ? uri.displayHost : current;
  const label = home ? "Nova aba" : tab.browser.contentTitle || site;
  tab.button.textContent = label;
  if (tab === active) {
    if (document.activeElement !== byId("address")) {
      byId("address").value = home ? "" : current;
    }
    byId("back").disabled = !tab.browser.canGoBack;
    byId("forward").disabled = !tab.browser.canGoForward;
    document.title = `${label} — Nous`;
  }
}

function selectTab(tab) {
  if (active) {
    active.browser.hidden = true;
    active.browser.docShellIsActive = false;
    active.button.setAttribute("aria-selected", "false");
  }
  active = tab;
  tab.browser.hidden = false;
  tab.browser.docShellIsActive = true;
  tab.button.setAttribute("aria-selected", "true");
  byId("address").blur();
  showMessage();
  updateTab(tab);
}

function closeTab(tab) {
  const index = tabs.indexOf(tab);
  if (tabs.length === 1) {
    window.close();
    return;
  }
  if (tab === active) {
    selectTab(tabs[index + 1] ?? tabs[index - 1]);
  }
  tabs.splice(index, 1);
  tab.browser.removeProgressListener(tab.progress);
  tab.browser.remove();
  tab.container.remove();
}

function newTab(address = HOME) {
  const container = document.createElementNS(HTML, "div");
  container.className = "tab";
  const button = document.createElementNS(HTML, "button");
  button.setAttribute("role", "tab");
  button.textContent = "Nova aba";
  const close = document.createElementNS(HTML, "button");
  close.className = "close";
  close.setAttribute("aria-label", "Fechar aba");
  close.textContent = "×";
  container.append(button, close);
  byId("tabs").append(container);

  const browser = document.createXULElement("browser");
  browser.setAttribute("type", "content");
  browser.setAttribute("remote", "true");
  browser.setAttribute("maychangeremoteness", "true");
  browser.setAttribute("messagemanagergroup", "browsers");
  byId("pages").append(browser);

  const tab = { browser, button, container, progress: null };
  tab.progress = {
    QueryInterface: ChromeUtils.generateQI(["nsIWebProgressListener", "nsISupportsWeakReference"]),
    onLocationChange(progress) {
      if (progress.isTopLevel) {
        updateTab(tab);
      }
    },
    onStateChange(progress, _request, flags) {
      if (progress.isTopLevel && (flags & Ci.nsIWebProgressListener.STATE_STOP)) {
        updateTab(tab);
      }
    },
  };
  browser.addProgressListener(tab.progress, WEB_PROGRESS.NOTIFY_LOCATION | WEB_PROGRESS.NOTIFY_STATE_NETWORK);
  browser.addEventListener("DOMTitleChanged", () => updateTab(tab));
  button.addEventListener("click", () => selectTab(tab));
  close.addEventListener("click", () => closeTab(tab));
  tabs.push(tab);
  selectTab(tab);
  load(tab, address);
}

function focusAddress() {
  byId("address").focus();
  byId("address").select();
}

window.addEventListener("load", () => {
  byId("navigation").addEventListener("submit", event => {
    event.preventDefault();
    showMessage();
    try {
      load(active, resolveAddress(byId("address").value));
      active.browser.focus();
    } catch (error) {
      showMessage(error.message);
    }
  });
  byId("back").addEventListener("click", () => active.browser.goBack());
  byId("forward").addEventListener("click", () => active.browser.goForward());
  byId("reload").addEventListener("click", () => active.browser.reload());
  byId("new-tab").addEventListener("click", () => {
    newTab();
    focusAddress();
  });
  window.addEventListener("keydown", event => {
    const key = event.key.toLowerCase();
    if (key === "f5") {
      active.browser.reload();
    } else if (event.altKey && key === "arrowleft") {
      active.browser.goBack();
    } else if (event.altKey && key === "arrowright") {
      active.browser.goForward();
    } else if (!event.ctrlKey || !["l", "r", "t", "w", "tab"].includes(key)) {
      return;
    } else if (key === "l") {
      focusAddress();
    } else if (key === "t") {
      newTab();
      focusAddress();
    } else if (key === "w") {
      closeTab(active);
    } else if (key === "tab") {
      const next = tabs.indexOf(active) + (event.shiftKey ? -1 : 1);
      selectTab(tabs[(next + tabs.length) % tabs.length]);
    } else {
      active.browser.reload();
    }
    event.preventDefault();
  });

  try {
    newTab(resolveAddress(Services.env.get("NOUS_GECKO_START_URL")));
  } catch (error) {
    showMessage(error.message);
  }
}, { once: true });
