//! Le parcours parallèle : un seul passage donne la taille de chaque dossier,
//! les plus gros fichiers et les dossiers jetables.

use crate::config::{key, JunkRule};
use rayon::prelude::*;
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;
use std::time::SystemTime;

pub struct Node {
    pub path: PathBuf,
    /// Taille totale, sous-dossiers compris.
    pub size: u64,
    /// Taille des fichiers posés directement dans ce dossier.
    pub own: u64,
    pub files: u64,
    /// Sous-dossiers triés du plus lourd au plus léger, gardés jusqu'à la profondeur demandée.
    pub children: Vec<Node>,
}

pub struct Junk {
    pub path: PathBuf,
    pub label: String,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

pub struct Results {
    pub files: Vec<(u64, PathBuf)>,
    pub junk: Vec<Junk>,
    pub errors: u64,
}

pub fn builtin_rules() -> Vec<JunkRule> {
    let r = |name: &str, label: &str, sibling: Option<&str>, child: Option<&str>| JunkRule {
        name: name.into(),
        label: label.into(),
        sibling: sibling.map(Into::into),
        child: child.map(Into::into),
    };
    vec![
        r("node_modules", "Node (node_modules)", None, None),
        r(".next", "Next.js (.next)", None, None),
        r(".nuxt", "Nuxt (.nuxt)", None, None),
        r(".svelte-kit", "SvelteKit", None, None),
        r(".turbo", "Turborepo", None, None),
        r(".parcel-cache", "Parcel", None, None),
        r(".angular", "Angular (cache)", None, None),
        r("target", "Rust (target)", Some("Cargo.toml"), None),
        r(".venv", "Python (.venv)", None, None),
        r("venv", "Python (venv)", None, Some("pyvenv.cfg")),
        r("__pycache__", "Python (__pycache__)", None, None),
        r(".pytest_cache", "Python (pytest)", None, None),
        r(".mypy_cache", "Python (mypy)", None, None),
        r(".ruff_cache", "Python (ruff)", None, None),
        r("vendor", "PHP (vendor)", Some("composer.json"), None),
        r(".gradle", "Gradle", None, None),
        r(".dart_tool", "Dart", None, None),
        r("Pods", "CocoaPods", Some("Podfile"), None),
    ]
}

/// Dossiers système jamais parcourus. Linux expose dans /proc des fichiers virtuels
/// géants (kcore), macOS montre une seconde fois les données sous /System/Volumes.
pub fn system_excludes() -> Vec<PathBuf> {
    let list: &[&str] = if cfg!(target_os = "linux") {
        &["/proc", "/sys", "/dev", "/run"]
    } else if cfg!(target_os = "macos") {
        &["/System/Volumes", "/dev"]
    } else {
        &[]
    };
    list.iter().map(PathBuf::from).collect()
}

/// Les `k` plus gros fichiers. Le seuil atomique évite le verrou pour presque tous les fichiers.
struct TopK {
    k: usize,
    min: AtomicU64,
    heap: Mutex<BinaryHeap<Reverse<(u64, PathBuf)>>>,
}

impl TopK {
    fn offer(&self, size: u64, path: impl FnOnce() -> PathBuf) {
        if self.k == 0 || size == 0 || size <= self.min.load(Relaxed) {
            return;
        }
        let mut h = self.heap.lock().unwrap();
        if h.len() < self.k {
            h.push(Reverse((size, path())));
        } else if size > h.peek().unwrap().0 .0 {
            h.pop();
            h.push(Reverse((size, path())));
        }
        if h.len() == self.k {
            self.min.store(h.peek().unwrap().0 .0, Relaxed);
        }
    }
}

pub struct Scanner {
    depth: usize,
    rules: Vec<JunkRule>,
    ignore_names: Vec<String>,
    exclude: HashSet<String>,
    top: TopK,
    junk: Mutex<Vec<Junk>>,
    errors: AtomicU64,
    /// Fichiers et octets vus jusqu'ici, lus par l'indicateur de progression.
    seen_files: AtomicU64,
    seen_bytes: AtomicU64,
}

impl Scanner {
    pub fn new(depth: usize, top_files: usize, rules: Vec<JunkRule>, ignore_names: Vec<String>, exclude: &[PathBuf]) -> Self {
        Self {
            depth,
            rules,
            ignore_names,
            exclude: exclude.iter().map(|p| key(p)).collect(),
            top: TopK { k: top_files, min: AtomicU64::new(0), heap: Mutex::new(BinaryHeap::new()) },
            junk: Mutex::new(Vec::new()),
            errors: AtomicU64::new(0),
            seen_files: AtomicU64::new(0),
            seen_bytes: AtomicU64::new(0),
        }
    }

