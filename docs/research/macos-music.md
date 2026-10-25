# macOS Music.app から再生情報を取る

Scrova（Rust 製 Last.fm scrobbler）の macOS 側で、Music.app の再生状態をどう取るかの調査。実装は書かない。主張は出典付き。Apple の公開ドキュメントに無いものは **非公式** と書く。

調査日: 2026-10-25。実機は macOS 27.0.1（`sw_vers` の BuildVersion は `26A434`）、Xcode 27.0（`27A266a`）。Music.app は `/System/Applications/Music.app`、`CFBundleShortVersionString` 1.7、`CFBundleVersion` 1.7.0。調査時点で Music は起動済みだった。

Apple の現行ドキュメントは JS 描画なので、本文は `https://developer.apple.com/tutorials/data/documentation/<path>.json` を取得して確認した。人間が読む URL は `https://developer.apple.com/documentation/...` を併記する。

## 結論（先に）

ライブ再生の正は Apple Event（Scripting Bridge か `NSAppleScript`）で、`current track` / `player state` / `player position` / `persistent ID` / `duration` を読む。Music.app は access group を公開しているので、sandbox するなら `com.apple.security.scripting-targets` が公式に推奨される経路。`com.apple.Music.playerInfo` は起動のきっかけには使えるが、スキーマは非公式で、再生位置を含まない。MediaRemote は使わない。未起動の Music に Apple Event を送ると起動するので、送る前に `NSRunningApplication` で確認する。

## 1. Distributed Notification `com.apple.Music.playerInfo`

### 公式ドキュメントは存在しない

`NSDistributedNotificationCenter`（Swift 名 `DistributedNotificationCenter`）の現行ドキュメントは、プロセス間に通知を配る仕組みだけを説明する。通知名 `com.apple.Music.playerInfo` も、その `userInfo` キーも載っていない。取得した JSON 本文に `playerInfo` は無かった。

