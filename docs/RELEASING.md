# Installation and releases

## macOS without Apple developer credentials

Preview builds use an ad-hoc code signature. They do not require an Apple account, but are not notarized. Copy `Photo Sorting Hat.app` into Applications. If macOS blocks first launch, use the normal **System Settings → Privacy & Security → Open Anyway** flow for an app you trust. Do not disable Gatekeeper globally.

[Apple's instructions](https://support.apple.com/en-euro/guide/mac-help/mh40616/mac) and [Tauri's signing guide](https://tauri.app/distribute/sign/macos/).

Apple Silicon and Intel builds must bundle a metadata runtime built for that architecture. Linux x86-64 releases use an AppImage built on Ubuntu 22.04. The CLI is distributed with a sibling `metadata` runtime directory. Linux may require executable permission on the AppImage; systems without FUSE can use AppImage's extract-and-run option.

## GitHub setup

Create public `photo-sorting-hat` in the authenticated user's account, push the source, and enable GitHub Actions. Never commit authentication tokens, signing keys, `.tools`, local databases, build output, or personal photos.

Updater authentication is independent of Apple signing. Generate the update key once outside the repository:

```sh
pnpm tauri signer generate -w /secure/location/photo-hat.key
```

Back up that private key securely. Configure repository Actions secrets `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`, and repository variable `TAURI_SIGNING_PUBLIC_KEY` containing the corresponding public key. Empty passwords are supported. Never generate a different key for an ordinary update.

`scripts/configure_release.py` writes the public endpoint and public key into the app at build time. With no release configuration, local development builds make no update requests. Published builds check the signed manifest from GitHub's latest non-prerelease release; initial previews are marked by version/documentation, not GitHub's prerelease flag, so this endpoint resolves consistently.

Push a `v0.1.0`-style tag matching both package versions. CI first builds all three platforms and uploads artifacts; a final release job creates one draft, attaches all artifacts and a combined signed-update manifest, and publishes only when every build has succeeded. Source changes run tests without secrets. Failed matrix jobs leave no partial public update manifest.

Before a public release, complete the manual import/install checklist in VALIDATION.md and inspect all artifacts. The checked-in workflow provides the pipeline; a successful local build alone does not establish clean-machine or cross-platform compatibility.
