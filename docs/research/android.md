# Android で他アプリの再生を見る

Scrova（Rust 製 Last.fm scrobbler）の Android 側で、Spotify / YouTube Music / Android 版 Apple Music など**全メディアアプリ**の再生をどう取るかの調査。実装は書かない。主張は出典付き。公式ドキュメントに無いものは【未確認】、公式の複数の記述から導いたものは【公式から導出】と書く。

調査日: 2026-10-25。実機計測はしていない。Android の公開ドキュメントは JS 描画なので、本文は取得した HTML と AOSP の現行 `main`（`android.googlesource.com`、`?format=TEXT`）で確認した。人間が読む URL は `developer.android.com` を併記する。

エンジン入力は `PlaybackEvent { track, state: Playing/Paused/Stopped, position, source, observed_at }`（`docs/architecture.md` §2.1）。`source` は再生アプリの package name。Session Key はネイティブが保持し、起動時に Rust へ渡す。Rust は永続化しない（同 §4.2）。

## 結論（先に）

他アプリの再生を取る公式の入口は、ユーザーが有効にした `NotificationListenerService` の `ComponentName` を `MediaSessionManager.getActiveSessions` に渡すこと。リスナーはシステムが bind する。bind を外すとプロセスは cached に落ち、LMK に殺され得る、とメモリガイドは書く。別の foreground service が要るとは書いていない。`mediaPlayback` は自アプリの再生継続用で、他アプリの監視には合わない。sideload と、アプリストア以外から落とされたアプリでは、Android 16 の CDD が notification listener を Restricted Settings の対象にしている。

位置は `getPosition()` が返すスナップショット（ms）で、現行 javadoc に外挿式は無い。観測時刻の位置は `getLastPositionUpdateTime()`（elapsed realtime）と `getPlaybackSpeed()` から自分で計算する。【公式から導出】

Session Key に `EncryptedSharedPreferences` は使わない。`androidx.security:security-crypto:1.1.0`（2025-07-30）で API が deprecated。代替は DataStore 1.3.0 の `datastore-tink` `AeadSerializer` と Android Keystore。

認証ページは Custom Tabs で開く。Scrova は `auth.getSession` をポーリングするので redirect URI は要らない。送信は `OneTimeWorkRequest` に `NetworkType.CONNECTED`。定期実行の下限は 15 分で、ライブ監視の代わりにはならない。

Play が音楽 scrobbler の notification listener を許すか、提出時に宣言フォームが要るかは、確認した公式ポリシー本文には無かった。【未確認】

## 1. 他アプリの再生を見る

### 入口は NotificationListenerService

`NotificationListenerService`（API 18）は、通知の投稿・削除・ランキング変化をシステムから受け取るサービス。拡張するにはマニフェストで `BIND_NOTIFICATION_LISTENER_SERVICE` を付け、`SERVICE_INTERFACE`（定数値 `"android.service.notification.NotificationListenerService"`）の intent-filter を入れる。現行リファレンスの例は次のとおり（`android:exported="false"` まで例に含まれている）。

```xml
<service android:name=".NotificationListener"
         android:label="@string/service_name"
         android:exported="false"
         android:permission="android.permission.BIND_NOTIFICATION_LISTENER_SERVICE">
    <intent-filter>
        <action android:name="android.service.notification.NotificationListenerService" />
    </intent-filter>
</service>
```

