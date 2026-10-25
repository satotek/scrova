# Last.fm Scrobbling API 調査ノート（Scrova 向け）

- 調査日: 2026-10-25
- 対象: Last.fm Web Services 2.0（Scrobbling 2.0 / Authentication 1.0）
- 調査方法: 下記「出典」に列挙した公式ページを実際に取得して確認。公式ページに書かれていない事項は「未確認」と明記。OSS クライアントの実装は GitHub の公開ソース／Issue を確認。
- 前提: Scrova は macOS/iOS のネイティブクライアントで、ユーザーの再生を Last.fm に送信する。以下の記述は API 仕様の調査結果であり、実装コードではない。

### 表記ルール

| 表記 | 意味 |
|---|---|
| 【公式】 | 出典ページに明記されている事実 |
| 【公式から導出】 | 公式の複数の記述から論理的に決まる事項（公式に一文で書かれているわけではない） |
| 【未確認】 | 公式に記述がない、または本調査では実測できなかった事項 |
| 【第三者】 | 公式以外の情報。参考情報として扱う |

---

## 0. 要点サマリ

1. Scrobble 条件は「曲長 > 30 秒」かつ「再生時間が曲長の 1/2 以上、または 4 分のいずれか早い方」。判定はクライアント側の責務として説明されている。【公式】
2. `timestamp` は「再生を開始した時刻」の UNIX epoch 秒（UTC）。終了時刻でも送信時刻でもない。【公式】
3. `track.scrobble` は 1 リクエスト最大 50 件（`i = 0..49`）。配列記法を省略すれば単件も送れる。【公式】
4. レスポンスは `lfm status="ok"` でも各曲が `ignoredMessage` で拒否され得る。`accepted` / `ignored` の件数と、曲ごとの code を必ず見る。【公式】
5. `api_sig` は「`format` と `callback` を除く全パラメータ」をパラメータ名の ASCII 昇順で並べ、`name + value` で連結し、最後に shared secret を付けて MD5（32 桁 hex）。【公式】
6. 認証は Web フローと Desktop フローで入口が違うが、最終的に `auth.getSession` で session key を得る点は同じ。token は 60 分有効・1 回限り、session key は既定で無期限。【公式】
7. Scrobble の再試行対象は公式には **11（Service Offline）と 16（一時的障害）のみ**。9 は再認証してから再試行。それ以外は「リクエスト不正」として再試行しない、と Scrobbling ガイドは明記。【公式】
8. Now Playing の失敗は再試行しない。【公式】
9. レート制限の具体的な数値は公式に公開されていない。エラー 29 が返る。「数 calls/second が続くとアカウント停止があり得る」という警告のみ。【公式】数値の 5 req/s 説は非公式。【第三者／未確認】
10. `user.getRecentTracks` は `limit` 最大 200（既定 50）、`page` は 1 始まり、`from`/`to` は epoch 秒。現在再生中の曲は `nowplaying="true"` で先頭に含まれる。【公式】
11. shared secret を配布バイナリに同梱すると、抽出された場合に「そのアプリの identity で署名できる」状態になる。ユーザーの session key を単独で生成することはできないが、アプリ全体の quota 消費・API key 停止（26）・全ユーザーへの影響・ローテーション困難というリスクがある。【公式から導出＋第三者事例】

---

## 1. Scrobble 判定ルールとタイムスタンプ

### 1.1 判定ルール（公式）

Scrobbling 2.0 の公式記述:

- 曲は **30 秒より長い**こと（"longer than 30 seconds"）。
- かつ、**曲長の半分以上**再生した、または **4 分**再生した、**どちらか早く到達した方**。

> "The track must be longer than 30 seconds. And the track has been played for at least half its duration, or for 4 minutes (whichever occurs earlier.)"

- 条件を満たした後は、**いつでも** scrobble リクエストを送ってよい。多くの場合、曲の再生終了時に送るのが便利、とされている。
- メタデータはファイル名から推測せず、ID3 タグなど構造化された情報源だけを使う。
- Now Playing が返した correction を、ユーザーが明示的に承認しない限り scrobble の入力に使わない。

出典: <https://www.last.fm/api/scrobbling>

【公式から導出】した閾値の例:

| 曲長 | 条件 | Scrobble 可能になる再生時間 |
|---|---|---|
| 30 秒以下 | 対象外 | 送らない |
| 31 秒 | 曲長 > 30 秒、半分 = 15.5 秒、4 分より早い | 15.5 秒 |
| 3 分（180 秒） | 半分 = 90 秒 | 90 秒 |
| 10 分（600 秒） | 半分 = 300 秒、4 分 = 240 秒 | 240 秒（4 分が先） |

注意: 上表は「曲長の半分」が 30 秒より短くても成立する。30 秒は「曲長」の条件であり、「30 秒以上再生した」という条件ではない。【公式の文言から導出】

### 1.2 timestamp の意味

`track.scrobble` の `timestamp[i]` の公式定義:

> "The time the track started playing, in UNIX timestamp format (integer number of seconds since 00:00:00, January 1st 1970 UTC). This must be in the UTC time zone."

つまり:

- **再生開始時刻**（曲の頭出し時点）である。再生終了時刻ではない。
- **scrobble を送信した時刻**でもない。
- UNIX epoch の**整数秒**。ミリ秒ではない。
- UTC の epoch 秒なので、ローカルタイムのオフセットを足し引きしてはいけない（epoch 秒は絶対時刻）。
- オフラインキャッシュから後で送る場合も、**元の再生開始時刻**を保持して送る必要がある。【公式から導出】

出典: <https://www.last.fm/api/show/track.scrobble>

### 1.3 再生状態に関する未確認事項

以下は公式に明記がない。Scrova のプレイヤー実装では仕様を決める必要がある。

- 一時停止した時間を「再生時間」に含めるか。【未確認】
- シーク（早送り・巻き戻し）後の再生時間の扱い。【未確認】
- 同じ曲のリピート再生を、どの単位で 1 scrobble とするか。【未確認】
- 曲長がメタデータと Last.fm カタログで異なる場合、どちらの duration で 1/2 を判定するか。【未確認】
- 30 秒以下・閾値未満の曲をクライアントが送った場合に、サーバーが追加で弾くか。【未確認】
- 再生開始時刻が不明な場合に何を送るべきか。【未確認】

出典（上記がいずれも明記されていないことの確認）: <https://www.last.fm/api/scrobbling> / <https://www.last.fm/api/show/track.scrobble>

### 1.4 chosenByUser

- `chosenByUser[i]` は「ユーザー自身が選んだ曲か」を示す。
- 公式の Scrobbling ガイドでは、Last.fm Radio や推薦サービス、DJ 番組など「ユーザー以外が選んだ」場合に `false` を設定し、曖昧なら送らない、と説明されている。
- `track.scrobble` のパラメータページでは `1`（ユーザーが選択）または `0`（他者が選択）で、**未指定時は 1 とみなす**とされている。
- 通常のローカル再生・自分のプレイリスト再生では、未指定（=1）でよい。【公式】

出典: <https://www.last.fm/api/scrobbling> / <https://www.last.fm/api/show/track.scrobble>

### 1.5 サーバー側フィルタ

Scrobble / Now Playing は、Last.fm 側のフィルタで拒否されることがある。これは**エラーではなく `status="ok"` のまま**返り、曲ごとに `ignoredMessage` が付く。【公式】

- バッチ内の各 scrobble は**個別に**フィルタされる。1 曲が拒否されても、他の曲は受理され得る。
- 拒否理由の code は 0〜5（詳細は 2.5）。
- 「将来 code が追加される可能性がある」と明記されている。

出典: <https://www.last.fm/api/scrobbling>

---

## 2. `track.scrobble`

### 2.1 エンドポイントとトランスポート

- API root: `http://ws.audioscrobbler.com/2.0/`（公式表記は http）。
- Write サービスは **HTTP POST**。`method` を含む**すべてのパラメータを POST body** に入れる。
- `application/x-www-form-urlencoded`、文字コードは **UTF-8**。
- 認証が必要（`api_key` + `api_sig` + `sk`）。
- パラメータ名は**大文字小文字を区別**する。

出典: <https://www.last.fm/api/show/track.scrobble> / <https://www.last.fm/api/rest> / <https://www.last.fm/api/scrobbling>

