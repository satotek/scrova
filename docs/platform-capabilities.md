# プラットフォーム別の能力と制約

最終更新: 2026-10-25

各 OS で「何が取れて、何が取れないか」をまとめる。根拠の詳細と出典 URL は `docs/research/` にある。
**公式** = Apple / Last.fm の現行ドキュメントで確認、**非公式** = 第三者情報や実測のみ、**未検証** = まだ確かめていない。

## 1. 早見表

| 能力 | macOS (Music.app) | iOS | Android | Windows |
|---|---|---|---|---|
| ライブの再生情報 | ◎ Apple Events（公式） | △ アプリ実行中のみ | ○ MediaSession（未調査） | ○ GSMTC（未調査） |
| 再生位置 | ◎ `player position` | ○ `currentPlaybackTime` | 未調査 | 未調査 |
| 安定したトラック ID | ◎ `persistent ID` | ○ `MPMediaItem.persistentID` | 未調査 | 未調査 |
| バックグラウンド常駐 | ◎ メニューバー常駐 | ✕ 保証なし | ○ Foreground Service | ◎ トレイ常駐 |
| 再生履歴の後追い | 不要（常駐できる） | △ 推定のみ | 未調査 | 未調査 |
| 他の再生アプリ | ✕ 公開 API なし | ✕ | ○ | ○ |

## 2. macOS

詳細: `docs/research/macos-music.md`

### 取得方法

- **正は Apple Events。** Music.app の scripting definition（`sdef /System/Applications/Music.app`）にある
  `player state` / `player position`（秒）/ `current track` の `persistent ID`（16 進、"does not change over time"）・
  `name` / `artist` / `album` / `album artist` / `duration`（秒）を読む（公式: sdef）
- Swift からは `NSAppleScript` でコンパイル済みスクリプトを数秒おきに実行する
- `com.apple.Music.playerInfo` 分散通知は**非公式**。ドキュメントがなく、`DistributedNotificationCenter` は
  配送遅延の上限なし・破棄あり得ると公式に書いている。再読込のきっかけにだけ使う
- MediaRemote（private framework）は macOS 15.4 以降、一般プロセスから Now Playing を読めなくなった（非公式）。使わない

実測（2026-10-25、macOS 27 / Music 1.7）: 上記スクリプトで
`paused, 0CCDA3922D2F39CA, <曲名>, <アーティスト>, <アルバム>, , 159.99…, 45.27…` が取れた。
`album artist` は空文字になり得る。

### 権限と配布

| 層 | 必要なもの |
|---|---|
| Hardened Runtime | `com.apple.security.automation.apple-events` |
| App Sandbox（MAS では必須） | `com.apple.security.scripting-targets`: `com.apple.Music` → `com.apple.Music.playback`, `com.apple.Music.library.read` |
| TCC | `NSAppleEventsUsageDescription`。初回にユーザー同意。拒否後は `errAEEventNotPermitted (-1743)` |

- Music.app は access group を公開しているので、`temporary-exception.apple-events` ではなく `scripting-targets` が公式の推奨経路
- `playback` だけで `persistent ID` / `duration` が読めるかは sdef から断定できない（**未検証**）。両 group を要求する
- Mac App Store 審査を通るかは**未検証**。Developer ID + 公証なら sandbox は必須ではない

### 注意点

- `tell application "Music"` は未起動の Music.app を**起動してしまう**。送る前に
  `NSRunningApplication.runningApplications(withBundleIdentifier:)` で確認する（公式）
- スリープ: `NSWorkspace.willSleepNotification` で `Paused` を送り、`didWakeNotification` で再取得
- ログイン時起動: `SMAppService.mainApp`（macOS 13+）。`requiresApproval` はエラーではなく「設定で承認待ち」
- メニューバー: `MenuBarExtra` + `LSUIElement`。ユーザーが extra を外すとアプリは終了する（公式）

## 3. iOS

### ライブ監視

- `MPMusicPlayerController.systemMusicPlayer` で Music アプリの再生中アイテム・状態・位置を読める（公式、iOS 3+。
  **macOS では使えない**）
