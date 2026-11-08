mod caches;
mod clean;
mod config;
mod out;
mod progress;
mod scan;
mod update;

use anstream::{eprint, eprintln, println};
use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use config::{existing_dir, key, same, CacheEntry, Config};
use out::{bold, dim, size, size_col};
use rayon::prelude::*;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::time::{Instant, SystemTime};

/// La version publiée, posée par le workflow Release, sinon celle de Cargo.toml.
const VERSION: &str = match option_env!("BALAI_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};

const EXAMPLES: &str = "\
Examples:
  balai                           your configured folders, or your home folder
  balai . -n 30 -d 2              30 rows per section, folder tree 2 levels deep
  balai junk                      the full list of disposable folders
  balai junk --days 30            those untouched for 30 days or more
  balai clean --days 30           shows them, asks, then deletes them
  balai clean -k node --dry-run   the node_modules that would go, touching nothing
  balai caches                    npm, pnpm, Cargo, pip… cache sizes

balai <command> --help for the details of a command.";

#[derive(Parser)]
#[command(
    name = "balai",
    version = VERSION,
    about = "Find what fills your disk, fast.",
    args_conflicts_with_subcommands = true,
    after_help = EXAMPLES
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    #[command(flatten)]
    scan: ScanArgs,
}

#[derive(Subcommand)]
enum Cmd {
    /// Scan folders, same as balai with no command
    #[command(visible_alias = "s")]
    Scan(ScanArgs),
    /// Full list of disposable folders (node_modules, target, .venv…)
    #[command(visible_alias = "j")]
    Junk(JunkArgs),
    /// Delete disposable folders, after showing them and asking
    Clean(CleanArgs),
    /// Size of package manager caches (npm, pnpm, Cargo, pip…)
    #[command(visible_alias = "c")]
    Caches,
    /// Show or change the configuration
    Config {
        #[command(subcommand)]
        action: ConfigCmd,
    },
    /// Install the latest version published on GitHub
    Update {
        /// Only check whether a new version exists
        #[arg(long)]
        check: bool,
    },
}

#[derive(Args)]
struct ScanArgs {
    /// Folders to scan, otherwise the configured ones
    paths: Vec<PathBuf>,
    /// Rows per section
    #[arg(short = 'n', long, default_value_t = 15)]
    top: usize,
    /// Depth of the folder tree
    #[arg(short, long, default_value_t = 1)]
    depth: usize,
    /// Hide the biggest files
    #[arg(long)]
    no_files: bool,
    /// Hide disposable folders
    #[arg(long)]
    no_junk: bool,
}

#[derive(Args)]
struct JunkArgs {
    /// Folders to scan, otherwise the configured ones
    paths: Vec<PathBuf>,
    /// Only folders untouched for at least this many days
    #[arg(long, value_name = "DAYS", default_value_t = 0)]
    days: u64,
    /// Only this type, matched on its label: node, target, rust, python, .venv… (repeatable)
    #[arg(short, long = "kind", value_name = "TYPE")]
    kinds: Vec<String>,
}

#[derive(Args)]
#[command(after_help = "Nothing is deleted without your confirmation, unless you pass --yes. \
A folder whose modification date is unknown is never deleted.")]
struct CleanArgs {
    /// Folders to scan, otherwise the configured ones
    paths: Vec<PathBuf>,
    /// Only folders untouched for at least this many days (0: all)
    #[arg(long, value_name = "DAYS", default_value_t = 30)]
    days: u64,
    /// Only this type, matched on its label: node, target, rust, python, .venv… (repeatable)
    #[arg(short, long = "kind", value_name = "TYPE")]
    kinds: Vec<String>,
    /// Show what would be deleted, delete nothing
    #[arg(long)]
    dry_run: bool,
    /// Delete without asking
    #[arg(short, long)]
    yes: bool,
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Show the configuration
    Show,
    /// Add folders scanned by default
    Add {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Exclude folders from every scan
    Exclude {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Add a cache to watch in balai caches
    Cache {
        name: String,
        path: PathBuf,
        /// Command that empties it
        #[arg(long)]
        clean: Option<String>,
    },
    /// Remove scanned folders, exclusions or caches
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
        Some(Cmd::Junk(a)) => junk(a),
        Some(Cmd::Clean(a)) => clean(a),
        Some(Cmd::Caches) => caches(),
        Some(Cmd::Config { action }) => config(action),
        Some(Cmd::Update { check }) => update::run(check),
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
            eprintln!("{}", dim(&format!("{} is already counted in {}", path(&items[i]).display(), path(&items[parent]).display())));
        }
    }
    items.into_iter().zip(outer).filter_map(|(i, keep)| keep.then_some(i)).collect()
}

