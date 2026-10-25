# Scrova ロードマップ

最終更新: 2026-10-25

当面のゴールは **macOS で Apple Music の再生を取得し、Last.fm に Scrobble できる最小の縦断実装**。
iOS はその直後に着手し、Android / Windows は FFI 境界を壊さないことだけを意識して後回しにする。

## Phase 0 — 基盤（完了）

- [x] Cargo workspace、Nix devShell（iOS ターゲット入りの Rust）
- [x] `scrova-core`: ドメインモデルと Scrobble 判定エンジン、単体テスト
- [x] `scrova-ffi`: UniFFI で `Scrova` オブジェクトを公開
- [x] `scripts/build-apple.sh`: XCFramework と Swift バインディング生成
- [x] `apps/apple/ScrovaKit`: Swift Package。macOS で Swift→Rust のテストが通り、iOS / iOS Simulator 向けにビルドできる
- [x] 調査: Last.fm API、macOS Music.app（`docs/research/`）

## Phase 1 — Rust Core の完成

- [x] `scrova-lastfm`: 署名、Desktop 認証、`updateNowPlaying`、`scrobble`（50 件バッチ）、`getRecentTracks`
- [x] `scrova-storage`: SQLite 送信キュー、状態遷移、クラッシュ復旧
- [x] `scrova-ffi`: エンジン・キュー・クライアントを束ねる
  - `handle_playback_event` → Scrobble を即キュー永続化
  - `update_now_playing` / `flush`（async）: ネイティブ側が呼ぶ。iOS のバックグラウンドタスクからも呼べる
  - `begin_authorization` / `complete_authorization`、Session Key はネイティブから注入
- [x] 非 JSON の HTTP エラー（502 の HTML など）を再送対象にする
- [x] Mock Playback Observer（`scrova-replay` CLI で再生ログを流してエンジンを検証）
- [x] Last.fm 実 API での疎通確認（ログイン、Now Playing、Scrobble）
- [ ] 不確定再送（送信後に応答を受け取れなかった場合）で重複するかを実測

## Phase 2 — macOS（最初のリリース候補）

- [x] SwiftUI メニューバーアプリ（`MenuBarExtra`、`LSUIElement`、XcodeGen）
- [x] `MusicObserver`: `com.apple.Music.playerInfo` 通知 + Apple Events ポーリング
  - Music.app 未起動時に Apple Event で起動させない
  - スリープ前に `Paused` を送り、復帰後に再取得
- [x] Last.fm ログイン（ブラウザで承認すると自動で検知 → Session Key を Keychain へ）
- [x] Now Playing / Scrobble 送信、送信待ち件数と最近の Scrobble の表示
- [x] ログイン時自動起動（`SMAppService.mainApp`）
- [x] 実機での通し確認（Automation 許可ダイアログ、sandbox 下での曲情報の読み取り、実 API への送信）
  - sandbox 下では `current track` を変数に入れると -10004 になるため、毎回 `… of current track` で読む
- [ ] 配布形態の決定（Developer ID + notarization か Mac App Store か。Apple Events の entitlement が論点）

## Phase 3 — iOS

- [x] SwiftUI アプリ、ScrovaKit 共有（`apps/apple/Shared/`）
- [x] フォアグラウンド中のライブ監視（`MPMusicPlayerController.systemMusicPlayer`）、バックグラウンド移行時に時計を止める
- [x] アプリを離れていた時間を、戻ったときに再生位置で精算する
- [ ] 履歴復旧
  - [x] ライブラリ曲: `MPMediaItem.playCount` / `lastPlayedDate` の差分（起動時と `BGAppRefreshTask`）
  - [ ] 実機で `lastPlayedDate` の更新タイミングを確かめる
  - [ ] 他デバイス分: `MusicRecentlyPlayedRequest`（再生日時なし → 確認待ちにしてから送る）
- [x] `BGAppRefreshTask` で `flush()`（履歴復旧は未実装）
- [x] 実機での通し確認（ログイン、ライブ監視、Scrobble 送信）
- [x] アルバムアートと Scrobble までの残り時間の表示（macOS / iOS 共通）
- [ ] Scrobble 履歴・設定 UI

## Phase 4 — Android

- [ ] `scrova-ffi` の Kotlin バインディング（`cdylib`）
- [ ] `NotificationListenerService` + `MediaSessionManager` で複数アプリを監視
- [ ] Jetpack Compose / Material 3

## Phase 5 — Windows

- [ ] C# バインディング方式の決定（`uniffi-bindgen-cs` か手書き C ABI）
- [ ] `GlobalSystemMediaTransportControlsSessionManager`
- [ ] WinUI 3、システムトレイ常駐、スタートアップ登録

## 将来

- ListenBrainz（宛先別配送状態はキュー設計に織り込み済み）
- `track.love` / `track.unlove`
- Live Activities、Control Center、Shortcuts
