<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/balai-dark.svg">
    <img src="assets/balai.svg" alt="balai" width="220">
  </picture>
</p>

<p align="center"><b>Your disk is full. balai tells you why, then cleans it up.</b></p>

6.2 million files (448 GB) scanned in 15 seconds. balai finds your heaviest folders, your biggest files, the forgotten `node_modules`, `target` and `.venv` folders, and how much your npm, pnpm, Cargo and pip caches weigh. Then it deletes them for you, after showing you exactly what goes. One command, on Windows, macOS and Linux.

## See what you'll get back

`balai clean --dry-run` lists every folder that would be deleted, then sums it up by type:

```text
$ balai clean --days 30 --dry-run

To delete  108.6 GB in 21,919 folders, untouched for 30 days or more
     5.4 GB  Python (.venv)         5 months ago  C:\Users\me\projects\ai-draw\.venv
     4.1 GB  Next.js (.next)        2 months ago  C:\Users\me\projects\shop\.next
     3.8 GB  Node (node_modules)     36 days ago  C:\Users\me\projects\website\node_modules
     …

Summary by type
  Type                  Folders       Size                        Share
  Node (node_modules)       674    76.9 GB  ████████████████████    71%
  Python (.venv)              7    16.2 GB  ████▎                   15%
  Next.js (.next)            28    11.5 GB  ███                     11%
  Python (__pycache__)   21,160     2.2 GB  ▋                        2%
  Python (venv)               4   884.4 MB  ▎                        1%
  PHP (vendor)               14   833.5 MB  ▎                        1%
  Gradle                      8    56.0 MB  ▏                       <1%
  Turborepo                  15    11.8 MB  ▏                       <1%
  ─────────────────────────────────────────────────────────────────────
  Total                  21,919   108.6 GB

Dry run, nothing was deleted. To delete these 21,919 folders (108.6 GB):
  balai clean --days 30
```

Without `--dry-run`, balai shows the same list, then asks `Delete these 21,919 folders (108.6 GB)? [y/N]`. Nothing goes without your *y*.

Everything it deletes rebuilds itself: `npm install`, `cargo build`, `pip install`, `next build`… A `target` folder only counts when a `Cargo.toml` sits next to it, a `vendor` folder only next to a `composer.json`, a `venv` only when it holds a `pyvenv.cfg`.

## Install

Grab the binary for your system from the [latest release](https://github.com/djdevpro/balai/releases/latest), or build it. After that, `balai update` keeps it up to date.

```bash
cargo install --git https://github.com/djdevpro/balai
```

## Use

```bash
balai                           # what takes space: folders, big files, disposable folders
balai . -n 30 -d 2              # 30 rows per section, folder tree 2 levels deep
balai junk                      # the full list of disposable folders
balai junk --days 30 -k node    # node_modules untouched for 30 days or more
balai clean --dry-run           # what would be deleted, without touching anything
balai clean --days 90           # delete what has slept for 3 months, after confirmation
balai caches                    # npm, pnpm, Cargo, pip… cache sizes
balai config add <folder>       # folders scanned by default
balai update                    # install the latest version
```

`balai --help` and `balai <command> --help` give every detail.

### `balai clean` options

| Option | What it does |
| --- | --- |
| `--days <DAYS>` | Only folders untouched for at least this many days. 30 by default, 0 for all. |
| `-k, --kind <TYPE>` | Only one type: `node`, `target`, `python`, `.next`… Can be repeated. |
| `--dry-run` | Show the list and the summary, delete nothing. |
| `-y, --yes` | Delete without asking. |

A folder whose modification date is unknown is never deleted. If a program holds a file open, balai says so and moves on to the next folder.

### While it scans

A live line shows files scanned and total size. From the second scan of the same folders on, balai knows how many files to expect and adds a percentage and the time left:

```text
⠸ Scanning… 8,602,820 files · 952.0 GB · 67% · ~6 s left · 13 s
```

## License

[MIT](LICENSE-MIT) or [Apache 2.0](LICENSE-APACHE), at your option.