    /// Fichiers et octets parcourus jusqu'ici, pendant l'analyse.
    pub fn progress(&self) -> (u64, u64) {
        (self.seen_files.load(Relaxed), self.seen_bytes.load(Relaxed))
    }

    pub fn scan(&self, root: &Path) -> Node {
        self.walk(root.to_path_buf(), 0, false)
    }

    pub fn finish(self) -> Results {
        let mut files: Vec<_> = self.top.heap.into_inner().unwrap().into_iter().map(|Reverse(x)| x).collect();
        files.sort_unstable_by(|a, b| b.0.cmp(&a.0));
        let mut junk = self.junk.into_inner().unwrap();
        junk.sort_unstable_by(|a, b| b.size.cmp(&a.size));
        Results { files, junk, errors: self.errors.into_inner() }
    }

    fn walk(&self, path: PathBuf, level: usize, in_junk: bool) -> Node {
        let mut own = 0;
        let mut files = 0;
        let mut dirs = Vec::new();
        let fail = || {
            self.errors.fetch_add(1, Relaxed);
        };
        match fs::read_dir(&path) {
            Ok(rd) => {
                for e in rd {
                    let Ok(e) = e else { fail(); continue };
                    let Ok(ft) = e.file_type() else { fail(); continue };
                    // Liens et jonctions, on ne les suit pas (boucles, doubles comptes).
                    if ft.is_symlink() {
                        continue;
                    }
                    if ft.is_dir() {
                        dirs.push(e);
                    } else {
                        match e.metadata() {
                            Ok(m) => {
                                let len = m.len();
                                own += len;
                                files += 1;
                                self.top.offer(len, || e.path());
                            }
                            Err(_) => fail(),
                        }
                    }
                }
            }
            Err(_) => fail(),
        }
        // Une mise à jour par dossier, pas par fichier : les threads ne se disputent pas le compteur.
        if files > 0 {
            self.seen_files.fetch_add(files, Relaxed);
            self.seen_bytes.fetch_add(own, Relaxed);
        }

        let visit = |e: fs::DirEntry| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            if self.ignore_names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                return None;
            }
            let p = e.path();
            if !self.exclude.is_empty() && self.exclude.contains(&key(&p)) {
                return None;
            }
            // Pas de détection à l'intérieur d'un dossier jetable (node_modules imbriqués).
            let rule = if in_junk { None } else { self.match_rule(&p, &name) };
            let child = self.walk(p, level + 1, in_junk || rule.is_some());
            if let Some(r) = rule {
                let modified = e.metadata().ok().and_then(|m| m.modified().ok());
                self.junk.lock().unwrap().push(Junk { path: child.path.clone(), label: r.label.clone(), size: child.size, modified });
            }
            Some(child)
        };
        // Rayon seulement quand il y a de quoi partager, la plupart des dossiers ont 0 ou 1 sous-dossier.
        let mut children: Vec<Node> = if dirs.len() < 2 {
            dirs.into_iter().filter_map(visit).collect()
        } else {
            dirs.into_par_iter().filter_map(visit).collect()
        };

