//! Bloqueador nativo de rastreadores, anúncios e telemetria em Rust puro.
//!
//! Usa uma tabela hash compartilhada (`OnceLock<HashSet<&'static str>>`) com busca
//! O(1) por domínio e sufixos pai (sem alocações no caminho quente), além de regras
//! customizadas de permissão/bloqueio e heurísticas leves de subdomínio e caminho.

use std::borrow::Borrow;
use std::collections::HashSet;
use std::hash::Hash;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};

use url::Url;

/// Lista nativa com os principais domínios de anúncios, rastreadores, analytics e telemetria.
const DEFAULT_BLOCKED_DOMAINS: &[&str] = &[
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
    // Ad Networks, Retargeting, Measurement & Verification
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
    // Marketing Automation / A/B Testing / Attribution
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
];

const TRACKER_SUBDOMAINS: &[&str] = &[
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
];

const TRACKER_PATH_PATTERNS: &[&str] = &[
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

const TRACKER_SCRIPT_FILES: &[&str] = &[
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
];

fn builtin_domains() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| DEFAULT_BLOCKED_DOMAINS.iter().copied().collect())
}

#[derive(Default)]
struct CustomRules {
    allow: HashSet<String>,
    block: HashSet<String>,
}

/// Filtro nativo de rastreadores, anúncios e telemetria.
pub struct TrackerBlocker {
    enabled: AtomicBool,
    blocked_count: AtomicU64,
    custom: RwLock<CustomRules>,
}

impl Default for TrackerBlocker {
    fn default() -> Self {
        Self::new(true)
    }
}

impl TrackerBlocker {
    pub fn new(enabled: bool) -> Self {
        TrackerBlocker {
            enabled: AtomicBool::new(enabled),
            blocked_count: AtomicU64::new(0),
            custom: RwLock::new(CustomRules::default()),
        }
    }

