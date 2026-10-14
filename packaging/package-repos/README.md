# Package repositories (prepared, not created)

Starting contents for the two repositories that make `brew install` and `scoop install` work:

| Folder | Becomes the repository | Holds |
|---|---|---|
| `homebrew-panoply/` | `github.com/anzal1/homebrew-panoply` | `Casks/panoply.rb` (carries the unsigned-app caveat) |
| `scoop-panoply/` | `github.com/anzal1/scoop-panoply` | `bucket/panoply.json` |

The manifests here are placeholders (version `0.1.0-beta`, all-zero checksums). The first real release replaces them:
`.github/workflows/update-package-repos.yml` renders both files from the published release's `SHA256SUMS` and pushes
them. Steps to go live are in `packaging/README.md`, section "The package repos".
