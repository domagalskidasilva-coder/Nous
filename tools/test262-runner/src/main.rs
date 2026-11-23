//! Runner do test262 para o `nous-js`.
//!
//! Uso: `test262-runner <pasta-do-test262> [filtro]`
//!
//! A pasta precisa ter `harness/` e `test/` (o layout do repositório oficial
//! tc39/test262). O filtro, se houver, é um trecho do caminho do teste.

use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use nous_js::{JsError, Vm};

/// Instruções máximas por teste (evita travar em laços infinitos).
const STEP_LIMIT: u64 = 5_000_000;
/// Quantas falhas mostrar no fim.
const SHOW_FAILURES: usize = 20;

#[derive(Debug, Default, PartialEq)]
struct Meta {
    includes: Vec<String>,
    flags: Vec<String>,
    negative_phase: Option<String>,
    negative_type: Option<String>,
}

/// Lê o bloco `/*--- ... ---*/` (YAML) com um parser mínimo: listas inline
/// `[a, b]`, listas em bloco `- a` e o mapa `negative:` com `phase`/`type`.
fn parse_meta(source: &str) -> Meta {
    let mut meta = Meta::default();
    let Some(start) = source.find("/*---") else {
        return meta;
    };
    let Some(len) = source[start..].find("---*/") else {
        return meta;
    };
    let yaml = &source[start + 5..start + len];
    let mut current: Option<&str> = None;
    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if indented {
            if let Some(item) = trimmed.strip_prefix("- ") {
                let item = item.trim().to_owned();
                match current {
                    Some("includes") => meta.includes.push(item),
                    Some("flags") => meta.flags.push(item),
                    _ => {}
                }
            } else if current == Some("negative")
                && let Some((k, v)) = trimmed.split_once(':')
            {
                let v = v.trim().to_owned();
                match k.trim() {
                    "phase" => meta.negative_phase = Some(v),
                    "type" => meta.negative_type = Some(v),
                    _ => {}
                }
            }
            continue;
        }
        let Some((key, value)) = trimmed.split_once(':') else {
            current = None;
            continue;
        };
        let key = key.trim();
        current = Some(match key {
            "includes" => "includes",
            "flags" => "flags",
            "negative" => "negative",
            _ => "",
        });
        let value = value.trim();
        if let Some(list) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
            let items = list
                .split(',')
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty());
            match key {
                "includes" => meta.includes.extend(items),
                "flags" => meta.flags.extend(items),
                _ => {}
            }
        }
    }
    meta
}

enum Outcome {
    Pass,
    Fail(String),
    Skip(&'static str),
}

struct Runner {
    root: PathBuf,
    harness_cache: HashMap<String, String>,
}

impl Runner {
    fn harness(&mut self, name: &str) -> Result<String, String> {
        if let Some(s) = self.harness_cache.get(name) {
            return Ok(s.clone());
        }
        let path = self.root.join("harness").join(name);
        let s = std::fs::read_to_string(&path)
            .map_err(|e| format!("harness {} não encontrado: {e}", path.display()))?;
        self.harness_cache.insert(name.to_owned(), s.clone());
        Ok(s)
    }

    fn run_test(&mut self, path: &Path) -> Outcome {
        let source = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => return Outcome::Fail(format!("leitura: {e}")),
        };
        let meta = parse_meta(&source);
        let has = |f: &str| meta.flags.iter().any(|x| x == f);
        if has("module") {
            return Outcome::Skip("module");
        }
        if has("async") {
            return Outcome::Skip("async");
        }
        if has("onlyStrict") {
            return Outcome::Skip("modo estrito ainda não implementado");
        }

        let mut full = String::new();
        if !has("raw") {
            let mut names = vec!["assert.js".to_owned(), "sta.js".to_owned()];
            names.extend(meta.includes.iter().cloned());
            for name in names {
                match self.harness(&name) {
                    Ok(h) => {
                        full.push_str(&h);
                        full.push('\n');
                    }
                    Err(e) => return Outcome::Fail(e),
                }
            }
        }
        full.push_str(&source);

        let result = panic::catch_unwind(AssertUnwindSafe(|| {
            let mut vm = Vm::new();
            vm.step_limit = Some(STEP_LIMIT);
            vm.output = Box::new(|_| {});
            vm.eval(&full).map(|_| ())
        }));
        let result = match result {
            Ok(r) => r,
            Err(p) => {
                let msg = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_else(|| "pânico".into());
                return Outcome::Fail(format!("pânico no motor: {msg}"));
            }
        };

