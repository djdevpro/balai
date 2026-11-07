//! Mise en forme du terminal. anstream retire les couleurs si la sortie n'est
//! pas un terminal ou si NO_COLOR est défini.

use crate::scan::{Node, Results};
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
    const UNITS: [&str; 5] = ["o", "Ko", "Mo", "Go", "To"];
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{b} o")
    } else {
        format!("{v:.1} {}", UNITS[i]).replace('.', ",")
    }
}

/// Nombre suivi du mot accordé. En français, 0 et 1 restent au singulier.
pub fn nb(n: u64, one: &str, many: &str) -> String {
    format!("{} {}", count(n), if n >= 2 { many } else { one })
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
            out.push(' ');
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
        0 => "aujourd'hui".into(),
        1..=59 => format!("il y a {days} j"),
        60..=729 => format!("il y a {} mois", days / 30),
        _ => format!("il y a {} ans", days / 365),
    }
}

pub fn duration(d: Duration) -> String {
    if d.as_millis() < 1000 {
        format!("{} ms", d.as_millis())
    } else {
        format!("{:.2} s", d.as_secs_f64()).replace('.', ",")
    }
}

fn name(n: &Node) -> String {
    n.path.file_name().map_or_else(|| n.path.display().to_string(), |s| s.to_string_lossy().into_owned())
}

pub fn print_scan(nodes: &[Node], res: &Results, top: usize, junk_on: bool, elapsed: Duration) {
    for n in nodes {
        println!();
        println!("{}  {}  {}", bold(&n.path.display().to_string()), paint(weight(n.size), &size(n.size)), dim(&nb(n.files, "fichier", "fichiers")));
        print_children(n, 1, top);
    }

    if !res.files.is_empty() {
        println!("\n{}", bold("Plus gros fichiers"));
        for (s, p) in &res.files {
            println!("  {}  {}", size_col(*s), p.display());
        }
    }

    if junk_on {
        let total: u64 = res.junk.iter().map(|j| j.size).sum();
        let n = res.junk.len() as u64;
        let detail = if n == 0 {
            "aucun".to_string()
        } else {
            let recree = if n >= 2 { "recréés" } else { "recréé" };
            format!("{} dans {}, {recree} par npm install, cargo build…", size(total), nb(n, "dossier", "dossiers"))
        };
        println!("\n{}  {}", bold("Dossiers jetables"), dim(&detail));
        for j in res.junk.iter().take(top) {
            println!("  {}  {:<20} {}  {}", size_col(j.size), j.label, dim(&format!("{:>14}", age(j.modified))), j.path.display());
        }
        if res.junk.len() > top {
            let rest: u64 = res.junk[top..].iter().map(|j| j.size).sum();
            let others = nb((res.junk.len() - top) as u64, "autre", "autres");
            println!("  {}", dim(&format!("… et {others} ({}), -n {} pour tout voir", size(rest), res.junk.len())));
        }
    }

    let files: u64 = nodes.iter().map(|n| n.files).sum();
    let mut stats = format!("Analysé en {} · {}", duration(elapsed), nb(files, "fichier", "fichiers"));
    if res.errors > 0 {
        stats.push_str(&format!(" · {}", nb(res.errors, "élément illisible", "éléments illisibles")));
    }
    println!("\n{}", dim(&stats));
}

fn print_children(n: &Node, indent: usize, top: usize) {
    let mut rows: Vec<(String, u64, Option<&Node>)> = n.children.iter().map(|c| (name(c), c.size, Some(c))).collect();
    if n.own > 0 && !rows.is_empty() {
        rows.push((dim("(fichiers à la racine)"), n.own, None));
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
        println!("{pad}{}", dim(&format!("… et {} ({})", nb((rows.len() - top) as u64, "autre", "autres"), size(rest))));
    }
}
