# homebrew-panoply

Homebrew tap for [Panoply](https://github.com/anzal1/panoply), the open-source file toolbox that runs on your own computer.

```bash
brew install --cask anzal1/panoply/panoply
```

This installs `Panoply.app` and puts the `panoply` command line tool on your PATH.

The beta is **not notarized** (no Apple Developer account yet), so macOS may block the first launch. Allow it in
System Settings > Privacy & Security > Open Anyway, or run once:

```bash
xattr -dr com.apple.quarantine /Applications/Panoply.app
```

`Casks/panoply.rb` is updated automatically from each Panoply release (the release workflow in the main repository).
Do not edit it by hand; the checksums come from the release's `SHA256SUMS`.
