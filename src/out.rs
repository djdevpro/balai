//! Mise en forme du terminal. anstream retire les couleurs si la sortie n'est
//! pas un terminal ou si NO_COLOR est défini.

use crate::scan::{Junk, Node, Results};
use anstream::println;
use std::time::{Duration, SystemTime};

const GIB: u64 = 1 << 30;
const MIB: u64 = 1 << 20;

pub fn paint(code: &str, s: &str) -> String {
    format!("\x1b[{code}m{s}\x1b[0m")
}
pub fn bold(s: &str) -> String {
    paint("1", s)
}
pub fn dim(s: &str) -> String {
    paint("2", s)
}

pub fn size(b: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

/// Nombre suivi du mot accordé. En français, 0 et 1 restent au singulier.
pub fn nb(n: u64, one: &str, many: &str) -> String {
    format!("{} {}", count(n), if n == 1 { one } else { many })
}

/// Couleur selon le poids.
fn weight(b: u64) -> &'static str {
    match b {
        b if b >= 10 * GIB => "1;31",
        b if b >= GIB => "33",
        b if b >= 100 * MIB => "36",
        _ => "2",
    }
}

/// Taille alignée à droite pour les colonnes.
pub fn size_col(b: u64) -> String {
    paint(weight(b), &format!("{:>9}", size(b)))
}

fn bar(ratio: f64) -> String {
    const W: usize = 12;
    let n = ((ratio * W as f64).round() as usize).min(W);
    format!("{}{}", paint("36", &"█".repeat(n)), dim(&"░".repeat(W - n)))
}

pub fn count(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn age(t: Option<SystemTime>) -> String {
    let Some(days) = t.and_then(|t| t.elapsed().ok()).map(|d| d.as_secs() / 86_400) else {
        return "?".into();
    };
    match days {
        0 => "today".into(),
        1 => "1 day ago".into(),
        2..=59 => format!("{days} days ago"),
        60..=729 => format!("{} months ago", days / 30),
        _ => format!("{} years ago", days / 365),
    }
}

pub fn duration(d: Duration) -> String {
    if d.as_millis() < 1000 {
        format!("{} ms", d.as_millis())
    } else {
        format!("{:.2} s", d.as_secs_f64())
    }
}

fn name(n: &Node) -> String {
    n.path.file_name().map_or_else(|| n.path.display().to_string(), |s| s.to_string_lossy().into_owned())
}

pub fn print_scan(nodes: &[Node], res: &Results, top: usize, junk_on: bool, elapsed: Duration) {
    for n in nodes {
        println!();
        println!("{}  {}  {}", bold(&n.path.display().to_string()), paint(weight(n.size), &size(n.size)), dim(&nb(n.files, "file", "files")));
        print_children(n, 1, top);
    }

    if !res.files.is_empty() {
        println!("\n{}", bold("Biggest files"));
        for (s, p) in &res.files {
            println!("  {}  {}", size_col(*s), p.display());
        }
    }

    if junk_on {
        let total: u64 = res.junk.iter().map(|j| j.size).sum();
        let n = res.junk.len() as u64;
        let detail = if n == 0 {
            "none".to_string()
        } else {
            format!("{} in {}, rebuilt by npm install, cargo build…", size(total), nb(n, "folder", "folders"))
        };
        println!("\n{}  {}", bold("Disposable folders"), dim(&detail));
        junk_rows(res.junk.iter().take(top));
        if res.junk.len() > top {
            let rest: u64 = res.junk[top..].iter().map(|j| j.size).sum();
            let others = nb((res.junk.len() - top) as u64, "other", "others");
            println!("  {}", dim(&format!("… and {others} ({}), balai junk to see them all", size(rest))));
        }
    }

    let files: u64 = nodes.iter().map(|n| n.files).sum();
    let mut stats = format!("Scanned in {} · {}", duration(elapsed), nb(files, "file", "files"));
    if res.errors > 0 {
        stats.push_str(&format!(" · {}", nb(res.errors, "unreadable item", "unreadable items")));
    }
    println!("\n{}", dim(&stats));
}

/// Une ligne par dossier jetable : taille, type, âge, chemin.
pub fn junk_rows<'a>(list: impl IntoIterator<Item = &'a Junk>) {
    for j in list {
        println!("  {}  {:<20} {}  {}", size_col(j.size), j.label, dim(&format!("{:>14}", age(j.modified))), j.path.display());
    }
}

/// Un type de dossier jetable et ce qu'il pèse au total.
#[derive(Debug, PartialEq)]
pub struct Group<'a> {
    pub label: &'a str,
    pub count: u64,
    pub size: u64,
}

/// Regroupe par type, du plus lourd au plus léger (à poids égal, par nom).
pub fn group_junk(list: &[Junk]) -> Vec<Group<'_>> {
    let mut groups: Vec<Group> = Vec::new();
    for j in list {
        match groups.iter_mut().find(|g| g.label == j.label) {
            Some(g) => {
                g.count += 1;
                g.size += j.size;
            }
            None => groups.push(Group { label: &j.label, count: 1, size: j.size }),
        }
    }
    groups.sort_by(|a, b| b.size.cmp(&a.size).then(a.label.cmp(b.label)));
    groups
}

