//! Le tri et la suppression des dossiers jetables.

use crate::scan::Junk;
use std::fs;
use std::io;
use std::path::Path;
use std::time::SystemTime;

/// Âge en jours depuis la dernière modification, `None` si le système ne la donne pas.
pub fn age_days(j: &Junk, now: SystemTime) -> Option<u64> {
    j.modified.map(|m| now.duration_since(m).map_or(0, |d| d.as_secs() / 86_400))
}

/// Le dossier passe-t-il les filtres ? `kinds` vide laisse passer tous les types ; un type
/// se compare au libellé sans tenir compte de la casse (`node`, `target`, `python`…).
/// Avec un âge minimum, un dossier d'âge inconnu ne passe pas : dans le doute, on garde.
pub fn keep(j: &Junk, days: u64, kinds: &[String], now: SystemTime) -> bool {
    let label = j.label.to_lowercase();
    let kind_ok = kinds.is_empty() || kinds.iter().any(|k| label.contains(&k.to_lowercase()));
    let age_ok = days == 0 || age_days(j, now).is_some_and(|d| d >= days);
    kind_ok && age_ok
}

/// Supprime un dossier et tout ce qu'il contient. Sous Windows, un fichier en lecture
/// seule (fréquent dans node_modules et .git) bloque la suppression : on retire
/// l'attribut et on recommence.
pub fn remove(p: &Path) -> io::Result<()> {
    match fs::remove_dir_all(p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(first) => {
            if !make_writable(p) {
                return Err(first);
            }
            fs::remove_dir_all(p)
        }
    }
}

/// Retire la lecture seule partout sous `p`. Vrai si au moins un fichier a changé.
fn make_writable(p: &Path) -> bool {
    let mut changed = false;
    let Ok(rd) = fs::read_dir(p) else { return false };
    for e in rd.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            changed |= make_writable(&e.path());
        } else if let Ok(m) = e.metadata() {
            let mut perm = m.permissions();
            if perm.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                perm.set_readonly(false);
                changed |= fs::set_permissions(e.path(), perm).is_ok();
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::Duration;

    fn junk(label: &str, days_old: Option<u64>, now: SystemTime) -> Junk {
        Junk {
            path: PathBuf::from("x"),
            label: label.into(),
            size: 1,
            modified: days_old.map(|d| now - Duration::from_secs(d * 86_400 + 60)),
        }
    }

    #[test]
    fn filtres_age_et_type() {
        let now = SystemTime::now();
        let vieux = junk("Rust (target)", Some(45), now);
        let recent = junk("Node (node_modules)", Some(3), now);
        let inconnu = junk("Python (.venv)", None, now);
        let none: &[String] = &[];

        assert!([&vieux, &recent, &inconnu].iter().all(|j| keep(j, 0, none, now)), "0 jour, tout passe");
        assert!(keep(&vieux, 30, none, now));
        assert!(!keep(&recent, 30, none, now));
        assert!(!keep(&inconnu, 30, none, now), "âge inconnu, on ne supprime pas");
        assert!(keep(&vieux, 45, none, now), "45 jours pile, ça compte");

        let rust = &["TARGET".to_string()];
        assert!(keep(&vieux, 0, rust, now));
        assert!(!keep(&recent, 0, rust, now));
        let deux = &["node".to_string(), "python".to_string()];
        assert!(keep(&recent, 0, deux, now) && keep(&inconnu, 0, deux, now) && !keep(&vieux, 0, deux, now));
    }

    #[test]
    fn supprime_meme_en_lecture_seule() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path().join("node_modules");
        let f = d.join("pkg").join(".git").join("objects").join("ab");
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(&f, b"objet git").unwrap();
        let mut perm = fs::metadata(&f).unwrap().permissions();
        perm.set_readonly(true);
        fs::set_permissions(&f, perm).unwrap();

        remove(&d).unwrap();
        assert!(!d.exists());
        assert!(tmp.path().exists(), "le parent reste");
        // Déjà parti, pas une erreur.
        remove(&d).unwrap();
    }
}
