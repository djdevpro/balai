# balai

balai trouve ce qui remplit ton disque. En une commande, il affiche les dossiers les plus lourds, les plus gros fichiers, les dossiers régénérables comme `node_modules` ou `target`, et la taille des caches de tes gestionnaires de paquets. Il tourne sous Windows, macOS et Linux.

balai ne supprime rien. Il te montre quoi trier, et tu décides.

Sur mon PC Windows, il parcourt 6,2 millions de fichiers (448 Go) en 13 à 25 secondes quand le système a déjà les dossiers en cache, et en 41 secondes au premier passage.

## Installer

Télécharge le fichier de ton système sur la [page des versions](https://github.com/djdevpro/balai/releases), puis place-le dans un dossier de ton `PATH`.

Sous Linux et macOS, le fichier téléchargé doit devenir exécutable. macOS bloque aussi les binaires non signés venus d'un navigateur, et la deuxième commande lève ce blocage.

```bash
chmod +x balai-macos-universal
xattr -d com.apple.quarantine balai-macos-universal
```

Avec Rust installé, tu peux aussi le compiler toi-même.

```bash
cargo install --git https://github.com/djdevpro/balai
```

## Utiliser

```bash
balai                    # analyse les dossiers de la configuration
balai D:\ -n 30 -d 2     # 30 lignes par section, arbre sur 2 niveaux
balai . --no-files       # sans la liste des gros fichiers
balai caches             # caches des gestionnaires de paquets
```

Sans dossier en argument ni dans la configuration, balai analyse ton dossier personnel. Si un de tes dossiers porte le nom d'une commande (`c`, `s`, `caches`, `config` ou `scan`), écris `balai scan c` pour l'analyser.

Le résultat tient en trois blocs. L'arbre donne le poids de chaque sous-dossier et sa part du total. La liste des gros fichiers donne leur chemin complet. Les dossiers jetables arrivent classés par taille, avec la date de leur dernière modification pour repérer les projets abandonnés.

## Les dossiers jetables

balai reconnaît les dossiers qu'un outil recrée tout seul. `target` compte seulement à côté d'un `Cargo.toml`, `vendor` à côté d'un `composer.json`, et `venv` s'il contient un `pyvenv.cfg`.

| Dossiers | Outil |
|---|---|
| `node_modules`, `.next`, `.nuxt`, `.svelte-kit`, `.turbo`, `.parcel-cache`, `.angular` | JavaScript |
| `target` | Rust |
| `.venv`, `venv`, `__pycache__`, `.pytest_cache`, `.mypy_cache`, `.ruff_cache` | Python |
| `vendor` | PHP |
| `.gradle`, `.dart_tool`, `Pods` | Gradle, Dart, CocoaPods |

Un `node_modules` rangé dans un autre `node_modules` compte une seule fois, dans le dossier du haut.

## Les caches

`balai caches` mesure les caches de npm, pnpm, Yarn, Bun, Deno, Cargo, rustup, pip, uv, Hugging Face, Ollama, NuGet, Gradle, Maven, Go, Composer, Playwright et Electron, avec le dossier Temp sous Windows et Homebrew sous macOS. Pour chacun, il affiche la commande qui le vide proprement quand elle existe, par exemple `pnpm store prune` ou `uv cache clean`.

## Configurer

La configuration tient dans un fichier TOML. balai le range dans `%APPDATA%\balai\config.toml` sous Windows, `~/Library/Application Support/balai/config.toml` sous macOS et `~/.config/balai/config.toml` sous Linux. `balai config show` affiche son chemin et son contenu.

```bash
balai config add "C:\Users\moi\Documents"          # dossier analysé par défaut
balai config exclude "C:\Users\moi\Documents\Vidéos"
balai config cache "Volta" "C:\Users\moi\AppData\Local\Volta" --clean "volta list"
balai config remove "C:\Users\moi\Documents\Vidéos"
```

Pour reconnaître un autre dossier jetable, ajoute une règle à la main dans le fichier. `sibling` et `child` sont facultatifs.

```toml
[[junk]]
name = "dist"
label = "Build (dist)"
sibling = "package.json"
```

## Limites

balai additionne la taille apparente des fichiers. Un fichier lié en dur compte à chaque endroit où il apparaît. C'est le cas des `node_modules` installés par pnpm, liés à son store. Les supprimer libère peu de place tant que le store garde les fichiers, et `pnpm store prune` fait le reste. Les fichiers OneDrive disponibles seulement en ligne comptent aussi pour leur taille complète.

balai ne suit pas les liens symboliques ni les jonctions Windows, pour ne pas compter deux fois le même dossier. Il saute `/proc`, `/sys`, `/dev` et `/run` sous Linux, et `/System/Volumes` sous macOS.

## Publier une version

Dans l'onglet Actions, lance le workflow Release et choisis patch, minor ou major. Il lance les tests sur les trois systèmes, compile balai pour Windows, macOS et Linux, puis publie les binaires. Le fichier macOS contient les puces Apple et Intel. Les notes de version reprennent les messages des commits depuis la dernière version, sauf si tu écris les tiennes. L'essai à blanc fait tout sauf la publication.

## Licence

balai est disponible sous [licence MIT](LICENSE-MIT) ou [Apache 2.0](LICENSE-APACHE), au choix.
