//! La configuration, dans `%APPDATA%\balai\config.toml`.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Dossiers analysés par `balai scan` quand on n'en donne pas.
    pub roots: Vec<PathBuf>,
    /// Dossiers jamais parcourus.
    pub exclude: Vec<PathBuf>,
    /// Noms de dossiers ignorés partout.
    pub ignore_names: Vec<String>,
    /// Dossiers jetables en plus de ceux connus (node_modules, target…).
    pub junk: Vec<JunkRule>,
    /// Caches en plus de ceux connus, affichés par `balai caches`.
    pub caches: Vec<CacheEntry>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            exclude: Vec::new(),
            ignore_names: vec!["$Recycle.Bin".into(), "System Volume Information".into()],
            junk: Vec::new(),
            caches: Vec::new(),
        }
    }
}

/// Un dossier régénérable, reconnu à son nom.
#[derive(Serialize, Deserialize, Clone)]
pub struct JunkRule {
    pub name: String,
    pub label: String,
    /// Fichier qui doit exister à côté (ex. `Cargo.toml` pour `target`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sibling: Option<String>,
    /// Fichier qui doit exister dedans (ex. `pyvenv.cfg` pour `venv`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct CacheEntry {
    pub name: String,
    pub path: PathBuf,
    /// Commande qui vide ce cache proprement.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clean: Option<String>,
}

const HEADER: &str = "# balai configuration. `balai config --help` to change it from the command line.\n\n";

pub fn path() -> Result<PathBuf> {
    Ok(dirs::config_dir().context("configuration folder not found")?.join("balai").join("config.toml"))
}

impl Config {
    pub fn load() -> Result<Self> {
        let p = path()?;
        match fs::read_to_string(&p) {
            Ok(s) => toml::from_str(&s).with_context(|| format!("unreadable configuration in {}", p.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("reading {}", p.display())),
        }
    }

    pub fn save(&self) -> Result<PathBuf> {
        let p = path()?;
        fs::create_dir_all(p.parent().unwrap())?;
        fs::write(&p, format!("{HEADER}{}", toml::to_string_pretty(self)?))?;
        Ok(p)
    }
}

/// Chemin absolu d'un dossier existant.
pub fn existing_dir(p: &Path) -> Result<PathBuf> {
    let abs = std::path::absolute(p)?;
    if !abs.is_dir() {
        bail!("{} is not a folder", abs.display());
    }
    Ok(abs)
}

/// Clé de comparaison des chemins, sans séparateur final. Windows mélange `/` et `\`
/// et ignore la casse, comme macOS par défaut. Linux distingue `Projets` et `projets`.
pub fn key(p: &Path) -> String {
    let mut s = p.to_string_lossy().into_owned();
    if cfg!(windows) {
        s = s.replace('/', "\\");
    }
    if cfg!(any(windows, target_os = "macos")) {
        s = s.to_lowercase();
    }
    s.trim_end_matches(['\\', '/']).to_string()
}

pub fn same(a: &Path, b: &Path) -> bool {
    key(a) == key(b)
}