【実測】2026-10-25 時点で `https://ws.audioscrobbler.com/2.0/` は HTTPS（HTTP/2）で到達できた。`GET` で `track.scrobble` を呼ぶと HTTP 400 で次の XML が返った。

```
<lfm status="failed">
  <error code="4">You must use POST method for this request</error>
</lfm>
```

これは公式ドキュメントに明記された挙動ではないが、少なくとも「write は POST 必須」という記述と整合する。HTTPS が公式に保証されているかは【未確認】（公式の URL 表記は http）。

### 2.2 パラメータ一覧

`[i]` はバッチのインデックス。単件送信時は配列記法を省略できる。【公式】

| パラメータ | 必須 | 意味・備考 |
|---|---|---|
| `artist[i]` | 必須 | アーティスト名 |
| `track[i]` | 必須 | トラック名 |
| `timestamp[i]` | 必須 | 再生開始時刻（UNIX epoch 秒、UTC）。1.2 参照 |
| `album[i]` | 任意 | アルバム名 |
| `albumArtist[i]` | 任意 | トラックアーティストと異なる場合のアルバムアーティスト |
| `trackNumber[i]` | 任意 | アルバム内のトラック番号 |
| `duration[i]` | 任意 | 曲長（秒） |
| `mbid[i]` | 任意 | MusicBrainz の Track ID |
| `chosenByUser[i]` | 任意 | `1` = ユーザーが選択、`0` = 他者が選択。未指定は `1` |
| `context[i]` | 任意 | サブクライアントバージョン。**非公開**で、特定の API key のみ有効 |
| `streamId[i]` | 任意 | Last.fm Radio の `radio.getPlaylist` が返す stream id。Radio 用 |
| `api_key` | 必須 | API key |
| `api_sig` | 必須 | 署名（4 章） |
| `sk` | 必須 | session key |
| `method` | 必須 | `track.scrobble`（REST の全 call で必須） |
| `format` | 任意 | `json` または `xml`（既定 xml）。**署名対象外** |
| `callback` | 任意 | JSONP の callback 名。**署名対象外** |
| `raw` | 任意 | `true` で `<lfm>` ラッパーを外す（REST の共通機能）。署名対象に含めるかは【未確認】 |

補足:

- `context` と `streamId` は Scrova のような通常のローカルプレイヤーでは使わない。
- `mbid` は「MusicBrainz Track ID」とだけ書かれており、Recording MBID か Track MBID かの厳密な区別は【未確認】。
- `duration` を送った場合に Last.fm 側の 1/2 判定に使われるかは【未確認】。1.1 の判定はクライアント側の責務として説明されている。
- パラメータの型はすべて form の文字列。数値も文字列として送る。【form-urlencoded から導出】

出典: <https://www.last.fm/api/show/track.scrobble> / <https://www.last.fm/api/rest>

### 2.3 バッチ

- 1 リクエスト最大 **50 scrobbles**。公式のインデックス表記は `[0 <= i <= 49]`。
- 単件なら配列記法を省略可能（`artist=...&track=...&timestamp=...`）。
- キャッシュした scrobble を送る場合はバッチが推奨される。
- 送信順は「古いものから」。キャッシュ済みを新規より先に送る。
- インデックスは 0 から連番である必要があるか、飛び番が許されるかは【未確認】。
- 50 件を超える場合は分割する。分割時の境界・順序はクライアント責務。【公式から導出】

出典: <https://www.last.fm/api/scrobbling> / <https://www.last.fm/api/show/track.scrobble>

### 2.4 レスポンス（XML）

公式サンプル（単件）:

```
<?xml version='1.0' encoding='utf-8'?>
<lfm status="ok">
  <scrobbles accepted="1" ignored="0">
    <scrobble>
      <track corrected="0">Test Track</track>
      <artist corrected="0">Test Artist</artist>
      <album corrected="0"></album>
      <albumArtist corrected="0"></albumArtist>
      <timestamp>1287140447</timestamp>
      <ignoredMessage code="0"></ignoredMessage>
    </scrobble>
  </scrobbles>
</lfm>
```

- `scrobbles` の `accepted`: 受理件数。
- `scrobbles` の `ignored`: 拒否件数。
- 各 `scrobble` に `track` / `artist` / `album` / `albumArtist` / `timestamp` / `ignoredMessage`。
- `corrected="1"` は Last.fm が表記を自動修正したことを示す。値は修正後の文字列。**自動適用してはいけない**（ユーザー承認が必要）。
- `ignoredMessage` は `code` 属性とテキスト（理由）を持つ。
- HTTP 200 でも `status="failed"` があり得る。HTTP status だけで成功判定しない。【公式】

出典: <https://www.last.fm/api/show/track.scrobble> / <https://www.last.fm/api/scrobbling>

**ドキュメント上の表記揺れ**: Scrobbling ガイドの例では `<albumartist>` / `<ignoredmessage>` と小文字、メソッドページの例では `<albumArtist>` / `<ignoredMessage>` とキャメルケース。XML は大文字小文字を区別するため、パーサは実際のレスポンスに合わせる必要がある。どちらが現行の実レスポンスかは【未確認】。

### 2.5 `ignoredMessage` code 一覧

| code | 意味（公式） | 解釈 |
|---|---|---|
| 0 | None（すべてのフィルタを通過） | 受理。ただし `scrobbles accepted` との整合はレスポンス全体で見る |
| 1 | Filtered artist | アーティスト名がフィルタ対象（例: "Unknown Artist"） |
| 2 | Filtered track | トラック名がフィルタ対象 |
| 3 | Timestamp too far in the past | 古すぎる timestamp |
| 4 | Timestamp too far in the future | 未来すぎる timestamp |
| 5 | Max daily scrobbles exceeded | 1 日の scrobble 上限超過 |

- code 0 は Scrobbling ガイドの一覧にはあるが、`track.scrobble` メソッドページの「Attributes」一覧は 1〜5 のみを列挙している（0 の記載漏れ）。【公式内の不一致】
- 「将来追加される可能性がある」ため、未知 code は「拒否」として扱い、理由表示はフォールバックするのが安全。【公式から導出】
- code 3/4 の具体的な時間窓（何時間前まで／何分先まで）は【未確認】。
- code 5 の 1 日あたりの上限件数は【未確認】。
- `ignoredMessage` の code は **lfm error code とは別物**。混同しないこと。

出典: <https://www.last.fm/api/scrobbling> / <https://www.last.fm/api/show/track.scrobble>

### 2.6 JSON レスポンス

`format=json` を付けると JSON で返る。公式の変換規則:

- 属性は「属性名をキーにした文字列メンバー」になる。
- 子要素は「要素名をキーにしたオブジェクト」になる。
- テキストノードは文字列。ただし要素が属性も持つ場合は `#text` キーに入る。
- **繰り返し登場する子要素は、同じキーの配列になる**。
- `callback` を付けると JSONP になり、MIME は `text/javascript`。付けなければ `application/json`。
- JSON のエラーは変換規則ではなく、`{"error": <code>, "message": "<text>"}` という単純形。

出典: <https://www.last.fm/api/rest>

`track.scrobble` の JSON は、上記規則から概ね次の形になる【公式から導出】:

- 単件: `scrobbles.scrobble` がオブジェクト。
- 複数件: `scrobbles.scrobble` が配列。
- `accepted` / `ignored` は `scrobbles` の属性として文字列で入る。

ただし、Last.fm の JSON は属性を `@attr` というキーにまとめる慣例がある。公式の変換規則の文面だけでは `@attr` になるとは読み切れないため、**正確なキー名は【未確認】**。実装時は実レスポンスで確認する必要がある。

【実測】認証なしで `GET https://ws.audioscrobbler.com/2.0/?method=user.getrecenttracks&user=rj&format=json` を呼ぶと、HTTP 400 で次の JSON が返った。

```json
{"message":"Invalid parameters - Your request is missing a required parameter","error":6}
```

JSON エラー形が `error` と `message` の 2 キーであることは実測で確認できた。キーの順序は保証されない。

### 2.7 実装上の落とし穴

