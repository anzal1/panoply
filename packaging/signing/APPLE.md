# Apple Developer account checklist (macOS signing and notarization)

For when the $99 a year Apple Developer Program account exists. Until then macOS builds are ad-hoc
signed (Apple Silicon refuses unsigned code, so this is the minimum) and Gatekeeper warns users on
first launch. Nothing here has been done; the release workflow already has the slot and skips it cleanly.

## 1. Account

- [ ] Enroll in the Apple Developer Program as an individual (fastest, publisher name is the legal
      name) or as an organization (needs a D-U-N-S number and takes longer).
- [ ] Turn on two-factor authentication for the Apple ID.
- [ ] Note the **Team ID** (Membership details page, ten characters).

## 2. Developer ID Application certificate

- [ ] On a Mac, Keychain Access > Certificate Assistant > Request a Certificate From a Certificate
      Authority; save the CSR.
- [ ] developer.apple.com > Certificates > create **Developer ID Application**, upload the CSR,
      download the .cer, double-click it to install.
- [ ] In Keychain Access, export the certificate together with its private key as `panoply-dev-id.p12`
      with a strong password. Back it up somewhere safe (losing the key means issuing a new
      certificate).
- [ ] Find the identity name: `security find-identity -v -p codesigning`, which prints
      `Developer ID Application: Your Name (TEAMID)`.

## 3. Notarization credentials

- [ ] appleid.apple.com > Sign-In and Security > **App-Specific Passwords**: create one named
      "panoply-notary".
- [ ] (Alternative to the Apple ID flow: an App Store Connect API key with the Developer role. The
      script uses the Apple ID flow; switching to `notarytool --key --key-id --issuer` is a small
      change in `notarize()` in `packaging/scripts/macos-package.sh`.)

## 4. Repository secrets (Settings > Secrets and variables > Actions)

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE_P12_BASE64` | `base64 -i panoply-dev-id.p12 \| pbcopy` |
| `APPLE_CERTIFICATE_PASSWORD` | the .p12 password |
| `APPLE_SIGNING_IDENTITY` | `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_ID` | the Apple ID email |
| `APPLE_TEAM_ID` | the ten-character Team ID |
| `APPLE_APP_SPECIFIC_PASSWORD` | the app-specific password from step 3 |

With the first three set the build is signed (hardened runtime, timestamped). With all six it is also
notarized and stapled (the .app, the .zip contents, the .dmg) and the CLI binary is notarized.
With none set the workflow prints a notice and ships the ad-hoc build.

## 5. What the script does with them (`packaging/scripts/macos-package.sh`)

1. Creates a throwaway keychain on the runner and imports the .p12.
2. Signs inside out: `Contents/MacOS/panoply-cli`, then `Panoply.app`, with
   `--options runtime --timestamp` and `packaging/macos/entitlements.plist`.
3. `notarytool submit --wait` on the zip, then `stapler staple` on the app, then re-zips.
4. Signs the .dmg, notarizes it, staples it.
5. The Homebrew cask is rendered without the "not notarized" caveats block.

Entitlements, and why: hardened runtime blocks writable-then-executable memory and loading libraries
Apple did not sign. Panoply needs `com.apple.security.cs.allow-unsigned-executable-memory` (wasmtime
compiles community modules to machine code in memory) and
`com.apple.security.cs.disable-library-validation` (the pdfium library is downloaded on first use and
loaded with dlopen). If a signed build crashes on launch or when a module runs, the first suspect is
a missing entitlement; check `log show --last 5m --predicate 'process == "Panoply"'`.

## 6. First signed release: verify before publishing the draft

- [ ] `codesign --verify --deep --strict --verbose=2 /Applications/Panoply.app`
- [ ] `spctl --assess --type execute --verbose /Applications/Panoply.app` says "accepted, source=Notarized Developer ID"
- [ ] `xcrun stapler validate Panoply.app` and on the .dmg
- [ ] Download the .zip through a browser (so it gets the quarantine flag) onto a second Mac, open it:
      no warning, a PDF tool runs (pdfium downloads and loads), a community module runs.
- [ ] Update the Homebrew tap with the rendered cask from the release's `panoply-package-manifests` zip.

## 7. Later

- Finder and share extensions are separate signed targets (Swift) with their own entitlements.
- An Apple Developer account also covers iOS signing and TestFlight for the iOS app.
- Renewal: the account is yearly; the Developer ID certificate lasts five years.
