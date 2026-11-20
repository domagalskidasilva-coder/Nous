//! Páginas internas `nous://` (geradas localmente, sem rede).

use nous_engine::{MemoryReport, escape_html};

const BASE_CSS: &str = "\
body{margin:0;background:#07080d;color:#f5f6fb;font-family:system-ui,sans-serif}\
main{max-width:720px;margin:0 auto;padding:72px 24px}\
h1{font-size:56px;margin:0;color:#7c5cff;font-weight:800}\
h2{font-size:18px;color:#22d3ee;margin-top:40px}\
p{color:#b8bacb;line-height:1.5}\
a:link{color:#22d3ee;text-decoration:none}\
li{margin:6px 0;color:#d5d6e2}\
.dim{color:#7d7f94;font-size:13px}\
";

pub fn new_tab() -> String {
    format!(
        "<!doctype html><html><head><title>Nova aba</title><style>{BASE_CSS}</style></head><body><main>\
         <h1>NOUS</h1>\
         <p>Navegador leve feito em Rust, com motor próprio. Digite um endereço ou uma busca na barra acima.</p>\
         <h2>Sites leves para testar o motor</h2>\
         <ul>\
         <li><a href=\"https://example.com\">example.com</a></li>\
         <li><a href=\"https://pt.wikipedia.org/wiki/Rust_(linguagem_de_programa%C3%A7%C3%A3o)\">Wikipédia: Rust</a></li>\
         <li><a href=\"https://text.npr.org\">text.npr.org</a></li>\
         <li><a href=\"https://lite.cnn.com\">lite.cnn.com</a></li>\
         <li><a href=\"https://news.ycombinator.com\">Hacker News</a></li>\
         <li><a href=\"https://html.duckduckgo.com/html/\">DuckDuckGo (HTML)</a></li>\
         </ul>\
         <h2>Atalhos</h2>\
         <ul><li>Ctrl+T nova aba · Ctrl+W fecha · Ctrl+L barra de endereço · Ctrl+Tab próxima aba</li>\
         <li>F5 recarrega · Alt+← / Alt+→ histórico · Ctrl+clique abre em outra aba</li></ul>\
         <p class=\"dim\">Fase 1 do motor: HTML e CSS sem JavaScript ainda. Páginas internas: \
         <a href=\"nous://memoria\">nous://memoria</a> · <a href=\"nous://sobre\">nous://sobre</a></p>\
         </main></body></html>"
    )
}

pub struct TabMemory {
    pub title: String,
    pub report: MemoryReport,
}

fn kb(bytes: usize) -> String {
    format!("{:.0} KB", bytes as f64 / 1024.0)
}

pub fn memory(tabs: &[TabMemory], process: Option<usize>) -> String {
    let process = match process {
        Some(b) => format!("{:.1} MB", b as f64 / (1024.0 * 1024.0)),
        None => "indisponível".into(),
    };
    let mut items = String::new();
    for t in tabs {
        let r = &t.report;
        let layout = if r.layout_alive {
            format!("layout {}", kb(r.layout_bytes))
        } else {
            "layout liberado (aba em segundo plano)".into()
        };
        items.push_str(&format!(
            "<li><b>{}</b><br><span class=\"dim\">{} nós · DOM {} · estilo {} · {}</span></li>",
            escape_html(&t.title),
            r.dom_nodes,
            kb(r.dom_bytes),
            kb(r.style_bytes),
            layout
        ));
    }
    format!(
        "<!doctype html><html><head><title>Memória</title><style>{BASE_CSS}</style></head><body><main>\
         <h1>Memória</h1>\
         <p>Processo inteiro (RAM física): <b>{process}</b></p>\
         <h2>Abas</h2><ul>{items}</ul>\
         <p class=\"dim\">Abas em segundo plano liberam estilo e layout e recalculam ao voltar. \
         Recarregue esta página (F5) para atualizar os números.</p>\
         </main></body></html>"
    )
}

pub fn about() -> String {
    format!(
        "<!doctype html><html><head><title>Sobre o Nous</title><style>{BASE_CSS}</style></head><body><main>\
         <h1>NOUS</h1><p>Versão {}</p>\
         <h2>Peças</h2><ul>\
         <li>Interface e páginas desenhadas na GPU com Vello (wgpu)</li>\
         <li>Texto com Parley · HTML com html5ever · rede com hyper/reqwest + rustls</li>\
         <li>DOM em arena, cascata CSS própria (o Stylo entra a seguir)</li>\
         <li>Motor JavaScript próprio em construção (nous-js)</li></ul>\
         </main></body></html>",
        env!("CARGO_PKG_VERSION")
    )
}
