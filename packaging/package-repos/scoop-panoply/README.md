# scoop-panoply

[Scoop](https://scoop.sh) bucket for [Panoply](https://github.com/anzal1/panoply), the open-source file toolbox that runs on your own computer.

```powershell
scoop bucket add panoply https://github.com/anzal1/scoop-panoply
scoop install panoply
```

This installs the desktop app (`panoply-desktop.exe`, with a Start menu shortcut) and the `panoply` command line tool.
Windows x64 and arm64 are supported. The beta is not code signed; Scoop installs avoid most of the SmartScreen warning
that a browser download gets.

`bucket/panoply.json` is updated automatically from each Panoply release (the release workflow in the main
repository) and also carries `checkver` and `autoupdate`, so Scoop's own tooling can bump it.
