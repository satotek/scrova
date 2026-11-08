# Scrova for Windows

WinUI 3（Windows App SDK、.NET）の最小スケルトンです。「Scrova」というタイトルのウィンドウに
Now Playing を出します。再生情報は GSMTC（Global System Media Transport Controls）から読み、
Rust コアにはまだ繋いでいません。

## GSMTC（再生中の曲の取得）

`Scrova/Playback/MediaSessionObserver.cs` が `Windows.Media.Control` の
`GlobalSystemMediaTransportControlsSessionManager` を購読し、Windows が「現在」とみなす
セッション（無ければ再生中のもの、それも無ければ先頭のもの）を追いかけて
`MediaSessionSnapshot`（曲名・アーティスト・アルバム・アプリ・再生状態・位置・長さ）を発行する。

- システムのメディアコントロールに登録するアプリ（Spotify、ブラウザ、メディア プレーヤーなど）は
  すべて対象で、アプリでの絞り込みはしない
- イベントは任意のスレッドで来るので、読み取りは 1 本の直列リフレッシュにまとめ、
  セッションが途中で消えても空のスナップショットに戻すだけにしている
- タイムラインをシークや状態変化のときにしか送らないアプリが多いので、再生中は 1 秒ごとに位置を推定して更新する
- セッションが無い・曲名が無いときは「Nothing playing」の空状態を表示する
- スクロブル判定や Last.fm 送信はまだ無い（Rust コア接続後）

## 前提（2026-11-08 に確認した環境）

- Visual Studio 2026 Community 18.11（ワークロード: C++ によるデスクトップ開発、.NET デスクトップ開発、
  WinUI アプリケーション開発 / Windows App SDK C# サポート）
  - MSVC 14.51（`cl.exe` は PATH に無くてよい。cargo / cc-rs が vswhere 経由で見つける）
  - Windows SDK 10.0.26100
- .NET SDK 10.0.403
- Rust stable（`x86_64-pc-windows-msvc`。ホストターゲットなので追加不要）

## ビルドと実行

```powershell
cd apps/windows/Scrova
dotnet build            # 既定は x64。ARM64 は -p:Platform=ARM64
dotnet run
```

- アンパッケージ（`WindowsPackageType=None`）かつ `WindowsAppSDKSelfContained=true` なので、
  Windows App SDK ランタイムをインストールせずに `bin/` から起動できる
- Visual Studio では `Scrova.csproj` を直接開けばよい

## Rust コア

```powershell
cargo build -p scrova-ffi --lib --target x86_64-pc-windows-msvc
```

`target/x86_64-pc-windows-msvc/debug/scrova_ffi.dll` ができる。C# からの呼び出し方式
（`uniffi-bindgen-cs` か手書き C ABI + P/Invoke）は未決（`docs/roadmap.md` Phase 5）。