- `accepted + ignored` が送信件数と一致するかは【未確認】。レスポンスの `scrobble` 要素数と突き合わせるのが安全。
- レスポンスの `scrobble` が**リクエスト順**で返るかは【未確認】。`timestamp` で対応付けるのが無難。
- バッチの一部だけが拒否された場合、拒否分だけを捨て、受理分を再送しない。【公式の「個別フィルタ」から導出】
- タイムアウト後にサーバー側で受理済みだった場合、再送すると重複する可能性がある。Last.fm が `(artist, track, timestamp)` で重複排除するかは【未確認】。
- `corrected="1"` の値を次回の scrobble に自動で使わない。【公式】
- XML の要素名の大小文字揺れ（2.4）に注意。
- `status="ok"` でも `ignored > 0` なら「成功」ではない。UI 上は「送信済み・一部拒否」として扱うのがよい。【公式から導出】

出典: <https://www.last.fm/api/show/track.scrobble> / <https://www.last.fm/api/scrobbling>

### 2.8 `track.scrobble` ページに列挙されたエラー

2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 16, 26, 29。16 がページ内で 2 回列挙されている（記載重複）。全体系と再試行判断は 6 章。

出典: <https://www.last.fm/api/show/track.scrobble>

---

## 3. `track.updateNowPlaying`

### 3.1 目的

- ユーザーが曲を再生し始めたことを Last.fm に通知する。
- チャートには影響しない。プロフィールの「現在再生中」表示と、使用クライアント表示に使われる。
- Scrobbling クライアントでは**任意だが推奨**。ユーザーが再生を開始したら**できるだけ早く**送る。
- 失敗しても**再試行しない**。【公式】

出典: <https://www.last.fm/api/scrobbling> / <https://www.last.fm/api/show/track.updateNowPlaying>

### 3.2 パラメータ

| パラメータ | 必須 | 意味・備考 |
|---|---|---|
| `artist` | 必須 | アーティスト名 |
| `track` | 必須 | トラック名 |
| `album` | 任意 | アルバム名 |
| `albumArtist` | 任意 | アルバムアーティスト |
| `trackNumber` | 任意 | トラック番号 |
| `duration` | 任意 | 曲長（秒） |
| `mbid` | 任意 | MusicBrainz Track ID |
| `context` | 任意 | サブクライアントバージョン（非公開） |
| `api_key` | 必須 | API key |
| `api_sig` | 必須 | 署名 |
| `sk` | 必須 | session key |
| `method` | 必須 | `track.updateNowPlaying` |

- `timestamp` は**存在しない**。Now Playing は「今」の状態のみ。
- バッチ用の `[i]` 記法もない（1 曲のみ）。
- `chosenByUser` もパラメータ一覧にない。

出典: <https://www.last.fm/api/show/track.updateNowPlaying>

### 3.3 レスポンス

```
<lfm status="ok">
  <nowplaying>
    <track corrected="0">Test Track</track>
    <artist corrected="0">Test Artist</artist>
    <album corrected="0"></album>
    <albumArtist corrected="0"></albumArtist>
    <ignoredMessage code="0"></ignoredMessage>
  </nowplaying>
</lfm>
```

- `corrected` は scrobble と同じ意味。
- `ignoredMessage` の code 一覧は scrobble と同じ 1〜5 が列挙されている。ただし Now Playing には timestamp がないため、code 3/4（timestamp）が実際に返るのかは【未確認】。ドキュメントのコピペの可能性がある。
- Now Playing の correction を scrobble に流用しない。【公式】

出典: <https://www.last.fm/api/show/track.updateNowPlaying> / <https://www.last.fm/api/scrobbling>

### 3.4 失敗時の扱い

- 公式は「Now Playing の失敗は再試行しない」と明記。
- したがって、オフライン時に Now Playing をキューに積む必要はない。次に再生が始まったら新しい Now Playing を送ればよい。【公式から導出】
- ただし、scrobble とは別物なので、Now Playing が失敗しても scrobble の送信は妨げない。

出典: <https://www.last.fm/api/scrobbling> / <https://www.last.fm/api/show/track.updateNowPlaying>

---

## 4. `api_sig` の生成

### 4.1 公式手順

Last.fm Authentication API 1.0 の Section 8（Auth Spec）に基づく。

1. その呼び出しで送る**すべてのパラメータ**を集める。
2. **`format` と `callback` は除外する**。
3. パラメータ名の**アルファベット順（ASCII 順）**に並べる。
4. 各パラメータを `名前 + 値` の形で連結する（区切り文字なし）。
5. 文字列は **UTF-8** として扱う。
6. 最後に **shared secret** を連結する。
7. その文字列の **MD5** を計算し、**32 桁の hexadecimal** を得る。

公式の例:

```
api_keyxxxxxxxxxxmethodauth.getSessiontokenyyyyyy
```

secret が `ilovecher` なら:

```
api signature = md5("api_keyxxxxxxxxxxmethodauth.getSessiontokenyyyyyyilovecher")
```

出典: <https://www.last.fm/api/authspec>（Section 8）

### 4.2 何を署名に含めるか

| パラメータ | 署名に含めるか | 根拠 |
|---|---|---|
| `api_key` | 含める | 公式例 |
| `method` | 含める | 公式例 |
| `sk` | 含める | 認証付き call の必須パラメータ。公式例には無いが「送るすべてのパラメータ」 |
| `artist` / `track` / `timestamp` など | 含める | 同上 |
| 配列パラメータ `artist[i]` など | すべて含める | 同上 |
| `format` | **含めない** | Auth Spec Section 8 が明記 |
| `callback` | **含めない** | Auth Spec Section 8 が明記 |
| `api_sig` 自身 | 含めない | 定義上（自分自身を署名に含められない）【公式から導出】 |
| `raw` | 記述なし | Auth Spec の除外リストにないため、含めるのが文面通り。【未確認】 |

**ドキュメントの不一致**: `format` / `callback` の除外は **Auth Spec Section 8 にのみ**明記されている。Web How-To と Desktop How-To の Section 6 には除外の一文がない。Auth Spec が「Version 1.0 の仕様」なので、Auth Spec を優先するのが妥当。【公式内の不一致】

出典: <https://www.last.fm/api/authspec> / <https://www.last.fm/api/webauth> / <https://www.last.fm/api/desktopauth> / <https://www.last.fm/api/mobileauth>

### 4.3 ソートの正確な意味

- 「アルファベット順」は**パラメータ名の ASCII 順**。大文字小文字は区別される（パラメータ名自体が case sensitive）。
- 配列記法では、**インデックスを数値としてソートしてはいけない**。`track.scrobble` のページに明記:

> "the parameter names MUST be sorted according to the ASCII table (i.e., artist[10] comes before artist[1])"

- ASCII では `[`（0x5B）は数字（0x30–0x39）より後、英大文字（0x41–0x5A）より後。したがって例えば:
  - `albumArtist[0]` は `album[0]` より前（`A` < `[`）。
  - `artist[0]`, `artist[1]`, `artist[10]`, `artist[11]`, ..., `artist[2]`, ... の順。
  - `artist[...]` は `chosenByUser[...]` より前、など。
- インデックスごとに `artist`, `track`, `timestamp` をまとめるのではなく、**全パラメータ名をグローバルに ASCII ソート**する。【公式の「すべてのパラメータを名前でソート」から導出】

出典: <https://www.last.fm/api/show/track.scrobble> / <https://www.last.fm/api/authspec>

### 4.4 値の扱い

- 公式は「UTF-8 エンコード」とだけ書いている。署名対象は**URL エンコード前の生の値**である、と明記はされていない。【未確認】
- ただし `name + value` の連結で、value を `%xx` や `+` に変換すると、サーバー側の再計算と一致しないのが通例。実装では「生の UTF-8 文字列を連結し、HTTP 送信時だけ form-urlencoded する」のが正しい。【公式から導出／実装上の通例】
- secret も URL エンコードせず、そのまま連結する。【公式から導出】
- MD5 の hex は小文字が一般的だが、公式は「32-character hexadecimal」としか書いていない。大文字小文字の指定は【未確認】。実装は小文字が無難。
- Unicode の正規化（NFC/NFD）に関する規定はない。macOS のファイル名やタグが NFD になる場合、Last.fm 側とバイト列が一致しない可能性がある。【未確認】

出典: <https://www.last.fm/api/authspec>（Section 8） / <https://www.last.fm/api/show/track.scrobble>

### 4.5 参考値（公式例から計算）

公式例の文字列をそのまま MD5 すると:

