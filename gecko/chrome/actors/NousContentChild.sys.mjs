// Nous Content Actor (Child) — Ultra-lightweight page interaction bridge.
// Runs in remote web content processes:
// - Real <link rel="icon"> favicon discovery (SVG/PNG/ICO)
// - Smart luminance-aware Kanso Adaptive Dark Web via privileged Stylo USER_SHEET (bypasses CSP!)
// - Link hover URL preview
// - Ctrl+Click / Middle-Click opening links in background tabs
// - Right-click context payload extraction (respecting site custom context menus)
// - HTML5 DOM Fullscreen (YouTube / video players)
// - Page actions (copy, cut, paste, selectAll, undo, redo)

const DARK_CSS = [
  "html {",
  "  filter: invert(0.92) hue-rotate(180deg) contrast(0.96) !important;",
  "  background-color: #ffffff !important;",
  "  min-height: 100% !important;",
  "}",
  "img, picture, video, canvas, iframe, embed, object, svg image, [style*='background-image'] {",
  "  filter: invert(1) hue-rotate(180deg) !important;",
  "}",
  "img *, picture img, picture source, video *, iframe *, [style*='background-image'] img, [style*='background-image'] video, [style*='background-image'] [style*='background-image'] {",
  "  filter: none !important;",
  "}",
].join("\n");

const DARK_SHEET_URI = "data:text/css;charset=utf-8," + encodeURIComponent(DARK_CSS);

export class NousContentChild extends JSWindowActorChild {
  #lastHoverUrl = "";
  #waitForAfterPaint = false;
  #lastTxId = 0;
  #darkSheetLoaded = false;
  #pendingMedia = new Map();