        let size = own + children.iter().map(|c| c.size).sum::<u64>();
        let files = files + children.iter().map(|c| c.files).sum::<u64>();
        if level < self.depth {
            children.sort_unstable_by(|a, b| b.size.cmp(&a.size));
        } else {
            children = Vec::new();
        }
        Node { path, size, own, files, children }
    }

    fn match_rule(&self, p: &Path, name: &str) -> Option<&JunkRule> {
        self.rules.iter().find(|r| {
            r.name.eq_ignore_ascii_case(name)
                && r.sibling.as_ref().map_or(true, |s| p.parent().is_some_and(|par| par.join(s).exists()))
                && r.child.as_ref().map_or(true, |c| p.join(c).exists())
        })
    }
}

/// Taille et nombre de fichiers d'un dossier, sans rien d'autre.
pub fn dir_size(p: &Path) -> (u64, u64) {
    let n = Scanner::new(0, 0, Vec::new(), Vec::new(), &[]).scan(p);
    (n.size, n.files)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &Path, len: usize) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, vec![0u8; len]).unwrap();
    }

    fn name(n: &Node) -> &str {
        n.path.file_name().unwrap().to_str().unwrap()
    }

    #[test]
    fn tailles_arbre_et_gros_fichiers() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        file(&r.join("a").join("gros.bin"), 3000);
        file(&r.join("a").join("sous").join("moyen.bin"), 1000);
        file(&r.join("b").join("petit.bin"), 500);
        file(&r.join("racine.bin"), 200);

        let s = Scanner::new(1, 2, Vec::new(), Vec::new(), &[]);
        let root = s.scan(r);
        assert_eq!((root.size, root.own, root.files), (4700, 200, 4));
        let kids: Vec<_> = root.children.iter().map(|c| (name(c), c.size)).collect();
        assert_eq!(kids, [("a", 4000), ("b", 500)]);
        assert!(root.children[0].children.is_empty(), "profondeur 1, pas de petits-enfants");

        assert_eq!(s.progress(), (4, 4700));
        let res = s.finish();
        assert_eq!(res.files, [(3000, r.join("a").join("gros.bin")), (1000, r.join("a").join("sous").join("moyen.bin"))]);
        assert_eq!(res.errors, 0);
    }

    #[test]
    fn dossiers_jetables() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        file(&r.join("web").join("package.json"), 10);
        file(&r.join("web").join("node_modules").join("lib").join("index.js"), 100);
        // Un node_modules dans un node_modules ne compte pas à part.
        file(&r.join("web").join("node_modules").join("lib").join("node_modules").join("dep").join("index.js"), 50);
        file(&r.join("rust").join("Cargo.toml"), 10);
        file(&r.join("rust").join("target").join("debug").join("app"), 300);
        // Un target sans Cargo.toml à côté, un venv sans pyvenv.cfg, rien de jetable.
        file(&r.join("notes").join("target").join("plan.txt"), 40);
        file(&r.join("py").join("venv").join("script.py"), 20);
        file(&r.join("py2").join("venv").join("pyvenv.cfg"), 5);
        file(&r.join("py2").join("venv").join("lib.py"), 20);

        let s = Scanner::new(0, 0, builtin_rules(), Vec::new(), &[]);
        s.scan(r);
        let junk: Vec<_> = s.finish().junk.into_iter().map(|j| (j.path, j.size)).collect();
        assert_eq!(
            junk,
            [(r.join("rust").join("target"), 300), (r.join("web").join("node_modules"), 150), (r.join("py2").join("venv"), 25)]
        );
    }

    #[test]
    fn exclusions() {
        let tmp = tempfile::tempdir().unwrap();
        let r = tmp.path();
        file(&r.join("garde").join("a.bin"), 100);
        file(&r.join("exclu").join("b.bin"), 1000);
        file(&r.join("$Recycle.Bin").join("c.bin"), 500);

        // Le nom ignoré est en minuscules exprès, la comparaison ignore la casse.
        let s = Scanner::new(1, 5, Vec::new(), vec!["$recycle.bin".into()], &[r.join("exclu")]);
        let root = s.scan(r);
        assert_eq!(root.size, 100);
        assert_eq!(root.children.len(), 1);
        assert_eq!(s.finish().files, [(100, r.join("garde").join("a.bin"))]);
    }
}