```
md5("api_keyxxxxxxxxxxmethodauth.getSessiontokenyyyyyyilovecher")
= b87d61da3cda91a8b6746c4aef55d6f8
```

これは公式ページに掲載された値ではなく、**公式の例から本調査で計算した参考値**。実装のテストベクタとして使える。

出典（元の例）: <https://www.last.fm/api/authspec>（Section 8）

### 4.6 よくある間違い

- `format=json` を署名に含めてしまう → error 13（Invalid method signature）。
- `callback` を署名に含めてしまう。
- `method` を署名に入れ忘れる。
- 配列インデックスを数値順にソートする（`artist[2]` が `artist[10]` より前になる）。
- `albumArtist` と `album[...]` の順序を誤る（ASCII では `albumArtist` が先）。
- 値を URL エンコードしてから連結する。
- 空白を `+` に変換してから連結する。
- `api_sig` を署名対象に含める。
- secret を URL エンコードする。
- パラメータ名の大文字小文字を正規化する（`albumArtist` を `albumartist` にするなど）。

出典: <https://www.last.fm/api/authspec>（Section 8） / <https://www.last.fm/api/show/track.scrobble> / <https://www.last.fm/api/errorcodes>

---

## 5. 認証フロー（Web / Desktop / Mobile）

### 5.1 比較表

| 項目 | Web フロー | Desktop フロー | Mobile フロー |
|---|---|---|---|
| 対象 | Web アプリ | デスクトップアプリ | スタンドアロンのモバイル |
| 事前設定 | callback URL が必須 | callback URL は不要（※） | 不要 |
| token の取得 | Last.fm が callback に redirect して渡す | アプリが `auth.getToken` を呼ぶ | なし |
| ユーザー承認 | `/api/auth?api_key=...` | `/api/auth?api_key=...&token=...` をブラウザで開く | `auth.getMobileSession` に username/password を送る |
| session 取得 | `auth.getSession`（サーバー側） | `auth.getSession`（アプリ側） | `auth.getMobileSession` が直接 session を返す |
| secret の置き場所 | サーバー | アプリのバイナリ内 | アプリのバイナリ内 |
| 向くケース | Web | macOS ネイティブ | パスワード入力を受容できる場合 |

（※）Desktop How-To の Section 1 には「callback URL を設定する必要がある」と書かれているが、実際の Desktop フロー（Section 2〜4）では callback に redirect しない。記述のコピペと見られる。【公式内の不一致】

出典: <https://www.last.fm/api/webauth> / <https://www.last.fm/api/desktopauth> / <https://www.last.fm/api/mobileauth> / <https://www.last.fm/api/authentication>

### 5.2 Web フロー

1. ユーザーを `https://www.last.fm/api/auth/?api_key=xxx` に送る（HTTP GET）。
2. 未ログインならログインページを経て、アプリの名称・説明・ロゴが表示され、許可を求める。
3. 許可後、登録した **callback URL** に `token` が GET パラメータで付いて redirect される。
   - callback にクエリがない場合: `<callback_url>/?token=xxxxxxx`
   - callback に既にクエリがある場合: `<callback_url>&token=xxxxxxx`
4. callback 側のサーバーが `auth.getSession` を呼び、`api_key` + `token` + `api_sig` で session key を得る。
5. 以後は `sk` + `api_key` + `api_sig` で認証付き call を行う。

- 任意で `cb` パラメータを付けると、登録した callback とは別の URL に飛ばせる。
  - 例: `https://www.last.fm/api/auth/?api_key=xxx&cb=http://example.com`
  - `cb` に指定できる URL の制限（同一ホストのみか、カスタムスキーム可か）は【未確認】。
- Web フローでは secret をブラウザに渡さない。`auth.getSession` はサーバー側で実行する。【公式から導出】

出典: <https://www.last.fm/api/webauth> / <https://www.last.fm/api/authspec>

### 5.3 Desktop フロー

1. アプリが `auth.getToken` を呼ぶ。パラメータは `api_key` + `api_sig`（+ `method`）。
2. レスポンスの `<token>` を得る。この時点では**未承認**で、session 作成には使えない。
3. ブラウザで `https://www.last.fm/api/auth/?api_key=xxx&token=yyyyyy` を開く。
4. ユーザーが許可すると、Last.fm は「ブラウザを閉じてアプリに戻るよう」表示する。**callback への redirect はない**。
5. アプリが `auth.getSession` を `api_key` + `token` + `api_sig` で呼び、session key を得る。
6. 以後は `sk` + `api_key` + `api_sig`。

- アプリは「ユーザーが許可したか」を自動検知できない。`auth.getSession` をポーリングする実装が一般的だが、**ポーリング間隔や回数の規定は公式にない**【未確認】。
- 未承認の token で `auth.getSession` を呼ぶと error 14（Unauthorized Token）が返る。期限切れは error 15（ただし 6 章の矛盾に注意）。
- token は 1 回限り。session 作成時に消費される。

出典: <https://www.last.fm/api/desktopauth> / <https://www.last.fm/api/authspec> / <https://www.last.fm/api/show/auth.getToken> / <https://www.last.fm/api/show/auth.getSession>

### 5.4 `auth.getToken`

- 目的: 未承認の request token を取得する。Desktop フローの step 2（Auth Spec では 4.1）。
- 認証は不要（`sk` 不要）。
- パラメータ: `api_key`（必須）、`api_sig`（必須）、`method`。
- レスポンス: `<lfm status="ok"><token>cf45fe5a3e3cebe168480a086d7fe481</token></lfm>`。
- token は 32 文字の hex（サンプルは 32 桁）。
- **ドキュメントの不一致**: パラメータ表は `api_sig` 必須としているが、Example URL は `api_key` のみで `api_sig` を付けていない。`api_sig` が本当に必須かは【未確認】。
- エラー: 8（token 発行失敗）、ほか共通エラー。

出典: <https://www.last.fm/api/show/auth.getToken>

### 5.5 `auth.getSession`

- 目的: token を session key に交換する。Web/Desktop 共通の最終ステップ。
- 認証は不要（`sk` 不要）。
- パラメータ: `token`（必須、32 文字の ASCII hex MD5）、`api_key`（必須）、`api_sig`（必須）、`method`。
- レスポンス:

```
<lfm status="ok">
  <session>
    <name>MyLastFMUsername</name>
    <key>d580d57f32848f5dcf574d1ce18d78b2</key>
    <subscriber>0</subscriber>
  </session>
</lfm>
```

- `name`: ユーザー名、`key`: session key（サンプルは 32 桁 hex）、`subscriber`: 有料会員か（0/1）。
- token は 1 回限り。消費される。
- エラー: 14（未承認 token）、15（期限切れ token）、4（Invalid authentication token supplied）など。

出典: <https://www.last.fm/api/show/auth.getSession>

### 5.6 token と session key の寿命

| 項目 | 公式の記述 | 出典 |
|---|---|---|
| token の有効期間 | **60 分**（granted から） | Auth Spec 6.1 / Web How-To 3.1 / Desktop How-To 2.1 |
| token の使用回数 | **1 回限り**。session 作成時に消費 | Auth Spec 6.1 / Web How-To 4 / Desktop How-To 4 |
| token のスコープ | 「API account specific」（Auth Spec）／「user and API account specific」（Web How-To） | 公式内で表現が揺れている |
| session key の寿命 | **既定で無期限** | Auth Spec 6.2 / Web How-To 4.1 / Desktop How-To 4.1 / Mobile How-To 2.1 |
| session key の失効 | ユーザーが Last.fm の設定画面でアプリ権限を取り消すと無効 | 同上 |
| session key の保管 | 安全に保存するよう推奨 | 同上 |

- session key が無効になった場合、API は error 9（Invalid session key）を返す。再認証が必要。
- token の期限切れは error 15 とされているが、グローバルな error 15 は「This item is not available for streaming」であり、**同じコードが文脈で別の意味を持つ**。6 章参照。
- token が「API account specific」と「user and API account specific」のどちらが正しいかは【未確認】。実装上は「ユーザーごとに新しい token を取る」のが安全。

### 5.7 Mobile フロー（参考）

