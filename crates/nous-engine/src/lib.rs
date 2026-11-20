//! API do motor web do Nous.
//!
//! - [`Engine`]: carrega páginas fora da thread da interface (rede → HTML → CSS) e avisa
//!   quando termina, por um callback.
//! - [`WebView`]: uma aba. Guarda histórico, a página carregada, o layout e a rolagem.
//!   Vive na thread da interface, que é dona do único [`LayoutEngine`] (sistema de fontes
//!   compartilhado entre a interface e as páginas).
//!
//! Hoje tudo roda num processo só, mas com esta fronteira (mensagens + callback) bem
//! definida; na Fase 4 o carregamento vai para processos de conteúdo isolados.

use std::sync::Arc;
use std::time::Instant;

use nous_dom::{Document, local_name};
use nous_layout::PageLayout;
use nous_net::NetClient;
use nous_style::{StyleMap, Stylist, Viewport};

pub use nous_layout::{LayoutEngine, Rect};
pub use nous_style::Viewport as PageViewport;
pub use url::Url;

pub type ViewId = u64;

#[cfg(target_os = "windows")]
const OS_TOKEN: &str = "Windows NT 10.0; Win64; x64";
#[cfg(target_os = "macos")]
const OS_TOKEN: &str = "Macintosh; Intel Mac OS X 10_15_7";
#[cfg(not(any(target_os = "windows", target_os = "macos")))]
const OS_TOKEN: &str = "X11; Linux x86_64";

/// Identificação enviada aos sites (honesta: diz que é o Nous).
pub fn user_agent() -> String {
    format!(
        "Mozilla/5.0 ({OS_TOKEN}) Nous/{}",
        env!("CARGO_PKG_VERSION")
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SearchEngine {
    DuckDuckGo,
    Google,
    Bing,
}

impl SearchEngine {
    pub const ALL: [SearchEngine; 3] = [
        SearchEngine::DuckDuckGo,
        SearchEngine::Google,
        SearchEngine::Bing,
    ];

    pub fn name(self) -> &'static str {
        match self {
            SearchEngine::DuckDuckGo => "DuckDuckGo",
            SearchEngine::Google => "Google",
            SearchEngine::Bing => "Bing",
        }
    }

    fn base(self) -> &'static str {
        match self {
            // versão HTML do DuckDuckGo: funciona sem JavaScript
            SearchEngine::DuckDuckGo => "https://html.duckduckgo.com/html/",
            SearchEngine::Google => "https://www.google.com/search",
            SearchEngine::Bing => "https://www.bing.com/search",
        }
    }

    pub fn search_url(self, query: &str) -> Url {
        Url::parse_with_params(self.base(), &[("q", query)]).expect("URL de busca válida")
    }
}