- ただし他アプリの再生を**常時**監視する手段はない。アプリがバックグラウンドに回ると実行は止まる
- `UIBackgroundModes` の `audio` で常駐を装うのは審査リスクが高く、採用しない
- `UIApplication.beginBackgroundTask` でわずかな猶予は得られるが、OS が早期に打ち切り得る

→ iOS のライブ監視は「アプリを開いている間」が基本。残りは履歴の後追いで埋める。

アプリを離れていた時間（Music アプリに切り替えた場合など）は、戻ったときに再生位置で精算する。

- 離れた瞬間のスナップショットを保持しておき、すぐには送らない
- 戻ったとき同じ曲なら何も足さない。エンジンが `min(位置の進み, 経過時間)` を加算するので、Music 側で一時停止していた時間は数えない
- 曲が変わっていたら、前の曲は最後まで聴いたのかスキップしたのかわからない。離れた時点の一時停止として閉じ、聴き終えていれば履歴の後追いで拾う

### 履歴の後追い

| 経路 | 取れるもの | 時刻の精度 | 制約 |
|---|---|---|---|
| `MPMediaItem.playCount` / `lastPlayedDate` の差分 | ライブラリ内の曲の再生回数増分 | 最後の 1 回だけ正確。それ以前は推定 | ライブラリに無い曲は不可。要 `MPMediaLibrary` 権限 |
| MusicKit `Song.lastPlayedDate` / `playCount` | 同上（iOS 16+、"on this device"） | 同上 | 同上 |
| MusicKit `MusicRecentlyPlayedRequest` / Apple Music API recently played | 他デバイス分も含む最近の再生 | **時刻なし**（推定するしかない） | 件数上限あり。Apple Music 契約が必要 |

- 復旧分は `TimestampAccuracy::Estimated` / `ScrobbleOrigin::HistoryRecovery` として区別する
- ライブラリ差分は自動でキューに入れる。`lastPlayedDate` を最後の再生の終了時刻とみなし、開始時刻は曲長を引いて求める
- 初回スキャンは基準を記録するだけで送らない（インストール前の再生を流さない）
- 重複排除: 同じ曲のライブ Scrobble が近い時刻にあれば、その分は送らない。`session_id` を曲と開始時刻から決めるので、同じスキャンを繰り返しても二重にならない
- 時刻がまったくわからない recently played は、送る前に確認を挟む前提（未実装）

### バックグラウンド処理

- `BGAppRefreshTask`（短時間、`fetch` capability）/ `BGProcessingTask`（長め、ネットワーク要件を付けられる）
- どちらも実行時期は OS 任せで単発。ハンドラ内で次回を再登録する
- 用途は「履歴の後追い」と「送信キューの flush」。ライブ監視の継続には使えない

### 未検証

- systemMusicPlayer の `currentPlaybackTime` がバックグラウンド復帰直後にどこまで正確か
- `lastPlayedDate` の更新タイミング（再生開始時か終了時か）
- Apple Music API recently played の件数上限の現行値

## 4. Android（Phase 4 で調査）

- `NotificationListenerService` の権限を得ると `MediaSessionManager.getActiveSessions` で
  複数アプリの `MediaController` を取得できる、という前提で設計している（**未検証**）
- 常駐には Foreground Service が必要になる見込み

## 5. Windows（Phase 5 で調査）

- `GlobalSystemMediaTransportControlsSessionManager` で SMTC に登録した全アプリの再生情報を取れる見込み（**未検証**）
- C# との接続方式は `docs/architecture.md` §5.3

## 6. Last.fm（全 OS 共通）

詳細: `docs/research/lastfm.md`

- 認証は Desktop フロー（`auth.getToken` → ブラウザ承認 → `auth.getSession`）。token は 60 分・1 回限り、Session Key は無期限
- `track.scrobble` は最大 50 件/リクエスト。`timestamp` は再生**開始**時刻（UTC epoch 秒）
- 成功レスポンスでも曲ごとに `ignoredMessage` で拒否され得る
- 再送すべきエラーは公式には 11 / 16 のみ（9 は再認証後）。29（レート制限）と 8 は運用上バックオフして再送
- レート制限の数値は非公開
- Shared Secret はアプリ配布で秘匿できない（PKCE 相当の仕組みがない）