- `auth.getMobileSession` に `username`、`password`（平文）、`api_key`、`api_sig` を送る。
- **POST + HTTPS 必須**。GET や HTTP では失敗する。
- `username` は Last.fm のユーザー名または**メールアドレス**を受け付ける。レスポンスの `name` を以後のユーザー名として使う。
- 旧方式の `authToken`（`md5(username + md5(password))`）は**非推奨**。新規実装では使わない。
- session key を直接返す。
- パスワードを平文で扱うため、iOS/macOS アプリでの採用は慎重に判断すべき。公式の Authentication ページは「モバイルアプリでもブラウザがあれば Desktop パスを使ってよい」としている。

出典: <https://www.last.fm/api/mobileauth> / <https://www.last.fm/api/show/auth.getMobileSession> / <https://www.last.fm/api/authentication>

### 5.8 macOS/iOS（Scrova）への含意

以下は公式の各フローとプラットフォーム制約から導く設計上の含意であり、公式の推奨手順ではない。

- macOS: Desktop フローが自然。`auth.getToken` → ブラウザ → `auth.getSession`。
- iOS: Desktop フローは callback がないため、`ASWebAuthenticationSession` の完了をどう検知するかが課題。
  - ユーザーが「完了」を押したら `auth.getSession` を呼ぶ、という UX が考えられるが、公式に規定はない。
  - Web フローを使ってカスタム URL スキームや Universal Link に redirect させる方法は、Last.fm の callback URL として登録できるかが【未確認】。
  - `auth.getSession` のポーリングが許容されるか、推奨間隔があるかは【未確認】。
- Mobile フロー（password）は公式に存在するが、パスワードをアプリが受け取る設計になるため、セキュリティ上の説明責任が増す。
- いずれのフローでも、得た session key は Keychain など安全なストレージに保存する。【公式の「store the key securely」から導出】

出典: <https://www.last.fm/api/desktopauth> / <https://www.last.fm/api/webauth> / <https://www.last.fm/api/authentication> / <https://www.last.fm/api/authspec>

---

## 6. エラーコードと再試行判断

### 6.1 公式のエラー一覧（`/api/errorcodes`）

| code | 公式メッセージ（要約） | 分類 | Scrova の扱い（案） |
|---|---|---|---|
| 1 | This error does not exist | 予約 | 無視・ログのみ |
| 2 | Invalid service | 恒久 | method/URL の実装ミス。再試行しない |
| 3 | Invalid Method | 恒久 | method 名の実装ミス。再試行しない |
| 4 | Authentication Failed | 恒久／文脈依存 | 認証設定を見直す。write に GET した場合も 4 が返る（実測） |
| 5 | Invalid format | 恒久 | `format` の値・署名除外を見直す |
| 6 | Invalid parameters（必須パラメータ欠落） | 恒久 | リクエスト構築のバグ。再試行しない |
| 7 | Invalid resource specified | 恒久 | ユーザー名・リソースの指定ミス |
| 8 | Operation failed（backend failure の可能性） | **公式内で矛盾** | 6.3 参照 |
| 9 | Invalid session key | 再認証 | 新しい session key を取得し、**1 回だけ**再試行 |
| 10 | Invalid API key | 恒久 | API key の設定ミス。再試行しない |
| 11 | Service Offline | **再試行** | バックオフして後で送る。キャッシュ保持 |
| 12 | Subscribers Only | 恒久 | Radio 系。Scrova では未使用 |
| 13 | Invalid method signature | 恒久 | 署名実装のバグ。再試行しない |
| 14 | Unauthorized Token | 認証やり直し | token が未承認。認証フローをやり直す |
| 15 | This item is not available for streaming | 恒久 | ストリーミング不可。※認証では「token 期限切れ」の意味でも使われる（6.2） |
| 16 | The service is temporarily unavailable | **再試行** | バックオフして後で送る。キャッシュ保持 |
| 17 | Login: User requires to be logged in | 認証やり直し | 再認証 |
| 18 | Trial Expired | 恒久 | Radio 系 |
| 19 | This error does not exist | 予約 | 無視・ログのみ |
| 20 | Not Enough Content | 恒久 | Radio 系 |
| 21 | Not Enough Members | 恒久 | Radio 系 |
| 22 | Not Enough Fans | 恒久 | Radio 系 |
| 23 | Not Enough Neighbours | 恒久 | Radio 系 |
| 24 | No Peak Radio | 恒久 | Radio 系 |
| 25 | Radio Not Found | 恒久 | Radio 系 |
| 26 | API Key Suspended | **停止** | 再試行しない。Last.fm に連絡。アプリ全体に影響 |
| 27 | Deprecated | 恒久 | method の移行 |
| 28 | （一覧に存在しない） | — | 未定義 |
| 29 | Rate Limit Exceeded | **バックオフ** | 即時再送しない。キャッシュ保持。間隔を空ける |

出典: <https://www.last.fm/api/errorcodes>

補足:

- 28 は一覧にない。29 の次がなく、28 は欠番。
- 1 と 19 は「This error does not exist」というプレースホルダ。
- 12〜25 の多くは Radio API 用。Scrova が Radio を使わないなら実質不要。

### 6.2 公式内のコードの意味の揺れ

- `auth.getSession` のページでは **4 = Invalid authentication token supplied**、**15 = This token has expired** と書かれている。
- グローバルの `/api/errorcodes` では **4 = Authentication Failed**、**15 = This item is not available for streaming**。
- したがって、同じ 4/15 でも「どの method のレスポンスか」で解釈を変える必要がある。【公式内の不一致】
- 26 も、`track.scrobble` ページは「Suspended API key - Access for your account has been suspended」、`/api/errorcodes` は「API Key Suspended - This application is not allowed to make requests」と、アカウント単位かアプリ単位かの表現が揺れている。
- 29 も、`/api/errorcodes` は「Your IP has made too many requests」、実際のアプリでは「This application has made too many requests」という文言が返ることがある（Navidrome の Issue）。【第三者】

出典: <https://www.last.fm/api/show/auth.getSession> / <https://www.last.fm/api/errorcodes> / <https://www.last.fm/api/show/track.scrobble> / <https://github.com/navidrome/navidrome/issues/2421>

### 6.3 Scrobble の再試行ポリシー（公式）

Scrobbling ガイドの明記:

- 再試行すべき lfm error code は **11** と **16**。
- **9** は「クライアントが再認証して新しい session key を取得してから再試行」。
- **それ以外のすべての code は、scrobble リクエストが何らかの形で不正に形成されたことを示すので、再試行しない。**

出典: <https://www.last.fm/api/scrobbling>

ただし、この「それ以外は再試行しない」は、29（レート制限）や 8（backend failure）を**恒久エラーとして捨てる**という意味ではないと解釈するのが自然。公式の分類と実運用の間には隙間がある。

- **29**: 一時的な制限。即時再送は制限を悪化させる。指数バックオフ＋ジッターで間隔を空け、キャッシュは保持する。公式の「11/16 以外は再試行しない」を文字通り適用して**捨てるのは不適切**と判断する。【公式の文面と実運用の差／未確認】
- **8**: `/api/errorcodes` は「Most likely the backend service failed. Please try again.」と再試行を示唆するが、Scrobbling ガイドの 11/16 リストには入っていない。**公式内で矛盾**。Scrova では「限定的に再試行（例: 数回まで）」とするか、11/16 と同じ扱いにするかを決める必要がある。【未確認】
- **26**: 再試行禁止。アプリの API key が停止されている可能性があり、リトライしても回復しない。ユーザーに通知し、開発者が Last.fm に連絡する。
- **9**: 再認証は**1 回だけ**。新しい session key でも再度 9 が返るなら、ユーザーによる権限取り消しやアカウント問題を疑う。無限ループにしない。【公式から導出】

### 6.4 Now Playing の再試行

- 公式: 「Now Playing requests that fail should not be retried.」
- HTTP status が 200 でも `status="failed"` があり得るため、body を見る。
- 失敗した Now Playing は破棄してよい。次の再生開始時に新しい Now Playing を送る。

出典: <https://www.last.fm/api/scrobbling>

### 6.5 HTTP status と lfm status の関係

公式の明記:

- HTTP status code だけでは判断できない。
- HTTP が 200 でも成功とは限らない。
- HTTP が 200 以外でも、それが「どう扱うべきか」を示すわけではない。
- 必ずレスポンス body の `lfm status` と `error code` を見る。
- 例: 必須パラメータ欠落は HTTP 400 + error 6。同じリクエストを再送しても同じ結果。

出典: <https://www.last.fm/api/scrobbling>

