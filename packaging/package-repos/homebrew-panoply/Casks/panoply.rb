# Homebrew cask for Panoply. Filled by .github/workflows/release.yml from packaging/scripts/render_manifests.py.
# Lives in the tap repository anzal1/homebrew-panoply (Casks/panoply.rb):
#   brew install --cask anzal1/panoply/panoply
cask "panoply" do
  version "0.1.0-beta"
  arch arm: "arm64", intel: "x64"
  sha256 arm:   "0000000000000000000000000000000000000000000000000000000000000000",
         intel: "0000000000000000000000000000000000000000000000000000000000000000"

  url "https://github.com/anzal1/panoply/releases/download/v#{version}/Panoply-#{version}-macos-#{arch}.zip"
  name "Panoply"
  desc "Every file tool in one fast app, running on your own computer"
  homepage "https://github.com/anzal1/panoply"

  livecheck do
    url :url
    strategy :github_latest
  end

  # LSMinimumSystemVersion in the app's Info.plist.
  depends_on macos: ">= :sonoma"

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
