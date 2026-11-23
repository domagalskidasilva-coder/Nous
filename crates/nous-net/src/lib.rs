//! Rede do Nous.
//!
//! - HTTP/1.1 e HTTP/2 via `reqwest`/`hyper`, TLS via `rustls` com o provedor `ring`
//!   (só precisa do compilador C do sistema, sem CMake/NASM).
//! - Certificados verificados pelo repositório do próprio sistema operacional
//!   (`rustls-platform-verifier`).
//! - Um runtime `tokio` pequeno e só da rede (2 threads), compartilhado por todas as abas.
//! - Esquemas `http`, `https`, `file` e `data`.
//! - Bloqueador nativo de rastreadores/telemetria e cache LRU em memória limitado em bytes.

pub mod blocker;
pub mod cache;

pub use blocker::TrackerBlocker;
pub use cache::ResponseCache;

use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use encoding_rs::{Encoding, UTF_8};
pub use url::Url;

/// Configuração do cliente de rede.
#[derive(Clone, Debug)]
pub struct NetConfig {
    pub block_trackers: bool,
    pub cache_budget_bytes: usize,
}

impl Default for NetConfig {
    fn default() -> Self {
        NetConfig {
            block_trackers: true,
            cache_budget_bytes: ResponseCache::DEFAULT_BUDGET_BYTES,
        }
    }
}

/// Tamanho máximo de uma resposta (protege a memória contra respostas gigantes).
pub const MAX_BODY_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug)]
pub enum NetError {
    /// Falha ao preparar o cliente de rede.
    Init(String),
    /// Esquema que a rede não sabe buscar (ex.: `ftp:`).
    UnsupportedScheme(String),
    /// URL `data:` ou `file:` malformada.
    InvalidUrl(String),
    /// Não conseguiu conectar (DNS, recusa, TLS…).
    Connect(String),
    /// Demorou demais.
    Timeout,
    /// Resposta maior que [`MAX_BODY_BYTES`].
    TooLarge,
    /// Requisição barrada pelo bloqueador nativo de rastreadores.
    Blocked(String),
    /// Outro erro HTTP.
    Http(String),
    /// Erro lendo arquivo local.
    Io(std::io::Error),
}

impl fmt::Display for NetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NetError::Init(m) => write!(f, "não foi possível iniciar a rede: {m}"),
            NetError::UnsupportedScheme(s) => write!(f, "o esquema \"{s}:\" não é suportado"),
            NetError::InvalidUrl(m) => write!(f, "endereço inválido: {m}"),
            NetError::Connect(m) => write!(f, "não foi possível conectar: {m}"),
            NetError::Timeout => write!(f, "o servidor demorou demais para responder"),
            NetError::TooLarge => write!(
                f,
                "a resposta passou do limite de {} MB",
                MAX_BODY_BYTES / (1024 * 1024)
            ),
            NetError::Blocked(u) => write!(f, "bloqueado pelo filtro de rastreadores: {u}"),
            NetError::Http(m) => write!(f, "erro de rede: {m}"),
            NetError::Io(e) => write!(f, "erro ao ler o arquivo: {e}"),
        }
    }
}

impl std::error::Error for NetError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            NetError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for NetError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            return NetError::Timeout;
        }
        // junta a cadeia de causas: "erro ao enviar: dns: host não encontrado"
        let mut msg = e.to_string();
        let mut source = std::error::Error::source(&e);
        while let Some(s) = source {
            msg.push_str(": ");
            msg.push_str(&s.to_string());
            source = s.source();
        }
        if e.is_connect() {
            NetError::Connect(msg)
        } else {
            NetError::Http(msg)
        }
    }
}