【実測】認証なしの GET で error 6 が HTTP 400、write への GET で error 4 が HTTP 400 として返った。HTTP status は「エラーの存在」を示すが、再試行可否は code で決める必要がある。

### 6.6 キャッシュと再送の設計原則（公式）

- 失敗した scrobble はローカルキャッシュに保持する。
- キャッシュは**クライアント再起動を跨いで生存**する必要がある。
- 送信は**古い順**。キャッシュ済みを新規より先に送る。
- バッチは最大 50 件。
- これは「ネットワーク断やサーバー障害で scrobble を失わない」ための推奨。

出典: <https://www.last.fm/api/scrobbling>

### 6.7 再試行フローの要約

1. リクエスト送信。
2. HTTP status と body を読む。
3. `lfm status="ok"`:
   - `ignored=0` なら完了。
   - `ignored>0` なら、拒否された曲だけを「送信済み・拒否」として記録。再送しない。
4. `lfm status="failed"`:
   - 9 → 再認証 → 1 回だけ再試行。
   - 11 / 16 → キャッシュに戻し、バックオフして再送。
   - 29 → キャッシュに戻し、より長いバックオフ。即時再送しない。
   - 8 → 公式内矛盾。限定再試行または諦める（要判断）。
   - 26 → 停止。再試行しない。
   - 2/3/4/5/6/7/10/13/27 → 恒久。再試行しない。ログ＋ユーザー通知。
5. 通信自体の失敗（タイムアウト、DNS、TLS）→ 11/16 と同様に扱う。【公式から導出】
6. 重複の可能性（送信は成功したがレスポンスを受け取れなかった）→ 再送すると重複し得る。冪等性の保証は【未確認】。

出典: <https://www.last.fm/api/scrobbling> / <https://www.last.fm/api/errorcodes>

---

## 7. レート制限

### 7.1 公式に書かれていること

- API Terms of Service 4.4:

> "Last.fm sets and enforces limits on use of the API to prevent abuse and ensure reliability of service (e.g. limiting the number of API requests that you may make or the number of users you may serve), in our sole discretion."

- 制限を超える利用は Last.fm の事前承諾が必要。承諾なしの回避は禁止。
- API Introduction:
  - 識別可能な **User-Agent** を付ける。ログと BAN リスク低減のため。
  - 「数 calls per second が続く」または「過剰な call」で**アカウントが停止され得る**。
  - Web アプリなら page load のたびに API を叩かない、などの best practice を推奨。
- ToS 4.3.4: Last.fm Data の保存は **100 MB** の "Reasonable Usage Cap" まで。HTTP ヘッダに従った caching を実装する。
- ToS 3.1/3.2: 非商用利用のみ。商用は別契約。
- エラー **29** がレート制限超過。

出典: <https://www.last.fm/api/tos> / <https://www.last.fm/api/intro> / <https://www.last.fm/api/errorcodes>

### 7.2 公式に書かれていないこと

- **具体的な req/s、req/min、日次の上限値**は公開されていない。【未確認】
- 制限の単位が **IP** なのか **API key** なのか **アプリ** なのか **ユーザー** なのかが一貫していない。
  - error 29 の文言は「Your IP」。
  - 実運用の文言は「This application」。
  - ToS は「API requests」と「number of users」の両方に言及。
  - → **未確認**。
- レート制限の HTTP ヘッダ（`X-RateLimit-*`、`Retry-After` など）が返るか。【未確認】
- 制限の回復時間（ウィンドウ長）。【未確認】
- アプリ申請時に個別の quota が提示されるか。【未確認】

出典（上記がいずれも明記されていないことの確認）: <https://www.last.fm/api/tos> / <https://www.last.fm/api/intro> / <https://www.last.fm/api/errorcodes>

### 7.3 非公式情報

- APIs.io の Last.fm rate limit ページは「約 5 requests/second per originating IP」としているが、**自ら「provider が公開したページで検証していない」と明記**している。鵜呑みにできない。【第三者／未確認】
- 同ページは「RFC 9239 の RateLimit ヘッダはなく、error 29 / 26 で通知される」としながら、「429 を返し、`limit` / `remaining` / `reset` / `retryAfter` ヘッダで通知する」とも書いており、記述自体が矛盾している。いずれも未検証。【第三者／未確認】
- Navidrome の Issue では、実際に error 29 の文言が「This application has made too many requests...」であったことが報告されている。【第三者】

出典: <https://apis.io/rate-limits/lastfm/lastfm-rate-limits> / <https://github.com/navidrome/navidrome/issues/2421>

### 7.4 Scrova での実務的な目安

以下は公式値ではなく、公式の「数 calls/second で停止があり得る」「29 が返る」という記述から導く**設計上の目安**。

- Scrobble の発生量は再生曲数に比例する。1 曲につき Now Playing 1 回 + Scrobble 1 回（バッチ可）。通常のリスニングでは過剰にならない。
- 初期履歴の一括取得や `user.getRecentTracks` のポーリングは call 数が増える。ページ取得の間に待機を入れる。
- 同時実行は 1〜2 に制限する。
- 11/16/29 は指数バックオフ＋ジッター。29 は特に長めに。
- 失敗しても捨てずに永続キャッシュへ。ただし 26 は停止。
- User-Agent にアプリ名・バージョン・連絡先を含める。
- 不必要な call を避ける（画面表示のたびに API を叩かない、キャッシュする）。
- レート制限が「アプリ単位」で効く場合、全ユーザーの合計が上限に達する。人気アプリほど厳しい。Pano Scrobbler は実際にレート制限の影響で機能を削減したと述べている。【第三者】

出典: <https://www.last.fm/api/intro> / <https://www.last.fm/api/tos> / <https://github.com/kawaiiDango/pano-scrobbler/issues/527>

---

## 8. `user.getRecentTracks`

### 8.1 目的

- ユーザーが最近聴いた曲の一覧を取得する。
- ユーザーが現在再生中なら、`nowplaying="true"` の属性を持つ曲を**先頭に含む**。
- 認証は不要（`sk` 不要）。`api_key` は必須。

出典: <https://www.last.fm/api/show/user.getRecentTracks>

### 8.2 パラメータ

| パラメータ | 必須 | 意味・備考 |
|---|---|---|
| `user` | 必須 | Last.fm ユーザー名 |
| `limit` | 任意 | 1 ページの件数。既定 **50**、最大 **200** |
| `page` | 任意 | ページ番号。既定は 1 ページ目（1 始まり） |
| `from` | 任意 | 範囲の開始 timestamp（epoch 秒、UTC）。この時刻**より後**の scrobble |
| `to` | 任意 | 範囲の終了 timestamp（epoch 秒、UTC）。この時刻**より前**の scrobble |
| `extended` | 任意 | `0` または `1`。`1` で artist の拡張データと loved 状態を含む |
| `api_key` | 必須 | API key |
| `method` | 必須 | `user.getRecentTracks` |

- `from`/`to` の「after」「before」が**境界を含むか含まないか**は【未確認】。
- `from > to` の挙動、`from` のみ／`to` のみの挙動は【未確認】。
- `limit` に 200 を超える値を指定した場合、エラーかクランプかは【未確認】。
- `page` に `totalPages` を超える値を指定した場合、空配列かエラーかは【未確認】。
- `user` が存在しない場合の code（6 か 7 か）は【未確認】。

出典: <https://www.last.fm/api/show/user.getRecentTracks>

### 8.3 レスポンス

公式サンプル:

```
<recenttracks user="RJ" page="1" perPage="10" totalPages="3019">
  <track nowplaying="true">
    <artist mbid="2f9ecbed-27be-40e6-abca-6de49d50299e">Aretha Franklin</artist>
    <name>Sisters Are Doing It For Themselves</name>
    <mbid/>
    <album mbid=""/>
    <url>www.last.fm/music/Aretha+Franklin/_/Sisters+Are+Doing+It+For+Themselves</url>
    <date uts="1213031819">9 Jun 2008, 17:16</date>
    <streamable>1</streamable>
  </track>
  ...
</recenttracks>
```

- `recenttracks` の属性: `user`、`page`、`perPage`、`totalPages`。
- `track` の主な子要素:
  - `artist`（`mbid` 属性あり）
  - `name`
  - `mbid`
  - `album`（`mbid` 属性あり）
  - `url`
  - `date`（`uts` 属性 + 表示用文字列）
  - `streamable`
  - `loved`（`extended=1` のとき）
  - `image`（実際のレスポンスには含まれることがあるが、公式サンプルにはない）【未確認】
