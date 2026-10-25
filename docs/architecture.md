# Scrova アーキテクチャ

最終更新: 2026-10-25

調査の一次資料は `docs/research/` にある（Last.fm API、macOS Music.app）。
本書はそれらを踏まえて決めた設計と、その理由を記録する。

## 1. 全体像

```text
┌────────────── Native (Swift / Kotlin / C#) ──────────────┐
│  Playback Observer  ─┐                                    │
│  (Music.app, MPMusicPlayerController, MediaSession, GSMTC)│
│                      │ PlaybackEvent                      │
│  Keychain / Keystore │      ▲ EngineOutput                │
│  UI / Lifecycle      ▼      │                             │
├──────────────── FFI (UniFFI / 将来 C ABI) ────────────────┤
│  scrova-ffi   : Scrova オブジェクト（同期・所有権・runtime）│
├──────────────────────── Rust ─────────────────────────────┤
│  scrova-core    : PlaybackEvent → NowPlaying / Scrobble   │
│  scrova-storage : SQLite 送信キュー・履歴                   │
│  scrova-lastfm  : Last.fm REST クライアント                │
└───────────────────────────────────────────────────────────┘
```

| クレート | 責務 | 依存してよいもの |
|---|---|---|
| `scrova-core` | ドメインモデル、Scrobble 判定エンジン | なし（`uniffi` は feature で任意） |
| `scrova-storage` | 送信キュー・状態遷移・履歴の永続化 | `scrova-core`, `rusqlite` |
| `scrova-lastfm` | 署名、認証、`track.*` / `user.getRecentTracks` | `reqwest`, `serde` |
| `scrova-ffi` | 上記を束ねた `Scrova` オブジェクトを UniFFI で公開 | すべて |
| `uniffi-bindgen-swift` | Swift バインディング生成 CLI（ビルド専用） | `uniffi[cli]` |

原則:

- `scrova-core` は I/O を持たず、OS API にも依存しない。時計も読まない。
- OS 固有処理（再生監視、権限、セキュアストレージ、バックグラウンド実行）はネイティブ側。
- `scrova-lastfm` は `scrova-core` に依存しない。将来 ListenBrainz 用クレートを並べても core が太らない。

## 2. Scrobble エンジン（`scrova-core`）

### 2.1 入力: `PlaybackEvent`

ネイティブの監視層が「その時点のプレイヤーのスナップショット」を送る。

| フィールド | 意味 |
|---|---|
| `track: Option<Track>` | 何もロードされていなければ `None` |
| `state` | `Playing` / `Paused` / `Stopped` |
| `position: Option<Duration>` | `observed_at` 時点の再生位置（取れなければ `None`） |
| `source` | 再生アプリの識別子（bundle ID など） |
| `observed_at: SystemTime` | 観測時刻 |

`Track` は曲名・アーティスト・アルバム・アルバムアーティスト・曲長・`source_track_id`（Music.app の persistent ID など）。

イベントの頻度は問わない。通知駆動でも、数秒おきのポーリングでも、その両方でもよい。
エンジンは `observed_at` と `position` の差分だけから再生時間を計算するため、
OS ごとの通知頻度の違いを吸収できる。

再生セッション ID は OS から取れないことが多いので、エンジンが UUID v4 で発行する。

### 2.2 再生時間の加算規則

直前の観測が `Playing` だった区間についてのみ加算する。

| 状況 | 加算量 | 理由 |
|---|---|---|
| 位置が前進（通常再生・前方シーク） | `min(位置の差, 経過時間)` | 前方シークで飛ばした分は経過時間を超えるので自然に除外される |
| 位置が後退（後方シーク） | 経過時間 | その間ユーザーは再生状態だった。巻き戻して聴いた分も聴取時間 |
| 位置が経過時間より遅い | 位置の差 | バッファリング停止や通知されなかった一時停止 |
| 位置が取れない | 経過時間 | 唯一の手がかり |
| 曲が変わった | 上記に加え `曲長 − 直前位置` で頭打ち | 前の曲は自分の終端を越えて再生されない |
| 常に | 曲長で頭打ち | スリープ中に pause イベントが来なかった場合の過大計上を防ぐ |

`Paused` 中の区間は加算しない。`observed_at` が逆行したイベントは捨てる（新しい観測の方が正しい）。

### 2.3 セッションの区切り

次のいずれかで現在のセッションを閉じ、必要なら新しいセッションを始める。

