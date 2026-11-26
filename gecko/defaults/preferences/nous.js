// Nous Gecko 153 ESR — Ultra-Lightweight Runtime Profile
// Prioritizes minimum RAM, single-process startup, zero telemetry/bloat, and crisp UI.

// Core chrome shell
pref("toolkit.defaultChromeURI", "chrome://nous/content/window.xhtml");
pref("toolkit.defaultChromeFeatures", "chrome,dialog=no,resizable,all");
pref("browser.tabs.remote.autostart", true);
pref("browser.tabs.inTitlebar", 1);
pref("ui.systemUsesDarkTheme", 1);

// ---------------------------------------------------------------------------
// 1. Lean Process Architecture (1 process on startup; 1 sandboxed web child)
// ---------------------------------------------------------------------------
// Keep compositing, networking, and media decoding lean so -app mode never spawns
// idle helper processes (GPU, Socket, RDD, Utility) that eat 40–100+ MB of RAM.
pref("layers.gpu-process.enabled", false);
pref("network.process.enabled", false);
pref("media.utility-process.enabled", false);
// Keep RDD enabled on-demand so YouTube/MP4/WebM video decoding works natively
// in Gecko 153 without NS_ERROR_DOM_MEDIA_FATAL_ERR.
pref("media.rdd-process.enabled", true);
pref("sanity-test.disabled", true);
pref("browser.launcherProcess.enabled", false);

// Never pre-spawn idle content processes burning RAM in the background.
pref("dom.ipc.processPrelaunch.enabled", false);
pref("dom.ipc.processPrelaunch.fission.number", 0);

// Keep web content sandboxed in a single shared remote process in Eco mode
// (Balanced/Turbo profiles can raise this dynamically).
pref("dom.ipc.processCount", 1);
pref("dom.ipc.processCount.webIsolated", 1);
pref("dom.ipc.processCount.file", 1);
pref("dom.ipc.processCount.privilegedabout", 1);
pref("dom.ipc.keepProcessesAlive.web", 0);
pref("fission.webContentIsolationStrategy", 0);

// Native PDF viewer & instant HTML5 fullscreen (zero fade lag)
pref("pdfjs.disabled", false);
pref("full-screen-api.enabled", true);
pref("full-screen-api.transition-duration.enter", "0 0");
pref("full-screen-api.transition-duration.leave", "0 0");
pref("full-screen-api.warning.timeout", 0);

// ---------------------------------------------------------------------------
// 2. Bounded Memory, Image Surface Cache, BFCache & SpiderMonkey GC Tuning
// ---------------------------------------------------------------------------
// Cap back/forward DOM cache (default -1 holds up to 8 full page DOMs in RAM).
pref("browser.sessionhistory.max_total_viewers", 1);
pref("browser.sessionhistory.max_entries", 20);

// Tight RAM & disk caches.
pref("browser.cache.memory.enable", true);
pref("browser.cache.memory.capacity", 12288);        // 12 MB max RAM cache
pref("browser.cache.memory.max_entry_size", 2048);   // 2 MB max single entry
pref("browser.cache.disk.enable", true);
pref("browser.cache.disk.capacity", 49152);          // 48 MB max disk cache
pref("browser.cache.disk.metadata_memory_limit", 200);
pref("browser.cache.disk.smart_size.enabled", false);

// Aggressive image surface discarding so off-screen/background tabs release RAM fast.
pref("image.mem.discardable", true);
pref("image.mem.animated.discardable", true);
pref("image.mem.surfacecache.max_size_kb", 16384);   // 16 MB decoded image cap
pref("image.mem.surfacecache.min_expiration_ms", 8000);
pref("image.mem.surfacecache.discard_factor", 2);
pref("gfx.webrender.precache-shaders", false);

// Compact, incremental JS garbage collection.
pref("javascript.options.mem.gc_incremental", true);
pref("javascript.options.mem.gc_compacting", true);
pref("javascript.options.mem.high_water_mark", 24);
pref("javascript.options.mem.gc_min_empty_chunk_count", 1);
pref("javascript.options.mem.gc_max_empty_chunk_count", 2);

// Throttle background tab timers aggressively to keep idle CPU near 0%.
pref("dom.min_background_timeout_value", 2000);
pref("dom.timeout.throttling_delay", 5000);
pref("dom.timeout.budget_throttling_max_delay", 15000);

// Unload tabs under OS memory pressure as an extra safety net.
pref("browser.tabs.unloadOnLowMemory", true);

