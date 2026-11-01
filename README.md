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

| Variable | Meaning |
|---|---|
| `HASHIBUILD_ARCHIVE` | local archive dir (or `--archive`) |
| `HASHIBUILD_FETCHER` | command run on a local miss, e.g. `aws s3 cp s3://bucket/[ZIP] [DEST]` |
| `HASHIBUILD_UPLOADER` | command run after a build, e.g. `aws s3 cp [ZIP] s3://bucket/` |

## Release

Publishing a GitHub release tagged `vX.Y.Z` builds binaries and wheels and publishes to PyPI
(Trusted Publishing, environment `pypi`).
