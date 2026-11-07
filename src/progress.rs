//! L'indicateur de progression : une ligne sur stderr, réécrite dix fois par seconde,
//! effacée à la fin. Rien quand stderr n'est pas un terminal (sortie redirigée, CI).
//!
//! Le pourcentage d'une analyse vient du nombre de fichiers trouvés la dernière fois
//! dans les mêmes dossiers, gardé dans `history.toml` à côté de la configuration.

use crate::config::key;
use crate::out::{count, duration, size};
use crate::scan::Scanner;
use std::collections::BTreeMap;
use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::time::{Duration, Instant};

const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
/// Une analyse plus courte que ça ne montre rien, pas de clignotement.
const QUIET: Duration = Duration::from_millis(300);

/// Fait `work` en affichant `line(écoulé)` jusqu'à ce qu'il ait fini.
pub fn ticker<T>(line: impl Fn(Duration) -> String + Sync, work: impl FnOnce() -> T) -> T {
    if !std::io::stderr().is_terminal() {
        return work();
    }
    let done = AtomicBool::new(false);
    let t0 = Instant::now();
    std::thread::scope(|s| {
        let h = s.spawn(|| {
            let mut err = std::io::stderr();
            let mut width = 0;
            let mut i = 0;
            loop {
                std::thread::park_timeout(Duration::from_millis(100));
                if done.load(Relaxed) {
                    break;
                }
                if t0.elapsed() < QUIET {
                    continue;
                }
                let text = format!("{} {}", SPINNER[i % SPINNER.len()], line(t0.elapsed()));
                i += 1;
                // Des espaces recouvrent la ligne précédente si elle était plus longue.
                let _ = write!(err, "\r{text:<width$}");
                let _ = err.flush();
                width = text.chars().count();
            }
            if width > 0 {
                let _ = write!(err, "\r{:width$}\r", "");
                let _ = err.flush();
            }
        });
        let r = work();
        done.store(true, Relaxed);
        h.thread().unpark();
        r
    })
}

/// Analyse `roots` avec `walk`, en montrant fichiers parcourus, volume et pourcentage estimé.
pub fn scan<T>(scanner: &Scanner, roots: &[PathBuf], walk: impl FnOnce() -> T) -> T {
    let id = roots_id(roots);
    let mut hist = History::load();
    let expected = hist.0.get(&id).copied().filter(|&n| n > 0);
    let r = ticker(|elapsed| scan_line(scanner.progress(), expected, elapsed), walk);
    hist.0.insert(id, scanner.progress().0);
    hist.save();
    r
}

fn scan_line((files, bytes): (u64, u64), expected: Option<u64>, elapsed: Duration) -> String {
    let mut s = format!("Analyse… {} fichiers · {}", count(files), size(bytes));
    if let Some(total) = expected {
        // Jamais 100 % avant la fin : le disque a pu se remplir depuis la dernière fois.
        let pct = (files.saturating_mul(100) / total).min(99);
        s.push_str(&format!(" · {pct} %"));
        if (5..99).contains(&pct) {
            let left = elapsed.mul_f64((100 - pct) as f64 / pct as f64);
            s.push_str(&format!(" · encore ~{}", duration_round(left)));
        }
    }
    s.push_str(&format!(" · {}", duration_round(elapsed)));
    s
}

/// Une durée à la seconde près, plus lisible qu'au centième sur une ligne qui bouge.
fn duration_round(d: Duration) -> String {
    if d.as_secs() < 1 {
        duration(d)
    } else {
        format!("{} s", d.as_secs())
    }
}

/// Une clé par ensemble de dossiers analysés, dans n'importe quel ordre.
fn roots_id(roots: &[PathBuf]) -> String {
    let mut keys: Vec<String> = roots.iter().map(|r| key(r)).collect();
    keys.sort();
    keys.join(" | ")
}

/// Nombre de fichiers trouvés à la dernière analyse, par ensemble de dossiers.
#[derive(Default)]
struct History(BTreeMap<String, u64>);

impl History {
    fn path() -> Option<PathBuf> {
        Some(crate::config::path().ok()?.with_file_name("history.toml"))
    }

    fn load() -> Self {
        let read = |p: &Path| toml::from_str(&fs::read_to_string(p).ok()?).ok();
        Self(Self::path().and_then(|p| read(&p)).unwrap_or_default())
    }

    /// Sans erreur : l'historique ne sert qu'au pourcentage.
    fn save(&self) {
        let Some(p) = Self::path() else { return };
        let Ok(text) = toml::to_string(&self.0) else { return };
        let _ = fs::create_dir_all(p.parent().unwrap());
        let _ = fs::write(p, text);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ligne_d_analyse() {
        let s = Duration::from_secs;
        assert_eq!(scan_line((1_234_567, 3 << 30), None, s(4)), "Analyse… 1 234 567 fichiers · 3,0 Go · 4 s");
        // La moitié en 10 s, il en reste à peu près autant.
        assert_eq!(scan_line((500, 0), Some(1000), s(10)), "Analyse… 500 fichiers · 0 o · 50 % · encore ~10 s · 10 s");
        // Plus de fichiers que la dernière fois : 99 %, sans temps restant.
        assert_eq!(scan_line((1500, 0), Some(1000), s(20)), "Analyse… 1 500 fichiers · 0 o · 99 % · 20 s");
        // Trop tôt pour estimer le temps restant.
        assert_eq!(scan_line((10, 0), Some(1000), s(1)), "Analyse… 10 fichiers · 0 o · 1 % · 1 s");
    }

    #[test]
    fn meme_cle_dans_n_importe_quel_ordre() {
        let a = PathBuf::from("/a");
        let b = PathBuf::from("/b");
        assert_eq!(roots_id(&[a.clone(), b.clone()]), roots_id(&[b, a]));
    }
}
