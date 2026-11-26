"use strict";

// Nous Downloads Engine — Lazy wrapper around Gecko's native Downloads.sys.mjs
// Zero startup RAM overhead until initialized on first download or Ctrl+J.

(() => {
  let DownloadsMod = null;
  let DownloadPathsMod = null;
  let listPromise = null;
  const items = [];
  const listeners = new Set();

  function loadModules() {
    if (!DownloadsMod) {
      DownloadsMod = ChromeUtils.importESModule("resource://gre/modules/Downloads.sys.mjs").Downloads;
    }
    if (!DownloadPathsMod) {
      DownloadPathsMod = ChromeUtils.importESModule("resource://gre/modules/DownloadPaths.sys.mjs").DownloadPaths;
    }
  }

  function formatBytes(bytes) {
    const n = Number(bytes) || 0;
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
    return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  function extractFilename(dl) {
    try {
      const p = dl.target?.path || "";
      if (p) {
        const idx = Math.max(p.lastIndexOf("\\"), p.lastIndexOf("/"));
        return idx >= 0 ? p.slice(idx + 1) : p;
      }
      const u = new URL(dl.source?.url || "");
      const seg = u.pathname.split("/").filter(Boolean).pop();
      return decodeURIComponent(seg || u.hostname || "download");
    } catch {
      return "download";
    }
  }

  function toSnapshot(dl) {
    const total = Number(dl.totalBytes) || 0;
    const current = Number(dl.currentBytes) || 0;
    const pct = dl.succeeded
      ? 100
      : dl.hasProgress && total > 0
      ? Math.min(100, Math.round((current / total) * 100))
      : dl.progress || 0;

    let state = "downloading";
    if (dl.succeeded) state = "done";
    else if (dl.error) state = "error";
    else if (dl.canceled) state = dl.hasPartialData ? "paused" : "canceled";
    else if (dl.stopped) state = "paused";

    return {
      _raw: dl,
      id: dl._nousId || (dl._nousId = `${Date.now()}-${Math.random().toString(36).slice(2, 7)}`),
      name: extractFilename(dl),
      url: dl.source?.url || "",
      path: dl.target?.path || "",
      state,
      percent: pct,
      currentBytes: current,
      totalBytes: total,
      speedBytes: Number(dl.speed) || 0,
      sizeLabel: total > 0
        ? (dl.succeeded ? formatBytes(total) : `${formatBytes(current)} / ${formatBytes(total)}`)
        : formatBytes(current),
      speedLabel: !dl.stopped && dl.speed > 0 ? `${formatBytes(dl.speed)}/s` : "",
    };
  }

  function emitChange() {
    const snap = getSnapshot();
    for (const fn of listeners) {
      try {
        fn(snap);
      } catch {}
    }
  }

  function syncDownload(dl) {
    const idx = items.indexOf(dl);
    if (idx === -1) {
      items.unshift(dl);
    }
    emitChange();
  }

  async function ensureInitialized() {
    if (listPromise) return listPromise;
    listPromise = (async () => {
      try {
        loadModules();
        const list = await DownloadsMod.getList(DownloadsMod.ALL);
        const existing = await list.getAll();
        items.length = 0;
        for (const dl of existing.reverse()) {
          items.push(dl);
        }
        await list.addView({
          onDownloadAdded(dl) {
            syncDownload(dl);
          },
          onDownloadChanged(dl) {
            syncDownload(dl);
          },
          onDownloadRemoved(dl) {
            const i = items.indexOf(dl);
            if (i !== -1) items.splice(i, 1);
            emitChange();
          },
        });
        emitChange();
        return list;
      } catch {
        return null;
      }
    })();
    return listPromise;
  }

  function getSnapshot() {
    const mapped = items.map(toSnapshot);
    const active = mapped.filter(d => d.state === "downloading");
    const avgPct = active.length > 0
      ? Math.round(active.reduce((s, d) => s + d.percent, 0) / active.length)
      : 0;
    return {
      items: mapped,
      activeCount: active.length,
      activePercent: avgPct,
    };
  }

  async function getDefaultDownloadsDir() {
    try {
      loadModules();
      return await DownloadsMod.getPreferredDownloadsDirectory();
    } catch {
      try {
        return Services.dirsvc.get("DfltDln", Ci.nsIFile).path;
      } catch {
        return Services.dirsvc.get("Home", Ci.nsIFile).path;
      }
    }
  }

  async function downloadUrl(url, suggestedName = "", isPrivate = false) {
    const list = await ensureInitialized();
    if (!list || !url) return null;
    loadModules();

    let name = suggestedName;
    if (!name) {
      try {
        const u = new URL(url);
        name = decodeURIComponent(u.pathname.split("/").filter(Boolean).pop() || "arquivo");
      } catch {
        name = "arquivo";
      }
    }
    name = DownloadPathsMod.sanitize(name) || "arquivo";

    const dirPath = await getDefaultDownloadsDir();
    const targetFile = Cc["@mozilla.org/file/local;1"].createInstance(Ci.nsIFile);
    targetFile.initWithPath(dirPath);
    targetFile.append(name);
    const uniqueFile = DownloadPathsMod.createNiceUniqueFile(targetFile);

    const dl = await DownloadsMod.createDownload({
      source: { url, isPrivate: Boolean(isPrivate) },
      target: uniqueFile.path,
    });
    await list.add(dl);
    dl.start().catch(() => {});
    return dl;
  }

  async function saveUrlWithPicker(win, url, suggestedName = "", isPrivate = false) {
    const fp = Cc["@mozilla.org/filepicker;1"].createInstance(Ci.nsIFilePicker);
    fp.init(win.browsingContext, "Salvar arquivo como", Ci.nsIFilePicker.modeSave);
    if (suggestedName) {
      fp.defaultString = suggestedName;
    } else {
      try {
        const u = new URL(url);
        fp.defaultString = decodeURIComponent(u.pathname.split("/").filter(Boolean).pop() || "arquivo");
      } catch {
        fp.defaultString = "arquivo";
      }
    }
    fp.appendFilters(Ci.nsIFilePicker.filterAll);
    const res = await new Promise(resolve => fp.open(resolve));
    if (res !== Ci.nsIFilePicker.returnOK && res !== Ci.nsIFilePicker.returnReplace) {
      return null;
    }
    const list = await ensureInitialized();
    if (!list) return null;
    loadModules();
    const dl = await DownloadsMod.createDownload({
      source: { url, isPrivate: Boolean(isPrivate) },
      target: fp.file.path,
    });
    await list.add(dl);
    dl.start().catch(() => {});
    return dl;
  }

  function findById(id) {
    return items.find(d => d._nousId === id) || null;
  }

  async function pauseOrResume(id) {
    const dl = findById(id);
    if (!dl) return;
    try {
      if (dl.stopped) await dl.start();
      else await dl.cancel();
    } catch {}
    emitChange();
  }

  async function cancelDownload(id) {
    const dl = findById(id);
    if (!dl) return;
    try {
      await dl.cancel();
      await dl.removePartialData();
    } catch {}
    emitChange();
  }

  async function openFile(id) {
    const dl = findById(id);
    if (!dl || !dl.succeeded) return;
    try {
      await dl.launch();
    } catch {}
  }

  async function showInFolder(id) {
    const dl = findById(id);
    if (!dl) return;
    try {
      await dl.showContainingDirectory();
    } catch {}
  }

  async function clearFinished() {
    const list = await ensureInitialized();
    if (!list) return;
    try {
      await list.removeFinished();
    } catch {}
    emitChange();
  }

  window.NousDownloads = {
    ensureInitialized,
    getSnapshot,
    downloadUrl,
    saveUrlWithPicker,
    pauseOrResume,
    cancelDownload,
    openFile,
    showInFolder,
    clearFinished,
    subscribe(fn) {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
  };
})();