- 曲が変わった（`source_track_id` が両方あればそれで比較。なければ曲名・アーティスト・アルバム・曲長）
- `Stopped` になった、または `track` が `None`
- リピート: 位置が `repeat_start_window`（既定 10 秒）以内まで戻り、かつ
  「すでに Scrobble 済み」または「直前位置が終端から `repeat_end_window`（既定 10 秒）以内」

曲名・アーティストだけの一致では同一曲とみなさない（ID が違えば別曲）。
閾値未達で曲の途中から頭に戻した場合はシーク扱いで同じセッションを継続する。

位置が取れないプレイヤーではリピートを検出できない。その場合ネイティブ側が
曲の再開時に `Stopped` を挟むことで区切れる。

### 2.4 Scrobble 判定

Last.fm の規則（`docs/research/lastfm.md` §1）に従う。

- 曲長が 30 秒を**超える**こと
- 加算した再生時間が `min(曲長 / 2, 4 分)` に達したこと
- 曲長不明なら 4 分（4 分聴けば 30 秒超も証明される）

閾値に達した**その時点で**一度だけ `EngineOutput::Scrobble` を出す。曲の終了を待たないのは、
アプリの強制終了やスリープで「終了イベント」を取り逃しても Scrobble を失わないため。

タイムスタンプは再生開始時刻。曲の途中から観測を始めた場合は `observed_at − position` で推定する
（その間連続再生していたと仮定するのが最善の推定）。同じ仮定で、開始時点の再生位置までを聴取済みとして加算する。
iOS では Music で再生を始めてからアプリを開くのが普通で、観測区間だけを数えると多くの曲が閾値に届かないため。

アプリを再起動すると、エンジンのセッションは消える。同じ再生に途中から合流すると再び閾値を超えるので、
同じ曲で開始時刻が ±30 秒以内の Scrobble が既にあればキューに入れない（`scrova-ffi`）。

### 2.5 出力: `EngineOutput`

- `NowPlaying { track, source }`: `Playing` でセッションが始まったとき、`Paused → Playing` で再開したとき
- `Scrobble { scrobble }`: `Scrobble` は `session_id`、`started_at`、`timestamp_accuracy`、`origin` を持つ

`timestamp_accuracy` は `Observed` / `Estimated`、`origin` は `LiveObservation` / `HistoryRecovery`。
iOS の履歴復旧（Apple Music API は再生日時を返さない）で作る Scrobble は `Estimated` とし、
UI と重複判定で「正確な履歴」と区別する。

## 3. 送信キュー（`scrova-storage`）

Last.fm の推奨（クライアント再起動を跨いでキャッシュ、古い順、50 件バッチ）に従う。

- Scrobble 本体と「宛先ごとの配送状態」を分ける。今は宛先 `lastfm` のみだが ListenBrainz を足せる
- `session_id` を UNIQUE にし、同じ Scrobble の二重登録を防ぐ（エンジン出力をネイティブ側が二度渡しても安全）
- 状態: `Pending` → `InFlight` → `Submitted` / `Ignored` / `Failed`、再試行時は `Pending` に戻して `next_attempt_at` を指数バックオフ
- 起動時に `InFlight` のまま残った項目を `Pending` に戻す。送信結果が不確定なので記録しておく

送信結果が不確定な再送で重複が起きるかは Last.fm 側の仕様が未確認（`lastfm.md` §10）。
現状は「失うよりは重複の可能性を許容」とし、実測で確認する。

### 3.1 エラー時の扱い

`lastfm.md` §6 に従う。

| 結果 | 扱い |
|---|---|
| `ok` かつ `ignored = 0` | `Submitted` |
| `ok` で一部 `ignoredMessage` | 該当分のみ `Ignored(code)`。再送しない |
| 11 / 16 / 8 / 通信失敗 | `Pending` に戻してバックオフ |
| 29（レート制限） | より長いバックオフ |
| 9（セッション無効） | 再認証を UI に要求。キューは保持 |
| 26（API キー停止） | 送信を止めて通知。キューは保持 |
| その他 | `Failed`（リクエスト不正。再送しても同じ） |

Now Playing は失敗しても再送しない（公式の指示）。

## 4. Last.fm 認証と秘密情報

### 4.1 認証フロー

- macOS / iOS とも **Desktop フロー**: `auth.getToken` → ブラウザで承認 → `auth.getSession`
- iOS は `ASWebAuthenticationSession` で承認ページを開き、閉じられたら `auth.getSession` を試す。
  未承認なら error 14 が返るので「もう一度」を出す
- Mobile フロー（パスワード入力）は採用しない

### 4.2 Session Key

- ネイティブ側が Keychain に保存し、起動時に Rust へ渡す。Rust 側は永続化しない
- Rust の型は `Debug` で値を伏せる。ログに出さない

