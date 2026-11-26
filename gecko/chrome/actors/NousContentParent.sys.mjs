// Nous Content Actor (Parent) — Bridges remote web pages to Nous chrome window.

export class NousContentParent extends JSWindowActorParent {
  receiveMessage(message) {
    const browser = this.browsingContext?.top?.embedderElement;
    const chromeWin = browser?.ownerGlobal;
    if (!chromeWin || !chromeWin.NousChromeBridge) return;

    switch (message.name) {
      case "Nous:Favicon":
        chromeWin.NousChromeBridge.onFavicon(browser, message.data);
        break;
      case "Nous:LinkHover":
        chromeWin.NousChromeBridge.onLinkHover(browser, message.data?.url || "");
        break;
      case "Nous:OpenLink":
        chromeWin.NousChromeBridge.onOpenLink(browser, message.data);
        break;
      case "Nous:ContextMenu":
        chromeWin.NousChromeBridge.onContextMenu(browser, message.data, this);
        break;
      case "Nous:DOMFullscreen:Request":
        chromeWin.NousChromeBridge.onDOMFullscreenRequest(browser, this);
        break;
      case "Nous:DOMFullscreen:Exit":
        chromeWin.NousChromeBridge.onDOMFullscreenExit(browser, this);
        break;
      case "Nous:DOMFullscreen:Painted":
        chromeWin.NousChromeBridge.onDOMFullscreenPainted(browser);
        break;
    }
  }
}