- `extended=1` の「artist の拡張データ」の具体的なフィールドは【未確認】。
- JSON では属性が `@attr` にまとまる可能性がある（2.6 と同じ問題）。【未確認】

出典: <https://www.last.fm/api/show/user.getRecentTracks>

### 8.4 nowplaying 属性

- 現在再生中の曲は `nowplaying="true"`。
- 公式説明では「先頭に含まれる」。
- **ドキュメントの矛盾**: サンプルの `nowplaying="true"` の track に `<date uts="...">` が付いている。しかし Now Playing はまだ scrobble されていない曲なので、本来 `date` はないはず。実際のレスポンスで `date` が付くかは【未確認】。
- Now Playing の曲を「聴いた曲」として履歴に取り込むと、後で scrobble されたときに二重計上になる。`nowplaying="true"` は履歴・統計から除外するのが安全。【公式の「チャートに影響しない」から導出】
- `nowplaying` の曲が `totalPages` や件数に含まれるかは【未確認】。

出典: <https://www.last.fm/api/show/user.getRecentTracks> / <https://www.last.fm/api/scrobbling>

### 8.5 ページング

- `page` は 1 始まり。`limit` は既定 50、最大 200。
- レスポンスの `totalPages` を使い、`page > totalPages` で終了する。
- `from`/`to` を指定した場合、`totalPages` がその範囲に対して計算されるのか、全履歴に対してなのかは【未確認】。
- 深いページ（古い履歴）を辿る場合、`page` を増やす方式と、`to` を遡る方式がある。`page` の上限や深さ制限は【未確認】。
- `page` と `from`/`to` を併用した場合の挙動は【未確認】。

出典: <https://www.last.fm/api/show/user.getRecentTracks>

### 8.6 用途と注意

- 初回起動時の履歴インポート。
- オフライン中に送れなかった scrobble の照合（Last.fm 側に既に入っているか）。
- 「現在再生中」の表示。
- 読み取り専用なので、11/16/29 はバックオフ再試行でよい。6/7/10 は恒久。
- レート制限を考慮し、ページ取得の間に待機を入れる。特に大量の履歴を一度に取得しない。
- `nowplaying` を除外して scrobble 履歴と突き合わせる。重複判定のキーは `(artist, track, timestamp)` が候補だが、Last.fm 側の正規化（correction）後の値と一致しないことがある。【未確認】

出典: <https://www.last.fm/api/show/user.getRecentTracks> / <https://www.last.fm/api/scrobbling> / <https://www.last.fm/api/intro>

---

## 9. クライアント配布アプリと shared secret

### 9.1 前提: secret は何を守っているか

- Last.fm の API key は**公開情報**。リクエストの `api_key` パラメータとして平文で送られる。
- shared secret は**アプリ（API account）ごとの秘密**。`api_sig` の生成に使い、リクエストが「そのアプリから来た」ことを示す。
- secret はユーザーごとではなく、**アプリ全体で 1 つ**（API account に紐づく）。【公式】
- secret があれば、そのアプリの `api_key` を使って任意の API call に**有効な署名**を作れる。
- ただし、secret だけでは**任意のユーザーの session key を新規に作ることはできない**。`auth.getSession` には、ユーザーが承認した token が必要。【公式から導出】
- 一方、端末に保存された session key が漏れていれば、secret と組み合わせてそのユーザーとして call できる（ユーザーが権限を取り消すまで）。【公式から導出】

出典: <https://www.last.fm/api/authspec> / <https://www.last.fm/api/tos> / <https://www.last.fm/api/errorcodes>

### 9.2 同梱のリスク

| リスク | 内容 | 根拠 |
|---|---|---|
| secret の抽出 | バイナリの strings、APK/IPA の decompile、Electron の asar、JS の source map、クラッシュログ、CI ログ、リポジトリ公開などから復元できる | 【公式から導出／第三者事例】 |
| アプリ identity の偽装 | 抽出した secret で、そのアプリとして署名した call を送れる | 【公式から導出】 |
| quota の消費 | 全ユーザーで 1 つの API key を共有するため、攻撃者や大量利用がアプリ全体のレート制限（29）を枯渇させる | 【公式から導出】 |
| API key 停止 | 悪用が続くと error 26（Suspended API key）。全ユーザーが scrobble できなくなる | 【公式】 |
| 機能の削減 | レート制限対策で機能を削らざるを得ない事例がある（Pano Scrobbler） | 【第三者】 |
| ローテーションの困難 | secret を変えると、旧バージョンのアプリは署名できなくなる。全ユーザーに更新を強制できない | 【公式から導出】 |
| リポジトリ汚染 | secret をコミットすると、fork・検索・Copilot の補完などを通じて拡散する | 【第三者】 |
| ToS リスク | 非商用限定、レート制限の遵守、100 MB の保存上限、ブランド表示義務など | 【公式】 |

### 9.3 secret で「できない」こと

- ユーザーのパスワードを取得する。
- ユーザーが承認していない token から session key を作る。
- TLS を復号する。
- Last.fm の公開 API 以外のデータを取得する。
- ユーザーが Last.fm 設定で権限を取り消した後も session key を使い続ける。

→ リスクは「ユーザー個人の乗っ取り」よりも、**アプリ全体の identity・quota・評判・停止**に集中する。【公式から導出】

出典: <https://www.last.fm/api/authspec> / <https://www.last.fm/api/show/auth.getSession> / <https://www.last.fm/api/show/auth.getMobileSession>

### 9.4 ToS 上の位置づけ

- Last.fm API ToS には「shared secret を配布バイナリに埋め込んではならない」という**明示的な禁止規定は見つからなかった**。【未確認】
- ただし、以下は課される:
  - 非商用利用（3.1、3.2）。商用は別契約。
  - レート制限の遵守（4.4）。回避禁止。
  - Last.fm Data の保存は 100 MB まで（4.3.4）。
  - Last.fm へのクレジットとリンク（2.7）。
  - Last.fm は任意にアクセスを停止できる（9.2）。
- したがって「secret 同梱が規約違反」とまでは言えないが、「無制限に配ってよい」わけでもない。【公式の条項から導出】

出典: <https://www.last.fm/api/tos>

### 9.5 他 OSS クライアントの扱い

| プロジェクト | 方式 | 出典 |
|---|---|---|
| Web Scrobbler（ブラウザ拡張） | API key と secret を**ソースにハードコード**して配布 | <https://github.com/web-scrobbler/web-scrobbler/blob/master/src/core/scrobbler/lastfm/lastfm-scrobbler.ts> |
| Pano Scrobbler（Android/Windows/Linux） | secret を**クライアント側コードに保存**。メンテナは「変更予定はない」と明言。レート制限で機能削減・キャッシュ強化の経験 | <https://github.com/kawaiiDango/pano-scrobbler/issues/527> |
| Navidrome（セルフホスト） | 0.51.0 で**共有 API key/secret を削除**。各インストールが自分の API key/secret を設定する方式に変更 | <https://github.com/navidrome/navidrome/issues/2896> / <https://www.navidrome.org/docs/usage/integration/external-services> |
| multi-scrobbler | ユーザーが `LASTFM_API_KEY` / `LASTFM_SECRET` を設定 | <https://foxxmd.github.io/multi-scrobbler/configuration/clients/lastfm> |
| Last.fm 公式デスクトップクライアント | クローズドソース。secret の保存方法は【未確認】 | — |

観察:

- 「ユーザーに API key/secret を作らせる」方式（Navidrome、multi-scrobbler）は、**開発者が secret を配らない**のでリスクは低いが、UX が悪い。
- 「同梱」方式（Web Scrobbler、Pano）は UX が良いが、secret は事実上公開情報になる。Pano のメンテナは、APK の decompile で抽出可能だと認識した上で、Git 管理からは外している。
- Last.fm には OAuth 2.0 の public client / PKCE に相当する仕組みがない。ネイティブアプリが secret を隠す公式の方法は、現状では「バックエンドを置く」以外にない。【公式の認証仕様から導出】

### 9.6 Scrova の選択肢（判断材料）