/// Resposta já baixada por completo.
#[derive(Debug, Clone)]
pub struct Response {
    /// URL final, depois dos redirecionamentos.
    pub url: Url,
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

impl Response {
    /// Tipo MIME sem parâmetros, em minúsculas (`text/html`).
    pub fn mime(&self) -> Option<String> {
        let ct = self.content_type.as_deref()?;
        let mime = ct.split(';').next()?.trim().to_ascii_lowercase();
        (!mime.is_empty()).then_some(mime)
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    /// Decodifica o corpo como texto. Ordem: BOM, `charset` do Content-Type,
    /// `<meta charset>`/`http-equiv` nos primeiros 1024 bytes e, por fim, UTF-8.
    pub fn text(&self) -> String {
        let encoding = self
            .content_type
            .as_deref()
            .and_then(charset_from_content_type)
            .or_else(|| sniff_meta_charset(&self.body))
            .unwrap_or(UTF_8);
        // `decode` também respeita um BOM no começo, que tem prioridade
        let (text, _, _) = encoding.decode(&self.body);
        text.into_owned()
    }
}

fn label_to_encoding(label: &str) -> Option<&'static Encoding> {
    let enc = Encoding::for_label(label.trim().trim_matches(['"', '\'']).as_bytes())?;
    // a especificação HTML manda tratar UTF-16 declarado em <meta> como UTF-8
    if enc == encoding_rs::UTF_16LE || enc == encoding_rs::UTF_16BE {
        return Some(UTF_8);
    }
    Some(enc)
}

fn charset_from_content_type(ct: &str) -> Option<&'static Encoding> {
    ct.split(';').skip(1).find_map(|param| {
        let (k, v) = param.split_once('=')?;
        k.trim()
            .eq_ignore_ascii_case("charset")
            .then(|| label_to_encoding(v))
            .flatten()
    })
}

/// Procura `charset` em tags `<meta>` dos primeiros 1024 bytes (pré-scan simplificado).
fn sniff_meta_charset(body: &[u8]) -> Option<&'static Encoding> {
    let head = &body[..body.len().min(1024)];
    let lower: String = head
        .iter()
        .map(|&b| (b as char).to_ascii_lowercase())
        .collect();
    let mut rest = lower.as_str();
    while let Some(start) = rest.find("<meta") {
        let tag_start = &rest[start + 5..];
        let end = tag_start.find('>').unwrap_or(tag_start.len());
        let tag = &tag_start[..end];
        if let Some(pos) = tag.find("charset") {
            let after = tag[pos + 7..].trim_start();
            if let Some(value) = after.strip_prefix('=') {
                let value = value.trim_start().trim_start_matches(['"', '\'']);
                let len = value
                    .find(|c: char| {
                        c == '"' || c == '\'' || c == ';' || c == '/' || c.is_whitespace()
                    })
                    .unwrap_or(value.len());
                if let Some(enc) = label_to_encoding(&value[..len]) {
                    return Some(enc);
                }
            }
        }
        rest = &tag_start[end..];
    }
    None
}

struct Inner {
    /// `Option` só para poder desligar o runtime sem bloquear no `Drop`.
    runtime: Option<tokio::runtime::Runtime>,
    client: reqwest::Client,
    user_agent: String,
    blocker: TrackerBlocker,
    cache: ResponseCache,
}

impl Inner {
    fn runtime(&self) -> &tokio::runtime::Runtime {
        self.runtime
            .as_ref()
            .expect("runtime da rede já foi desligado")
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // `shutdown_background` não bloqueia: seguro mesmo se o último clone
        // for solto dentro de uma tarefa do próprio runtime.
        if let Some(rt) = self.runtime.take() {
            rt.shutdown_background();
        }
    }
}

/// Cliente de rede compartilhado (clonar é barato: só incrementa um contador).
#[derive(Clone)]
pub struct NetClient {
    inner: Arc<Inner>,
}

impl NetClient {
    pub fn new(user_agent: &str) -> Result<NetClient, NetError> {
        Self::with_config(user_agent, NetConfig::default())
    }

    pub fn with_config(user_agent: &str, config: NetConfig) -> Result<NetClient, NetError> {
        // O reqwest está compilado sem provedor de criptografia: instalamos o `ring`.
        // Se outro código já instalou um provedor, mantemos o que existe.
        let _ = rustls::crypto::ring::default_provider().install_default();

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("nous-rede")
            .enable_all()
            .build()
            .map_err(|e| NetError::Init(e.to_string()))?;

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static(
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            ),
        );
        headers.insert(
            reqwest::header::ACCEPT_LANGUAGE,
            reqwest::header::HeaderValue::from_static("pt-BR,pt;q=0.9,en;q=0.8"),
        );

