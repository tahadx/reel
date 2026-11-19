# Release

Push a version tag. GitHub Actions builds the static engine from that tag,
checks that the committed version files agree with it, writes the release
notes, and publishes the release. Nothing else is needed after the tag push.

## Make a release

1. Make sure `main` holds the commits you want to ship.
2. Stamp the version and rebuild the shipped engine:

   ```sh
   bin/stamp-version X.Y.Z
   bin/refresh-dist
   ```

   `stamp-version` writes the version into `manifest.json` and
   `crates/reel/Cargo.toml`. `refresh-dist` rebuilds the x86_64 musl engine,
   refreshes its source fingerprint, and updates `Cargo.lock` while cargo
   builds.
3. Commit the version files and the engine:

   ```sh
   git add manifest.json crates/reel/Cargo.toml Cargo.lock \
     bin/dist/reel-native bin/dist/reel-native.fingerprint
   git commit -m "chore(release): vX.Y.Z"
   ```

4. Create an annotated tag on that commit. The tag body is the user-facing
   highlights: the workflow places it above the full commit list it always
   keeps. A lightweight tag ships commits only.

   ```sh
   git tag -a vX.Y.Z -m "Reel X.Y.Z" -m "The ... release: ..."
   git push origin vX.Y.Z
   ```

   This checkout signs tags when `tag.gpgSign` is set. Verify the tag points at
   the commit you just made:

   ```sh
   git show vX.Y.Z
   ```

5. Follow the `release` workflow in the Actions tab, or from the terminal:

   ```sh
   gh run list --workflow release.yml
   gh run watch
   gh release view vX.Y.Z
   ```

## What the workflow does

- Resolves the tag. A manual dispatch can rebuild a tag that already exists.
- Refuses a tag that is not on `origin`.
- Re-stamps the tag version into `manifest.json` and
  `crates/reel/Cargo.toml`, and fails if that changes either file. This is why
  the version commit must exist before the tag: a tag whose files disagree can
  never ship a plugin that reports the wrong version.
- Rebuilds `bin/dist/reel-native` for x86_64 musl and verifies that
  `reel --version` reports the tag version.
- Stages `reel-<version>-linux-x86_64`, `reel-<version>-licenses.tar.gz` with
  `LICENSE`, `THIRD-PARTY.md`, and `licenses`, and `checksums.txt`.
- Generates the notes: the annotated tag body first, then contributors, every
  commit since the previous stable tag, and the engine asset details.
- Publishes the release for the tag, or updates it when the tag already has
  one.

## Versions

Use `v` and three numbers, for example `v0.5.0`. A tag with a hyphen is a
prerelease, for example `v0.6.0-beta.1`; the release page marks that tag as a
prerelease.

A stable release compares against the previous stable tag and skips
prerelease tags, so its notes cover the full beta change set. A prerelease
compares against the nearest tag.

## Rebuild a release

Run the `release` workflow by hand and enter a tag that already exists. It
rebuilds the engine from that tag and replaces the assets and notes of the
same release, so a rebuild cannot leave stale assets behind.

Do not move or retag a version that is already published. Cut a new version
instead.

## Checks

The pre-push hook runs formatting, clippy, the Rust tests, QML lint and tests,
the installer test, `bin/check-release`, and the dist freshness check.
`bin/check-release` verifies that the manifest, crate, and engine agree and
that the engine reports the manifest version.

The build workflow also rebuilds the shipped engine and fails unless it
matches the committed `bin/dist/reel-native` byte for byte. If a local build
differs, download the `reel-dist` artifact from that CI run and commit it.
