# Homebrew cask for Panoply. Filled by .github/workflows/update-manifests.yml from packaging/scripts/render_manifests.py,
# which opens a pull request that puts it at Casks/panoply.rb in this repository (the repository is the tap):
#   brew tap anzal1/panoply https://github.com/anzal1/panoply
#   brew install --cask anzal1/panoply/panoply
cask "panoply" do
  arch arm: "arm64", intel: "x64"

  version "0.3.0-beta"
  sha256 arm:   "c6ced559feade569e1450087aa08cec54e62d3fbec16f50e13b4355739d42c54",
         intel: "d429efe2038332c118bae5cc176ecd14c058a397a5ca85c1f8de6a6fc4a0d705"

  url "https://github.com/anzal1/panoply/releases/download/v#{version}/Panoply-#{version}-macos-#{arch}.zip"
  name "Panoply"
  desc "Every file tool in one fast app, running on your own computer"
  homepage "https://github.com/anzal1/panoply"

  # Every release is a prerelease for now, and the GitHub strategies skip prereleases, so read the tags.
  livecheck do
    url "https://github.com/anzal1/panoply.git"
    regex(/^v(\d+(?:\.\d+)+(?:-beta(?:\.\d+)?)?)$/i)
    strategy :git
  end

  # LSMinimumSystemVersion in the app's Info.plist.
  depends_on macos: :sonoma

  app "Panoply.app"
  # The command line tool rides inside the app (named panoply-cli there because the app
  # executable is Panoply and macOS volumes are case-insensitive).
  binary "#{appdir}/Panoply.app/Contents/MacOS/panoply-cli", target: "panoply"

  zap trash: [
    "~/Library/Application Support/Panoply",
    "~/Library/Preferences/dev.panoply.desktop.plist",
    "~/Library/Saved Application State/dev.panoply.desktop.savedState",
  ]

  caveats <<~EOS
    Panoply is not signed or notarized (no Apple Developer account yet), so macOS blocks the first launch.
    Allow it once in System Settings > Privacy & Security > Open Anyway, or run:
      xattr -dr com.apple.quarantine /Applications/Panoply.app
  EOS
end