/// Les dossiers à analyser : ceux donnés, sinon ceux de la configuration, sinon le dossier perso.
fn roots(paths: &[PathBuf], cfg: &Config) -> Result<Vec<PathBuf>> {
    let roots: Vec<PathBuf> = if !paths.is_empty() {
        paths.iter().map(|p| existing_dir(p)).collect::<Result<_>>()?
    } else if !cfg.roots.is_empty() {
        let found: Vec<PathBuf> = cfg
            .roots
            .iter()
            .filter(|r| {
                let ok = r.is_dir();
                if !ok {
                    eprintln!("{}", dim(&format!("{} not found, skipped", r.display())));
                }
                ok
            })
            .cloned()
            .collect();
        if found.is_empty() {
            bail!("none of the configured folders exists, balai config show to see them");
        }
        found
    } else {
        let home = dirs::home_dir().context("home folder not found")?;
        eprintln!("{}", dim(&format!("No folder configured, scanning {}. Add some with balai config add <folder>.", home.display())));
        vec![home]
    };
    // Un dossier donné deux fois, ou rangé dans un autre dossier analysé, ne compte qu'une fois.
    Ok(distinct(roots, |r| r))
}

/// Le scanner réglé par la configuration : règles jetables en plus, exclusions, noms ignorés.
fn scanner(cfg: &Config, depth: usize, top_files: usize, junk_on: bool) -> scan::Scanner {
    let mut rules = scan::builtin_rules();
    rules.extend(cfg.junk.iter().cloned());
    let mut exclude = cfg.exclude.clone();
    exclude.extend(scan::system_excludes());
    scan::Scanner::new(depth, top_files, if junk_on { rules } else { Vec::new() }, cfg.ignore_names.clone(), &exclude)
}

/// Parcourt tous les dossiers avec l'indicateur de progression.
fn walk(scanner: &scan::Scanner, roots: &[PathBuf]) -> Vec<scan::Node> {
    progress::scan(scanner, roots, || roots.par_iter().map(|r| scanner.scan(r)).collect())
}

fn scan(a: ScanArgs) -> Result<()> {
    let cfg = Config::load()?;
    let roots = roots(&a.paths, &cfg)?;
    let junk_on = !a.no_junk;
    let scanner = scanner(&cfg, a.depth, if a.no_files { 0 } else { a.top }, junk_on);

    let t = Instant::now();
    let nodes = walk(&scanner, &roots);
    let res = scanner.finish();
    out::print_scan(&nodes, &res, a.top, junk_on, t.elapsed());
    Ok(())
}

/// Les dossiers jetables qui passent les filtres, du plus lourd au plus léger.
fn find_junk(paths: &[PathBuf], days: u64, kinds: &[String]) -> Result<Vec<scan::Junk>> {
    let cfg = Config::load()?;
    let roots = roots(paths, &cfg)?;
    let scanner = scanner(&cfg, 0, 0, true);
    walk(&scanner, &roots);
    let now = SystemTime::now();
    Ok(scanner.finish().junk.into_iter().filter(|j| clean::keep(j, days, kinds, now)).collect())
}

/// Ce que filtrent `--days` et `--kind`, pour les titres : « inchangés depuis 30 jours ou plus, type node ».
fn filters(days: u64, kinds: &[String]) -> String {
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("untouched for {} or more", out::nb(days, "day", "days")));
    }
    if !kinds.is_empty() {
        parts.push(format!("type {}", kinds.join(", ")));
    }
    parts.join(", ")
}

/// Titre et liste complète, sans limite de lignes.
fn print_junk_list(title: &str, list: &[scan::Junk], days: u64, kinds: &[String]) {
    let total: u64 = list.iter().map(|j| j.size).sum();
    let mut detail = format!("{} in {}", size(total), out::nb(list.len() as u64, "folder", "folders"));
    let f = filters(days, kinds);
    if !f.is_empty() {
        detail.push_str(&format!(", {f}"));
    }
    println!("\n{}  {}", bold(title), dim(&detail));
    out::junk_rows(list);
    if !list.is_empty() {
        out::junk_summary(list);
    }
}

/// La commande tapée, sans `--dry-run` : ce qu'il faut relancer pour supprimer pour de bon.
fn command_without_dry_run() -> String {
    let args = std::env::args().skip(1).filter(|a| a != "--dry-run").map(|a| {
        if a.is_empty() || a.contains([' ', '\'', '"']) {
            format!("\"{a}\"")
        } else {
            a
        }
    });
    std::iter::once("balai".to_string()).chain(args).collect::<Vec<_>>().join(" ")
}

