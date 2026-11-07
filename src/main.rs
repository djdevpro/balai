mod caches;
mod config;
mod out;
mod scan;

use anstream::{eprintln, println};
use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use config::{existing_dir, key, same, CacheEntry, Config};
use out::{bold, dim, size, size_col};
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// La version publiée, posée par le workflow Release, sinon celle de Cargo.toml.
const VERSION: &str = match option_env!("BALAI_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

#[derive(Parser)]
#[command(name = "balai", version = VERSION, about = "Trouve ce qui remplit ton disque, vite.", args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    #[command(flatten)]
    scan: ScanArgs,
}

#[derive(Subcommand)]
enum Cmd {
    /// Analyse des dossiers, comme balai sans commande
    #[command(visible_alias = "s")]
    Scan(ScanArgs),
    /// Taille des caches des gestionnaires de paquets (npm, pnpm, Cargo, pip…)
    #[command(visible_alias = "c")]
    Caches,
    /// Lit ou modifie la configuration
    Config {
        #[command(subcommand)]
        action: ConfigCmd,
    },
}

#[derive(Args)]
struct ScanArgs {
    /// Dossiers à analyser, sinon ceux de la configuration
    paths: Vec<PathBuf>,
    /// Lignes par section
    #[arg(short = 'n', long, default_value_t = 15)]
    top: usize,
    /// Profondeur de l'arbre des dossiers
    #[arg(short, long, default_value_t = 1)]
    depth: usize,
    /// Sans la liste des plus gros fichiers
    #[arg(long)]
    no_files: bool,
    /// Sans la liste des dossiers jetables
    #[arg(long)]
    no_junk: bool,
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Affiche la configuration
    Show,
    /// Ajoute des dossiers à analyser par défaut
    Add {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Exclut des dossiers de toutes les analyses
    Exclude {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Ajoute un cache à surveiller dans balai caches
    Cache {
        name: String,
        path: PathBuf,
        /// Commande qui le vide
        #[arg(long)]
        clean: Option<String>,
    },
    /// Retire des dossiers analysés, exclus ou des caches
    Remove {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();

    // Plus de threads que de cœurs, le parcours attend surtout le système de fichiers.
    // RAYON_NUM_THREADS garde la main pour mesurer.
    let mut pool = rayon::ThreadPoolBuilder::new().stack_size(8 << 20);
    if std::env::var_os("RAYON_NUM_THREADS").is_none() {
        pool = pool.num_threads(std::thread::available_parallelism().map_or(8, |n| n.get() * 2).min(64));
    }
    let _ = pool.build_global();

    let result = match cli.cmd {
        None => scan(cli.scan),
        Some(Cmd::Scan(a)) => scan(a),
        Some(Cmd::Caches) => caches(),
        Some(Cmd::Config { action }) => config(action),
    };
    if let Err(e) = result {
        let chain: Vec<String> = e.chain().map(ToString::to_string).collect();
        eprintln!("{}", out::paint("1;31", &chain.join(". ")));
        std::process::exit(1);
    }
}

/// `child` est-il strictement sous `parent` ? Les deux sont des clés de `config::key`.
fn inside(child: &str, parent: &str) -> bool {
    child.len() > parent.len() && child.starts_with(parent) && child[parent.len()..].starts_with(['\\', '/'])
}

/// Garde un seul exemplaire de chaque chemin, et rien qui soit rangé dans un autre.
fn distinct<T>(items: Vec<T>, path: impl Fn(&T) -> &Path) -> Vec<T> {
    let mut seen = HashSet::new();
    let items: Vec<T> = items.into_iter().filter(|i| seen.insert(key(path(i)))).collect();
    let keys: Vec<String> = items.iter().map(|i| key(path(i))).collect();
    let outer: Vec<bool> = keys.iter().map(|k| !keys.iter().any(|p| inside(k, p))).collect();
    for (i, keep) in outer.iter().enumerate() {
        if !keep {
            let parent = keys.iter().position(|p| inside(&keys[i], p)).unwrap();
            eprintln!("{}", dim(&format!("{} est déjà compté dans {}", path(&items[i]).display(), path(&items[parent]).display())));
        }
    }
    items.into_iter().zip(outer).filter_map(|(i, keep)| keep.then_some(i)).collect()
}

fn scan(a: ScanArgs) -> Result<()> {
    let cfg = Config::load()?;
    let roots: Vec<PathBuf> = if !a.paths.is_empty() {
        a.paths.iter().map(|p| existing_dir(p)).collect::<Result<_>>()?
    } else if !cfg.roots.is_empty() {
        let found: Vec<PathBuf> = cfg
            .roots
            .iter()
            .filter(|r| {
                let ok = r.is_dir();
                if !ok {
                    eprintln!("{}", dim(&format!("{} introuvable, ignoré", r.display())));
                }
                ok
            })
            .cloned()
            .collect();
        if found.is_empty() {
            bail!("aucun dossier de la configuration n'existe, balai config show pour les voir");
        }
        found
    } else {
        let home = dirs::home_dir().context("dossier personnel introuvable")?;
        eprintln!("{}", dim(&format!("Aucun dossier configuré, analyse de {}. Ajoutes-en avec balai config add <dossier>.", home.display())));
        vec![home]
    };

    // Un dossier donné deux fois, ou rangé dans un autre dossier analysé, ne compte qu'une fois.
    let roots = distinct(roots, |r| r);

    let mut rules = scan::builtin_rules();
    rules.extend(cfg.junk.iter().cloned());
    let junk_on = !a.no_junk;
    let mut exclude = cfg.exclude.clone();
    exclude.extend(scan::system_excludes());
    let scanner = scan::Scanner::new(
        a.depth,
        if a.no_files { 0 } else { a.top },
        if junk_on { rules } else { Vec::new() },
        cfg.ignore_names.clone(),
        &exclude,
    );

    let t = Instant::now();
    let nodes: Vec<scan::Node> = roots.par_iter().map(|r| scanner.scan(r)).collect();
    let res = scanner.finish();
    out::print_scan(&nodes, &res, a.top, junk_on, t.elapsed());
    Ok(())
}

fn caches() -> Result<()> {
    let cfg = Config::load()?;
    let mut list = caches::known();
    list.extend(cfg.caches.iter().cloned());
    list.retain(|c| c.path.is_dir());
    // Un cache rangé dans un autre ne s'ajoute pas deux fois au total.
    let list = distinct(list, |c| &c.path);

    let t = Instant::now();
    let mut sized: Vec<(CacheEntry, u64)> = list
        .into_par_iter()
        .map(|c| {
            let (s, _) = scan::dir_size(&c.path);
            (c, s)
        })
        .collect();
    sized.sort_by(|a, b| b.1.cmp(&a.1));

    let total: u64 = sized.iter().map(|x| x.1).sum();
    println!("\n{}  {}", bold("Caches"), dim(&format!("{} au total", size(total))));
    for (c, s) in &sized {
        println!("  {}  {:<20} {}", size_col(*s), c.name, dim(&c.path.display().to_string()));
        if let Some(cmd) = &c.clean {
            println!("  {:>9}  {:<20} {}", "", "", out::paint("32", &format!("→ {cmd}")));
        }
    }
    println!("\n{}", dim(&format!("Analysé en {}", out::duration(t.elapsed()))));
    Ok(())
}

fn absolute_all(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    paths.iter().map(|p| Ok(std::path::absolute(p)?)).collect()
}

fn in_config(cfg: &Config, p: &Path) -> bool {
    cfg.roots.iter().chain(&cfg.exclude).any(|r| same(r, p)) || cfg.caches.iter().any(|c| same(&c.path, p))
}

fn config(action: ConfigCmd) -> Result<()> {
    let mut cfg = Config::load()?;
    // Chaque commande vérifie tous ses chemins avant de toucher à la configuration.
    match action {
        ConfigCmd::Show => {
            println!("{}", dim(&format!("# {}", config::path()?.display())));
            print!("{}", toml::to_string_pretty(&cfg)?);
            return Ok(());
        }
        ConfigCmd::Add { paths } => {
            let paths = paths.iter().map(|p| existing_dir(p)).collect::<Result<Vec<_>>>()?;
            for p in paths {
                if cfg.roots.iter().any(|r| same(r, &p)) {
                    println!("{} déjà présent", p.display());
                } else {
                    println!("+ {}", p.display());
                    cfg.roots.push(p);
                }
            }
        }
        ConfigCmd::Exclude { paths } => {
            for p in absolute_all(&paths)? {
                if !cfg.exclude.iter().any(|r| same(r, &p)) {
                    println!("exclu {}", p.display());
                    cfg.exclude.push(p);
                }
            }
        }
        ConfigCmd::Cache { name, path, clean } => {
            let path = existing_dir(&path)?;
            cfg.caches.retain(|c| !same(&c.path, &path));
            println!("+ cache {name} ({})", path.display());
            cfg.caches.push(CacheEntry { name, path, clean });
        }
        ConfigCmd::Remove { paths } => {
            let paths = absolute_all(&paths)?;
            if let Some(p) = paths.iter().find(|p| !in_config(&cfg, p)) {
                bail!("{} n'est pas dans la configuration", p.display());
            }
            for p in &paths {
                cfg.roots.retain(|r| !same(r, p));
                cfg.exclude.retain(|r| !same(r, p));
                cfg.caches.retain(|c| !same(&c.path, p));
                println!("- {}", p.display());
            }
        }
    }
    let saved = cfg.save()?;
    println!("{}", dim(&format!("enregistré dans {}", saved.display())));
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    /// clap vérifie la définition des commandes (noms, alias, arguments en conflit).
    #[test]
    fn ligne_de_commande_valide() {
        super::Cli::command().debug_assert();
    }
}
