# Scrova for Android

Android 版の最小スケルトンです。UI は Jetpack Compose、Rust コアは UniFFI の
Kotlin バインディング経由で呼び出します。

## 前提

- Nix + direnv（JDK 21 と Gradle wrapper を提供）
- Android SDK（`ANDROID_HOME`、platform 37.1 / build-tools 36.0.0 付近）
- 未設定なら `apps/android/local.properties` に `sdk.dir=...` を書く

Gradle wrapper は Gradle 9.6.0 を使用します（AGP 9.4.1 の最小要件）。

## Rust ブリッジの生成

リポジトリルートで:

```sh
scripts/build-android.sh
```

これが `app/src/main/jniLibs/<abi>/libscrova_ffi.so` と
`app/src/main/generated/kotlin`（`uniffi.*` パッケージ）を生成します。
生成物はコミットしません。生成ディレクトリが無い場合、Gradle は
`checkUniffiBindings` で分かりやすいエラーを出して停止します。

## ビルド

```sh
cd apps/android
./gradlew assembleDebug
```

APK は `app/build/outputs/apk/debug/app-debug.apk` に出力されます。