        match (&meta.negative_phase, result) {
            (None, Ok(())) => Outcome::Pass,
            (None, Err(e)) => Outcome::Fail(e.to_string()),
            (Some(phase), Ok(())) => {
                Outcome::Fail(format!("esperava erro ({phase}) e rodou sem erro"))
            }
            (Some(phase), Err(e)) => {
                let expected = meta.negative_type.as_deref().unwrap_or("");
                let ok = match (phase.as_str(), &e) {
                    ("parse" | "early", JsError::Syntax(_)) => expected == "SyntaxError",
                    ("runtime" | "resolution", JsError::Thrown(msg)) => msg.starts_with(expected),
                    _ => false,
                };
                if ok {
                    Outcome::Pass
                } else {
                    Outcome::Fail(format!("esperava {expected} ({phase}), veio: {e}"))
                }
            }
        }
    }
}

fn collect_tests(dir: &Path, filter: Option<&str>, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_tests(&path, filter, out);
        } else if path.extension().is_some_and(|e| e == "js") {
            let s = path.to_string_lossy();
            if s.contains("_FIXTURE") {
                continue;
            }
            if filter.is_none_or(|f| s.replace('\\', "/").contains(f)) {
                out.push(path);
            }
        }
    }
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(root) = args.next() else {
        eprintln!("uso: test262-runner <pasta-do-test262> [filtro]");
        return ExitCode::FAILURE;
    };
    let filter = args.next();
    let root = PathBuf::from(root);
    let mut tests = Vec::new();
    collect_tests(&root.join("test"), filter.as_deref(), &mut tests);
    if tests.is_empty() {
        eprintln!("nenhum teste encontrado em {}", root.join("test").display());
        return ExitCode::FAILURE;
    }

    // Pânicos viram falhas do teste; não precisam sujar a saída.
    panic::set_hook(Box::new(|_| {}));
    let started = Instant::now();
    let mut runner = Runner {
        root: root.clone(),
        harness_cache: HashMap::new(),
    };
    let (mut pass, mut fail) = (0usize, 0usize);
    let mut skips: HashMap<&'static str, usize> = HashMap::new();
    let mut failures = Vec::new();
    for t in &tests {
        match runner.run_test(t) {
            Outcome::Pass => pass += 1,
            Outcome::Fail(msg) => {
                fail += 1;
                if failures.len() < SHOW_FAILURES {
                    let rel = t.strip_prefix(&root).unwrap_or(t).display().to_string();
                    failures.push((rel, msg));
                }
            }
            Outcome::Skip(why) => *skips.entry(why).or_default() += 1,
        }
    }
    let _ = panic::take_hook();

    let skipped: usize = skips.values().sum();
    let ran = pass + fail;
    let pct = |n: usize, d: usize| {
        if d == 0 {
            0.0
        } else {
            n as f64 * 100.0 / d as f64
        }
    };
    if !failures.is_empty() {
        println!("Primeiras falhas:");
        for (path, msg) in &failures {
            let msg: String = msg.chars().take(160).collect();
            println!("  FALHOU {path}\n         {msg}");
        }
        println!();
    }
    println!("test262 — nous-js");
    println!("  testes encontrados: {}", tests.len());
    println!(
        "  passaram:           {pass} ({:.2}% dos executados, {:.2}% do total)",
        pct(pass, ran),
        pct(pass, tests.len())
    );
    println!("  falharam:           {fail}");
    println!("  pulados:            {skipped}");
    let mut skip_list: Vec<_> = skips.into_iter().collect();
    skip_list.sort();
    for (why, n) in skip_list {
        println!("    - {why}: {n}");
    }
    println!("  tempo:              {:.2?}", started.elapsed());
    if fail == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter() {
        let src = "/*---\nincludes: [a.js, b.js]\nflags:\n  - raw\n  - noStrict\nnegative:\n  phase: parse\n  type: SyntaxError\nfeatures: [x]\n---*/\ncode";
        let m = parse_meta(src);
        assert_eq!(m.includes, vec!["a.js", "b.js"]);
        assert_eq!(m.flags, vec!["raw", "noStrict"]);
        assert_eq!(m.negative_phase.as_deref(), Some("parse"));
        assert_eq!(m.negative_type.as_deref(), Some("SyntaxError"));
        assert_eq!(parse_meta("sem frontmatter"), Meta::default());
    }

    #[test]
    fn fixtures_passam() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
        let mut tests = Vec::new();
        collect_tests(&root.join("test"), None, &mut tests);
        assert!(!tests.is_empty());
        let mut runner = Runner {
            root,
            harness_cache: HashMap::new(),
        };
        for t in &tests {
            match runner.run_test(t) {
                Outcome::Pass => {}
                Outcome::Fail(msg) => panic!("fixture {} falhou: {msg}", t.display()),
                Outcome::Skip(why) => panic!("fixture {} pulada: {why}", t.display()),
            }
        }
    }
}