    /// Quantidade de domínios na lista embutida.
    pub fn builtin_domain_count() -> usize {
        builtin_domains().len()
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn blocked_count(&self) -> u64 {
        self.blocked_count.load(Ordering::Relaxed)
    }

    pub fn reset_blocked_count(&self) {
        self.blocked_count.store(0, Ordering::Relaxed);
    }

    /// Adiciona um domínio (e seus subdomínios) à lista de exceções permitidas.
    pub fn allow_domain(&self, domain: &str) {
        let norm = normalize_domain(domain);
        if norm.is_empty() {
            return;
        }
        let mut guard = self.custom.write().unwrap_or_else(|e| e.into_inner());
        guard.block.remove(&norm);
        guard.allow.insert(norm);
    }

    /// Adiciona um domínio (e seus subdomínios) à lista customizada de bloqueio.
    pub fn block_domain(&self, domain: &str) {
        let norm = normalize_domain(domain);
        if norm.is_empty() {
            return;
        }
        let mut guard = self.custom.write().unwrap_or_else(|e| e.into_inner());
        guard.allow.remove(&norm);
        guard.block.insert(norm);
    }

    /// Remove uma regra customizada de permissão.
    pub fn remove_allow(&self, domain: &str) -> bool {
        let norm = normalize_domain(domain);
        let mut guard = self.custom.write().unwrap_or_else(|e| e.into_inner());
        guard.allow.remove(&norm)
    }

    /// Remove uma regra customizada de bloqueio.
    pub fn remove_block(&self, domain: &str) -> bool {
        let norm = normalize_domain(domain);
        let mut guard = self.custom.write().unwrap_or_else(|e| e.into_inner());
        guard.block.remove(&norm)
    }

    /// Verifica se a URL seria bloqueada (sem incrementar o contador).
    pub fn is_blocked(&self, url: &Url) -> bool {
        if !self.is_enabled() {
            return false;
        }
        if !matches!(url.scheme(), "http" | "https") {
            return false;
        }
        let Some(raw_host) = url.host_str() else {
            return false;
        };
        let host_trimmed = raw_host.trim_end_matches('.');
        if host_trimmed.is_empty() {
            return false;
        }
        let lowered;
        let host = if host_trimmed.bytes().any(|b| b.is_ascii_uppercase()) {
            lowered = host_trimmed.to_ascii_lowercase();
            lowered.as_str()
        } else {
            host_trimmed
        };

        {
            let guard = self.custom.read().unwrap_or_else(|e| e.into_inner());
            if !guard.allow.is_empty() && matches_domain_or_suffix(&guard.allow, host) {
                return false;
            }
            if !guard.block.is_empty() && matches_domain_or_suffix(&guard.block, host) {
                return true;
            }
        }

        if matches_domain_or_suffix(builtin_domains(), host) {
            return true;
        }

        if matches_subdomain_heuristic(host) {
            return true;
        }

        matches_path_heuristic(url.path())
    }

    /// Verifica se a URL deve ser bloqueada e, se sim, incrementa o contador atômico.
    pub fn check(&self, url: &Url) -> bool {
        if self.is_blocked(url) {
            self.blocked_count.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }
}

fn normalize_domain(raw: &str) -> String {
    let s = raw.trim();
    let s = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .unwrap_or(s);
    let host = s.split(['/', '?', '#']).next().unwrap_or("");
    host.trim_start_matches(['*', '.'])
        .trim_end_matches('.')
        .to_ascii_lowercase()
}

fn matches_domain_or_suffix<T>(set: &HashSet<T>, host: &str) -> bool
where
    T: Eq + Hash + Borrow<str>,
{
    let mut cur = host;
    loop {
        if set.contains(cur) {
            return true;
        }
        let Some(dot) = cur.find('.') else {
            return false;
        };
        cur = &cur[dot + 1..];
    }
}

fn is_ip_literal(host: &str) -> bool {
    host.starts_with('[') || host.bytes().all(|b| b.is_ascii_digit() || b == b'.')
}

fn matches_subdomain_heuristic(host: &str) -> bool {
    if is_ip_literal(host) {
        return false;
    }
    let Some(first_dot) = host.find('.') else {
        return false;
    };
    let rest = &host[first_dot + 1..];
    // Só aplica heurística de subdomínio quando há ao menos 3 rótulos (sub.domínio.tld).
    if !rest.contains('.') {
        return false;
    }
    let first = &host[..first_dot];
    TRACKER_SUBDOMAINS.contains(&first)
        || first.starts_with("ads-")
        || first.starts_with("adserver")
        || first.starts_with("adservice")
        || first.starts_with("adsystem")
}

fn matches_path_heuristic(path: &str) -> bool {
    if path.is_empty() || path == "/" {
        return false;
    }
    let lowered;
    let p = if path.bytes().any(|b| b.is_ascii_uppercase()) {
        lowered = path.to_ascii_lowercase();
        lowered.as_str()
    } else {
        path
    };
    if TRACKER_PATH_PATTERNS.iter().any(|pat| p.contains(pat)) {
        return true;
    }
    let file = p.rsplit('/').next().unwrap_or("");
    TRACKER_SCRIPT_FILES.contains(&file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bloqueia_dominios_conhecidos_e_subdominios() {
        assert!(TrackerBlocker::builtin_domain_count() >= 60);
        let b = TrackerBlocker::default();
        for u in [
            "https://doubleclick.net/ad.js",
            "https://stats.g.doubleclick.net/r/collect",
            "https://www.google-analytics.com/analytics.js",
            "https://www.googletagmanager.com/gtm.js?id=GTM-123",
            "https://connect.facebook.net/en_US/fbevents.js",
            "https://static.hotjar.com/c/hotjar-1.js",
            "https://api.mixpanel.com/track/",
            "https://o123.ingest.sentry.io/api/1/envelope/",
            "https://cdn.taboola.com/libtrc/impl.js",
            "https://widgets.outbrain.com/outbrain.js",
            "https://static.criteo.net/js/ld/ld.js",
            "https://ib.adnxs.com/getuid",
            "https://sb.scorecardresearch.com/beacon.js",
            "https://pixel.quantserve.com/pixel.gif",
            "https://www.clarity.ms/tag/abc",
            "https://js-agent.newrelic.com/nr-spa.js",
            "https://www.datadoghq.com/browser-agent.js",
        ] {
            let url = Url::parse(u).unwrap();
            assert!(b.is_blocked(&url), "deveria bloquear {u}");
        }
    }

    #[test]
    fn permite_sites_normais_e_respeita_heuristicas() {
        let b = TrackerBlocker::default();
        for ok in [
            "https://example.com/",
            "https://pt.wikipedia.org/wiki/Rust",
            "https://doc.rust-lang.org/book/ch01-00-getting-started.html",
            "https://example.com/blog/analytics-guide",
            "file:///C:/temp/pagina.html",
            "data:text/html,<p>oi</p>",
        ] {
            let url = Url::parse(ok).unwrap();
            assert!(!b.is_blocked(&url), "não deveria bloquear {ok}");
        }

        // Heurísticas de subdomínio e caminho em domínios desconhecidos
        assert!(b.is_blocked(&Url::parse("https://telemetry.exemplo.org/event").unwrap()));
        assert!(b.is_blocked(&Url::parse("https://ads.exemplo.org/banner.png").unwrap()));
        assert!(b.is_blocked(&Url::parse("https://cdn.exemplo.org/gtag/js?id=G-1").unwrap()));
        assert!(b.is_blocked(&Url::parse("https://cdn.exemplo.org/assets/fbevents.js").unwrap()));
    }

    #[test]
    fn toggle_regras_customizadas_e_contador() {
        let b = TrackerBlocker::new(true);
        let ga = Url::parse("https://www.google-analytics.com/g/collect").unwrap();
        assert!(b.check(&ga));
        assert_eq!(b.blocked_count(), 1);

        // Exceção customizada tem precedência sobre lista nativa e heurísticas
        b.allow_domain("google-analytics.com");
        assert!(!b.check(&ga));
        assert_eq!(b.blocked_count(), 1);

        // Bloqueio customizado funciona para domínio arbitrário e subdomínios
        b.block_domain("*.rastreador.local");
        let custom = Url::parse("https://sub.rastreador.local/script.js").unwrap();
        assert!(b.check(&custom));
        assert_eq!(b.blocked_count(), 2);

        // Desativar não bloqueia nem incrementa contador
        b.set_enabled(false);
        assert!(!b.is_enabled());
        assert!(!b.check(&custom));
        assert_eq!(b.blocked_count(), 2);

        b.set_enabled(true);
        assert!(b.remove_block("rastreador.local"));
        assert!(!b.check(&custom));
    }
}