| 方式 | UX | secret の秘匿 | 運用コスト | リスク |
|---|---|---|---|---|
| A. バイナリに同梱 | 最良（ユーザーはログインだけ） | 不可（抽出可能） | 低 | quota 枯渇・26・ローテーション困難 |
| B. バックエンド proxy | 良い | 可能 | 高（サーバー、プライバシー、可用性） | サーバーが単一障害点。session key の扱いがサーバーに移る |
| C. ユーザーが自分の API key/secret を入力 | 最悪 | 不要（各ユーザーの責任） | 低 | 設定が難しい。サポート負荷 |
| D. A + C のハイブリッド | 良い | 不可（A 部分） | 中 | A のリスクを許容しつつ、上級者は C に逃げられる |

- A を選ぶ場合でも、secret を**リポジトリにコミットしない**（ビルド時の注入、CI secret、Xcode の xcconfig など）。ただしバイナリからは抽出可能。
- Rust でも `include_str!` やビルド時環境変数で埋め込んだ secret は、リリースバイナリから `strings` などで抽出できる。言語やビルド方式は秘匿性に影響しない。
- 難読化は「抽出の手間を少し増やす」だけで、セキュリティではない。
- クライアント側でレート制限・バッチ・永続キャッシュを実装し、29/26 を監視する。
- secret のローテーション手順を用意する（新 secret を配ったアプリに切り替え、旧アプリのサポート終了）。
- 商用化する場合は Last.fm の commercial use agreement が必要。
- B を選ぶ場合、ユーザーの session key をサーバーに保存することになるため、プライバシーとセキュリティの説明が増える。Last.fm の ToS 上もデータ保存の制約（100 MB）に注意。

出典: <https://www.last.fm/api/authspec> / <https://www.last.fm/api/tos> / <https://github.com/kawaiiDango/pano-scrobbler/issues/527> / <https://github.com/navidrome/navidrome/issues/2896>

### 9.7 結論

- ネイティブアプリで「ユーザーに何も設定させない」UX を取る限り、secret の同梱は避けられない。Last.fm の認証仕様がそうなっている。
- 同梱するなら「secret は公開されるもの」として設計する。守るべきは secret そのものではなく、**アプリ全体の quota と可用性**。
- 他 OSS の実例は、同梱（Web Scrobbler、Pano）とユーザー設定（Navidrome、multi-scrobbler）に分かれる。どちらも許容されているが、同梱勢はレート制限・停止リスクと隣り合わせ。
- Scrova が無料・非商用で小規模なら A は現実的。将来ユーザー数が増えるなら、B への移行か C の提供を検討する価値がある。

出典: <https://www.last.fm/api/tos> / <https://www.last.fm/api/authspec> / <https://github.com/kawaiiDango/pano-scrobbler/issues/527> / <https://www.navidrome.org/docs/usage/integration/external-services>

---

## 10. 未確認事項一覧

1. 曲長 1/2 判定に使う duration は、クライアントのタグ値か Last.fm カタログ値か。
2. 一時停止・シーク・リピート時の「再生時間」の定義。
3. 30 秒以下・閾値未満の scrobble をサーバーが追加で拒否するか。
4. `ignoredMessage` code 3/4 の時間窓（何秒/何時間まで許容か）。
5. `ignoredMessage` code 5 の 1 日あたりの上限件数。
6. `track.scrobble` のレスポンスがリクエスト順を保つか、`scrobble` 要素の対応付け方法。
7. バッチのインデックスに飛び番が許されるか。
8. `accepted + ignored` が送信件数と常に一致するか。
9. タイムアウト後の再送で重複が発生するか。Last.fm の重複排除キー。
10. JSON レスポンスの正確なキー名（`@attr` の有無、`#text` の扱い）。
11. XML の要素名が `albumArtist` か `albumartist` か（実レスポンス）。
12. `auth.getToken` に `api_sig` が本当に必須か（Example URL は省略）。
13. Desktop フローで `auth.getSession` をポーリングしてよいか、推奨間隔。
14. iOS のカスタム URL スキーム／Universal Link を callback URL として登録できるか。
15. Web フローの `cb` に指定できる URL の制約。
16. token のスコープが「API account specific」か「user and API account specific」か。
17. session key の形式・長さの保証（サンプルは 32 桁 hex）。
18. レート制限の具体的な数値、単位（IP/API key/アプリ/ユーザー）、回復時間。
19. レート制限の HTTP ヘッダの有無。
20. error 8 を再試行対象に含めるべきか（公式内で矛盾）。
21. error 29 を「再試行しない」に含めるか（公式の scrobble ガイドの文言）。
22. `raw=true` を署名に含めるか。
23. 署名対象の値の URL エンコード可否、Unicode 正規化の要否。
24. MD5 hex の大文字小文字の指定。
25. `user.getRecentTracks` の `from`/`to` の境界包含、`from > to` の挙動。
26. `user.getRecentTracks` の `totalPages` が `from`/`to` 範囲に対して計算されるか。
27. `nowplaying` の track に `date` が付くか、件数に含まれるか。
28. `extended=1` の正確な追加フィールド。
29. 公式デスクトップクライアントの secret 保存方法。
30. Last.fm が secret のローテーションや複数 secret を許すか。

---

## 11. 出典

### 11.1 公式（Last.fm）

- Scrobbling 2.0 Documentation: <https://www.last.fm/api/scrobbling>
- Authentication API Specification 1.0: <https://www.last.fm/api/authspec>
- Authentication: Web Application How-To: <https://www.last.fm/api/webauth>
- Authentication: Desktop Application How-To: <https://www.last.fm/api/desktopauth>
- Authentication: Mobile Application How-To: <https://www.last.fm/api/mobileauth>
- User Authentication（認証パスの選択）: <https://www.last.fm/api/authentication>
- `track.scrobble`: <https://www.last.fm/api/show/track.scrobble>
- `track.updateNowPlaying`: <https://www.last.fm/api/show/track.updateNowPlaying>
- `auth.getToken`: <https://www.last.fm/api/show/auth.getToken>
- `auth.getSession`: <https://www.last.fm/api/show/auth.getSession>
- `auth.getMobileSession`: <https://www.last.fm/api/show/auth.getMobileSession>
- `user.getRecentTracks`: <https://www.last.fm/api/show/user.getRecentTracks>
- Error Codes: <https://www.last.fm/api/errorcodes>
- API Terms of Service: <https://www.last.fm/api/tos>
- Introduction（API root、User-Agent、レート制限の注意）: <https://www.last.fm/api/intro>
- REST Requests（JSON 変換規則、POST、`raw`）: <https://www.last.fm/api/rest>

### 11.2 第三者（参考）

- Pano Scrobbler Issue #527「Where is the Last.fm shared secret stored?」: <https://github.com/kawaiiDango/pano-scrobbler/issues/527>
- Web Scrobbler の Last.fm クライアント実装（key/secret のハードコード）: <https://github.com/web-scrobbler/web-scrobbler/blob/master/src/core/scrobbler/lastfm/lastfm-scrobbler.ts>
- Navidrome Issue #2896（0.51.0 で共有 API key/secret を削除）: <https://github.com/navidrome/navidrome/issues/2896>
- Navidrome External Integrations（`LastFM.ApiKey` / `LastFM.Secret` をユーザーが設定）: <https://www.navidrome.org/docs/usage/integration/external-services>
- multi-scrobbler Last.fm client（`LASTFM_API_KEY` / `LASTFM_SECRET` をユーザーが設定）: <https://foxxmd.github.io/multi-scrobbler/configuration/clients/lastfm>
- Navidrome Issue #2421（error 29 の実運用メッセージ）: <https://github.com/navidrome/navidrome/issues/2421>
- APIs.io Last.fm Rate Limits（未検証の第三者情報）: <https://apis.io/rate-limits/lastfm/lastfm-rate-limits>

---

## 付録: 本調査で実際に行った確認

- 上記の公式ページを 2026-10-25 に取得し、本文を確認。
- `https://ws.audioscrobbler.com/2.0/` への未認証リクエストで、HTTPS 到達性、JSON エラー形、write への GET が error 4 になることを確認。
- Web Scrobbler の公開ソースを取得し、key/secret がソース内に定義されていることを確認（値は本ドキュメントに転記しない）。
- Pano Scrobbler、Navidrome、multi-scrobbler の公開 Issue／ドキュメントを確認。