// ---------------------------------------------------------------------------
// 3. Disable Unused Heavy SQLite DBs & Background Subsystems
// ---------------------------------------------------------------------------
// Nous keeps its own tiny JSON bookmarks/history (<4 KB) instead of spinning up
// Places SQLite, Form History SQLite, Password Manager DBs, or Page Thumbnails.
pref("places.history.enabled", false);
pref("browser.formfill.enable", false);
pref("signon.rememberSignons", false);
pref("browser.pagethumbnails.capturing_disabled", true);
pref("browser.newtabpage.enabled", false);
pref("browser.shell.checkDefaultBrowser", false);
pref("extensions.autoDisableScopes", 15);
pref("extensions.enabledScopes", 0);
pref("media.gmp-provider.enabled", false);
pref("media.gmp-manager.url", "");
pref("media.hardwaremediakeys.enabled", false);
pref("dom.gamepad.enabled", false);
pref("dom.vr.enabled", false);

// ---------------------------------------------------------------------------
// 4. Zero Telemetry, Remote Settings, AI/ML, SafeBrowsing DBs & Update Noise
// ---------------------------------------------------------------------------
pref("toolkit.telemetry.enabled", false);
pref("toolkit.telemetry.unified", false);
pref("toolkit.telemetry.archive.enabled", false);
pref("toolkit.telemetry.server", "");
pref("toolkit.telemetry.firstShutdownPing.enabled", false);
pref("toolkit.telemetry.newProfilePing.enabled", false);
pref("toolkit.telemetry.shutdownPingSender.enabled", false);
pref("toolkit.telemetry.updatePing.enabled", false);
pref("toolkit.telemetry.bhrPing.enabled", false);
pref("toolkit.telemetry.coverage.opt-out", true);
pref("datareporting.healthreport.uploadEnabled", false);
pref("datareporting.policy.dataSubmissionEnabled", false);
pref("datareporting.usage.uploadEnabled", false);
pref("app.shield.optoutstudies.enabled", false);
pref("app.normandy.enabled", false);
pref("nimbus.telemetry.targetingContextEnabled", false);
pref("breakpad.reportURL", "");
pref("browser.crashReports.unsubmittedCheck.autoSubmit2", false);
pref("toolkit.crashreporter.enabled", false);

// Disable heavy AI/ML runtime (ONNX), built-in translations, Pocket, Sync.
pref("browser.ml.enable", false);
pref("browser.ml.chat.enabled", false);
pref("browser.translations.enable", false);
pref("extensions.pocket.enabled", false);
pref("identity.fxaccounts.enabled", false);

// Stop background Remote Settings & SafeBrowsing downloads that bloat disk and RAM.
pref("services.settings.server", "");
pref("services.settings.poll_interval", 31536000);
pref("browser.safebrowsing.malware.enabled", false);
pref("browser.safebrowsing.phishing.enabled", false);
pref("browser.safebrowsing.downloads.enabled", false);
pref("browser.safebrowsing.downloads.remote.enabled", false);
pref("browser.safebrowsing.downloads.remote.url", "");

// Disable background app/addon update checks and blocklist polling.
pref("app.update.enabled", false);
pref("app.update.auto", false);
pref("app.update.background.scheduling.enabled", false);
pref("extensions.update.enabled", false);
pref("extensions.update.autoUpdateDefault", false);
pref("extensions.systemAddon.update.enabled", false);
pref("extensions.getAddons.cache.enabled", false);
pref("extensions.blocklist.enabled", false);
pref("browser.startup.homepage_override.mstone", "ignore");

// ---------------------------------------------------------------------------
// 5. Lean Networking & Privacy Defaults
// ---------------------------------------------------------------------------
pref("network.prefetch-next", false);
pref("network.dns.disablePrefetch", true);
pref("network.http.speculative-parallel-limit", 0);
pref("network.predictor.enabled", false);
pref("network.connectivity-service.enabled", false);
pref("network.captive-portal-service.enabled", false);
pref("beacon.enabled", false);
pref("dom.push.enabled", false);
pref("geo.enabled", false);
pref("dom.battery.enabled", false);
pref("media.webspeech.synth.enabled", false);
pref("media.webspeech.recognition.enable", false);
pref("media.autoplay.default", 5);
pref("privacy.donottrackheader.enabled", true);
pref("privacy.globalprivacycontrol.enabled", true);
pref("dom.security.https_first", true);
pref("general.smoothScroll", true);
pref("browser.download.useDownloadDir", true);

// ---------------------------------------------------------------------------
// 6. Nous Application Defaults
// ---------------------------------------------------------------------------
pref("nous.theme", "kanso");             // kanso | washi | sumi | stone
pref("nous.layout", "top");              // top (single-bar) | sidebar
pref("nous.density", "normal");          // compact | normal
pref("nous.perfProfile", "eco");         // eco | balanced | turbo
pref("nous.searchEngine", "duckduckgo"); // duckduckgo | google | brave | startpage
pref("nous.blocker.enabled", true);
pref("nous.hibernation.minutes", 2);     // 0 disables auto-hibernation
pref("nous.animations.enabled", true);
