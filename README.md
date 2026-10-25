# Scrova

Cross-platform music scrobbler for Last.fm. The scrobbling logic is written once
in Rust; each platform observes playback with its native APIs and provides a
native UI.

Status: early development. macOS (Apple Music) comes first, then iOS.

| Platform | Playback source | UI | Status |
|---|---|---|---|
| macOS | Music.app via Apple Events | SwiftUI menu bar app | in progress |
| iOS | MediaPlayer / MusicKit | SwiftUI | planned |
| Android | MediaSession | Jetpack Compose | planned |
| Windows | GSMTC | WinUI 3 | planned |

## Layout

```text
crates/
  scrova-core/           scrobble engine and domain model (no I/O)
  scrova-storage/        SQLite submission queue
  scrova-lastfm/         Last.fm API client
  scrova-ffi/            UniFFI bindings (Swift / Kotlin)
  uniffi-bindgen-swift/  binding generator used by the build script
apps/apple/
  ScrovaKit/             Swift package wrapping the Rust core (XCFramework)
  macOS/                 menu bar app
docs/                    architecture, platform notes, roadmap, research
scripts/build-apple.sh   builds ScrovaFFI.xcframework and Swift bindings
```

See [docs/architecture.md](docs/architecture.md),
[docs/platform-capabilities.md](docs/platform-capabilities.md) and
[docs/roadmap.md](docs/roadmap.md).

## Development

Requirements: Nix with flakes and direnv, Xcode 27.

```sh
direnv allow                       # Rust toolchain with Apple targets, xcodegen
cargo test                         # Rust tests
scripts/build-apple.sh             # XCFramework + Swift bindings (add --debug for a faster build)
(cd apps/apple/ScrovaKit && swift test)
```

Last.fm API credentials are not committed. Copy
`apps/apple/Config/Secrets.example.xcconfig` to `Secrets.xcconfig` and fill in
your own key from <https://www.last.fm/api/account/create>.

## License

MIT OR Apache-2.0
