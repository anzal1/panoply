# Homebrew cask for Panoply. Filled by .github/workflows/update-manifests.yml from packaging/scripts/render_manifests.py,
# which opens a pull request that puts it at Casks/panoply.rb in this repository (the repository is the tap):
#   brew tap anzal1/panoply https://github.com/anzal1/panoply
#   brew install --cask anzal1/panoply/panoply
cask "panoply" do
  arch arm: "arm64", intel: "x64"

  version "0.3.0"
  sha256 arm:   "6b00f5595db1f20540242dbf45d342488368381f6b1bf7e8afddf83eef2dec0a",
         intel: "0e21dd6766ee2da082f0df2cfed3646e202bc58ecf9f3e5cee6cd340a82acee1"

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
