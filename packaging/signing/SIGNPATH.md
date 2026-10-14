# SignPath Foundation: free Windows signing for open source

Draft application and checklist. Nothing here has been submitted. Requirements were read from
signpath.org/terms on 2026-10-14; the page was marked draft, so re-read it and the Apply page on
signpath.org before applying.

## What it gives us

A code signing certificate issued to SignPath Foundation (so the publisher shown on Windows is
"SignPath Foundation"), used through SignPath's hosted signing service, free for qualifying open
source projects. Signed binaries and MSIs stop the "Unknown publisher" wall and, with time, build
SmartScreen reputation. winget and Scoop installs already skirt most of SmartScreen, so this is for
people who download the .msi or .zip by hand.

## What they require, and where Panoply stands

| Requirement | Status |
|---|---|
| OSI-approved license on all components, no commercial dual licensing | MIT OR Apache-2.0, both OSI approved. Dependency policy is permissive-only (deny.toml, ADR 0002). WiX is build tooling and is not shipped. |
| Public repository | github.com/anzal1/panoply is public |
| Built on CI from source we own | `.github/workflows/release.yml` builds on GitHub-hosted runners; SignPath's GitHub connector verifies the artifact came from the repository |
| Actively maintained | yes |
| The form to be signed is already released | **Not yet.** Apply right after the first beta is published as an unsigned release |
| Functionality described on the download page | README and the release notes; add a short download page section |
| No malware, no hacking tools, no proprietary components | none |
| Privacy policy for data collection | Panoply collects nothing. The one network use is the first-run PDF engine download from github.com/bblanchon/pdfium-binaries (checksum pinned); state it in the policy |
| Warn before system changes | The MSI shows the PATH and Explorer menu entries as opt-out features in the setup dialog |
| Uninstall | MSI uninstall removes everything it added; Scoop and winget uninstall too |
| MFA for all team members on SignPath and GitHub | turn on for every member before applying |
| Defined roles (author, reviewer, approver) | below |
| "Code signing policy" section on the home and download pages | text below |
| Product name and version metadata on every signed binary | **To do.** The Windows exes need a version resource (ProductName "Panoply", ProductVersion from the tag), e.g. a `build.rs` with the `winresource` crate (MIT). The MSI already carries Name and Version |

## Application text (paste into the Apply form)

> **Project:** Panoply, https://github.com/anzal1/panoply
>
> Panoply is an open-source file toolbox for macOS, Windows and Linux: PDF, image, audio and video
> tools and a universal file converter in one native app and a command line tool. Everything runs on
> the user's own computer; nothing is uploaded and there is no telemetry. It is licensed MIT OR
> Apache-2.0 (both OSI approved) and every dependency is permissively licensed, enforced in CI with
> cargo-deny.
>
> **Build and release:** all release binaries are built by a GitHub Actions workflow
> (`.github/workflows/release.yml`) on GitHub-hosted runners from a signed-off tag, packaged as a
> .zip and a .msi for Windows x64 and arm64, and uploaded to a draft GitHub release that a
> maintainer reviews and publishes. Artifacts carry build provenance attestations. We would use the
> SignPath GitHub connector, so only artifacts built by this workflow can be signed, with manual
> approval of each signing request.
>
> **Team:** the maintainer builds and owns the project and the repository. Roles: Author and
> Approver, Anzal Abidi (@anzal1); Reviewer, to be added as contributors join. MFA is enabled on
> GitHub and will be on SignPath.
>
> **Privacy:** Panoply collects no data. Its only network use is a one-time, checksum-verified
> download of the pdfium PDF engine from github.com/bblanchon/pdfium-binaries, disclosed in the
> README and on first use.
>
> **System changes:** the installer offers two optional features, adding `panoply` to PATH and an
> Explorer right-click entry, both selectable in the setup dialog and removed on uninstall.
>
> **Released form:** the first beta (v0.1.0-beta.1) is published unsigned at
> https://github.com/anzal1/panoply/releases. (Fill in the real tag at application time.)

## Code signing policy (publish on the website and README)

> Free code signing provided by [SignPath.io](https://signpath.io), certificate by
> [SignPath Foundation](https://signpath.org).
>
> Roles: Authors and Approvers: Anzal Abidi (@anzal1). Reviewers: none yet.
>
> Privacy: Panoply does not collect or send any data. The only network access is downloading the
> pdfium PDF engine from github.com/bblanchon/pdfium-binaries on first use of a PDF tool.

## Wiring it into the workflow once approved

SignPath gives an organization id, a project slug and a signing-policy slug. Add the API token as the
repository secret `SIGNPATH_API_TOKEN` and the ids as repository variables
`SIGNPATH_ORGANIZATION_ID`, `SIGNPATH_PROJECT_SLUG`, `SIGNPATH_POLICY_SLUG`. In
`.github/workflows/release.yml`, job `package-windows`, replace the single "Package" step with the
three steps below. `packaging/windows/package.ps1` already has the two `SIGNING HOOK` places; the
simplest wiring is two rounds through SignPath, one for the exes and one for the MSI, as sketched:

```yaml
      # 1. Upload the unsigned exes, have SignPath sign them, put the signed ones back in stage/.
      - id: unsigned-exes
        uses: actions/upload-artifact@<pinned sha>
        with: { name: unsigned-exes-${{ matrix.arch }}, path: stage/*.exe }
      - uses: signpath/github-action-submit-signing-request@<pinned sha>   # check the current major
        with:
          api-token: ${{ secrets.SIGNPATH_API_TOKEN }}
          organization-id: ${{ vars.SIGNPATH_ORGANIZATION_ID }}
          project-slug: ${{ vars.SIGNPATH_PROJECT_SLUG }}
          signing-policy-slug: ${{ vars.SIGNPATH_POLICY_SLUG }}
          github-artifact-id: ${{ steps.unsigned-exes.outputs.artifact-id }}
          wait-for-completion: true
          output-artifact-directory: stage
      # 2. package.ps1 builds the zips and the MSI from the signed exes (add -SkipMsiSign), then
      #    the same upload / submit / replace dance runs on dist/*.msi.
```

Gate the signing steps with `if: vars.SIGNPATH_ORGANIZATION_ID != ''` so the workflow keeps working
unsigned when the variables are absent. Zip archives are signed by signing their contents first, as
above; SignPath does not need to sign the zip itself.

## After approval

- Keep the policy text on the site and the download page; SignPath audits for it.
- Each signing request needs a human approval in SignPath. Do not automate around it.
- First signed release: check the MSI properties show "SignPath Foundation", then submit it to
  winget (the manifest hash must be of the signed MSI, which the workflow already computes after signing).
