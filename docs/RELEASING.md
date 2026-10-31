---
created_on: 2026-10-30 20:37
last_modified: 2026-10-30 20:37
status: current
---

# GitHub releases

This fork distributes binaries through `alexgorbatchev/syswatch` GitHub Releases.
The workflow uses the repository's automatic `GITHUB_TOKEN`; crates.io and
upstream's Homebrew tap are not publication destinations.

## Validate without publishing

1. Push the release workflow and source changes to `main`.
2. In GitHub Actions, select **Release → Run workflow** on `main` (or run
   `gh workflow run release.yml --ref main`).
3. Require both native check jobs and all seven build jobs to pass. Download
   the workflow artifacts to inspect the versioned archives. Manual runs do
   not publish a GitHub release. Native Linux x86_64 and macOS arm64 archives
   are extracted and smoke-tested; the other targets are cross-compiled.

## Publish

1. Set the intended version in `Cargo.toml`, regenerate `Cargo.lock`, and keep
   the Nix package version aligned. Commit and push the verified changes.
2. Create a new tag matching the package version exactly, for example
   `git tag -a v0.14.3 -m 'SysWatch 0.14.3'`, then `git push origin v0.14.3`.
   Never replace an existing tag. A tag/version mismatch fails the build.
3. Wait for the Release workflow to pass. It runs formatting, Clippy, and tests
   on Linux and macOS, then builds all targets before publishing archives and
   `SHA256SUMS`. Tags containing `-` are marked as prereleases.
4. Verify the release's seven archives and checksum manifest. Each archive
   contains the executable named `syswatch` and `LICENSE`. Edit the release
   description to describe the user-visible changes actually shipped.

If a check or build fails, no release is published. Diagnose the failed job
before retrying. Workflow artifacts expire after seven days; release assets
remain attached to the release.
