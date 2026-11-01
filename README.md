# hashibuild

Content hash for a source subtree of a git repo, with optional hash-keyed build caching.

The hash covers the git-tracked files of the subtree (`git ls-files`) plus the lock and toolchain
files from its parent directories (`Cargo.lock`, `package-lock.json`, `pnpm-lock.yaml`, `go.sum`, ...),
so it changes whenever anything that affects the built result changes.

```
pip install hashibuild        # or: uvx hashibuild, cargo install --path .
hashibuild path/to/component            # print the hash
hashibuild path/to/component -m         # per-file manifest + total
hashibuild -c app.json --build          # build with caching
```

## Hashing

| Option | Meaning |
|---|---|
| `--only PREFIX`, `--exclude PREFIX` | case-insensitive prefix filters on subtree-relative paths (only first, then exclude) |
| `-i PATHSPEC` | extra repo-relative files to include |
| `-n NAME` | extra file name to look for in parent dirs |
| `--no-defaults` | skip the built-in lock file list |
| `--untracked` | also hash untracked, non-ignored files |
| `--salt`, `--build-param` | mixed into the hash |
| `-l`, `-m`, `-s N` | list files, manifest, short hash |

## Build caching

`--build` looks for `<archive>/<name>_<hash>.zip`; on a hit it unzips into the output root, otherwise it
runs `buildCmd`, zips the output dirs and runs the uploader.

```json
{
  "name": "myapp",
  "inputRoot": "../myapp",
  "outputDir": "dist",
  "buildCmd": "npm run build",
  "exclude": ["tests/"]
}
```

A config file is optional; everything can be given as flags:

```
hashibuild services/api --build --name api --archive ~/.cache/hb \
  --build-cmd "npm run build" --output-dir dist --exclude tests/
```

| Flag | Env var | Meaning |
|---|---|---|
| `--archive DIR` | `HASHIBUILD_ARCHIVE` | local archive dir |
| `--fetcher CMD` | `HASHIBUILD_FETCHER` | run on a local miss, e.g. `aws s3 cp s3://bucket/[ZIP] [DEST]` |
| `--uploader CMD` | `HASHIBUILD_UPLOADER` | run after a build, e.g. `aws s3 cp [ZIP] s3://bucket/` |
| `--name`, `--output-root`, `--build-cmd`, `--output-dir` | | archive name prefix (default: component dir name), output base dir, build command, output dirs |

Flags take precedence over env vars and config values.

`--vacuum` (alone or before `--build`) deletes archive zips older than `--max-age-days` (default 3), then
keeps only the newest ones within `--max-size-mb` (default 500). It only touches `*.zip` in the archive dir.

## Release

Publishing a GitHub release tagged `vX.Y.Z` builds binaries and wheels and publishes to PyPI
(Trusted Publishing, environment `pypi`).
