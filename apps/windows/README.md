# Scrova for Windows

WinUI 3（Windows App SDK、.NET）の最小スケルトンです。今は「Scrova」というタイトルの
ウィンドウに Now Playing のプレースホルダーを出すだけで、GSMTC と Rust コアにはまだ繋いでいません。

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
