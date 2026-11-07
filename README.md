<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/balai-dark.svg">
    <img src="assets/balai.svg" alt="balai" width="220">
  </picture>
</p>

<p align="center"><b>Ton disque est plein. balai te dit pourquoi.</b></p>

6,2 millions de fichiers (448 Go) analysés en 15 secondes. balai sort les dossiers les plus lourds, les plus gros fichiers, les `node_modules` et `target` oubliés, et le poids des caches npm, pnpm, Cargo ou pip. Une commande, sous Windows, macOS et Linux. Rien n'est supprimé sans ton accord : `balai clean` montre la liste et demande confirmation.

## Installer

Prends le binaire de ton système dans la [dernière version](https://github.com/djdevpro/balai/releases/latest), ou compile-le. Ensuite, `balai update` le met à jour tout seul.

```bash
cargo install --git https://github.com/djdevpro/balai
```

## Utiliser

```bash
balai                    # les dossiers de ta config, sinon ton dossier perso
balai . -n 30 -d 2       # 30 lignes par section, arbre sur 2 niveaux
balai junk               # la liste complète des node_modules, target, .venv…
balai clean --days 30    # les montre, demande confirmation, puis les supprime
balai clean -k node --dry-run   # ce qui partirait, sans rien toucher
balai caches             # le poids des caches npm, pnpm, Cargo, pip…
balai config add <dossier>
balai update             # passe à la dernière version
```

## Licence

[MIT](LICENSE-MIT) ou [Apache 2.0](LICENSE-APACHE), au choix.