fn junk(a: JunkArgs) -> Result<()> {
    let t = Instant::now();
    let list = find_junk(&a.paths, a.days, &a.kinds)?;
    print_junk_list("Disposable folders", &list, a.days, &a.kinds);
    let mut tail = format!("Scanned in {}", out::duration(t.elapsed()));
    if !list.is_empty() {
        tail.push_str(" · balai clean to delete some, balai clean --help for options");
    }
    println!("\n{}", dim(&tail));
    Ok(())
}

fn clean(a: CleanArgs) -> Result<()> {
    let list = find_junk(&a.paths, a.days, &a.kinds)?;
    if list.is_empty() {
        let f = filters(a.days, &a.kinds);
        println!("No disposable folder to delete{}.", if f.is_empty() { String::new() } else { format!(" ({f})") });
        return Ok(());
    }
    print_junk_list("To delete", &list, a.days, &a.kinds);
    let total: u64 = list.iter().map(|j| j.size).sum();
    let what = if list.len() == 1 {
        format!("this folder ({})", size(total))
    } else {
        format!("these {} folders ({})", out::count(list.len() as u64), size(total))
    };

    if a.dry_run {
        println!("\n{} To delete {what}:", dim("Dry run, nothing was deleted."));
        println!("  {}", out::paint("32", &command_without_dry_run()));
        return Ok(());
    }
    if !a.yes && !confirm(&format!("\nDelete {what}? [y/N] "))? {
        println!("Nothing was deleted.");
        return Ok(());
    }

    let t = Instant::now();
    let done = AtomicU64::new(0);
    let freed = AtomicU64::new(0);
    let n = list.len();
    let failures: Vec<(&scan::Junk, std::io::Error)> = progress::ticker(
        |_| format!("Deleting… {}/{n} folders · {} freed", done.load(Relaxed), size(freed.load(Relaxed))),
        || {
            list.par_iter()
                .filter_map(|j| {
                    let r = clean::remove(&j.path);
                    done.fetch_add(1, Relaxed);
                    match r {
                        Ok(()) => {
                            freed.fetch_add(j.size, Relaxed);
                            None
                        }
                        Err(e) => Some((j, e)),
                    }
                })
                .collect()
        },
    );

    for (j, e) in &failures {
        eprintln!("{}", out::paint("31", &format!("failed {}: {e}", j.path.display())));
    }
    let removed = (n - failures.len()) as u64;
    let mut msg = format!("{} freed, {} deleted", size(freed.into_inner()), out::nb(removed, "folder", "folders"));
    if !failures.is_empty() {
        msg.push_str(&format!(", {} failed (file in use? permissions?)", failures.len()));
    }
    println!("\n{}  {}", bold(&msg), dim(&format!("in {}", out::duration(t.elapsed()))));
    if !failures.is_empty() {
        bail!("{}", out::nb(failures.len() as u64, "folder not deleted", "folders not deleted"));
    }
    Ok(())
}

/// Pose la question sur stderr et lit la réponse. Seuls « o », « oui », « y », « yes » valent oui ;
/// une entrée fermée (rien à lire) vaut non.
fn confirm(question: &str) -> Result<bool> {
    eprint!("{question}");
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).context("reading the answer")?;
    // PowerShell met une marque d'ordre des octets devant ce qu'on lui fait passer par un tube.
    let answer = answer.trim().trim_start_matches('\u{feff}').to_lowercase();
    Ok(matches!(answer.as_str(), "o" | "oui" | "y" | "yes"))
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
    println!("\n{}  {}", bold("Caches"), dim(&format!("{} in total", size(total))));
    for (c, s) in &sized {
        println!("  {}  {:<20} {}", size_col(*s), c.name, dim(&c.path.display().to_string()));
        if let Some(cmd) = &c.clean {
            println!("  {:>9}  {:<20} {}", "", "", out::paint("32", &format!("→ {cmd}")));
        }
    }
    println!("\n{}", dim(&format!("Scanned in {}", out::duration(t.elapsed()))));
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
                    println!("{} already there", p.display());
                } else {
                    println!("+ {}", p.display());
                    cfg.roots.push(p);
                }
            }
        }
        ConfigCmd::Exclude { paths } => {
            for p in absolute_all(&paths)? {
                if !cfg.exclude.iter().any(|r| same(r, &p)) {
                    println!("excluded {}", p.display());
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
                bail!("{} is not in the configuration", p.display());
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
    println!("{}", dim(&format!("saved to {}", saved.display())));
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
