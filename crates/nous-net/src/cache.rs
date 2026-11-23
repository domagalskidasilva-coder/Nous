//! Cache LRU em memória com teto rígido em bytes para sub-recursos HTTP/HTTPS.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use url::Url;

use crate::Response;

/// Orçamento padrão do cache de respostas (8 MB).
pub const DEFAULT_CACHE_BUDGET_BYTES: usize = 8 * 1024 * 1024;
/// Limite de entradas para respostas vazias (0 byte), evitando crescimento ilimitado.
const MAX_ENTRIES: usize = 4096;

struct CacheEntry {
    response: Response,
    bytes: usize,
    seq: u64,
}

struct CacheInner {
    budget_bytes: usize,
    used_bytes: usize,
    next_seq: u64,
    entries: HashMap<String, CacheEntry>,
    lru: VecDeque<(String, u64)>,
}

impl CacheInner {
    fn new(budget_bytes: usize) -> Self {
        CacheInner {
            budget_bytes,
            used_bytes: 0,
            next_seq: 0,
            entries: HashMap::new(),
            lru: VecDeque::new(),
        }
    }

    fn next_generation(&mut self) -> u64 {
        self.next_seq = self.next_seq.wrapping_add(1);
        self.next_seq
    }

    fn compact_queue_if_needed(&mut self) {
        let limit = self.entries.len().saturating_mul(2).max(64);
        if self.lru.len() <= limit {
            return;
        }
        let mut live: Vec<(u64, String)> = self
            .entries
            .iter()
            .map(|(k, e)| (e.seq, k.clone()))
            .collect();
        live.sort_unstable_by_key(|(seq, _)| *seq);
        self.lru.clear();
        for (seq, key) in live {
            self.lru.push_back((key, seq));
        }
        self.lru.shrink_to_fit();
    }

    fn evict_one(&mut self) -> bool {
        while let Some((key, seq)) = self.lru.pop_front() {
            let is_live = self.entries.get(&key).is_some_and(|e| e.seq == seq);
            if is_live && let Some(removed) = self.entries.remove(&key) {
                self.used_bytes -= removed.bytes;
                return true;
            }
        }
        false
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.lru.clear();
        self.used_bytes = 0;
        self.entries.shrink_to_fit();
        self.lru.shrink_to_fit();
    }

    fn enforce_limits(&mut self) {
        while self.used_bytes > self.budget_bytes || self.entries.len() > MAX_ENTRIES {
            if !self.evict_one() {
                break;
            }
        }
        if self.entries.is_empty() {
            self.used_bytes = 0;
            self.lru.clear();
        }
    }
}

/// Cache LRU limitado estritamente a `budget_bytes`.
pub struct ResponseCache {
    inner: Mutex<CacheInner>,
}

impl Default for ResponseCache {
    fn default() -> Self {
        Self::new(DEFAULT_CACHE_BUDGET_BYTES)
    }
}

impl ResponseCache {
    pub const DEFAULT_BUDGET_BYTES: usize = DEFAULT_CACHE_BUDGET_BYTES;

    pub fn new(budget_bytes: usize) -> Self {
        ResponseCache {
            inner: Mutex::new(CacheInner::new(budget_bytes)),
        }
    }