出典: [NotificationListenerService](https://developer.android.com/reference/android/service/notification/NotificationListenerService)（ページ最終更新 2026-09-16）。同じ文は AOSP `main` のクラス javadoc にもある（[NotificationListenerService.java](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/core/java/android/service/notification/NotificationListenerService.java)）。

例に続く `default_filter_types` / `disabled_filter_types` の meta-data は、どの通知種別を橋渡しするかの話で、再生監視には不要。付けない。

操作は `onListenerConnected()` まで待つ。このイベントの前、または `onListenerDisconnected()` の後に安全に呼べるのは `requestRebind(ComponentName)` だけ。`onListenerConnected()`（API 21）の時点で `getActiveNotifications()` を呼んでよい。`onListenerDisconnected()`（API 24）の後はイベントが来ず、呼んでよいのは `requestRebind` だけ。API 24 以降、コールバックはメインスレッド。N より前はスレッドの保証が無い。

出典: 上の NotificationListenerService リファレンス。

このクラスの本文は `MediaSessionManager` と再生監視を結びつけていない。結びつくのは次の `MediaSessionManager` 側。

### セッション一覧は有効なリスナーの ComponentName で取る

`MediaSessionManager`（API 21）は、アプリが公開した再生状態を扱う。`getActiveSessions(ComponentName)`（API 21）は進行中セッションの `MediaController` を、重要なものから順に返す。index 0 が最重要。

要件は次のどちらか。

- 呼び出し元が `MEDIA_CONTENT_CONTROL` を持っている。
- アプリが有効な notification listener で、その `ComponentName` を渡す。引数は null でもよい、と書いてある。null で取れるのは権限を持っている場合。

`getActiveSessions` と `addOnActiveSessionsChangedListener` の公開 javadoc に `SecurityException` は無い。`SecurityException` を明記しているのは、メディアキーセッション系（`getMediaKeyEventSession` など、API 33）と `getActiveSessionsForPackage`（API 37）。権限も有効なリスナーも無いときに `getActiveSessions` が空リストを返すのか例外なのかは、公開 javadoc には無い。【未確認】

`addOnActiveSessionsChangedListener`（API 21）の要件文は `getActiveSessions` と同じ。3 引数版は `Handler` を取れる。null なら呼び出し元スレッドに届く。

出典: [MediaSessionManager](https://developer.android.com/reference/android/media/session/MediaSessionManager)（最終更新 2026-08-03）。AOSP: [MediaSessionManager.java](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/media/java/android/media/session/MediaSessionManager.java)。

`MEDIA_CONTENT_CONTROL` は通常のアプリに付与されない signature 権限なので、Scrova が使えるのはリスナー経路だけ。この文は権限の保護レベルを上のページからは確認していない。サードパーティに付かない、という扱いはプラットフォームの一般知識であり、今回取得したページの本文には protection level の文が無かった。設計上はリスナー経路だけを使う。

### 変化は MediaController.Callback

`MediaController.Callback`（API 21）は `registerCallback` で登録する。

- `onMetadataChanged(MediaMetadata)`: 現在のメタデータが変わったとき。引数は現在のメタデータ、無ければ null。
- `onPlaybackStateChanged(PlaybackState)`: 再生状態が変わったとき。引数は新しい状態。null があり得る。
- `onSessionDestroyed()`: セッションが破棄された。この呼び出しの後、セッションは無効で、呼び出しは無視される。

いつプラットフォームがこれらを呼ぶか（`setMetadata` / `setPlaybackState` との対応）は、Callback のページには書いていない。

出典: [MediaController.Callback](https://developer.android.com/reference/android/media/session/MediaController.Callback)。AOSP: [MediaController.java](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/media/java/android/media/session/MediaController.java)。

再生アプリの識別子は `MediaController.getPackageName()`。「Get the session owner's package name.」これを `PlaybackEvent.source` にする。

出典: 上の MediaController.java。公開リファレンスは [MediaController](https://developer.android.com/reference/android/media/session/MediaController)。

一覧は優先順であって、「今鳴っているのは index 0 だけ」とは書いていない。複数セッションを全部見るか、index 0 だけ見るかは実機確認（§9）。

### 許可の判定と設定画面

有効かどうかは `NotificationManager.isNotificationListenerAccessGranted(ComponentName)`（API 27）。リスナーは呼び出し元アプリに属していなければならない。ユーザーを送る先として、同じメソッドの説明が `Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS` を挙げている。

出典: [NotificationManager.isNotificationListenerAccessGranted](https://developer.android.com/reference/android/app/NotificationManager#isNotificationListenerAccessGranted(android.content.ComponentName))。

設定画面の Intent:

- `Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS`（API 22）。「Show Notification listener settings.」一致する Activity が無い場合があるので、呼べないときに備える。Input / Output は Nothing。定数値 `"android.settings.ACTION_NOTIFICATION_LISTENER_SETTINGS"`。
- `Settings.ACTION_NOTIFICATION_LISTENER_DETAIL_SETTINGS`（API 30）。アプリの notification listener 許可ページ。`EXTRA_NOTIFICATION_LISTENER_COMPONENT_NAME`（API 30）で `ComponentName` を渡す。

出典: [Settings](https://developer.android.com/reference/android/provider/Settings)。

`Settings.Secure` の公開リファレンス（[Settings.Secure](https://developer.android.com/reference/android/provider/Settings.Secure)、最終更新 2026-09-16）の HTML に `ENABLED_NOTIFICATION_LISTENERS` は無かった。有効判定にこの定数を読まない。API 27 未満での判定方法は、公開リファレンスには無い。【未確認】。Play の target は API 36 なので、判定 API の下限は 27 で足りる。

### 再 bind

`requestRebind(ComponentName)`（API 24、static）は、先に `requestUnbind` したあとで再 bind を頼む。ユーザーが許可していないリスナーでは失敗する。

`requestUnbind()`（インスタンス）のあと、更新は来ず、メソッド呼び出しの成功は保証されない。次に `onListenerConnected()` が来るまで。システムはこの呼び出しのあとサービスを殺し得る。

出典: [NotificationListenerService](https://developer.android.com/reference/android/service/notification/NotificationListenerService)。

既定の自動 bind は meta-data `android.service.notification.default_autobind_listenerservice`（`META_DATA_DEFAULT_AUTOBIND`、API 34）。値が `'false'` なら on-demand で、`requestRebind` で bind する。値が無いときの既定は `'true'`。

出典: 上のリファレンス、および AOSP の `META_DATA_DEFAULT_AUTOBIND` javadoc。

force-stop のあとシステムが再 bind するか、再起動のあと autobind が効くかは、取得したページに無い。【未確認】

### Restricted settings（sideload）

developer.android.com の Android 13–17 behavior-change ページには、sideload アプリの notification listener 有効化を塞ぐ、という文は無かった。端末要件は CDD にある。

Android 16 CDD（handheld、最終更新 2025-12-02）の Restricted Settings は、次のアプリを Covered Applications と呼ぶ。

- アプリストア以外のアプリ（メッセージやブラウザ）経由でダウンロードされ、`PACKAGE_SOURCE_DOWNLOADED_FILE` と識別されるもの。本文のリンク先は `PackageInstaller.PACKAGE_SOURCE_DOWNLOADED_FILE`。表示上の定数名は `PACKAGE_DOWNLOADED_FILE`。
- ローカルファイルから入れたもの（例として sideload）。`PACKAGE_SOURCE_LOCAL_FILE`。

handheld は、これらのアプリに権限を渡す前に警告と確認を出さなければならない。対象の special permission に Notification listener（`AppOpsManager.OPSTR_ACCESS_NOTIFICATIONS`）が含まれる。確認は、そのアプリの AppInfo ページから `EnhancedConfirmationManager` で取る以外に許されない。既定で有効で、全アプリ一括で無効にする導線は強く非推奨。

出典: [Android 16 Compatibility Definition](https://source.android.com/docs/compatibility/16/android-16-cdd#restricted-settings) の 9.8/H-0-1 から H-0-4。これは端末実装者向けの MUST で、アプリ向け behavior-change ページではない。Android 15 / 17 の CDD にも同じアンカーがあるが、本文を突き合わせたのは Android 16 だけ。

「Google Play からのインストールは対象外」とは、CDD は書いていない。Covered Applications の定義に入っているのは、非ストア経由のダウンロードと local file だけである。【公式から導出】

ユーザー向けの解除手順は Android Help。一部の手順は Android 13 以降でのみ動く、とある。Settings → Apps → 対象アプリ → More → Allow restricted settings。例として挙がっているのはアクセシビリティで、notification listener とは書いていない。

出典: [Learn about restricted settings](https://support.google.com/android/answer/12623953)。

文書になっている、設計に効く制限は次。

- low-RAM 端末の Android Q 以下では、notification listener は通知アクセスを得られず、システムに bind されない。work profile ではシステムが listener を無視する。work profile 発の通知を `DevicePolicyManager` が止め得る。出典: [NotificationListenerService](https://developer.android.com/reference/android/service/notification/NotificationListenerService)。
- Android 15: 信頼されていない `NotificationListenerService` は、OTP を検出した通知の本文を redact されていない形では読めない。companion device manager の関連付けのような信頼されたアプリは除外。出典: [Behavior changes: all apps（Android 15）](https://developer.android.com/about/versions/15/behavior-changes-all)。これは通知本文の話で、`MediaSession` のメタデータが redact されるとは書いていない。Scrova は通知本文を読まない。
- Android 17（API 37）: オーディオフレームワークが、バックグラウンドでの再生・audio focus・音量 API を制限する。target 37 のアプリがバックグラウンドでオーディオに触れるには、foreground service が要り、さらに while-in-use 能力か、exact alarm 権限かつ `USAGE_ALARM` が要る、とある。出典: [Behavior changes: Apps targeting Android 17](https://developer.android.com/about/versions/17/behavior-changes-17)。この節は自アプリのオーディオ操作の話で、他アプリの `MediaSession` を読むことには触れていない。監視への影響は【未確認】。

## 2. 位置とメタデータ

### 公式が約束している単位

`PlaybackState`（API 21）は状態、再生位置、操作能力を持つ。

| API | 公式の文 | 単位 |
|---|---|---|
| `getPosition()` | 「Get the current playback position in ms.」 | ms。外挿式は無い |
| `getLastPositionUpdateTime()` | elapsed real time。位置を一度もセットしていなければ 0（原文末尾はセミコロン） | `SystemClock.elapsedRealtime()` と同じ時計 |
| `getPlaybackSpeed()` | 通常再生の倍数。巻き戻しは負。1 が通常、0 が一時停止 | 無次元 |
| `PLAYBACK_POSITION_UNKNOWN` | 位置が不明なときの値。定数は -1 | — |

`PlaybackState.Builder.setState(int, long, float)` は、更新時刻を現在の `SystemClock.elapsedRealtime()` にする、と書く。4 引数版は、未知位置でなければ位置を更新した時刻を渡す必要があり、直前に位置を取ったなら `elapsedRealtime()` でよい、と書く。速度は通常 1.0、一時停止は 0、巻き戻しは負。

出典: [PlaybackState](https://developer.android.com/reference/android/media/session/PlaybackState)、[PlaybackState.Builder](https://developer.android.com/reference/android/media/session/PlaybackState.Builder)（いずれも最終更新 2026-08-03）。AOSP の `getPosition()` は `mPosition` を返すだけで、呼び出し時点への外挿はしていない（[PlaybackState.java](https://android.googlesource.com/platform/frameworks/base/+/refs/heads/main/media/java/android/media/session/PlaybackState.java)）。

### 観測時刻の位置（公式の式ではない）

`getPosition()` は最終更新時点の位置である。`observed_at` 時点の位置は、上の単位から次で求める。【公式から導出】。現行 javadoc にこの式は無い。

- `getPosition() == PLAYBACK_POSITION_UNKNOWN`（-1）、または `getLastPositionUpdateTime() == 0`（一度もセットされていない）なら、位置は `None`。
- `getPlaybackSpeed() == 0` なら、保存されている位置をそのまま使う。一時停止中に時計で進めない。
- それ以外は `position + speed * (SystemClock.elapsedRealtime() - getLastPositionUpdateTime())`。引き算は wall clock（`System.currentTimeMillis()`）ではない。`getLastPositionUpdateTime()` は elapsed realtime だから。
- 負の速度（巻き戻し）はこの式に含まれる。状態が `STATE_PLAYING` でないときに速度が非ゼロ、という組み合わせを各アプリがどう出すかは【未確認】。
- 曲長で clamp するかは API の要求ではない。エンジンは位置の後退を経過時間として加算するので（`docs/architecture.md` §2.2）、clamp しない。

### 状態の対応

すべて API 21。出典は [PlaybackState](https://developer.android.com/reference/android/media/session/PlaybackState)。

| 定数 | 値 | 公式の文 | `PlaybackEvent.state` |
|---|---|---|---|
| `STATE_NONE` | 0 | 既定。まだメディアが無い、またはリセットされて再生するものが無い | `Stopped` |
| `STATE_STOPPED` | 1 | この項目は停止している | `Stopped` |
| `STATE_PAUSED` | 2 | この項目は一時停止している | `Paused` |
| `STATE_PLAYING` | 3 | この項目は再生している | `Playing` |
| `STATE_BUFFERING` | 6 | バッファ中で、十分なデータが溜まれば再生を始める | `Playing` にしない |

`STATE_FAST_FORWARDING` / `STATE_REWINDING` / `STATE_CONNECTING` / `STATE_SKIPPING_TO_*` / `STATE_ERROR` を `Playing` として聴取時間に入れてよいかは、公式が scrobble 向けに定義していない。【未確認】。方針では `Playing` にしない（§9）。バッファ中もアプリが `STATE_PLAYING` のままにするかはアプリ次第で、公式の保証は無い。【未確認】

### メタデータ

`MediaMetadata`（API 21）。出典: [MediaMetadata](https://developer.android.com/reference/android/media/MediaMetadata)。

| キー | 定数値 | 公式の文 |
|---|---|---|
| `METADATA_KEY_TITLE` | `android.media.metadata.TITLE` | 曲のタイトル |
| `METADATA_KEY_ARTIST` | `android.media.metadata.ARTIST` | アーティスト |
| `METADATA_KEY_ALBUM` | `android.media.metadata.ALBUM` | アルバム名 |
| `METADATA_KEY_ALBUM_ARTIST` | `android.media.metadata.ALBUM_ARTIST` | 元ソースのアルバムのアーティスト |
| `METADATA_KEY_DURATION` | `android.media.metadata.DURATION` | 長さ。単位は ms。負は不明（または無限） |
| `METADATA_KEY_MEDIA_ID` | `android.media.metadata.MEDIA_ID` | コンテンツを識別する文字列。提供サービスの固有値。使うなら、基になるコンテンツの永続的で一意なキーであるべき |

`METADATA_KEY_DISPLAY_TITLE` は表示用で、多くの形式では `TITLE` と同じだが違い得る。表示するならこちらを優先、とある。scrobble の曲名は `TITLE` を正にする。表示用タイトルを曲名にすると、ラジオの「今の番組」のような文字列を送り得る。どちらが埋まるかはアプリ次第。【未確認】

`MEDIA_ID` は「should be a persistent unique key」であって、全アプリが埋める保証でも、アプリをまたいで一意である保証でもない。無いときは `source_track_id` を空にし、エンジンの曲名比較に落とす（`docs/architecture.md` §2.1）。

アートはエンジンに渡さない。キーだけ記録する。`METADATA_KEY_ART` / `METADATA_KEY_ALBUM_ART` は `Bitmap`。大きいとシステムが縮小し得る。高解像度は `METADATA_KEY_ART_URI` / `METADATA_KEY_ALBUM_ART_URI`。出典: 上の MediaMetadata。

Spotify、YouTube Music、Android 版 Apple Music が曲長・位置・アルバムアーティスト・`MEDIA_ID`・速度を欠くか、は公式資料に無い。【未確認】

## 3. プロセス寿命、Doze、foreground service

### プロセスは、システムが bind しているあいだ上がる

`NotificationListenerService` のクラス javadoc に、「bind 中はプロセスが生き続ける」「kill されたらシステムが必ず再 bind する」とは無い。`requestUnbind()` のあと、システムはサービスを殺し得る（§1）。

メモリガイドは、システムの bind の目的をコールドスタート回避と書いている。通知は 1 日に何百もあり、bind を外すとプロセスは cached に落ち、LMK に殺され得る。次の通知でまたコールドスタートするより、bind したままにする方が CPU とバッテリーを食わない、という説明である。出典: [Service bindings and process states](https://developer.android.com/topic/performance/memory/guide/service-bindings)（最終更新 2026-10-01）。

Android 8.0 のバックグラウンド実行制限では、別のフォアグラウンドアプリが notification listener に bind しているアプリは、サービス起動の制限上フォアグラウンドとみなす、と書く。出典: [Background Execution Limits](https://developer.android.com/about/versions/oreo/background)（最終更新 2026-03-03）。この文は「別のフォアグラウンドアプリ」の bind で、システムサーバーの bind そのものとは書いていない。システムの bind がこの条項に入るかは【未確認】。入らなくても、上のメモリガイドはシステムの bind が cached 落ちを防ぐ、と書いている。

同じガイドは、常時 bind されるサービスを重い UI と同じプロセスに置くと、UI を閉じても `CACHED` に落ちず、ビットマップが特権的な OOM スコアのまま残る、と書く。リスナーを別プロセス（`android:process`）に分けるのが、その節の回避策の一つ。Scrova の UI が重くなったときの話で、最初の実装で必須とは読んでいない。

既定 autobind が true なので、ユーザーが有効にしていればシステムが bind しようとする、と読むのは `META_DATA_DEFAULT_AUTOBIND` からの導出。force-stop のあと再 bind するかは、取得したページに無い。【未確認】

Doze と App Standby のガイドは、未使用時にバックグラウンドの CPU とネットワークを遅延させる、と書く。Doze 中はネットワークを止め、ジョブ・同期・通常のアラームを遅延する。`JobScheduler` は走らない。WorkManager は内部で `JobScheduler` を使うので、その仕事も走らない。出典: [Optimize for Doze and App Standby](https://developer.android.com/training/monitoring-device-state/doze-standby)。このページに `NotificationListenerService` も `MediaController.Callback` も無い。Doze 中にセッションのコールバックが届くかは【未確認】。

したがって、ライブ監視を WorkManager の定期実行で置き換えない。定期実行は Doze で不正確になる（§7）。idle 回避だけを理由に foreground service を起動しない。Doze ページは「アプリが idle と判定されないようにするためだけに foreground service を起動するな」と書く。

### foreground service は、文書上は要らない

Android 14（API 34）以降、foreground service には型の宣言と、その型の権限が要る。目的に合う API があるなら、そちらの方がよい、とガイドは書く。出典: [Foreground service types](https://developer.android.com/develop/background-work/services/fgs/service-types)（最終更新 2026-10-23）。

監視に近い名前の型は、説明が合わない。

| 型 | 公式の説明 | 監視への適合 |
|---|---|---|
| `mediaPlayback` | バックグラウンドでオーディオまたはビデオの再生を続ける。Android TV の DVR を含む | 自アプリの再生。他アプリの `MediaSession` を読む用途とは書いていない |
| `dataSync` | アップロード、ダウンロード、バックアップ、インポート、ローカルファイル処理、クラウドとの転送。target 15 以上では、バックグラウンドで 24 時間あたり合計 6 時間まで | 継続的な再生監視ではない。6 時間制限もある |
| `specialUse` | 他の型に入らない正当な用途。マニフェストの `PROPERTY_SPECIAL_USE_FGS_SUBTYPE` に自由文の説明を書き、Play Console の審査で見られる | 文書化された「リスナー用」の型ではない |

`dataSync` と `mediaProcessing` の 6 時間制限は [Restrictions on background starts](https://developer.android.com/develop/background-work/services/fgs/timeout)（最終更新 2026-10-16）。同じページに `mediaPlayback` の 6 時間制限は無い。target 15 以上では、`mediaPlayback` も `dataSync` も `BOOT_COMPLETED` から起動できない。出典: [Foreground service types](https://developer.android.com/develop/background-work/services/fgs/service-types)。

target が Android 14 以上で foreground service を使うなら、Play Console の App content で型を宣言する。Play が許すのは、ユーザーに益がありコア機能に関係し、ユーザーが開始するか知覚でき、ユーザーが止められ、システムが遅延すると体験が壊れる仕事。`TYPE_MEDIA_PLAYBACK` の例は、ユーザーが開始したオーディオまたはビデオ再生の継続。アプリを開いただけで鳴る自動 BGM は該当しない。出典: [Understanding foreground service and full-screen intent requirements](https://support.google.com/googleplay/android-developer/answer/16559646)、[Declare foreground service types](https://support.google.com/googleplay/android-developer/answer/13392821)。

公式ドキュメントは、他アプリの `MediaSession` を読むために foreground service が要るとは書いていない。`mediaPlayback` を「常駐のため」に宣言するのは、説明と Play の例のどちらとも一致しない。要否は実機で、システムの bind が切れたあとにプロセスが死ぬかを見てから決める（§9）。

## 4. Google Play のポリシー

確認した公式ページ:

- [Permissions and APIs that Access Sensitive Information](https://support.google.com/googleplay/android-developer/answer/16558241)（旧 URL `answer/9888170` はここへリダイレクトする。見出しに Notification Listener は無い）
- [Declare permissions for your app](https://support.google.com/googleplay/android-developer/answer/9214102)
- [User Data](https://support.google.com/googleplay/android-developer/answer/10144311)
- [Use of the AccessibilityService API](https://support.google.com/googleplay/android-developer/answer/10964491)
- [play.google.com の Permissions ポリシー](https://play.google.com/about/privacy-security-deception/permissions/)（2026-10-25 に取得。本文に notification listener は無かった）
- [Target API level requirements](https://support.google.com/googleplay/android-developer/answer/11926878)

`BIND_NOTIFICATION_LISTENER_SERVICE` の protection level は signature。「only the system can bind to it」。出典: [Manifest.permission.BIND_NOTIFICATION_LISTENER_SERVICE](https://developer.android.com/reference/android/Manifest.permission#BIND_NOTIFICATION_LISTENER_SERVICE)。ポリシー本文の Restricted Permissions 一覧（SMS、Call Log、Location、Accessibility、Exact Alarm など）には、この権限も notification listener も出てこない。

`AccessibilityService` には、API 31 以上を target するアプリ向けの宣言が 2021-11-03 からある。アクセシビリティツールでなくても禁止はしないが、目立つ開示が要る。Permissions Declaration Form の例は SMS と Call Log で、notification listener は例に無い。出典: [Use of the AccessibilityService API](https://support.google.com/googleplay/android-developer/answer/10964491)、[Declare permissions](https://support.google.com/googleplay/android-developer/answer/9214102)。

notification listener に相当する「この用途なら受理」というページは、上の本文取得では見つからなかった。

したがって次は【未確認】。

- 音楽 scrobbler が notification listener の受理される用途か。
- 提出時に Permissions Declaration Form が出るか。
- 「Device & app notifications」のような宣言項目が現行 Console にあるか。

ポリシー本文に無い、と書いてあるコミュニティ投稿は公式ではない。設計の根拠にしない。

文書になっている、周辺の義務は次。

- 個人データや機密データは安全に扱う。送信は現代的な暗号（例: HTTPS）。出典: [User Data](https://support.google.com/googleplay/android-developer/answer/10144311)。再生情報を Last.fm に送るなら、このポリシーの対象になり得る。開示文の要否は、NLS が「sensitive permission」一覧に入っているかを公式ページで確認できなかったので【未確認】。アプリ内では、通知アクセスが通知本文も見える権限であることを、設定画面へ送る前に説明する（§9）。実装が通知本文を読まないことと、権限の幅は別。
- foreground service を使うなら型の宣言（§3）。使わないなら、この宣言は出ない、と読む。使わない判断が審査で問題になるかは【未確認】。

target API は §8。

## 5. Session Key の保存

Android Keystore は暗号鍵をコンテナに入れ、端末からの抽出を難しくする。鍵材料はアプリプロセスからも端末からも抜き出せない。鍵は生成またはインポート時に用途を指定し、あとから変えられない。プロバイダ名は `AndroidKeyStore`。出典: [Android Keystore system](https://developer.android.com/privacy-and-security/keystore)（最終更新 2026-10-23）。

Keystore が守るのは鍵であって、Last.fm の session key そのものではない。session key は鍵で暗号化してからファイルに書く。

`EncryptedSharedPreferences`、`EncryptedFile`、`MasterKey` は使わない。

- `androidx.security:security-crypto:1.1.0` と `security-crypto-ktx:1.1.0` は 2025-07-30 に出た。1.1.0-alpha07（2025-04-09）と 1.1.0-beta01（2025-06-04）のリリースノートは「Deprecated all APIs in favour of existing platform APIs and direct use of Android Keystore.」と書く。出典: [Security release notes](https://developer.android.com/jetpack/androidx/releases/security)。
- 現行 API リファレンスはクラスを deprecated としている。`EncryptedSharedPreferences` の代替文は `SharedPreferences`。`EncryptedFile` は `java.io.File`。`MasterKey` は `KeyGenerator` と `AndroidKeyStore`。出典: [androidx.security.crypto](https://developer.android.com/reference/androidx/security/crypto/package-summary)。

代替文の `SharedPreferences` は暗号化しない。session key を平文の `SharedPreferences` に置く読み方はしない。

Jetpack が今出している暗号化は DataStore 側にある。`androidx.datastore:datastore-*:1.3.0`（2026-10-21）が `androidx.datastore:datastore-tink` を追加し、`AeadSerializer` で Tink の AEAD を使って透過的に暗号化・復号する、と書く。出典: [DataStore release notes](https://developer.android.com/jetpack/androidx/releases/datastore)、[AeadSerializer](https://developer.android.com/reference/androidx/datastore/tink/AeadSerializer)。

方針: session key は DataStore + `AeadSerializer`。AEAD の鍵は Android Keystore に置く。起動時に読んで Rust に渡し、Rust には保存しない（`docs/architecture.md` §4.2）。ログに出さない。shared secret はバイナリに入る前提で、session key とは別（`docs/research/lastfm.md`）。

`AeadSerializer` のコンストラクタが Android Keystore の鍵を自分で生成するのか、呼び出し側が `Aead` を渡すのかは、リリースノートの要約には手順が無い。実装時は [AeadSerializer](https://developer.android.com/reference/androidx/datastore/tink/AeadSerializer) のシグネチャに従う。鍵の生成手順をこの文書では推測しない。

## 6. 認証ページを開く

Last.fm は Desktop フロー（`auth.getToken` → ブラウザ承認 → `auth.getSession`）。token は 60 分・1 回限り。承認前の `auth.getSession` はエラーになるので、閉じたあとにポーリングする（`docs/architecture.md` §4.1、`docs/research/lastfm.md`）。redirect URI は要らない。

Custom Tabs は、ユーザーの既定ブラウザのエンジンで、アプリ内にブラウザ体験を足す。自前コンテンツだけを出すなら WebView、外の URL へ送るなら Custom Tabs、と概要は書く。Cookie はブラウザと共有されるので、Last.fm にログイン済みなら再ログインを求めにくい。戻るボタンでアプリに戻れる。Custom Tab の使用中、起動したアプリはシステムに追い出されず、重要度はフォアグラウンド相当に上がる、と概要は書く。

出典: [Overview of Android Custom Tabs](https://developer.android.com/develop/ui/views/layout/webapps/overview-of-android-custom-tabs)（最終更新 2026-02-09）。起動は Intent で、属性は [CustomTabsIntent](https://developer.android.com/reference/androidx/browser/customtabs/CustomTabsIntent)。

古い URL `https://developer.android.com/develop/ui/views/layout/webapps/custom-tabs` は 2026-10-25 時点で 404。使わない。

対応ブラウザが無いかは `CustomTabsClient.getPackageName`。既定の VIEW ハンドラが Custom Tabs をサポートしていればそれを返す。出典: [CustomTabsClient](https://developer.android.com/reference/androidx/browser/customtabs/CustomTabsClient)。null のときの公式のフォールバック手順は、概要ページには無い。通常のブラウザ Intent に落とすのは実装判断で、公式の必須手順としては【未確認】。

ライブラリは `androidx.browser:browser:1.10.0`（2026-03-25）。出典: [Browser release notes](https://developer.android.com/jetpack/androidx/releases/browser)。

## 7. バックグラウンド送信

WorkManager は、遅延してよい確実な仕事向け。出典: [WorkManager](https://developer.android.com/reference/androidx/work/WorkManager)、[WorkManager releases](https://developer.android.com/jetpack/androidx/releases/work)（安定版 2.12.0、2026-09-23）。

`PeriodicWorkRequest` の間隔の下限は 15 分（`MIN_PERIODIC_INTERVAL_MILLIS`）。意図は、連続実行のあいだにかなり一定の遅延が欲しく、Doze やバッテリー最適化による不正確さを受け入れる用途。制約があると、間隔を過ぎても制約が満たされるまで実行されない。出典: [PeriodicWorkRequest](https://developer.android.com/reference/androidx/work/PeriodicWorkRequest)（最終更新 2026-06-24）。

ネットワーク制約は `Constraints.Builder.setRequiredNetworkType`。`NetworkType.CONNECTED` は「何らかの使えるネットワーク」。出典: [NetworkType](https://developer.android.com/reference/androidx/work/NetworkType)。例は [Firebase JobDispatcher からの移行](https://developer.android.com/develop/background-work/background-tasks/persistent/migrate-from-legacy/firebase)。

ライブの再生監視には使わない。キューに溜まった scrobble を送るときに `OneTimeWorkRequest` + `NetworkType.CONNECTED` を使う。15 分の定期実行は、取りこぼした送信の掃除に限る。Now Playing の失敗は再送しない（`docs/research/lastfm.md`）。再送すべき API エラーは公式には 11 と 16 だけ、という Last.fm 側の規則は変えない。

Doze がジョブを遅延することは §3。メンテナンス窓まで送信が遅れることは公式にあり得る。scrobble の `timestamp` は送信時刻ではなく再生開始時刻なので（`docs/research/lastfm.md`）、遅延そのものは API 違反ではない。

## 8. 現行の安定版（2026-10-25）

| もの | 値 | 出典 |
|---|---|---|
| Android Gradle Plugin | 9.4.0。9.5 と 9.6 は preview。Gradle の下限は 9.6.0。対応する最大 API は 37 | [About Android Gradle plugin](https://developer.android.com/build/releases/about-agp)（最終更新 2026-10-22）。リリースノートは [AGP 9.4.0](https://developer.android.com/build/releases/agp-9-4-0-release-notes) |
| API 37 に必要な AGP の下限 | 9.1.1（Android Studio Panda 3 Patch 1） | 上の about-agp の「Minimum versions of tools for Android API level」 |
| Kotlin | 2.4.21（2026-10-08、bug fix）。2.4 の言語リリースは 2026-06-03。次の安定予定は 2.5.0（2026-12） | [Kotlin release process](https://kotlinlang.org/docs/releases.html) |
| Compose BOM | セットアップページの現行例は `androidx.compose:compose-bom:2026.09.00`。「Always use the latest」 | [Compose BOM](https://developer.android.com/develop/ui/compose/bom) |
| Play の target | 2026-08-31 以降、新規アプリと更新は Android 16（API 36）以上。既存アプリが新しい OS の新規ユーザーに残るには API 35 以上。延長は 2026-11-01 まで申請可。Wear OS / Automotive は API 35、TV / XR は API 34 | [Target API level requirements](https://support.google.com/googleplay/android-developer/answer/11926878) |
| `androidx.browser:browser` | 1.10.0（2026-03-25） | [Browser releases](https://developer.android.com/jetpack/androidx/releases/browser) |
| `androidx.work:work-*` | 2.12.0（2026-09-23） | [WorkManager releases](https://developer.android.com/jetpack/androidx/releases/work) |
| `androidx.datastore` | 1.3.0（2026-10-21）。`datastore-tink` はこの版で追加 | [DataStore releases](https://developer.android.com/jetpack/androidx/releases/datastore) |
| JNA（UniFFI Kotlin） | 公式ユーザーガイドは「JNA 5.12.0 以上が必要」で、例は `net.java.dev.jna:jna:5.12.0@aar` | [Integrating with Gradle](https://mozilla.github.io/uniffi-rs/latest/kotlin/gradle.html) |

about-agp のサンプルは Kotlin `2.4.10` を AGP 9.4.0 と並べている。それはサンプルであって、Kotlin の最新安定版ではない。最新安定は 2.4.21。AGP 9.0 は Kotlin Gradle plugin 2.2.10 に実行時依存する、と 9.0 のリリースノートは書く（[AGP 9.0.1](https://developer.android.com/build/releases/agp-9-0-0-release-notes)）。2.4.21 との組み合わせを 9.4 のリリースノートは「この組が検証済み」とは書いていない。ビルドが通るかは実装時に確認する。【未確認】

`compileSdk` の必須値を Play は指定していない。指定があるのは target API。AGP 9.4 は API 37 までビルドできる。Android 17 の API を使わないなら `compileSdk` 36 / `targetSdk` 36 で Play の現行要件に合う。API 37 の動作変更（§1 のバックグラウンドオーディオ）を target で受ける必要は、監視の公式記述が無いので今は無い。

JNA の「現行ピン」を UniFFI のリリースノートは 5.12.0 より新しい番号では示していない。5.12.0 以上、という下限だけが公式。それより新しい特定バージョンをこの文書では指定しない。

リポジトリの UniFFI は 0.32（`docs/architecture.md` §5.1）。Android は同じ `scrova-ffi` を `cdylib` にして Kotlin バインディングを生成する（同 §5.3）。JNA はその Kotlin バインディングがネイティブメソッドを呼ぶための依存で、監視 API とは独立。

## 9. Scrova の方針

1. 監視は `NotificationListenerService` + `MediaSessionManager`。通知本文は読まない。`MEDIA_CONTENT_CONTROL` は宣言しない。
2. マニフェストは公式例に合わせ、`exported="false"`、`BIND_NOTIFICATION_LISTENER_SERVICE`、`SERVICE_INTERFACE` の intent-filter。filter 用 meta-data は付けない。autobind は既定（true）のまま。
3. 設定へ送る前に、この権限は通知本文も見える、とアプリ内で説明する。判定は `isNotificationListenerAccessGranted`。画面は `ACTION_NOTIFICATION_LISTENER_DETAIL_SETTINGS`（API 30 の extra 付き）。Activity が無ければ `ACTION_NOTIFICATION_LISTENER_SETTINGS`。
4. `onListenerConnected` で `getActiveSessions(自分の ComponentName)` を取り、各 `MediaController` に `Callback` を登録する。一覧の変化は `addOnActiveSessionsChangedListener`。`onSessionDestroyed` でそのコントローラを捨てる。
5. `source` は `getPackageName()`。状態は `STATE_PLAYING` → `Playing`、`STATE_PAUSED` → `Paused`、`STATE_NONE` / `STATE_STOPPED` → `Stopped`。それ以外は `Playing` にしない。位置は §2 の外挿。不明なら `None`。曲名は `TITLE`、`MEDIA_ID` が無い `source_track_id` は空。
6. foreground service は最初から起動しない。`mediaPlayback` は使わない。実機でプロセスが死ぬと分かってから、型を選び直す。
7. Session Key は DataStore + `AeadSerializer` + Android Keystore。`EncryptedSharedPreferences` は使わない。Rust には起動時に渡すだけ。
8. 承認ページは Custom Tabs（`androidx.browser:browser:1.10.0`）。redirect は作らない。閉じたあとに `auth.getSession` を試す。
9. 送信は `OneTimeWorkRequest` + `NetworkType.CONNECTED`（WorkManager 2.12.0）。15 分の定期実行は掃除だけ。
10. ビルドは AGP 9.4.0、Kotlin 2.4.21、Compose BOM `2026.09.00`、`compileSdk` 36、`targetSdk` 36。JNA は UniFFI ガイドの下限 5.12.0 以上。

### 実機で確認すること

公式資料では埋まらない。

- sideload（`PACKAGE_SOURCE_LOCAL_FILE`）と、ブラウザ経由のインストールで、通知アクセスが AppInfo の Allow restricted settings の後ろに隠れるか。Play からのインストールでは隠れないか。CDD は後者を Covered Applications に入れていないが、実機の installer source は見ていない。
- タスク一覧からスワイプしたあと、force-stop のあと、再起動のあと、Doze 中に、`onListenerConnected` と `MediaController.Callback` が来るか。来ないなら、どの型の foreground service が審査説明に耐えるか。
- Spotify、YouTube Music、Android 版 Apple Music が `TITLE` / `ARTIST` / `ALBUM` / `ALBUM_ARTIST` / `DURATION` / `MEDIA_ID` / `getPosition` / `getPlaybackSpeed` を埋めるか。`DURATION` が負、位置が -1、速度が 0 のまま `STATE_PLAYING`、ということはあるか。
- 同時に複数セッションがあるとき、index 0 が聞こえているセッションか。全部見る必要があるか。
- シーク、一時停止、曲送りで、外挿した位置がエンジンの加算（位置の差と経過時間の min）と矛盾しないか。
- Android 15 で、メディアのメタデータが OTP redact の影響を受けないこと。通知本文は読まないので、本文が redact されること自体は問題にしない。
- Android 17 端末で、他アプリの `MediaSession` 読み取りがバックグラウンドオーディオ制限の影響を受けるか。
- Play に出す前に、現行 Console が notification listener の宣言フォームを出すか。この文書の調査時点では公式本文が無い。
