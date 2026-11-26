// Nous Content Actor (Child) — Ultra-lightweight page interaction bridge.
// Runs in remote web content processes:
// - Real <link rel="icon"> favicon discovery (SVG/PNG/ICO)
// - Link hover URL preview
// - Ctrl+Click / Middle-Click opening links in background tabs
// - Right-click context payload extraction (respecting site custom context menus)
// - HTML5 DOM Fullscreen (YouTube / video players)
// - Page actions (copy, cut, paste, selectAll, reload, media control)

export class NousContentChild extends JSWindowActorChild {
  #lastHoverUrl = "";
  #waitForAfterPaint = false;
  #lastTxId = 0;

  handleEvent(event) {
    switch (event.type) {
      case "DOMContentLoaded":
      case "pageshow":
      case "DOMLinkAdded":
        if (this.browsingContext.isTopContent) {
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

      case "Nous:GetPageInfo": {
        this.#emitBestFavicon();
        return {
          title: this.document?.title || "",
          url: this.document?.location?.href || "",
          charset: this.document?.characterSet || "UTF-8",
        };
      }
    }
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