        let client = reqwest::Client::builder()
            .user_agent(user_agent)
            .default_headers(headers)
            .cookie_store(true)
            .redirect(reqwest::redirect::Policy::limited(10))
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(30))
            .gzip(true)
            .brotli(true)
            .deflate(true)
            // conexões ociosas custam memória e sockets: guarda poucas, por pouco tempo
            .pool_max_idle_per_host(4)
            .pool_idle_timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| NetError::Init(e.to_string()))?;

        Ok(NetClient {
            inner: Arc::new(Inner {
                runtime: Some(runtime),
                client,
                user_agent: user_agent.to_owned(),
                blocker: TrackerBlocker::new(config.block_trackers),
                cache: ResponseCache::new(config.cache_budget_bytes),
            }),
        })
    }

    pub fn user_agent(&self) -> &str {
        &self.inner.user_agent
    }

    pub fn blocker(&self) -> &TrackerBlocker {
        &self.inner.blocker
    }

    pub fn set_blocker_enabled(&self, enabled: bool) {
        self.inner.blocker.set_enabled(enabled);
    }

    pub fn is_blocker_enabled(&self) -> bool {
        self.inner.blocker.is_enabled()
    }

    pub fn blocked_count(&self) -> u64 {
        self.inner.blocker.blocked_count()
    }

    pub fn cache(&self) -> &ResponseCache {
        &self.inner.cache
    }

    pub fn cache_bytes(&self) -> usize {
        self.inner.cache.used_bytes()
    }

    pub fn purge_cache(&self) {
        self.inner.cache.clear();
    }

    pub fn set_cache_budget(&self, budget_bytes: usize) {
        self.inner.cache.set_budget(budget_bytes);
    }

    /// Roda uma tarefa no runtime da rede (sem esperar por ela).
    pub fn spawn<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.inner.runtime().spawn(fut);
    }

    /// Espera um `Future` terminar, bloqueando a thread atual.
    /// Não chame de dentro de uma tarefa do próprio runtime (o tokio entra em pânico).
    pub fn block_on<F: Future>(&self, fut: F) -> F::Output {
        self.inner.runtime().block_on(fut)
    }

    /// Busca uma URL inteira. Pode ser aguardado de qualquer executor: o trabalho de
    /// rede roda sempre no runtime interno.
    pub async fn fetch(&self, url: &Url) -> Result<Response, NetError> {
        if self.inner.blocker.check(url) {
            return Err(NetError::Blocked(url.to_string()));
        }
        match url.scheme() {
            "http" | "https" => {
                if let Some(cached) = self.inner.cache.get(url) {
                    return Ok(cached);
                }
                let client = self.inner.client.clone();
                let req_url = url.clone();
                let task = self.inner.runtime().spawn(fetch_http(client, req_url));
                let resp = task.await.map_err(|e| NetError::Http(e.to_string()))??;
                if resp.is_success() {
                    self.inner.cache.insert(url, resp.clone());
                }
                Ok(resp)
            }
            "file" => {
                let url = url.clone();
                let task = self
                    .inner
                    .runtime()
                    .spawn_blocking(move || fetch_file(&url));
                task.await.map_err(|e| NetError::Http(e.to_string()))?
            }
            "data" => fetch_data(url),
            other => Err(NetError::UnsupportedScheme(other.to_owned())),
        }
    }

    /// Busca várias URLs em paralelo respeitando o limite `max_concurrent` via semáforo,
    /// devolvendo os resultados na mesma ordem da entrada.
    pub async fn fetch_many(
        &self,
        urls: Vec<Url>,
        max_concurrent: usize,
    ) -> Vec<Result<Response, NetError>> {
        if urls.is_empty() {
            return Vec::new();
        }
        let limit = max_concurrent.max(1);
        let sem = Arc::new(tokio::sync::Semaphore::new(limit));
        let mut handles = Vec::with_capacity(urls.len());
        for url in urls {
            let net = self.clone();
            let sem = Arc::clone(&sem);
            let handle = self.inner.runtime().spawn(async move {
                let _permit = sem
                    .acquire_owned()
                    .await
                    .map_err(|e| NetError::Http(e.to_string()))?;
                net.fetch(&url).await
            });
            handles.push(handle);
        }
        let mut out = Vec::with_capacity(handles.len());
        for handle in handles {
            match handle.await {
                Ok(res) => out.push(res),
                Err(e) => out.push(Err(NetError::Http(e.to_string()))),
            }
        }
        out
    }
}