- 仕組み: [DistributedNotificationCenter](https://developer.apple.com/documentation/foundation/distributednotificationcenter)（JSON: [distributednotificationcenter.json](https://developer.apple.com/tutorials/data/documentation/foundation/distributednotificationcenter.json)）
- 同じ JSON の entitlements 索引にも `playerInfo` は無い（[entitlements.json](https://developer.apple.com/tutorials/data/documentation/bundleresources/entitlements.json)）
- Music の scripting definition（次節）にもこの通知名は無い

公式に書いてある、設計に効く制約は次だけ。

- 投稿は高い。システム全体のサーバが、登録している全タスクへ配る。
- 投稿から他タスクへの到着までの遅延に上限は無い。
- 投稿が多すぎてサーバのキューが埋まると、通知は捨てられ得る。
- 配送はタスクの run loop 経由。run loop が回っていないと届かない。

出典は上の DistributedNotificationCenter JSON の Overview。

したがって、この通知を scrobble 判定の唯一の入力にしてはいけない。届かないことが公式にあり得る。

### 通知名は Music 1.7 のバイナリに残っている（非公式の確認）

`/System/Applications/Music.app/Contents/MacOS/Music` の文字列に、次が隣接して入っている（本機、Music 1.7）。

- `Player State`
- `Playing`
- `Library PersistentID`
- `com.apple.iTunes.playerInfo`
- `com.apple.iTunes.player`
- `com.apple.Music.playerInfo`

名前の変更元は iTunes の `com.apple.iTunes.playerInfo` で、Catalina で Music.app になってからも両方がバイナリに残っている。通知名の発見手順自体が非公式（`strings` でバンドル ID を探す）であることは、Catalina 当時の記事が書いている。

- 非公式: [Finding Distributed Notifications on macOS Catalina](https://medium.com/macoclock/finding-distributed-notifications-on-macos-catalina-b2a292aac5a1)（2020-04-13）。`com.apple.iTunes.playerInfo` の後継として `com.apple.Music.playerInfo` をバイナリ文字列から特定し、受信できた、と書いている。`Display Line 0` は Music 側で消えた、とも書いている。
- 非公式: [NSDistributedNotificationCenter no longer supports nil names](https://mjtsai.com/blog/2019/10/04/nsdistributednotificationcenter-no-longer-supports-nil-names/)（2019-10-04）。Catalina 以降、特権の無いプロセスが `name: nil` で全 distributed notification を購読できなくなった、という観察。現行の DistributedNotificationCenter JSON にはこの制限の説明は無い。名前を明示して購読する必要がある、と扱う。

本調査では再生を止めて live の `userInfo` を採取していない。上の隣接文字列は「投稿箇所の近くにこれらの文字列がある」ことまでで、辞書の全キーの証明ではない。`Total Time` や `Persistent ID` もバイナリ内にあるが、それらは iTunes Library の plist キーの塊（`Current Version` や `Compatible Version` の隣）にもあり、通知辞書のキーとは限らない。

### `userInfo` キー（非公式。iTunes 時代の実ダンプが根拠）

iTunes が投げていた `com.apple.iTunes.playerInfo` の実ダンプは複数ある。Music が同じ辞書を今も全キー埋めて出す、という公式保証は無い。Catalina の記事は「iTunes と少し違う」と書いている。macOS 27 での live 採取は未実施。キーは次を **非公式・継承** として扱う。

繰り返し観測されているキー（ローカルファイル再生の例）:

| キー | 観測された型と意味 | 注意 |
| --- | --- | --- |
| `Name` | 文字列。曲名 | |
| `Artist` | 文字列 | 空のストリームがある |
| `Album` | 文字列 | 欠けることがある |
| `Album Artist` | 文字列 | 無いダンプもある |
| `Total Time` | 数値。**ミリ秒** | AppleScript の `duration`（秒）と単位が違う |
| `Player State` | 文字列。観測例は `Playing` / `Paused` / `Stopped` | 早送り・巻き戻しの文字列は、下のダンプには出ていない |
| `PersistentID` | 64-bit ID の符号付き十進文字列。負になり得る | AppleScript の `persistent ID` は16進文字列。同じ値の別表現、という扱いは非公式 |
| `Location` | `file://` URL | ローカルファイルでは出る。Apple Music のストリームやクラウドのみの曲では欠ける、という報告が多い（非公式） |
| `Library PersistentID` | ライブラリの ID | 本機バイナリの通知名の隣にもこの文字列がある |
| `Playlist PersistentID` | プレイリスト ID | |
| `Store URL` | `itms://` URL | ストア曲。ストリームでは欠けることがある |
| `Genre` `Composer` `Year` | メタデータ | 欠けることがある |
| `Track Number` `Track Count` `Disc Number` `Disc Count` | 数値 | |
| `Play Count` `Skip Count` `Play Date` | 再生履歴 | 通知のたびに信頼できるとは限らない |
| `Rating` `Rating Computed` `Album Rating` `Album Rating Computed` | 0–100 と、計算値かどうか | |
| `Artwork Count` | 数値 | 画像そのものは入らない |

出典（いずれも非公式の実ダンプ）:

- [tternes/distnote](https://github.com/tternes/distnote) の `com.apple.iTunes.playerInfo` ログ（2014）
- [Stack Overflow: does itunes fire events…](https://stackoverflow.com/questions/24358098/does-itunes-fire-events-that-other-processes-on-the-os-can-listen-for)（2014）
- [Cory Wiles, Now Playing with Spotify and iTunes](https://blog.corywiles.com/now-playing-with-spotify-and-itunes)
- 追加キー（`Display Line 0`、`Back Button State`、`Like Status`、`elapsedStr`）を含む iTunes ダンプ: [いま聴いている曲の iTunes Store での trackId を取得する旅](https://blog.nishimu.land/entry/2016/04/19/090000)（2016）。Music では `Display Line 0` が消えた、という Catalina 記事と矛盾し得る。macOS 27 では未確認。

通知の `object` は iTunes 時代は `com.apple.iTunes.player`。本機バイナリにもその文字列はある。`com.apple.Music.player` は今回の隣接文字列には無かった。Music 通知の `object` は未確認。購読側は `object: nil` にする。

再生位置（elapsed）は、上の主要なダンプには無い。`elapsedStr` が出た例は 2016 年の iTunes ダンプ 1 件だけで、安定したキーとは扱えない。Last.fm の「曲の半分、または 4 分」判定には、この通知だけでは足りない。位置は Apple Event の `player position` で取る。

配信が止まった、という報告もある。非公式: [SketchyBar discussion #151](https://github.com/FelixKratz/SketchyBar/discussions/151)（2024-10、Sonoma で `com.apple.Music.playerInfo` が SketchyBar に届かなくなった、という利用者報告）。macOS 27 で第三者がまだ受け取れるかは未検証。バイナリに名前が残っていることと、第三者へ届くことは別。

sandbox が受信を禁じる、という現行ドキュメントは見つからなかった。受信は Apple Event の送信ではない。送信側の entitlement（次の第 3 節）とは別問題。

## 2. Apple Event / Scripting Bridge / `NSAppleScript` と Music.sdef

### 確認方法

本機で次を実行した。

```sh
sdef /System/Applications/Music.app
```

出力は 608 行。`/System/Applications/Music.app/Contents/Resources/com.apple.Music.sdef` とバイト一致（`cmp`）。`sdef` はバンドル内の sdef を出しており、この確認のために Music を新たに起動してはいない（起動済みだったが、結果はバンドルファイルと同一）。用語の正は、起動中のプロセスに問い合わせた結果ではなく、この sdef。

辞書の開き方（公式）: Script Editor の Library、またはアプリバンドル内の `.sdef`。[Cocoa Scripting の用語](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/ScriptableCocoaApplications/SApps_glossary/SAppsGlossary.html) は、sdef がコマンド・クラス・プロパティを記述すると説明している。

### スイートと access group

3 スイート: Standard Suite（`****`）、Music Suite（`hook`）、Internet suite（`gurl`）。

Music Suite が宣言している access group（sandbox の `scripting-targets` で指定する識別子）:

| identifier | 主な対象 |
| --- | --- |
| `*` | Standard Suite 全体。`version` プロパティにも付く |
| `com.apple.Music.playback` | 再生コマンドと、現在の曲・再生状態 |
| `com.apple.Music.library.read` | ライブラリ要素の読み取り |
| `com.apple.Music.library.read-write` | ライブラリの変更（`add`、`download`、`convert` など） |
| `com.apple.Music.user-interface` | `reveal`、`select`、ウィンドウ |

scrobbler が必要とするのは `playback` と、プロパティによっては `library.read`。`library.read-write` と `user-interface` は再生情報の読み取りには不要。

### `application` の、再生に効くプロパティ

sdef の `application`（code `capp`、cocoa class `ITNSApplication`）から抜粋。行番号は本機の sdef。

| プロパティ | code | 型 | スクリプト上の access | access group | 説明（sdef 原文の要旨） |
| --- | --- | --- | --- | --- | --- |
| `current track` | `pTrk` | `track` | `r` | `com.apple.Music.playback` / `r` | the current targeted track |
| `player position` | `pPos` | `real` | 属性なし（スクリプトからは読み書き） | `com.apple.Music.playback` / `rw` | 現在の曲の中の位置。**秒** |
| `player state` | `pPlS` | `ePlS` | `r` | `com.apple.Music.playback` / `rw` | stopped / paused / playing か |
| `current stream title` | `pStT` | `text` | `r` | `playback` / `r` | ストリーム側が渡す現在曲名 |
| `current stream URL` | `pStU` | `text` | `r` | `playback` / `r` | ストリーム URL |
| `current playlist` | `pPla` | `playlist` | `r` | `playback` / `r` | 現在の曲が入っているプレイリスト |

`player state` の列挙 `ePlS`:

| 名前 | four-char code |
| --- | --- |
| `stopped` | `kPSS` |
| `playing` | `kPSP` |
| `paused` | `kPSp` |
| `fast forwarding` | `kPSF` |
| `rewinding` | `kPSR` |

プロパティ自体は `access="r"` なので、スクリプトから `player state` を代入してはいけない。再生の変更は `play` / `pause` / `playpause` / `stop` コマンドで、いずれも access group は `com.apple.Music.playback`。scrobbler はこれらを送らない。

`player position` にプロパティレベルの `access="r"` は無い。秒単位の実数で、スクリプトから書き換えるとシークになる。読むだけにする。

### `current track` の ID と長さ

`track`（code `cTrk`）は `item` を継承する。クラス自体の access group は `playback`（`r`）、`library.read`（`r`）、`library.read-write`（`rw`）。

scrobble に使う属性:

| プロパティ | 所属 | code | 型 | 説明（sdef） |
| --- | --- | --- | --- | --- |
| `id` | `item` | `ID  ` | `integer` | the id of the item。AppleScript のオブジェクト id。永続とは書かれていない |
| `persistent ID` | `item` | `pPIS` | `text` | hexadecimal string。**This id does not change over time.** |
| `name` | `item` | `pnam` | `text` | このプロパティだけ追加で `playback` / `r` が付く |
| `database ID` | `track` | `pDID` | `integer` | the common, unique ID for this track。別プレイリストで同じ database ID なら同じデータを共有している、と sdef が書いている。時間を超えて不変、とは書いていない |
| `duration` | `track` | `pDur` | `real` | **秒** |
| `time` | `track` | `pTim` | `text` | `MM:SS` 文字列。機械可読な長さには使わない |
| `artist` | `track` | `pArt` | `text` | the artist/source of the track |
| `album` | `track` | `pAlb` | `text` | |
| `album artist` | `track` | `pAlA` | `text` | |
| `played count` | `track` | `pPlC` | `integer` | 再生回数。ライブ監視の主入力にはしない |
| `played date` | `track` | `pPlD` | `date` | 最後に再生した日時 |
| `location` | `file track` のみ | `pLoc` | `file` | ファイルの場所。`track` 一般には無い |
| `address` | `URL track` のみ | `pURL` | `text` | ネットワークストリームの URL |

ID は 3 種類ある。混ぜない。

- `persistent ID`: 時間を超えて変わらない、と sdef が明記している。ライブラリ内の曲の同一性にはこれを使う。16進文字列。
- `database ID`: プレイリストをまたいで同じ音源を指す、と sdef が書いている。永続性の文は無い。
- `id`: その時点のオブジェクト id。

`file track` 以外（Apple Music のストリーム、`URL track`、`shared track`）には `location` が無い。曲名が空で `current stream title` だけが入るケースは sdef の説明と整合する。sdef はストリームのメタデータがサーバ提供だと書いている。

`persistent ID` と `database ID` と `duration` には、プロパティ個別の access group が無い。クラス側の group に従う。`current track` と `name` は明示的に `playback`。`playback` だけを entitlement したとき、`persistent ID` や `duration` まで読めるかは sdef の継承規則としては文書化が薄い。sandbox で読むなら `com.apple.Music.playback` と `com.apple.Music.library.read` の両方を要求し、足りないプロパティは実行時に確認する。これは sdef の読みであって、Apple が「playback だけで persistent ID が読める」とは書いていない。

### 3 つの API の関係

いずれも結局 Apple Event を Music（bundle ID `com.apple.Music`）へ送る。

- [SBApplication](https://developer.apple.com/documentation/scriptingbridge/sbapplication): Objective-C から scriptable アプリへ Apple Event を送り、返事を受け取る仕組み。アプリ固有のサブクラスを、bundle identifier などで取る。現行ドキュメントの例は iTunes の `current track`（code `pTrk`）で、Music でも同じ code。
- [SBApplication.isRunning](https://developer.apple.com/documentation/scriptingbridge/sbapplication/isrunning): 対象が起動中かどうか。**イベントを送る必要が無い限り起動しない。** 逆に、イベントを送る必要が生じると SBApplication は対象を起動する、とこのページが書いている。
- [NSAppleScript](https://developer.apple.com/documentation/foundation/nsapplescript): スクリプトのロード、コンパイル、実行。`executeAndReturnError:` でスクリプト全体を実行すると、スクリプト内の `tell application "Music"` は Apple Event を送る。同ページは `executeAppleEvent:error:` では他アプリへイベントを送れない、とも書いている。これは「ハンドラ呼び出し用のそのメソッド」の制限で、スクリプト本文が `tell` するな、という意味ではない。

`sdp` で sdef からヘッダを生成する手順は存在するが、Swift からそのヘッダを直接使うのは壊れやすい、という報告は非公式（[nishimu.land の記事](https://blog.nishimu.land/entry/2016/04/19/090000)）。Rust からなら、生成ヘッダに依存せず、Apple Event の code（`pTrk`、`pPos`、`pPlS`、`pPIS`、`pDur`、`pDID`）を直接読む方が sdef に近い。code は上の表のとおりで、sdef が正。

## 3. Entitlement、TCC、Mac App Store

3 層を混ぜない。

1. **Hardened Runtime** の `com.apple.security.automation.apple-events`。Apple Event を他アプリへ送ってよいか。sandbox とは別。
2. **App Sandbox** の `scripting-targets` または `temporary-exception.apple-events`。sandbox 内から他アプリへ Apple Event を送る穴。
3. **TCC** の `NSAppleEventsUsageDescription` と利用者の同意。sandbox の有無と独立。同意が無いとイベントは拒否される。

distributed notification の受信は、この 3 層のどれにも公式には属さない。

### `com.apple.security.automation.apple-events`

現行ドキュメントがある。

- [Apple Events Entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.automation.apple-events)（JSON 取得済み）
- 要旨: 他アプリへ Apple Event を送る許可を利用者に求めてよいか、という Boolean。自分自身、または同じ Team ID で署名されたプロセスへ送るだけなら不要。
- 付け方: Hardened Runtime を有効にし、Resource Access の Apple Events を選ぶ。
- [Hardened Runtime](https://developer.apple.com/documentation/security/hardened-runtime): 公証（notarization）に提出する macOS アプリは Hardened Runtime を有効にする必要がある。Boolean entitlement の既定は false で、true のときだけ署名に含める。

Mac App Store 外の Developer ID 配布でも、Music へ Apple Event を送るならこれが要る。sandbox は必須ではない。

### `com.apple.security.scripting-targets`

現行の entitlements JSON 索引（2026-10-25 に取得）には、このキーは無かった。文書はアーカイブ側にある。

- [Enabling Scripting of Other Apps](https://developer.apple.com/library/archive/documentation/Miscellaneous/Reference/EntitlementKeyReference/Chapters/EnablingAppSandbox.html)（Entitlement Key Reference）
- [QA1888: Sandboxing and Automation in OS X](https://developer.apple.com/library/archive/qa/qa1888/_index.html)（2015-05-23）
- 導入の説明: [WWDC 2012 session 206](https://developer.apple.com/videos/play/wwdc2012/206/)（Secure Automation Techniques）

公式の形は、対象アプリの code signing identifier をキーにし、access group の配列を値にする辞書。Mail の例は `com.apple.mail` → `com.apple.mail.compose`。対象が access group を出していない場合は temporary exception を使え、と QA1888 と Entitlement Key Reference が書いている。temporary は恒久のつもりで使うな、とも書いている。

Music.app は access group を出している（第 2 節）。読み取りに必要な指定は次の形になる。

```xml
<key>com.apple.security.scripting-targets</key>
<dict>
    <key>com.apple.Music</key>
    <array>
        <string>com.apple.Music.playback</string>
        <string>com.apple.Music.library.read</string>
    </array>
</dict>
```

`library.read` を足す理由は、`persistent ID` などの個別 group が sdef に無いため。`library.read-write` は再生情報の読み取りには不要で、要求しない。

この経路が、sandbox 下で公式に推奨されている。temporary exception より審査で説明しやすい。ただし「playback group だけで `duration` が読めるか」は sdef から断定できないので、両方を要求する案は調査上の推奨であり、Apple がこの組み合わせを審査で必ず通すとは書いていない。

### `com.apple.security.temporary-exception.apple-events`

これも現行 entitlements JSON 索引には無かった。アーカイブが正。

- [Apple Event Temporary Exception](https://developer.apple.com/library/archive/documentation/Miscellaneous/Reference/EntitlementKeyReference/Chapters/AppSandboxTemporaryExceptionEntitlements.html)
- 要旨: sandbox は Apple Event の受信・自分自身への送信は許す。他アプリへの送信は、`scripting-targets` か、この temporary exception が無いとできない。
- 値は bundle ID の配列。例は `com.apple.iphoto`。Music なら `com.apple.Music`。
- temporary は移行用。必要ならバグレポートを出し、App Store Connect の App Sandbox Entitlement Usage Information に、entitlement と issue 番号と理由を書け、と書いている。
- QA1888: access group が無いときだけこれを使え。Finder と System Events への temporary exception は審査で拒否されやすい、とまで書いている。

Music は access group を持っているので、公式の優先順位では temporary exception の条件を満たさない。

審査の温度感（Apple 社員のフォーラム発言。ガイドライン本文そのものではない）:

- [Close other apps in sandbox mode](https://developer.apple.com/forums/thread/122020) で DTS の Quinn は、App Review は `temporary-exception.apple-events` を好意的に見ない、と書いている（2019）。sandbox と TCC は別レイヤだ、とも書いている。
- [Can a Mac App Store app use an App… temporary exception](https://developer.apple.com/forums/thread/840990) で Apple 社員は、sandbox 制限を迂回する temporary exception の利用は許可されない、と書いている（Notes を作る事例。Music の再生読み取りそのものの裁定ではない）。

### `NSAppleEventsUsageDescription` と TCC

- [NSAppleEventsUsageDescription](https://developer.apple.com/documentation/bundleresources/information-property-list/nsappleeventsusagedescription)。macOS 10.14+。型は string。表示名は Privacy - AppleEvents Sending Usage Description。
- 公式の Discussion: Apple Event で他アプリを操作すると、そのアプリ経由で機微なデータへ届き得る。**Apple Event を送る API を使うならこのキーは必須。**
- 10.14 SDK 以降でこのキーが無いと、プロンプトも出ずに拒否される。エラーは `errAEEventNotPermitted`。出典: DTS の Quinn、[System Events on Xcode error 1743](https://developer.apple.com/forums/thread/109561)。Mojave beta での開発者報告（同じスレッド）も、キー追加後に制御の許可プロンプトが出て、Security & Privacy の Automation に対象が載った、と書いている。
- 同意は sandbox の有無と独立（Quinn、上の thread 122020）。
- 決定のリセットは `tccutil`。`man tccutil`（本機、Darwin April 3, 2012 のマニュアル）: `tccutil reset` は指定サービスの同意を消し、次のアクセスで再びプロンプトを出す。bundle ID を付けるとそのアプリだけ。サービス名としてマニュアルの例は AddressBook。Apple Events のサービス名が `AppleEvents` であることはマニュアル本文には無い。`tccutil reset AppleEvents <bundle-id>` は広く使われるが、本機の man ではサービス名を確認していない。ここは **非公式の運用知識** として扱う。

ダイアログの挙動（公式に文言が固定されているわけではない）:

- 初回の送信時に、対象アプリごとの同意を求める。10.14 で入った。Quinn は [WWDC 2018 session 702](https://developer.apple.com/videos/play/wwdc2018/702/) を参照している（thread 109561）。
- プロンプトの説明文に `NSAppleEventsUsageDescription` が使われる。
- 拒否すると以降は再プロンプトせず、イベントは `errAEEventNotPermitted` で失敗する。再表示には同意のリセットが要る（上の非公式の `tccutil`）。
- システム設定の場所は、現行 UI では プライバシーとセキュリティ → オートメーション。Mojave 当時の文書は Security & Privacy → Privacy → Automation。設定アプリの名前は OS で変わっている。
- ダイアログのボタン文言（許可 / 許可しない）は OS のローカライズに従う。本調査ではダイアログを出していない。

Rust バイナリでも同じ。entitlement は `codesign` 時に付け、`NSAppleEventsUsageDescription` はアプリバンドルの Info.plist に入れる。言語の問題ではない。

### Mac App Store に出せるか

出せる条件と、出せない条件を分ける。

出せる側の公式条件:

- [App Sandbox](https://developer.apple.com/documentation/security/app-sandbox): **Mac App Store で配る macOS アプリは App Sandbox を有効にしなければならない。**
- QA1888: sandbox は Mac App Store 配布の要件。Apple Event を送るなら `scripting-targets` を優先し、temporary exception は正当化して App Store Connect に書け。
- 公証ではなくストア審査なので、Hardened Runtime の「公証には必須」という文はそのまま MAS の文ではない。ただし `automation.apple-events` が無いと他アプリへ Apple Event を送れない、という entitlement の説明は配布経路を限っていない。MAS ビルドにも付ける。

審査リスク:

- `scripting-targets` で `com.apple.Music` の `playback` と `library.read` だけを要求する形が、文書上いちばん筋が良い。Music が group を公開しているため。
- `temporary-exception.apple-events` はアーカイブが「一時的」と書き、DTS と最近のフォーラム回答は迂回としての使用を否定している。MAS の前提にはしない。
- MediaRemote などの private API は、公開ドキュメントに利用許可が無い。PrivateFrameworks 配下にある。MAS の前提にはしない（第 4 節）。
- メニューバー専用（`LSUIElement`）であること自体は、MAS 禁止とは書かれていない。

MAS 外（Developer ID + 公証）なら sandbox は必須ではない。その場合でも Hardened Runtime、`automation.apple-events`、`NSAppleEventsUsageDescription`、TCC 同意は要る。sandbox を外すと `scripting-targets` は不要になる。TCC は残る。

## 4. MediaRemote private framework（使わない。根拠だけ）

`MediaRemote.framework` は public framework ではない。本機では `/System/Library/PrivateFrameworks/MediaRemote.framework` がまだ存在する。存在は利用許可ではない。

公開の Apple ドキュメントで、macOS 15.4 の制限を説明したページは本調査では見つかっていない。制限は private API の内部挙動なので、リリースノートの公開 API 変更としては書かれない。事実関係は第三者の解析と、公開 API を求める Feedback が根拠。**非公式。**

- 非公式: [LyricFever issue #94](https://github.com/aviwad/LyricFever/issues/94)（2025-03）。macOS 15.4 の `mediaremoted` が、接続クライアントの entitlement を見るようになった、という解析。`MRMediaRemoteGetNowPlayingInfo` が一般のプロセスから空またはエラーになる。同スレッドは、再生コマンド（`MRMediaRemoteCommand`）はまだ動く、とも書いている。挙げられている entitlement 名は `com.apple.mediaremote.now-playing-read-access` と `com.apple.mediaremote.full-now-playing-read-access`。現行の公開 entitlements JSON 索引にはこれらのキーは無い。
- 非公式: [FB17228659](https://github.com/feedback-assistant/reports/issues/637)（提出 2025-04-15、macOS 15.4 RC）。15.4 で Now Playing 情報へ届くのは entitlement を持つ Apple のプロセスだけになり、第三者アプリが使えなくなった。公開 API を求めた。ステータスは Open。
- 非公式: [The Apple Wiki: MediaRemote.framework](https://theapplewiki.com/wiki/Dev:MediaRemote.framework)。private framework だと明記されている。
- 非公式: [Reading Now Playing on macOS 15.4 and later](https://remolo.app/blog/now-playing-macos-15-4)（2026-10-06）。15.4 以降、Apple 署名のプロセス以外は `MRMediaRemoteGetNowPlayingInfo` などに応答が無い、と書いている。macOS 27 で本調査が同じ呼び出しを試したわけではない。フレームワークが残っていることと、一般プロセスから読めることは別。

回避（Apple 署名の `/usr/bin/perl` に private framework をロードさせる、SIP を切って `mediaremoted` へ注入する、など）は存在するが、private API の迂回である。使わない。手順は書かない。

Music.app の再生情報は、第 2 節の公開の scripting interface で足りる。システム全体の Now Playing（Spotify やブラウザ）は MediaRemote が担っていた領域で、公開の代替は FB17228659 が求めている状態のまま。Scrova の最初のターゲットは Music.app なので、この穴は踏まない。

## 5. 未起動の Music に Apple Event を送ると起動する

### 公式に言えること

- [SBApplication.isRunning](https://developer.apple.com/documentation/scriptingbridge/sbapplication/isrunning): このプロパティの参照は、イベントを送る必要が無いので起動しない。イベントを送る必要が生じたとき、SBApplication は対象を起動する。
- [NSRunningApplication.runningApplications(withBundleIdentifier:)](https://developer.apple.com/documentation/appkit/nsrunningapplication/runningapplications(withbundleidentifier:)): 指定 bundle ID の起動中アプリの配列を返す。一致が無ければ空配列。Apple Event は送らないので、Music は起動しない。
- `NSAppleScript` で `tell application "Music"` を実行するのは、Apple Event を送ることになる。SBApplication と同じく、未起動なら起動する、と扱う。

AppleScript 言語ガイド（アーカイブ）も、`tell` の対象アプリが未起動なら起動する、と説明している。`application "Music" is running` を `tell` の外で評価する場合は起動しない、という記述もある。

- [AppleScript Language Guide（アーカイブ）](https://developer.apple.com/library/archive/documentation/AppleScript/Conceptual/AppleScriptLangGuide/introduction/ASLR_intro.html)
- 制御文: [tell](https://developer.apple.com/library/archive/documentation/AppleScript/Conceptual/AppleScriptLangGuide/reference/ASLR_control_statements.html)

落とし穴: `tell application "Music"` の内側で `it is running` を見ても遅い。`tell` に入った時点で起動する。確認は `tell` の外、または Apple Event を使わない API で行う。

### 回避

送る前に、bundle ID `com.apple.Music` で起動中かを見る。

- 推奨: `NSRunningApplication.runningApplications(withBundleIdentifier: "com.apple.Music")` が空なら、Apple Event を送らない。起動もしない。
- Scripting Bridge を使うなら、コマンドの前に `isRunning` を見る。`isRunning` 以外のプロパティ（`currentTrack` を含む）へ触れると起動し得る。
- AppleScript なら、`tell` の外で `application id "com.apple.Music" is running` を見てから `tell` する。

Music が終了したことの検知は、ポーリングだけに頼らなくてよい。[NSWorkspace.didTerminateApplicationNotification](https://developer.apple.com/documentation/appkit/nsworkspace/didterminateapplicationnotification) の `userInfo` に `NSRunningApplication` が入る。同じ通知センター制約がある（次節）。公式の注意: `LSUIElement` のアプリについては、この通知が飛ばないことがある。Music は通常のアプリなので、この注意の対象ではない。自分側が `LSUIElement` でも、購読自体はできる。

終了後に残った「再生中」状態を、次の Apple Event で起こしに行かない。

## 6. スリープと復帰

購読先は `NSWorkspace.shared.notificationCenter`。`NotificationCenter.default` や distributed notification center では届かない、と各ページが書いている。`userInfo` は無い。

| 通知 | 公式の意味 | URL |
| --- | --- | --- |
| `willSleepNotification` | システムがスリープする前。ハンドラは最大 30 秒までスリープを遅らせられる | [willSleepNotification](https://developer.apple.com/documentation/appkit/nsworkspace/willsleepnotification) |
| `didWakeNotification` | スリープから復帰した | [didWakeNotification](https://developer.apple.com/documentation/appkit/nsworkspace/didwakenotification) |
| `screensDidSleepNotification` | 画面がスリープした。システムスリープとは別 | [screensDidSleepNotification](https://developer.apple.com/documentation/appkit/nsworkspace/screensdidsleepnotification) |
| `screensDidWakeNotification` | 画面が復帰した | [screensDidWakeNotification](https://developer.apple.com/documentation/appkit/nsworkspace/screensdidwakenotification) |
| `willPowerOffNotification` | ログアウトまたは電源断の要求 | [willPowerOffNotification](https://developer.apple.com/documentation/appkit/nsworkspace/willpoweroffnotification) |

Swift concurrency 用の message 型も各ページにある。Rust からなら、この通知名を `NSWorkspace` の notification center で購読する。

scrobble への含意（公式が scrobble について書いているわけではない。上の意味から導く）:

- `willSleepNotification` の時点で、経過時間の加算を止める。スリープ中の壁時計を再生時間に足さない。30 秒の猶予は、キューをディスクへ書く程度に使う。曲の終わりを待つのに使わない。
- `didWakeNotification` のあと、Music がまだ起動しているときだけ再生状態と `player position` を読み直す。同じ曲が続きから鳴っているとは仮定しない。スリープで一時停止するかどうかは Music の挙動であり、公開ドキュメントには無かった。
- 画面スリープ（`screensDidSleepNotification`）ではシステムも Music も動き続け得る。経過時間を止める理由にはしない。
- ログアウト / 電源断（`willPowerOffNotification`）では、未送信キューを書いて終わる。

画面ロックの `com.apple.screenIsLocked` は distributed notification として知られるが、公開ドキュメントの通知一覧には無い。**非公式。** スリープ判定には使わない。

## 7. ログイン時起動 `SMAppService.mainApp`

macOS 13 以降。`SMLoginItemSetEnabled` の置き換えとして文書化されている。

- [SMAppService](https://developer.apple.com/documentation/servicemanagement/smappservice): アプリバンドル内の helper を登録する。Login Item、Launch Agent、Launch Daemon。
- [mainApp](https://developer.apple.com/documentation/servicemanagement/smappservice/mainapp): メインアプリ自身をログイン項目にするサービスオブジェクト。ログイン時に起動する設定に使う、と Discussion が書いている。
- [register()](https://developer.apple.com/documentation/servicemanagement/smappservice/register()): 利用者の承認を条件に起動できるよう登録する。既に登録済みならエラーを返す。利用者の承認が無い場合もエラーを返す、と Discussion が書いている（JSON のエラー型名はリンクのまま落ちた。型名は断定しない）。
- [unregister()](https://developer.apple.com/documentation/servicemanagement/smappservice/unregister()): 以降の起動をやめる。Login Item / Agent / Daemon が実行中ならシステムが終了させる。**main app の場合はプロセスは止めず、登録だけ外す。**
- [openSystemSettingsLoginItems()](https://developer.apple.com/documentation/servicemanagement/smappservice/opensystemsettingsloginitems()): システム設定のログイン項目を開く。

[SMAppService.Status](https://developer.apple.com/documentation/servicemanagement/smappservice/status-swift.enum) の case（JSON で確認）:

| case | 公式の要旨 |
| --- | --- |
| `notRegistered` | 未登録、または登録済みのものを再登録しようとした |
| `enabled` | 登録され、実行してよい |
| `requiresApproval` | 登録は成功したが、システム設定で利用者の操作が要る。利用者が同意を取り消したときもこれ |
| `notFound` | フレームワークがこのサービスを見つけられなかった |

`requiresApproval` のとき `openSystemSettingsLoginItems()` で設定を開く、が公式 API の組み合わせ。トグルを「オン」に見せるかは製品判断。登録は成功しているが起動はまだ、という状態なので、失敗扱いにすると利用者が二重に押す。

`notFound` は、Launch Services がアプリを解決できないときに出る。Xcode の DerivedData から動かしたログイン項目がここに落ちる、という報告はフォーラムに多い。**その「`/Applications` に置かないとダメ」という文自体は、今回取得した `mainApp` / `notFound` の abstract には無かった。** abstract は「見つけられなかった」まで。配布物は `/Applications` に置いた状態で確認する、が安全側。

Rust から呼ぶなら ServiceManagement のこのクラスを（Swift の薄い helper か objc 経由で）呼ぶ。古い `SMLoginItemSetEnabled` や `LSSharedFileList` のログイン項目は、macOS 13 以降の置き換え対象として SMAppService の Overview が書いている。

## 8. メニューバーアプリ: `MenuBarExtra` と `LSUIElement`

### `LSUIElement`

- [LSUIElement](https://developer.apple.com/documentation/bundleresources/information-property-list/lsuielement)。macOS 10.0+。Boolean。
- 現行 abstract: バックグラウンドで動く agent app で、Dock に出ない。
- アーカイブの Information Property List Key Reference は、Dock に加え Force Quit ウィンドウにも出ない、と書いていた。現行 abstract は Dock だけを明記している。Force Quit に出ない、は現行文面としては確認していない。
- 実行時の対応は `NSApplication` の activation policy `.accessory`。Info.plist の `LSUIElement` が起動時の既定。Rust のアプリバンドルでも Info.plist に `<key>LSUIElement</key><true/>` を入れる。SwiftUI 専用ではない。

`LSUIElement` のアプリは、通常のアプリ起動・終了通知の対象外になることがある（[didTerminateApplicationNotification](https://developer.apple.com/documentation/appkit/nsworkspace/didterminateapplicationnotification) の Important）。自分のプロセスの生死を、他アプリがこの通知だけで見ることはできない。Scrova 自身の起動検出をこの通知に依存させない。

### `MenuBarExtra`

- [MenuBarExtra](https://developer.apple.com/documentation/swiftui/menubarextra)。SwiftUI の scene。メニューバーに残るコントロールとして自分を描画する。
- アプリがアクティブでなくても、よく使う機能へ届かせるためのもの、と Overview が書いている。
- メニューバーだけのアプリは、利用者が extra をメニューバーから外すと **自動で終了する**。
- Dock とアプリ切り替えにアイコンを出さないには、Info.plist の `LSUIElement` を true にする、と MenuBarExtra のページが書いている（`LSUIElement` 自身の abstract より、こちらの方が app switcher に触れている）。
- スタイルは既定のメニューと、`.menuBarExtraStyle(.window)` のポップオーバー風ウィンドウ。後者は通常の SwiftUI コントロールを置ける。

SwiftUI 専用。Rust の本体が直接 `MenuBarExtra` を使うことはできない。選択肢は次のどちらか。

- メニューバー UI を小さな SwiftUI helper にし、`MenuBarExtra` と `LSUIElement` を公式の組み合わせで使う。extra を外すと helper が終了する、という公式の挙動を製品として受け入れる。
- AppKit の `NSStatusItem` を（Rust の tray 実装か、薄い AppKit helper で）使い、`LSUIElement` だけ公式キーに乗せる。`MenuBarExtra` の「外すと終了」は適用されない。ポップオーバーは自前。

メニューバー専用にしても、Apple Event の TCC と sandbox の要件は減らない。Dock に出ないことと、Music を操作する許可は別。

## 設計に落とすとき

実装は別タスク。調査から言える範囲:

- ライブ状態の正は sdef の `current track`、`player state`、`player position`、`persistent ID`、`duration`。単位は秒。ID は16進の `persistent ID`。
- `database ID` と `id` は別名。永続キーにしない。
- `com.apple.Music.playerInfo` は非公式。来たら読み直すきっかけには使える。スキーマと到達性を保証にしない。再生位置は入っていないものとして扱う。
- Apple Event の前に `NSRunningApplication` で `com.apple.Music` の起動を確認する。未起動なら送らない。
- sandbox するなら `scripting-targets` で `com.apple.Music.playback` と `com.apple.Music.library.read`。`temporary-exception.apple-events` は MAS の前提にしない。
- 配布形態にかかわらず `NSAppleEventsUsageDescription` と、公証または hardened runtime なら `com.apple.security.automation.apple-events`。
- MediaRemote は使わない。
- スリープ前に経過加算を止め、復帰後に状態を読み直す。購読は `NSWorkspace` の notification center。
- ログイン項目は `SMAppService.mainApp`。`requiresApproval` と `notFound` を失敗と同一視しない。
- Dock を消すのは `LSUIElement`。`MenuBarExtra` は SwiftUI 専用で、外すとプロセスが終了する。
