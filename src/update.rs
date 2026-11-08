//! Mise à jour depuis les versions publiées sur GitHub.

use anstream::println;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::time::Duration;

const REPO: &str = "djdevpro/balai";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// Le fichier publié par le workflow Release pour ce système.
fn asset_name() -> Option<&'static str> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some("balai-windows-x64.exe")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("balai-linux-x64")
    } else if cfg!(target_os = "macos") {
        Some("balai-macos-universal")
    } else {
        None
    }
}

/// `v1.2.3` ou `1.2.3-rc.1` en nombres comparables.
fn parse(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.trim_start_matches('v').split(['.', '-', '+']).map(|p| p.parse().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

/// La somme attendue pour `name` dans un fichier au format de `sha256sum`.
fn expected_sum(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        (file.trim_start_matches([' ', '*']) == name).then(|| hash.to_lowercase())
    })
}

fn download(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    agent.get(url).call()?.into_reader().take(200 << 20).read_to_end(&mut buf)?;
    Ok(buf)
}

pub fn run(check_only: bool) -> Result<()> {
    let current = crate::VERSION;
    let agent = ureq::AgentBuilder::new().user_agent(&format!("balai/{current}")).timeout(Duration::from_secs(60)).build();
    let release: Release = agent
        .get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .set("Accept", "application/vnd.github+json")
        .call()
        .context("GitHub is not responding")?
        .into_json()?;

    let latest = release.tag_name.trim_start_matches('v');
    let (Some(new), Some(cur)) = (parse(latest), parse(current)) else {
        bail!("unreadable version ({} or {current})", release.tag_name);
    };
    if new <= cur {
        println!("balai {current} is already the latest version.");
        return Ok(());
    }
    println!("balai {latest} is out, you have {current}.");
    if check_only {
        println!("balai update to install it.");
        return Ok(());
    }

    let name = asset_name().context("no published binary for this system, cargo install --git https://github.com/djdevpro/balai")?;
    let url = |n: &str| release.assets.iter().find(|a| a.name == n).map(|a| a.browser_download_url.as_str());
    let bin_url = url(name).with_context(|| format!("{name} is missing from version {latest}"))?;
    let sums_url = url("SHA256SUMS.txt").with_context(|| format!("SHA256SUMS.txt is missing from version {latest}"))?;

    let bin = download(&agent, bin_url)?;
    let sums = String::from_utf8(download(&agent, sums_url)?)?;
    let expected = expected_sum(&sums, name).with_context(|| format!("no checksum for {name}"))?;
    let got: String = Sha256::digest(&bin).iter().map(|b| format!("{b:02x}")).collect();
    if got != expected {
        bail!("the downloaded file does not match its checksum, nothing changed");
    }

    // Sous Windows, l'exe en cours d'exécution est verrouillé. self_replace le met de côté,
    // installe le nouveau à sa place et supprime l'ancien à la sortie.
    let tmp = std::env::temp_dir().join(format!("balai-{latest}-{}", std::process::id()));
    std::fs::write(&tmp, &bin)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    let replaced = self_replace::self_replace(&tmp);
    let _ = std::fs::remove_file(&tmp);
    replaced.context("could not replace balai, check the permissions on its folder")?;
    println!("balai is now at version {latest}.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_et_sommes() {
        assert!(parse("v0.10.0") > parse("0.9.3"), "comparaison en nombres, pas en texte");
        assert_eq!(parse("v1.2.3-rc.1"), Some((1, 2, 3)));
        assert_eq!(parse("v1.2"), None);

        let sums = "aaa111  balai-linux-x64\nBBB222 *balai-windows-x64.exe\n";
        assert_eq!(expected_sum(sums, "balai-windows-x64.exe").as_deref(), Some("bbb222"));
        assert_eq!(expected_sum(sums, "balai-linux-x64").as_deref(), Some("aaa111"));
        assert_eq!(expected_sum(sums, "balai-macos-universal"), None);
    }
}