  get #isTopLevel() {
    return !this.browsingContext?.parent;
  }

  observe(subject, topic) {
    if (topic !== "getUserMedia:request") return;
    try {
      const req = subject;
      if (!req || req.windowID !== this.contentWindow?.windowGlobalChild?.outerWindowId &&
          req.innerWindowID !== this.contentWindow?.windowGlobalChild?.innerWindowId) {
        return;
      }
      const callID = req.callID;
      const constraints = typeof req.getConstraints === "function" ? req.getConstraints() : {};
      const devices = typeof req.getDevices === "function" ? req.getDevices() : [];
      const hasVideo = Boolean(constraints.video || constraints.picture);
      const hasAudio = Boolean(constraints.audio);
      const mediaSource = typeof constraints.video === "object" ? constraints.video?.mediaSource : "";
      const isScreen = Boolean(mediaSource && mediaSource !== "camera");

      this.#pendingMedia.set(callID, { devices, hasVideo, hasAudio, isScreen });
      this.sendAsyncMessage("Nous:MediaRequest", {
        callID,
        host: this.document?.location?.hostname?.replace(/^www\./, "") || "",
        video: hasVideo && !isScreen,
        audio: hasAudio,
        screen: isScreen,
      });
    } catch {}
  }

  handleEvent(event) {
    switch (event.type) {
      case "DOMContentLoaded":
      case "pageshow":
      case "load":
        if (this.#isTopLevel) {
          this.#emitBestFavicon();
          this.#applyAdaptiveDark();
          if (event.type === "DOMContentLoaded") {
            this.contentWindow?.setTimeout(() => this.#applyAdaptiveDark(), 500);
          }
        }
        break;

      case "DOMLinkAdded":
        if (this.#isTopLevel) {
          this.#emitBestFavicon();
        }
        break;

      case "mouseover":
      case "focusin": {
        const a = this.#findLink(event.target);
        const href = a ? this.#safeHref(a) : "";
        if (href !== this.#lastHoverUrl) {
          this.#lastHoverUrl = href;
          this.sendAsyncMessage("Nous:LinkHover", { url: href });
        }
        break;
      }

      case "mouseout":
      case "focusout": {
        if (this.#lastHoverUrl) {
          const next = event.relatedTarget ? this.#findLink(event.relatedTarget) : null;
          const href = next ? this.#safeHref(next) : "";
          if (href !== this.#lastHoverUrl) {
            this.#lastHoverUrl = href;
            this.sendAsyncMessage("Nous:LinkHover", { url: href });
          }
        }
        break;
      }

      case "click": {
        if (event.defaultPrevented || event.button !== 0) return;
        if (!(event.ctrlKey || event.metaKey)) return;
        const a = this.#findLink(event.target);
        const href = a ? this.#safeHref(a) : "";
        if (href && /^https?:\/\//i.test(href)) {
          event.preventDefault();
          event.stopPropagation();
          this.sendAsyncMessage("Nous:OpenLink", {
            url: href,
            inBackground: !event.shiftKey,
          });
        }
        break;
      }

      case "auxclick": {
        if (event.defaultPrevented || event.button !== 1) return;
        const a = this.#findLink(event.target);
        const href = a ? this.#safeHref(a) : "";
        if (href && /^https?:\/\//i.test(href)) {
          event.preventDefault();
          event.stopPropagation();
          this.sendAsyncMessage("Nous:OpenLink", {
            url: href,
            inBackground: !event.shiftKey,
          });
        }
        break;
      }

      case "contextmenu": {
        if (event.defaultPrevented) return;
        event.preventDefault();
        const payload = this.#buildContextPayload(event);
        this.sendAsyncMessage("Nous:ContextMenu", payload);
        break;
      }

      case "MozDOMFullscreen:Request": {
        this.sendAsyncMessage("Nous:DOMFullscreen:Request", {});
        break;
      }

      case "MozDOMFullscreen:Exit": {
        this.sendAsyncMessage("Nous:DOMFullscreen:Exit", {});
        break;
      }

      case "MozDOMFullscreen:Entered":
      case "MozDOMFullscreen:Exited": {
        const wu = this.contentWindow?.windowUtils;
        if (wu) {
          this.#waitForAfterPaint = true;
          this.#lastTxId = wu.lastTransactionId;
        }
        break;
      }

      case "MozAfterPaint": {
        if (this.#waitForAfterPaint) {
          const wu = this.contentWindow?.windowUtils;
          if (!wu || wu.lastTransactionId > this.#lastTxId) {
            this.#waitForAfterPaint = false;
            this.sendAsyncMessage("Nous:DOMFullscreen:Painted", {});
          }
        }
        break;
      }
    }
  }

  receiveMessage(message) {
    const win = this.contentWindow;
    if (!win) return;

    switch (message.name) {
      case "Nous:DOMFullscreen:Entered": {
        try {
          const wu = win.windowUtils;
          if (wu) {
            this.#waitForAfterPaint = true;
            this.#lastTxId = wu.lastTransactionId;
            if (!wu.handleFullscreenRequests() && !this.document?.fullscreenElement) {
              this.sendAsyncMessage("Nous:DOMFullscreen:Exit", {});
            }
          }
        } catch {}
        break;
      }

      case "Nous:DOMFullscreen:CleanUp": {
        try {
          const wu = win.windowUtils;
          if (this.document?.fullscreenElement && wu) {
            wu.exitFullscreen();
          }
        } catch {}
        break;
      }

      case "Nous:ExecCommand": {
        const cmd = message.data?.command;
        if (!cmd) return;
        try {
          if (cmd === "copy" || cmd === "cut" || cmd === "paste" || cmd === "selectAll" || cmd === "undo" || cmd === "redo") {
            this.document.execCommand(cmd);
          }
        } catch {}
        break;
      }

      case "Nous:UpdateDarkWeb": {
        if (this.#isTopLevel) {
          this.#applyAdaptiveDark();
        }
        break;
      }

      case "Nous:MediaResponse": {
        const { callID, allow } = message.data || {};
        const pending = this.#pendingMedia.get(callID);
        this.#pendingMedia.delete(callID);
        if (!callID) return;
        try {
          if (!allow || !pending) {
            Services.obs.notifyObservers(null, "getUserMedia:response:deny", callID);
            return;
          }
          const allowed = Cc["@mozilla.org/array;1"].createInstance(Ci.nsIMutableArray);
          let pickedVideo = false;
          let pickedAudio = false;
          for (const dev of pending.devices || []) {
            const type = dev.type || "";
            if (!pickedVideo && pending.hasVideo && (type === "videoinput" || type === "screen" || type === "window" || type === "browser")) {
              allowed.appendElement(dev);
              pickedVideo = true;
            } else if (!pickedAudio && pending.hasAudio && type === "audioinput") {
              allowed.appendElement(dev);
              pickedAudio = true;
            }
          }
          if (allowed.length > 0) {
            Services.obs.notifyObservers(allowed, "getUserMedia:response:allow", callID);
          } else {
            Services.obs.notifyObservers(null, "getUserMedia:response:deny", callID);
          }
        } catch {
          try {
            Services.obs.notifyObservers(null, "getUserMedia:response:deny", callID);
          } catch {}
        }
        break;
      }

      case "Nous:GetPageInfo": {
        this.#emitBestFavicon();
        return {
          title: this.document?.title || "",
          url: this.document?.location?.href || "",
          charset: this.document?.characterSet || "UTF-8",
          darkSheetLoaded: this.#darkSheetLoaded,
        };
      }
    }
  }

  #setDarkSheet(active) {
    const wu = this.contentWindow?.windowUtils;
    if (!wu) return;
    if (active && !this.#darkSheetLoaded) {
      try {
        wu.loadSheetUsingURIString(DARK_SHEET_URI, wu.USER_SHEET);
        this.#darkSheetLoaded = true;
      } catch {}
    } else if (!active && this.#darkSheetLoaded) {
      try {
        wu.removeSheetUsingURIString(DARK_SHEET_URI, wu.USER_SHEET);
        this.#darkSheetLoaded = false;
      } catch {}
    }
  }

  #parseRgbLuminance(colorStr) {
    if (!colorStr || colorStr === "transparent") return null;
    const m = colorStr.match(/rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)(?:\s*,\s*([\d.]+))?\s*\)/i);
    if (!m) return null;
    const alpha = m[4] !== undefined ? parseFloat(m[4]) : 1;
    if (alpha < 0.15) return null;
    const r = parseInt(m[1], 10) / 255;
    const g = parseInt(m[2], 10) / 255;
    const b = parseInt(m[3], 10) / 255;
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  }

  #isPageAlreadyDark(doc, win) {
    try {
      const wasLoaded = this.#darkSheetLoaded;
      if (wasLoaded) this.#setDarkSheet(false);

      const root = doc.documentElement;
      const body = doc.body;
      const rootLum = root ? this.#parseRgbLuminance(win.getComputedStyle(root).backgroundColor) : null;
      const bodyLum = body ? this.#parseRgbLuminance(win.getComputedStyle(body).backgroundColor) : null;
      const textLum = body ? this.#parseRgbLuminance(win.getComputedStyle(body).color) : null;

      // Also inspect first major container covering a substantial part of the viewport
      // if root & body are transparent (e.g. HN table / SPA root div).
      let mainLum = null;
      if (rootLum === null && bodyLum === null && body) {
        const candidates = body.querySelectorAll("main, #app, #root, #__next, center > table, table, body > div");
        const minW = Math.max(240, (win.innerWidth || 800) * 0.45);
        const minH = Math.min(240, Math.max(120, (win.innerHeight || 600) * 0.3));
        for (let i = 0; i < Math.min(candidates.length, 8); i++) {
          const probe = candidates[i];
          const lum = this.#parseRgbLuminance(win.getComputedStyle(probe).backgroundColor);
          if (lum !== null) {
            const rect = probe.getBoundingClientRect();
            if (rect.width >= minW && rect.height >= minH) {
              mainLum = lum;
              break;
            }
          }
        }
      }

      if (wasLoaded) this.#setDarkSheet(true);

      const bgLum = bodyLum !== null ? bodyLum : (rootLum !== null ? rootLum : mainLum);
      if (bgLum !== null) {
        return bgLum < 0.36;
      }
      if (textLum !== null && textLum > 0.62) {
        return true;
      }
      return false;
    } catch {
      return false;
    }
  }

  #applyAdaptiveDark() {
    const doc = this.document;
    const win = this.contentWindow;
    if (!doc || !win || !doc.documentElement) return;

    const loc = doc.location;
    if (!loc || (loc.protocol !== "http:" && loc.protocol !== "https:" && loc.protocol !== "file:")) {
      this.#setDarkSheet(false);
      return;
    }

    let enabled = true;
    let isDarkTheme = true;
    let excluded = false;

    try {
      enabled = Services.prefs.getBoolPref("nous.forceDarkWeb", true);
      isDarkTheme = Services.prefs.getIntPref("layout.css.prefers-color-scheme.content-override", 0) === 0;
      const rawEx = Services.prefs.getStringPref("nous.darkWebExclude", "");
      if (rawEx && loc.hostname) {
        const host = loc.hostname.toLowerCase().replace(/^www\./, "");
        const set = rawEx.split(",").map(s => s.trim().toLowerCase()).filter(Boolean);
        excluded = set.includes(host);
      }
    } catch {}

    if (!enabled || !isDarkTheme || excluded) {
      this.#setDarkSheet(false);
      return;
    }

    if (this.#isPageAlreadyDark(doc, win)) {
      this.#setDarkSheet(false);
      return;
    }

    this.#setDarkSheet(true);
  }

  #findLink(node) {
    let cur = node;
    while (cur && cur.nodeType !== 9) {
      if (cur.nodeType === 1 && cur.localName === "a" && cur.hasAttribute("href")) {
        return cur;
      }
      cur = cur.parentNode || cur.host || null;
    }
    return null;
  }

  #safeHref(anchor) {
    try {
      const href = anchor.href;
      if (!href || href.startsWith("javascript:")) return "";
      return href;
    } catch {
      return "";
    }
  }

  #isEditable(node) {
    if (!node || node.nodeType !== 1) return false;
    if (node.isContentEditable) return true;
    const tag = node.localName;
    if (tag === "textarea") return !node.readOnly && !node.disabled;
    if (tag === "input") {
      const t = (node.type || "text").toLowerCase();
      return !["button", "submit", "reset", "checkbox", "radio", "file", "image", "range", "color"].includes(t) &&
        !node.readOnly && !node.disabled;
    }
    return false;
  }

  #buildContextPayload(event) {
    const target = event.composedTarget || event.target;
    const link = this.#findLink(target);
    const linkUrl = link ? this.#safeHref(link) : "";
    const linkText = link ? (link.textContent || "").trim().slice(0, 80) : "";

    let mediaType = null;
    let mediaUrl = "";
    if (target && target.nodeType === 1) {
      if (target.localName === "img" && target.currentSrc) {
        mediaType = "image";
        mediaUrl = target.currentSrc;
      } else if ((target.localName === "video" || target.localName === "audio") && (target.currentSrc || target.src)) {
        mediaType = target.localName;
        mediaUrl = target.currentSrc || target.src;
      }
    }

    let selectionText = "";
    try {
      selectionText = (this.contentWindow?.getSelection()?.toString() || "").trim().slice(0, 200);
    } catch {}

    return {
      screenX: event.screenX,
      screenY: event.screenY,
      clientX: event.clientX,
      clientY: event.clientY,
      pageUrl: this.document?.location?.href || "",
      pageTitle: this.document?.title || "",
      linkUrl,
      linkText,
      mediaType,
      mediaUrl,
      selectionText,
      isEditable: this.#isEditable(target),
    };
  }

  #emitBestFavicon() {
    const doc = this.document;
    if (!doc || !doc.head) return;
    const loc = doc.location;
    if (!loc || (loc.protocol !== "http:" && loc.protocol !== "https:")) return;

    const links = doc.querySelectorAll('link[rel*="icon" i][href]');
    let bestUrl = "";
    let bestScore = -1;

    for (const el of links) {
      const rel = (el.getAttribute("rel") || "").toLowerCase();
      if (rel.includes("mask-icon")) continue;
      let href = "";
      try {
        href = new URL(el.getAttribute("href"), doc.baseURI).href;
      } catch {
        continue;
      }
      if (!href.startsWith("http:") && !href.startsWith("https:") && !href.startsWith("data:")) {
        continue;
      }
      let score = 10;
      const type = (el.getAttribute("type") || "").toLowerCase();
      const sizes = (el.getAttribute("sizes") || "").toLowerCase();
      if (type.includes("svg") || href.endsWith(".svg")) score += 40;
      else if (type.includes("png") || href.endsWith(".png")) score += 25;
      if (sizes.includes("32x32") || sizes.includes("48x48") || sizes.includes("64x64")) score += 20;
      else if (sizes.includes("16x16")) score += 10;
      if (rel.includes("apple-touch-icon")) score -= 8;

      if (score > bestScore) {
        bestScore = score;
        bestUrl = href;
      }
    }

    if (!bestUrl) {
      bestUrl = `${loc.origin}/favicon.ico`;
    }

    this.sendAsyncMessage("Nous:Favicon", {
      pageUrl: loc.href,
      iconUrl: bestUrl,
    });
  }
}