/// Transforma o que foi digitado na barra de endereço em URL (endereço ou busca).
pub fn resolve_input(input: &str, search: SearchEngine) -> Url {
    let t = input.trim();
    if let Ok(u) = Url::parse(t)
        && matches!(u.scheme(), "http" | "https" | "file" | "nous" | "about")
    {
        return u;
    }
    let looks_like_host = !t.contains(char::is_whitespace)
        && (t.contains('.') || t.starts_with("localhost") || t.contains(':'));
    if looks_like_host {
        let scheme = if t.starts_with("localhost") || t.starts_with("127.") {
            "http"
        } else {
            "https"
        };
        if let Ok(u) = Url::parse(&format!("{scheme}://{t}")) {
            return u;
        }
    }
    search.search_url(t)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Timings {
    pub fetch_ms: f32,
    pub parse_ms: f32,
    pub style_ms: f32,
}

/// Página carregada: DOM e folhas de estilo (o estilo é recalculado quando a largura muda).
pub struct LoadedPage {
    pub url: Url,
    pub title: String,
    pub document: Document,
    pub stylist: Stylist,
    pub timings: Timings,
    pub error: Option<String>,
}

pub enum EngineEvent {
    Loaded {
        view: ViewId,
        nav: u64,
        page: Box<LoadedPage>,
    },
}

type Notify = Arc<dyn Fn(EngineEvent) + Send + Sync>;

#[derive(Clone)]
pub struct Engine {
    net: NetClient,
    notify: Notify,
}

impl Engine {
    pub fn new(notify: impl Fn(EngineEvent) + Send + Sync + 'static) -> Result<Engine, String> {
        let net = NetClient::new(&user_agent()).map_err(|e| e.to_string())?;
        Ok(Engine {
            net,
            notify: Arc::new(notify),
        })
    }

    /// Começa a carregar `url` em segundo plano; o resultado chega por `notify`.
    pub fn load(&self, view: ViewId, nav: u64, url: Url) {
        let net = self.net.clone();
        let notify = self.notify.clone();
        self.net.spawn(async move {
            let started = Instant::now();
            let page = match net.fetch(&url).await {
                Ok(resp) => {
                    let fetch_ms = started.elapsed().as_secs_f32() * 1000.0;
                    let final_url = resp.url.clone();
                    let ct = resp
                        .content_type
                        .clone()
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    let html = if ct.is_empty() || ct.contains("html") || ct.contains("xml") {
                        resp.text()
                    } else if ct.starts_with("text/")
                        || ct.contains("json")
                        || ct.contains("javascript")
                    {
                        format!("<pre>{}</pre>", escape_html(&resp.text()))
                    } else {
                        message_html(
                            "Tipo de conteúdo ainda não suportado",
                            &format!("O Nous ainda não exibe conteúdo do tipo {ct}."),
                        )
                    };
                    let mut page = build_page(final_url.clone(), &html);
                    page.timings.fetch_ms = fetch_ms;
                    if resp.status >= 400 && page.title.is_empty() {
                        page.title = format!("Erro {}", resp.status);
                    }
                    load_stylesheets(&net, &mut page).await;
                    page
                }
                Err(e) => {
                    let html = message_html(
                        "Não foi possível abrir a página",
                        &format!("{}\n\n{}", url, e),
                    );
                    let mut page = build_page(url.clone(), &html);
                    page.error = Some(e.to_string());
                    page
                }
            };
            notify(EngineEvent::Loaded {
                view,
                nav,
                page: Box::new(page),
            });
        });
    }
}

/// Monta uma página a partir de HTML já em memória (usado também pelas páginas internas).
pub fn build_page(url: Url, html: &str) -> LoadedPage {
    let t = Instant::now();
    let document = nous_html::parse_html(html);
    let parse_ms = t.elapsed().as_secs_f32() * 1000.0;
    let mut stylist = Stylist::with_user_agent_sheet();
    // <style> embutidos, na ordem do documento
    for n in document.descendants(nous_dom::NodeId::DOCUMENT) {
        if let Some(el) = document.element(n)
            && el.is(&local_name!("style"))
        {
            let media = el
                .attr("media")
                .map(|m| Arc::new(nous_style::parse::parse_media_list(m)));
            stylist.add_author_sheet(&document.text_content(n), media);
        }
    }
    let title = document.title().unwrap_or_default();
    LoadedPage {
        url,
        title,
        document,
        stylist,
        timings: Timings {
            parse_ms,
            ..Default::default()
        },
        error: None,
    }
}

/// Busca as folhas `<link rel=stylesheet>` (até 12, em ordem).
async fn load_stylesheets(net: &NetClient, page: &mut LoadedPage) {
    let base = base_url(&page.document, &page.url);
    let mut links = Vec::new();
    for n in page.document.descendants(nous_dom::NodeId::DOCUMENT) {
        let Some(el) = page.document.element(n) else {
            continue;
        };
        if !el.is(&local_name!("link")) {
            continue;
        }
        let is_sheet = el.attr("rel").is_some_and(|r| {
            r.split_ascii_whitespace()
                .any(|w| w.eq_ignore_ascii_case("stylesheet"))
        });
        let alternate = el.attr("rel").is_some_and(|r| r.contains("alternate"));
        if is_sheet
            && !alternate
            && let Some(href) = el.attr("href")
            && let Ok(u) = base.join(href)
        {
            links.push((u, el.attr("media").map(str::to_owned)));
        }
    }
    let t = Instant::now();
    for (u, media) in links.into_iter().take(12) {
        if let Ok(resp) = net.fetch(&u).await
            && resp.status < 400
        {
            let media = media.map(|m| Arc::new(nous_style::parse::parse_media_list(&m)));
            page.stylist.add_author_sheet(&resp.text(), media);
        }
    }
    page.timings.fetch_ms += t.elapsed().as_secs_f32() * 1000.0;
}

fn base_url(doc: &Document, url: &Url) -> Url {
    doc.base_href()
        .and_then(|b| url.join(b).ok())
        .unwrap_or_else(|| url.clone())
}

pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

pub fn message_html(title: &str, body: &str) -> String {
    format!(
        "<!doctype html><html><head><title>{t}</title><style>\
         body{{font-family:sans-serif;background:#f6f6fa;color:#1d1d28;margin:0}}\
         div{{max-width:640px;margin:80px auto;padding:32px;background:#fff;border-radius:12px;border:1px solid #e3e3ee}}\
         h1{{font-size:22px;color:#7c5cff;margin-top:0}} pre{{white-space:pre-wrap;color:#555}}\
         </style></head><body><div><h1>{t}</h1><pre>{b}</pre></div></body></html>",
        t = escape_html(title),
        b = escape_html(body)
    )
}

// ------------------------------------------------------------------ WebView

#[derive(Clone, Copy, Debug, Default)]
pub struct MemoryReport {
    pub dom_nodes: usize,
    pub dom_bytes: usize,
    pub style_bytes: usize,
    pub layout_bytes: usize,
    pub layout_alive: bool,
}

pub struct WebView {
    pub id: ViewId,
    history: Vec<Url>,
    index: usize,
    nav_counter: u64,
    pending: Option<(u64, Url)>,
    page: Option<LoadedPage>,
    styles: Option<StyleMap>,
    layout: Option<PageLayout>,
    styled_for: Option<Viewport>,
    laid_out_width: f32,
    pub scroll_y: f32,
}

impl WebView {
    pub fn new(id: ViewId) -> Self {
        WebView {
            id,
            history: Vec::new(),
            index: 0,
            nav_counter: 0,
            pending: None,
            page: None,
            styles: None,
            layout: None,
            styled_for: None,
            laid_out_width: 0.0,
            scroll_y: 0.0,
        }
    }

    pub fn is_loading(&self) -> bool {
        self.pending.is_some()
    }

    pub fn url(&self) -> Option<&Url> {
        self.pending
            .as_ref()
            .map(|(_, u)| u)
            .or_else(|| self.history.get(self.index))
    }

    pub fn title(&self) -> String {
        match &self.page {
            Some(p) if !p.title.is_empty() => p.title.clone(),
            Some(p) => p.url.host_str().unwrap_or(p.url.as_str()).to_owned(),
            None => match self.url() {
                Some(u) => u.host_str().unwrap_or("Nova aba").to_owned(),
                None => "Nova aba".to_owned(),
            },
        }
    }

    pub fn page(&self) -> Option<&LoadedPage> {
        self.page.as_ref()
    }

    pub fn can_go_back(&self) -> bool {
        self.index > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.index + 1 < self.history.len()
    }

    fn next_nav(&mut self) -> u64 {
        self.nav_counter += 1;
        self.nav_counter
    }

    /// Navega para `url` criando uma entrada nova no histórico.
    pub fn navigate(&mut self, engine: &Engine, url: Url) {
        if !self.history.is_empty() {
            self.history.truncate(self.index + 1);
        }
        self.history.push(url.clone());
        self.index = self.history.len() - 1;
        self.start(engine, url);
    }

    /// Mostra HTML gerado localmente (páginas `nous://`), sem passar pela rede.
    pub fn show_internal(&mut self, url: Url, html: &str, new_entry: bool) {
        if new_entry {
            if !self.history.is_empty() {
                self.history.truncate(self.index + 1);
            }
            self.history.push(url.clone());
            self.index = self.history.len() - 1;
        }
        self.pending = None;
        self.set_page(build_page(url, html));
    }

    fn start(&mut self, engine: &Engine, url: Url) {
        let nav = self.next_nav();
        self.pending = Some((nav, url.clone()));
        engine.load(self.id, nav, url);
    }

    pub fn reload(&mut self, engine: &Engine) -> Option<Url> {
        let url = self.history.get(self.index)?.clone();
        if url.scheme() == "nous" {
            return Some(url);
        }
        self.start(engine, url);
        None
    }

    /// Volta no histórico. Devolve a URL se ela for interna (o app gera o HTML).
    pub fn go_back(&mut self, engine: &Engine) -> Option<Url> {
        if !self.can_go_back() {
            return None;
        }
        self.index -= 1;
        self.go_to_current(engine)
    }

    pub fn go_forward(&mut self, engine: &Engine) -> Option<Url> {
        if !self.can_go_forward() {
            return None;
        }
        self.index += 1;
        self.go_to_current(engine)
    }

    fn go_to_current(&mut self, engine: &Engine) -> Option<Url> {
        let url = self.history[self.index].clone();
        if url.scheme() == "nous" {
            return Some(url);
        }
        self.start(engine, url);
        None
    }

    pub fn stop(&mut self) {
        self.pending = None;
    }

    /// Recebe o resultado do motor. Ignora respostas de navegações antigas.
    pub fn on_loaded(&mut self, nav: u64, page: LoadedPage) -> bool {
        match &self.pending {
            Some((n, _)) if *n == nav => {
                // redirecionamentos: a entrada do histórico passa a ser a URL final
                if let Some(entry) = self.history.get_mut(self.index) {
                    *entry = page.url.clone();
                }
                self.pending = None;
                self.set_page(page);
                true
            }
            _ => false,
        }
    }

    fn set_page(&mut self, page: LoadedPage) {
        self.page = Some(page);
        self.styles = None;
        self.layout = None;
        self.styled_for = None;
        self.scroll_y = 0.0;
    }

    /// Calcula estilo e layout se faltarem ou se a largura mudou.
    pub fn ensure_layout(&mut self, le: &mut LayoutEngine, viewport: Viewport) {
        let Some(page) = &mut self.page else { return };
        let restyle = match self.styled_for {
            Some(vp) => {
                (vp.width - viewport.width).abs() > 0.5 || (vp.height - viewport.height).abs() > 0.5
            }
            None => true,
        };
        if restyle || self.styles.is_none() {
            let t = Instant::now();
            self.styles = Some(nous_style::compute_styles(
                &page.document,
                &page.stylist,
                viewport,
            ));
            page.timings.style_ms = t.elapsed().as_secs_f32() * 1000.0;
            self.styled_for = Some(viewport);
            self.layout = None;
        }
        if self.layout.is_none() || (self.laid_out_width - viewport.width).abs() > 0.5 {
            let styles = self.styles.as_ref().expect("estilos");
            self.layout = Some(nous_layout::layout_document(
                &page.document,
                styles,
                le,
                viewport.width,
            ));
            self.laid_out_width = viewport.width;
        }
        self.clamp_scroll(viewport.height);
    }

    /// Libera estilo e layout (aba em segundo plano). Recalculados ao voltar.
    pub fn discard_layout(&mut self) {
        self.styles = None;
        self.layout = None;
        self.styled_for = None;
    }

    pub fn content_height(&self) -> f32 {
        self.layout.as_ref().map(|l| l.height).unwrap_or(0.0)
    }

    pub fn canvas_color(&self) -> Option<nous_style::Rgba> {
        self.layout.as_ref().map(|l| l.canvas_color)
    }

    pub fn clamp_scroll(&mut self, viewport_height: f32) {
        let max = (self.content_height() - viewport_height).max(0.0);
        self.scroll_y = self.scroll_y.clamp(0.0, max);
    }

    pub fn scroll_by(&mut self, dy: f32, viewport_height: f32) {
        self.scroll_y += dy;
        self.clamp_scroll(viewport_height);
    }

    pub fn scroll_to(&mut self, y: f32, viewport_height: f32) {
        self.scroll_y = y;
        self.clamp_scroll(viewport_height);
    }

    /// Desenha a página. `transform` leva coordenadas da área de conteúdo para a tela.
    pub fn paint(
        &self,
        scene: &mut vello::Scene,
        transform: vello::kurbo::Affine,
        viewport: Viewport,
    ) {
        let Some(layout) = &self.layout else { return };
        let visible = Rect {
            x: 0.0,
            y: self.scroll_y,
            w: viewport.width,
            h: viewport.height,
        };
        let t = transform * vello::kurbo::Affine::translate((0.0, -self.scroll_y as f64));
        nous_paint::paint_page(scene, layout, t, visible);
    }

    /// Link no ponto (coordenadas da área de conteúdo), já resolvido contra a URL da página.
    pub fn link_at(&self, x: f32, y: f32) -> Option<Url> {
        let layout = self.layout.as_ref()?;
        let page = self.page.as_ref()?;
        let href = layout.link_at(x, y + self.scroll_y)?;
        let base = base_url(&page.document, &page.url);
        let url = base.join(href.trim()).ok()?;
        matches!(url.scheme(), "http" | "https" | "file" | "nous").then_some(url)
    }

    pub fn memory(&self) -> MemoryReport {
        let mut r = MemoryReport::default();
        if let Some(p) = &self.page {
            r.dom_nodes = p.document.len();
            r.dom_bytes = p.document.approx_heap_bytes();
        }
        if let Some(s) = &self.styles {
            r.style_bytes = s.approx_heap_bytes();
        }
        if let Some(l) = &self.layout {
            r.layout_bytes = l.approx_heap_bytes();
            r.layout_alive = true;
        }
        r
    }
}

/// RAM física usada pelo processo, se o sistema informar.
pub fn process_memory_bytes() -> Option<usize> {
    memory_stats::memory_stats().map(|m| m.physical_mem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entrada_da_barra() {
        let d = SearchEngine::DuckDuckGo;
        assert_eq!(
            resolve_input("example.com", d).as_str(),
            "https://example.com/"
        );
        assert_eq!(resolve_input("https://a.b/c", d).as_str(), "https://a.b/c");
        assert_eq!(
            resolve_input("localhost:8080", d).as_str(),
            "http://localhost:8080/"
        );
        assert_eq!(resolve_input("nous://memoria", d).scheme(), "nous");
        let s = resolve_input("rust lang", SearchEngine::Bing);
        assert_eq!(s.host_str(), Some("www.bing.com"));
        assert_eq!(s.query(), Some("q=rust+lang"));
    }

    #[test]
    fn historico_interno() {
        let mut v = WebView::new(1);
        v.show_internal(
            Url::parse("nous://nova-aba").unwrap(),
            "<title>A</title>",
            true,
        );
        v.show_internal(
            Url::parse("nous://memoria").unwrap(),
            "<title>B</title>",
            true,
        );
        assert_eq!(v.title(), "B");
        assert!(v.can_go_back());
        assert!(!v.can_go_forward());
    }
}