### 4.3 API Key / Shared Secret

Last.fm には PKCE のような公開クライアント向けの仕組みがなく、独自サーバーを置かない限り
secret はバイナリに入る（`lastfm.md` §9）。方針:

- 配布ビルドには同梱する。ただし「抽出される前提」で扱い、守る対象は secret ではなく
  アプリ全体のレート上限と可用性（バッチ送信・バックオフ・29/26 の監視）
- リポジトリにはコミットしない。ビルド時に xcconfig / 環境変数で注入する
- 上級者向けに自分の API キーを設定できる経路を残す（ローテーション時の逃げ道にもなる）

## 5. FFI 境界

### 5.1 UniFFI（Swift / Kotlin）

- UniFFI 0.32、proc-macro 方式（UDL なし）
- 公開型は `scrova-core` 側で `#[cfg_attr(feature = "uniffi", derive(uniffi::Record))]` を付ける。
  FFI 用に型を二重定義しないため
- `scrova-ffi` は薄く保つ。振る舞いは core 側に置き、ffi はスレッド同期とオブジェクトの寿命だけを持つ
- 時刻は `SystemTime` / `Duration` をそのまま使う（Swift では `Date` / `TimeInterval`）

### 5.2 Apple 向けビルド

`scripts/build-apple.sh` が以下を行う。

1. `scrova-ffi` を 4 ターゲット（macOS arm64/x86_64、iOS、iOS Simulator）で `staticlib` としてビルド
2. macOS の 2 アーキテクチャを `lipo` で結合
3. `uniffi-bindgen-swift`（library mode）で Swift ソース・ヘッダー・modulemap を生成
4. `xcodebuild -create-xcframework` で `ScrovaFFI.xcframework` を作成

これを `apps/apple/ScrovaKit`（Swift Package）が `binaryTarget` として取り込み、
生成 Swift を同パッケージのソースとしてコンパイルする。アプリは `ScrovaKit` に依存するだけでよい。

落とし穴（実際に踏んだもの）:

- 複数クレートが UniFFI 型を持つと、生成 Swift はクレートごとに `import <crate>FFI` するが、
  modulemap は 1 モジュールにまとめられる。各クレートの `uniffi.toml` で `ffi_module_name = "ScrovaFFI"`
  に揃え、ヘッダー名は `ffi_module_filename` で別々にする
- `--xcframework` オプションは `framework module` を出力するため、静的ライブラリ型の XCFramework には使わない
- Nix の `cc` ラッパーが PATH にあると、iOS ターゲットのリンクで macOS SDK と Nix の libiconv が混入する。
  `.cargo/config.toml` で Apple ターゲットのリンカと `CC_*` を `/usr/bin/clang` に固定している

### 5.3 将来の Android / Windows

- Android: 同じ `scrova-ffi` を `cdylib` として Kotlin バインディングを生成する（`crate-type` に含めてある）
- Windows: UniFFI に公式の C# バックエンドはない。候補は
  (a) サードパーティの `uniffi-bindgen-cs`、(b) `scrova-ffi` と並ぶ `scrova-capi` クレートで手書きの C ABI + P/Invoke。
  どちらを選ぶにしても、機能は core / storage / lastfm 側に置き、ffi 層を「唯一の実装場所」にしないこと
- 非同期 API（Last.fm 通信）を FFI に出すときは UniFFI の async（tokio runtime）を使う。
  C ABI 側ではコールバックかポーリングに落とす必要があるため、境界は「イベントを渡す / 状態を問い合わせる」
  という粗い粒度に保つ

## 6. データフロー（macOS の縦断実装）

1. `MusicObserver`（Swift）が `com.apple.Music.playerInfo` 通知と数秒おきのポーリングで
   Music.app の状態を Apple Events で読む（`docs/research/macos-music.md`）
2. `PlaybackEvent` を `Scrova.handlePlaybackEvent` に渡す
3. Rust がエンジンを回し、`Scrobble` をキューに積み、`NowPlaying` を送る
4. 送信ワーカーがキューから 50 件ずつ取り出して `track.scrobble`
5. 結果をキューの状態に反映し、UI は履歴とキュー件数を表示

## 7. 未決事項

- Last.fm が同一 `(artist, track, timestamp)` の再送を重複排除するか（実測が必要）
- iOS で systemMusicPlayer をどこまでバックグラウンドで読めるか（実機検証が必要）
- 送信ワーカーを Rust（tokio）で回すか、ネイティブのタイマーから駆動するか。
  iOS のバックグラウンド制約を考えると「ネイティブが `flush()` を呼ぶ」形が有力