const FINE_W: usize = 20;

/// Barre au huitième de caractère près. Un poids non nul garde au moins un trait,
/// pour ne pas le confondre avec rien.
fn fine_bar(ratio: f64) -> String {
    const PARTS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    let eighths = ((ratio.clamp(0.0, 1.0) * (FINE_W * 8) as f64).round() as usize).max(usize::from(ratio > 0.0));
    let (full, part) = (eighths / 8, eighths % 8);
    let body = format!("{}{}", "█".repeat(full), PARTS[part]);
    let used = full + usize::from(part > 0);
    format!("{}{}", paint("36", &body), " ".repeat(FINE_W - used))
}

/// Part du total, avec « <1 % » plutôt qu'un 0 % trompeur.
fn share(ratio: f64) -> String {
    if ratio > 0.0 && ratio < 0.005 {
        "<1%".into()
    } else {
        format!("{:.0}%", ratio * 100.0)
    }
}

/// Le tableau récapitulatif : une ligne par type avec nombre, poids, barre et part du total,
/// puis le total. La barre compare chaque type au plus lourd, pour que les petits restent visibles.
pub fn junk_summary(list: &[Junk]) {
    let groups = group_junk(list);
    let total: u64 = groups.iter().map(|g| g.size).sum();
    let biggest = groups.first().map_or(0, |g| g.size);
    let n: u64 = groups.iter().map(|g| g.count).sum();
    let lw = groups.iter().map(|g| g.label.chars().count()).max().unwrap_or(0).max("Total".len()).max("Type".len());
    let cw = count_width(n);
    let ratio = |s: u64, of: u64| if of == 0 { 0.0 } else { s as f64 / of as f64 };

    println!("\n{}", bold("Summary by type"));
    println!("  {}", dim(&format!("{:<lw$}  {:>cw$}  {:>9}  {:<FINE_W$}  {:>5}", "Type", "Folders", "Size", "", "Share")));
    for g in &groups {
        println!(
            "  {:<lw$}  {:>cw$}  {}  {}  {}",
            g.label,
            count(g.count),
            size_col(g.size),
            fine_bar(ratio(g.size, biggest)),
            dim(&format!("{:>5}", share(ratio(g.size, total))))
        );
    }
    let width = lw + 2 + cw + 2 + 9 + 2 + FINE_W + 2 + 5;
    println!("  {}", dim(&"─".repeat(width)));
    println!("  {}  {}  {}", bold(&format!("{:<lw$}", "Total")), bold(&format!("{:>cw$}", count(n))), bold(&format!("{:>9}", size(total))));
}

/// Largeur de la colonne des nombres : le plus grand nombre affiché, ou le titre.
fn count_width(max: u64) -> usize {
    count(max).chars().count().max("Folders".len())
}

fn print_children(n: &Node, indent: usize, top: usize) {
    let mut rows: Vec<(String, u64, Option<&Node>)> = n.children.iter().map(|c| (name(c), c.size, Some(c))).collect();
    if n.own > 0 && !rows.is_empty() {
        rows.push((dim("(files at the top)"), n.own, None));
        rows.sort_by(|a, b| b.1.cmp(&a.1));
    }
    let pad = "  ".repeat(indent);
    for (label, s, child) in rows.iter().take(top) {
        let ratio = if n.size == 0 { 0.0 } else { *s as f64 / n.size as f64 };
        println!("{pad}{} {} {}  {label}", size_col(*s), bar(ratio), dim(&format!("{:>3.0}%", ratio * 100.0)));
        if let Some(c) = child {
            if !c.children.is_empty() {
                print_children(c, indent + 1, top);
            }
        }
    }
    if rows.len() > top {
        let rest: u64 = rows[top..].iter().map(|r| r.1).sum();
        println!("{pad}{}", dim(&format!("… and {} ({})", nb((rows.len() - top) as u64, "other", "others"), size(rest))));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn barre_fine_et_part() {
        let plain = |r: f64| anstream::adapter::strip_str(&fine_bar(r)).to_string();
        assert_eq!(plain(1.0), "█".repeat(FINE_W));
        assert_eq!(plain(0.0), " ".repeat(FINE_W));
        assert!(plain(1e-9).starts_with('▏'), "un poids minuscule reste visible");
        assert!(plain(0.33).chars().count() == FINE_W, "largeur constante");
        assert_eq!((share(0.0), share(0.001), share(0.71)), ("0%".into(), "<1%".into(), "71%".into()));
    }

    #[test]
    fn regroupe_par_type() {
        let j = |label: &str, size| Junk { path: PathBuf::from("x"), label: label.into(), size, modified: None };
        let list = [j("Node (node_modules)", 10), j("Rust (target)", 50), j("Node (node_modules)", 30), j("Python (__pycache__)", 1)];
        assert_eq!(
            group_junk(&list),
            [
                Group { label: "Rust (target)", count: 1, size: 50 },
                Group { label: "Node (node_modules)", count: 2, size: 40 },
                Group { label: "Python (__pycache__)", count: 1, size: 1 },
            ]
        );
    }
}
