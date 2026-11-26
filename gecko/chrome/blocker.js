"use strict";

// Nous Ultra-Lightweight Tracker & Ad Blocker
// Ported from crates/nous-net/src/blocker.rs — O(1) Set lookup, zero SQLite DB,
// cancels tracker requests via Gecko's http-on-modify-request before network/JS work.

(() => {
  const BLOCKED_DOMAINS = new Set([
    // Google Ads / Analytics / Tag Manager
    "doubleclick.net",
    "googlesyndication.com",
    "google-analytics.com",
    "googletagmanager.com",
    "googleadservices.com",
    "googletagservices.com",
    "adservice.google.com",
    "pagead2.googlesyndication.com",
    "analytics.google.com",
    "app-measurement.com",
    "crashlytics.com",
    "admob.com",
    "2mdn.net",
    // Meta / Facebook
    "connect.facebook.net",
    "facebook.net",
    "pixel.facebook.com",
    "an.facebook.com",
    // X / Twitter
    "ads-twitter.com",
    "analytics.twitter.com",
    "static.ads-twitter.com",
    // LinkedIn / TikTok / Pinterest / Reddit / Snap / Bing / Yahoo / Amazon
    "ads.linkedin.com",
    "px.ads.linkedin.com",
    "snap.licdn.com",
    "ads.tiktok.com",
    "analytics.tiktok.com",
    "ct.pinterest.com",
    "ads.pinterest.com",
    "ads.reddit.com",
    "events.reddit.com",
    "tr.snapchat.com",
    "sc-static.net",
    "bat.bing.com",
    "c.bing.com",
    "ads.yahoo.com",
    "analytics.yahoo.com",
    "amazon-adsystem.com",
    "aax.amazon-adsystem.com",
    // Analytics, Heatmaps & Session Replay
    "hotjar.com",
    "mixpanel.com",
    "segment.io",
    "segment.com",
    "cdn.segment.com",
    "amplitude.com",
    "clarity.ms",
    "heapanalytics.com",
    "fullstory.com",
    "logrocket.com",
    "crazyegg.com",
    "mouseflow.com",
    "luckyorange.com",
    "inspectlet.com",
    "kissmetrics.com",
    "plausible.io",
    "matomo.cloud",
    "chartbeat.com",
    "chartbeat.net",
    "parsely.com",
    // Telemetry & Error Tracking
    "sentry.io",
    "ingest.sentry.io",
    "sentry-cdn.com",
    "newrelic.com",
    "nr-data.net",
    "bam.nr-data.net",
    "datadoghq.com",
    "browser-intake-datadoghq.com",
    "bugsnag.com",
    "rollbar.com",
    // Ad Networks, Retargeting & Measurement
    "taboola.com",
    "outbrain.com",
    "criteo.com",
    "criteo.net",
    "adnxs.com",
    "scorecardresearch.com",
    "quantserve.com",
    "pubmatic.com",
    "rubiconproject.com",
    "openx.net",
    "casalemedia.com",
    "indexww.com",
    "advertising.com",
    "adform.net",
    "adroll.com",
    "smartadserver.com",
    "sharethrough.com",
    "triplelift.com",
    "3lift.com",
    "mediavine.com",
    "adthrive.com",
    "yieldmo.com",
    "lijit.com",
    "sovrn.com",
    "bidswitch.net",
    "rlcdn.com",
    "crwdcntrl.net",
    "demdex.net",
    "omtrdc.net",
    "2o7.net",
    "moatads.com",
    "doubleverify.com",
    "adsafeprotected.com",
    "zedo.com",
    "serving-sys.com",
    "mathtag.com",
    "everesttech.net",
    "bluekai.com",
    "exelator.com",
    "tapad.com",
    "lotame.com",
    "liveintent.com",
    "skimresources.com",
    "viglink.com",
    "buysellads.com",
    "carbonads.com",
    "cdn.carbonads.com",
    // Marketing Automation / Attribution
    "hs-analytics.net",
    "hs-scripts.com",
    "hsadspixel.net",
    "track.hubspot.com",
    "mktoresp.com",
    "pardot.com",
    "optimizely.com",
    "cdn.optimizely.com",
    "appsflyer.com",
    "adjust.com",
    "api2.branch.io",
    "cdn.branch.io",
    "addthis.com",
    "sharethis.com",
  ]);

  const TRACKER_SUBDOMAINS = new Set([
    "ads",
    "adserver",
    "adservice",
    "adsystem",
    "adtech",
    "adtrack",
    "analytics",
    "telemetry",
    "metrics",
    "tracker",
    "tracking",
    "pixel",
    "beacon",
    "clicktrack",
    "pagead",
    "tagmanager",
  ]);

  const TRACKER_PATH_PATTERNS = [
    "/gtag/js",
    "/gtm.js",
    "/ga.js",
    "/analytics.js",
    "/fbevents.js",
    "/adsbygoogle.js",
    "/pagead/",
    "/doubleclick/",
    "/adsense/",
    "/g/collect",
    "/j/collect",
    "/cdn-cgi/rum",
    "/_vercel/insights/",
    "/_vercel/speed-insights/",
    "/pixel.gif",
    "/pixel.png",
    "/beacon.js",
    "/prebid.js",
    "/telemetry/",
  ];

  const TRACKER_SCRIPT_FILES = new Set([
    "gtm.js",
    "analytics.js",
    "fbevents.js",
    "adsbygoogle.js",
    "gpt.js",
    "bat.js",
    "uwt.js",
    "clarity.js",
    "prebid.js",
    "tracker.js",
    "pixel.js",
  ]);

  function matchesDomainOrSuffix(host) {
    let cur = host;
    while (cur) {
      if (BLOCKED_DOMAINS.has(cur)) {
        return true;
      }
      const dot = cur.indexOf(".");
      if (dot === -1) {
        return false;
      }
      cur = cur.slice(dot + 1);
    }
    return false;
  }

  function matchesSubdomainHeuristic(host) {
    if (!host || host.startsWith("[") || /^[\d.]+$/.test(host)) {
      return false;
    }
    const firstDot = host.indexOf(".");
    if (firstDot === -1) {
      return false;
    }
    const rest = host.slice(firstDot + 1);
    if (!rest.includes(".")) {
      return false;
    }
    const first = host.slice(0, firstDot);
    return (
      TRACKER_SUBDOMAINS.has(first) ||
      first.startsWith("ads-") ||
      first.startsWith("adserver") ||
      first.startsWith("adservice") ||
      first.startsWith("adsystem")
    );
  }

  function matchesPathHeuristic(path) {
    if (!path || path === "/") {
      return false;
    }
    const lower = path.toLowerCase();
    for (let i = 0; i < TRACKER_PATH_PATTERNS.length; i++) {
      if (lower.includes(TRACKER_PATH_PATTERNS[i])) {
        return true;
      }
    }
    const q = lower.indexOf("?");
    const clean = q === -1 ? lower : lower.slice(0, q);
    const slash = clean.lastIndexOf("/");
    const file = slash === -1 ? clean : clean.slice(slash + 1);
    return TRACKER_SCRIPT_FILES.has(file);
  }

  class NousBlocker {
    constructor() {
      this.enabled = true;
      this.totalBlocked = 0;
      this.installed = false;
      this.onBlocked = null;
      this.allowlist = new Set();
      this._observer = {
        observe: (subject, topic) => {
          if (topic !== "http-on-modify-request" || !this.enabled) {
            return;
          }
          try {
            const channel = subject.QueryInterface(Ci.nsIHttpChannel);
            const loadInfo = channel.loadInfo;
            // Never block top-level document navigations that the user explicitly requested.
            if (
              loadInfo &&
              loadInfo.externalContentPolicyType === Ci.nsIContentPolicy.TYPE_DOCUMENT
            ) {
              return;
            }
            // Respect per-site allowlist based on top-level document origin.
            if (this.allowlist.size > 0) {
              try {
                const topHost = (
                  loadInfo?.topLevelPrincipal?.asciiHost ||
                  loadInfo?.triggeringPrincipal?.asciiHost ||
                  ""
                ).toLowerCase().replace(/^www\./, "");
                if (topHost && this.allowlist.has(topHost)) {
                  return;
                }
              } catch {}
            }
            const uri = channel.URI;
            if (!uri || (!uri.schemeIs("http") && !uri.schemeIs("https"))) {
              return;
            }
            let host = "";
            try {
              host = (uri.asciiHost || "").toLowerCase().replace(/\.$/, "");
            } catch {
              return;
            }
            if (!host) {
              return;
            }
            const path = uri.filePath || "";
            if (
              matchesDomainOrSuffix(host) ||
              matchesSubdomainHeuristic(host) ||
              matchesPathHeuristic(path)
            ) {
              channel.cancel(Cr.NS_BINDING_ABORTED);
              this.totalBlocked += 1;
              let browsingContextId = null;
              try {
                browsingContextId = loadInfo?.topBrowsingContext?.id ?? null;
              } catch {
                browsingContextId = null;
              }
              if (this.onBlocked) {
                this.onBlocked({ host, browsingContextId, total: this.totalBlocked });
              }
            }
          } catch {
            // Never interfere with request pipeline on unexpected channel states.
          }
        },
      };
    }

    get domainCount() {
      return BLOCKED_DOMAINS.size;
    }

    normalizeHost(host) {
      return String(host || "").trim().toLowerCase().replace(/^www\./, "");
    }

    isHostAllowed(host) {
      const h = this.normalizeHost(host);
      return Boolean(h && this.allowlist.has(h));
    }

    setHostAllowed(host, allowed) {
      const h = this.normalizeHost(host);
      if (!h) return;
      if (allowed) this.allowlist.add(h);
      else this.allowlist.delete(h);
    }

    getAllowedHosts() {
      return Array.from(this.allowlist);
    }

    setAllowedHosts(hosts) {
      this.allowlist.clear();
      if (Array.isArray(hosts)) {
        for (const h of hosts) {
          const norm = this.normalizeHost(h);
          if (norm) this.allowlist.add(norm);
        }
      }
    }

    install(enabled = true) {
      this.enabled = Boolean(enabled);
      if (!this.installed) {
        Services.obs.addObserver(this._observer, "http-on-modify-request");
        this.installed = true;
      }
    }

    uninstall() {
      if (this.installed) {
        try {
          Services.obs.removeObserver(this._observer, "http-on-modify-request");
        } catch {}
        this.installed = false;
      }
    }

    setEnabled(enabled) {
      this.enabled = Boolean(enabled);
    }
  }

  window.NousBlocker = new NousBlocker();
})();