    /// Total exato de bytes de corpo atualmente armazenados no cache.
    pub fn used_bytes(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).used_bytes
    }

    pub fn budget_bytes(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).budget_bytes
    }

    pub fn len(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Ajusta o orçamento e despeja as entradas menos recentes até caber no novo limite.
    pub fn set_budget(&self, budget_bytes: usize) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.budget_bytes = budget_bytes;
        if budget_bytes == 0 {
            guard.clear();
            return;
        }
        guard.enforce_limits();
        guard.compact_queue_if_needed();
    }

    /// Remove todas as entradas e devolve a capacidade ociosa das estruturas internas.
    pub fn clear(&self) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.clear();
    }

    /// Busca uma resposta em cache para a URL e a promove para mais recente (MRU).
    pub fn get(&self, url: &Url) -> Option<Response> {
        if !matches!(url.scheme(), "http" | "https") {
            return None;
        }
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let key = url.as_str();
        if !guard.entries.contains_key(key) {
            return None;
        }
        let seq = guard.next_generation();
        let entry = guard.entries.get_mut(key)?;
        entry.seq = seq;
        let resp = entry.response.clone();
        guard.lru.push_back((key.to_owned(), seq));
        guard.compact_queue_if_needed();
        Some(resp)
    }

    /// Insere uma resposta HTTP/HTTPS bem-sucedida se couber no orçamento.
    pub fn insert(&self, url: &Url, mut response: Response) {
        if !matches!(url.scheme(), "http" | "https") || !response.is_success() {
            return;
        }
        response.body.shrink_to_fit();
        let bytes = response.body.len();
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let key = url.as_str().to_owned();

        if let Some(prev) = guard.entries.remove(&key) {
            guard.used_bytes -= prev.bytes;
        }

        if guard.budget_bytes == 0 || bytes > guard.budget_bytes {
            return;
        }

        let seq = guard.next_generation();
        guard.used_bytes += bytes;
        guard.entries.insert(
            key.clone(),
            CacheEntry {
                response,
                bytes,
                seq,
            },
        );
        guard.lru.push_back((key, seq));
        guard.enforce_limits();
        guard.compact_queue_if_needed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_resp(url: &str, size: usize, status: u16) -> (Url, Response) {
        let u = Url::parse(url).unwrap();
        let r = Response {
            url: u.clone(),
            status,
            content_type: Some("text/css".into()),
            body: vec![b'a'; size],
        };
        (u, r)
    }

    #[test]
    fn lru_eviction_e_contagem_exata() {
        let cache = ResponseCache::new(100);
        let (u1, r1) = fake_resp("https://a.test/1.css", 40, 200);
        let (u2, r2) = fake_resp("https://a.test/2.css", 40, 200);
        let (u3, r3) = fake_resp("https://a.test/3.css", 40, 200);

        cache.insert(&u1, r1);
        cache.insert(&u2, r2);
        assert_eq!(cache.used_bytes(), 80);
        assert_eq!(cache.len(), 2);

        // Acessar u1 torna u2 o menos recentemente usado (LRU)
        assert!(cache.get(&u1).is_some());

        cache.insert(&u3, r3);
        assert_eq!(cache.used_bytes(), 80);
        assert!(cache.get(&u1).is_some());
        assert!(cache.get(&u2).is_none());
        assert!(cache.get(&u3).is_some());
    }

    #[test]
    fn redimensionamento_limpeza_e_regras() {
        let cache = ResponseCache::new(100);
        let (u1, r1) = fake_resp("https://a.test/1.js", 50, 200);
        let (u2, r2) = fake_resp("https://a.test/2.js", 40, 200);
        let (u_err, r_err) = fake_resp("https://a.test/404.js", 10, 404);
        let (u_big, r_big) = fake_resp("https://a.test/big.js", 200, 200);

        cache.insert(&u1, r1);
        cache.insert(&u2, r2);
        cache.insert(&u_err, r_err);
        cache.insert(&u_big, r_big);
        assert_eq!(cache.used_bytes(), 90);
        assert!(cache.get(&u_err).is_none());
        assert!(cache.get(&u_big).is_none());

        cache.set_budget(45);
        assert!(cache.used_bytes() <= 45);
        assert!(cache.get(&u1).is_none());
        assert!(cache.get(&u2).is_some());
        assert_eq!(cache.used_bytes(), 40);

        cache.clear();
        assert_eq!(cache.used_bytes(), 0);
        assert!(cache.is_empty());

        cache.set_budget(0);
        let (u3, r3) = fake_resp("https://a.test/3.js", 10, 200);
        cache.insert(&u3, r3);
        assert_eq!(cache.used_bytes(), 0);
    }
}
