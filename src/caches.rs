//! Les caches connus des gestionnaires de paquets et outils de dev, rangés
//! différemment sous Windows, macOS et Linux.

use crate::config::CacheEntry;
use std::path::PathBuf;

pub fn known() -> Vec<CacheEntry> {
    // Sans dossier personnel, les chemins deviendraient relatifs au dossier courant.
    let Some(home) = dirs::home_dir() else { return Vec::new() };
    // %LOCALAPPDATA% sous Windows, ~/Library/Caches sous macOS, ~/.cache sous Linux.
    let cache = dirs::cache_dir().unwrap_or_else(|| home.join(".cache"));
    let win = cfg!(windows);
    let mac = cfg!(target_os = "macos");
    // Le même cache selon le système, dans l'ordre Windows, macOS, Linux.
    let os = |w: PathBuf, m: PathBuf, l: PathBuf| if win { w } else if mac { m } else { l };
    let c = |name: &str, path: PathBuf, clean: &str| CacheEntry {
        name: name.into(),
        path,
        clean: (!clean.is_empty()).then(|| clean.into()),
    };

    let mut list = vec![
        c("npm", if win { cache.join("npm-cache") } else { home.join(".npm") }, "npm cache clean --force"),
        c(
            "pnpm (store)",
            os(cache.join("pnpm").join("store"), home.join("Library").join("pnpm").join("store"), home.join(".local").join("share").join("pnpm").join("store")),
            "pnpm store prune",
        ),
        c("pnpm (cache)", if win { cache.join("pnpm-cache") } else { cache.join("pnpm") }, ""),
        c("Yarn", os(cache.join("Yarn").join("Cache"), cache.join("Yarn"), cache.join("yarn")), "yarn cache clean"),
        c("Bun", home.join(".bun").join("install").join("cache"), "bun pm cache rm"),
        c("Deno", cache.join("deno"), ""),
        c("Cargo (registry)", home.join(".cargo").join("registry"), "cargo install cargo-cache && cargo cache -a"),
        c("Cargo (git)", home.join(".cargo").join("git"), ""),
        c("Rustup (toolchains)", home.join(".rustup").join("toolchains"), "rustup toolchain list / rustup toolchain uninstall <name>"),
        c("pip", if win { cache.join("pip").join("Cache") } else { cache.join("pip") }, "pip cache purge"),
        c("uv", if win { cache.join("uv").join("cache") } else { home.join(".cache").join("uv") }, "uv cache clean"),
        c("Hugging Face", home.join(".cache").join("huggingface"), ""),
        c("Ollama (models)", home.join(".ollama").join("models"), "ollama list / ollama rm <model>"),
        c("NuGet", home.join(".nuget").join("packages"), "dotnet nuget locals all --clear"),
        c("Gradle", home.join(".gradle").join("caches"), ""),
        c("Maven", home.join(".m2").join("repository"), ""),
        c("Go (modules)", home.join("go").join("pkg").join("mod"), "go clean -modcache"),
        c("Composer", if win { cache.join("Composer") } else { cache.join("composer") }, "composer clear-cache"),
        c("Playwright", cache.join("ms-playwright"), ""),
        c("Electron", if win { cache.join("electron").join("Cache") } else { cache.join("electron") }, ""),
    ];
    if win {
        list.push(c("Windows Temp", cache.join("Temp"), "Settings > System > Storage > Temporary files"));
    }
    if mac {
        list.push(c("Homebrew", cache.join("Homebrew"), "brew cleanup --prune=all"));
    }
    list
}