async fn fetch_http(client: reqwest::Client, url: Url) -> Result<Response, NetError> {
    let mut resp = client.get(url).send().await?;
    let final_url = resp.url().clone();
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    if resp
        .content_length()
        .is_some_and(|n| n as usize > MAX_BODY_BYTES)
    {
        return Err(NetError::TooLarge);
    }
    let mut body = Vec::with_capacity(resp.content_length().unwrap_or(0).min(1 << 20) as usize);
    while let Some(chunk) = resp.chunk().await? {
        if body.len() + chunk.len() > MAX_BODY_BYTES {
            return Err(NetError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    body.shrink_to_fit();
    Ok(Response {
        url: final_url,
        status,
        content_type,
        body,
    })
}

fn mime_for_path(path: &std::path::Path) -> &'static str {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "html" | "htm" | "xhtml" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "json" => "application/json",
        "txt" | "md" => "text/plain",
        "xml" => "application/xml",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "application/octet-stream",
    }
}

fn fetch_file(url: &Url) -> Result<Response, NetError> {
    let path = url
        .to_file_path()
        .map_err(|_| NetError::InvalidUrl(url.to_string()))?;
    let meta = std::fs::metadata(&path).map_err(NetError::Io)?;
    if meta.len() as usize > MAX_BODY_BYTES {
        return Err(NetError::TooLarge);
    }
    let body = std::fs::read(&path).map_err(NetError::Io)?;
    Ok(Response {
        url: url.clone(),
        status: 200,
        content_type: Some(mime_for_path(&path).to_owned()),
        body,
    })
}

/// `data:[<mime>][;base64],<dados>`
fn fetch_data(url: &Url) -> Result<Response, NetError> {
    let raw = &url.as_str()["data:".len()..];
    let (meta, data) = raw
        .split_once(',')
        .ok_or_else(|| NetError::InvalidUrl("URL data: sem vírgula".into()))?;
    let is_base64 = meta.to_ascii_lowercase().ends_with(";base64");
    let mime = meta.trim_end_matches(";base64").trim_end_matches(";BASE64");
    let content_type = if mime.is_empty() {
        "text/plain;charset=US-ASCII".to_owned()
    } else {
        mime.to_owned()
    };
    let bytes = percent_decode(data);
    let body = if is_base64 {
        base64_decode(&bytes).ok_or_else(|| NetError::InvalidUrl("base64 inválido".into()))?
    } else {
        bytes
    };
    Ok(Response {
        url: url.clone(),
        status: 200,
        content_type: Some(content_type),
        body,
    })
}

fn percent_decode(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2]))
        {
            out.push(h << 4 | l);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

fn hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn base64_decode(input: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for &c in input {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client() -> NetClient {
        NetClient::new("Nous/teste").expect("cliente")
    }

    #[test]
    fn busca_arquivo_local() {
        let dir = std::env::temp_dir().join(format!("nous-net-teste-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pagina.html");
        std::fs::write(&path, "<p>olá, arquivo</p>").unwrap();
        let url = Url::from_file_path(&path).unwrap();
        let c = client();
        let resp = c.block_on(c.fetch(&url)).expect("fetch file://");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.mime().as_deref(), Some("text/html"));
        assert_eq!(resp.text(), "<p>olá, arquivo</p>");
        let missing = Url::from_file_path(dir.join("nao-existe.html")).unwrap();
        assert!(matches!(
            c.block_on(c.fetch(&missing)),
            Err(NetError::Io(_))
        ));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn charset_latin1_pelo_meta() {
        let mut body = b"<html><head><meta charset=\"iso-8859-1\"></head><body>".to_vec();
        body.extend_from_slice(&[b'a', 0xE7, 0xE3, b'o']); // "ação" em Latin-1
        let resp = Response {
            url: Url::parse("http://exemplo.test/").unwrap(),
            status: 200,
            content_type: Some("text/html".into()),
            body,
        };
        assert!(resp.text().ends_with("ação"));

        // http-equiv também vale
        let body = b"<meta http-equiv=\"Content-Type\" content=\"text/html; charset=windows-1252\">\x93x\x94".to_vec();
        let resp = Response {
            body,
            ..resp.clone()
        };
        assert!(resp.text().ends_with("\u{201C}x\u{201D}"));
    }

    #[test]
    fn charset_do_content_type_vence_o_meta() {
        let resp = Response {
            url: Url::parse("http://exemplo.test/").unwrap(),
            status: 200,
            content_type: Some("text/html; charset=UTF-8".into()),
            body: "<meta charset=iso-8859-1>ação".as_bytes().to_vec(),
        };
        assert!(resp.text().ends_with("ação"));
    }

    #[test]
    fn url_data() {
        let c = client();
        let url = Url::parse("data:text/html,%3Cp%3Eoi%3C%2Fp%3E").unwrap();
        let resp = c.block_on(c.fetch(&url)).unwrap();
        assert_eq!(resp.mime().as_deref(), Some("text/html"));
        assert_eq!(resp.text(), "<p>oi</p>");
        let url = Url::parse("data:text/plain;base64,T2zDoSBOb3Vz").unwrap();
        assert_eq!(c.block_on(c.fetch(&url)).unwrap().text(), "Olá Nous");
        let ftp = Url::parse("ftp://exemplo.test/").unwrap();
        assert!(matches!(
            c.block_on(c.fetch(&ftp)),
            Err(NetError::UnsupportedScheme(_))
        ));
    }

    #[test]
    fn bloqueador_e_cache_no_cliente() {
        let c = NetClient::with_config(
            "Nous/teste",
            NetConfig {
                block_trackers: true,
                cache_budget_bytes: 1024,
            },
        )
        .unwrap();
        assert!(c.is_blocker_enabled());
        assert_eq!(c.blocked_count(), 0);
        assert_eq!(c.cache_bytes(), 0);

        let tracker = Url::parse("https://www.google-analytics.com/analytics.js").unwrap();
        assert!(matches!(
            c.block_on(c.fetch(&tracker)),
            Err(NetError::Blocked(_))
        ));
        assert_eq!(c.blocked_count(), 1);

        c.set_blocker_enabled(false);
        assert!(!c.is_blocker_enabled());

        // Cache direto via cache() para testar cache_bytes / set_cache_budget / purge_cache sem rede externa
        let u1 = Url::parse("https://exemplo.test/a.css").unwrap();
        let u2 = Url::parse("https://exemplo.test/b.css").unwrap();
        c.cache().insert(
            &u1,
            Response {
                url: u1.clone(),
                status: 200,
                content_type: Some("text/css".into()),
                body: vec![b'x'; 600],
            },
        );
        assert_eq!(c.cache_bytes(), 600);
        // Hit em cache mesmo offline
        let cached = c.block_on(c.fetch(&u1)).unwrap();
        assert_eq!(cached.body.len(), 600);

        c.cache().insert(
            &u2,
            Response {
                url: u2.clone(),
                status: 200,
                content_type: Some("text/css".into()),
                body: vec![b'y'; 600],
            },
        );
        assert!(c.cache_bytes() <= 1024);
        c.set_cache_budget(500);
        assert!(c.cache_bytes() <= 500);
        c.purge_cache();
        assert_eq!(c.cache_bytes(), 0);
    }

    #[test]
    fn fetch_many_paralelo_e_ordenado() {
        let dir = std::env::temp_dir().join(format!("nous-net-many-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p1 = dir.join("um.css");
        let p2 = dir.join("dois.js");
        std::fs::write(&p1, "body { margin: 0; }").unwrap();
        std::fs::write(&p2, "console.log(1);").unwrap();

        let urls = vec![
            Url::parse("data:text/plain,primeiro").unwrap(),
            Url::from_file_path(&p1).unwrap(),
            Url::parse("https://doubleclick.net/ad.js").unwrap(),
            Url::from_file_path(&p2).unwrap(),
            Url::parse("data:text/html;base64,PGI+b2k8L2I+").unwrap(),
            Url::from_file_path(dir.join("faltando.txt")).unwrap(),
        ];

        let c = client();
        assert!(c.block_on(c.fetch_many(vec![], 4)).is_empty());

        let results = c.block_on(c.fetch_many(urls, 2));
        assert_eq!(results.len(), 6);
        assert_eq!(results[0].as_ref().unwrap().text(), "primeiro");
        assert_eq!(results[1].as_ref().unwrap().text(), "body { margin: 0; }");
        assert_eq!(
            results[1].as_ref().unwrap().mime().as_deref(),
            Some("text/css")
        );
        assert!(matches!(results[2], Err(NetError::Blocked(_))));
        assert_eq!(results[3].as_ref().unwrap().text(), "console.log(1);");
        assert_eq!(results[4].as_ref().unwrap().text(), "<b>oi</b>");
        assert!(matches!(results[5], Err(NetError::Io(_))));
        assert_eq!(c.blocked_count(), 1);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    #[ignore = "usa a internet"]
    fn https_de_verdade() {
        let c = client();
        let url = Url::parse("https://example.com/").unwrap();
        let resp = c.block_on(c.fetch(&url)).expect("HTTPS");
        assert_eq!(resp.status, 200);
        assert!(resp.text().contains("Example Domain"));
    }
}
