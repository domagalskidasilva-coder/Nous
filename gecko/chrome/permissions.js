"use strict";

// Nous Site Permissions, Cookies & Clear Browsing Data Controller
// Lazy-uses Gecko's Services.perms, Services.clearData & nsIContentPermissionPrompt.

(() => {
  const PERMISSION_LABELS = {
    "camera": "Câmera",
    "microphone": "Microfone",
    "screen": "Compartilhamento de tela",
    "desktop-notification": "Notificações",
    "geo": "Localização",
    "clipboard-read": "Área de transferência",
    "autoplay-media": "Reprodução automática",
    "persistent-storage": "Armazenamento persistente",
  };

  function principalForUrl(url) {
    try {
      const uri = Services.io.newURI(url);
      return Services.scriptSecurityManager.createContentPrincipal(uri, {});
    } catch {
      return null;
    }
  }

  function getSitePermissions(url) {
    const principal = principalForUrl(url);
    if (!principal) return [];
    const results = [];
    for (const [type, label] of Object.entries(PERMISSION_LABELS)) {
      try {
        const perm = Services.perms.testExactPermissionFromPrincipal(principal, type);
        if (perm === Ci.nsIPermissionManager.ALLOW_ACTION || perm === Ci.nsIPermissionManager.DENY_ACTION) {
          results.push({
            type,
            label,
            allowed: perm === Ci.nsIPermissionManager.ALLOW_ACTION,
          });
        }
      } catch {}
    }
    return results;
  }

  function setSitePermission(url, type, action) {
    const principal = principalForUrl(url);
    if (!principal) return false;
    try {
      if (action === "allow") {
        Services.perms.addFromPrincipal(principal, type, Ci.nsIPermissionManager.ALLOW_ACTION);
      } else if (action === "deny") {
        Services.perms.addFromPrincipal(principal, type, Ci.nsIPermissionManager.DENY_ACTION);
      } else {
        Services.perms.removeFromPrincipal(principal, type);
      }
      return true;
    } catch {
      return false;
    }
  }

  function clearSiteData(url) {
    return new Promise(resolve => {
      try {
        const u = new URL(url);
        const baseDomain = u.hostname.replace(/^www\./, "");
        Services.clearData.deleteDataFromHost(
          baseDomain,
          true,
          Ci.nsIClearDataService.CLEAR_COOKIES |
            Ci.nsIClearDataService.CLEAR_DOM_STORAGES |
            Ci.nsIClearDataService.CLEAR_ALL_CACHES,
          () => resolve(true)
        );
      } catch {
        resolve(false);
      }
    });
  }

  function clearBrowsingData({ rangeMs = 0, cookies = true, cache = true, storage = true } = {}) {
    return new Promise(resolve => {
      try {
        let flags = 0;
        if (cookies) flags |= Ci.nsIClearDataService.CLEAR_COOKIES;
        if (cache) flags |= Ci.nsIClearDataService.CLEAR_ALL_CACHES;
        if (storage) flags |= Ci.nsIClearDataService.CLEAR_DOM_STORAGES;
        if (!flags) {
          resolve(true);
          return;
        }
        if (rangeMs > 0) {
          const nowUs = Date.now() * 1000;
          const fromUs = Math.max(0, (Date.now() - rangeMs) * 1000);
          Services.clearData.deleteDataInTimeRange(fromUs, nowUs, true, flags, () => resolve(true));
        } else {
          Services.clearData.deleteData(flags, () => resolve(true));
        }
      } catch {
        resolve(false);
      }
    });
  }

  function labelForPermissionType(type) {
    return PERMISSION_LABELS[type] || type;
  }

  window.NousPermissions = {
    getSitePermissions,
    setSitePermission,
    clearSiteData,
    clearBrowsingData,
    labelForPermissionType,
  };
})();
