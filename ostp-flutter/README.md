# OSTP for Android

Android client built with Flutter. The protocol code is the same Rust as everywhere else, compiled into a native library through
[`ostp-jni`](../ostp-jni) and loaded by the app.

Ready-made APKs (arm64 and armv7) are on the [Releases](https://github.com/ospab/ostp/releases) page. User documentation:
[docs/en/client.md](../docs/en/client.md).

## Build

Needs Rust with the Android targets, [cargo-ndk](https://github.com/bbqsrc/cargo-ndk), the Android NDK and Flutter.

```bash
rustup target add aarch64-linux-android armv7-linux-androideabi
cargo install cargo-ndk

# native library, from the repository root
cd ostp-jni
cargo ndk -t arm64-v8a -o ../ostp-flutter/android/app/src/main/jniLibs build --release
cd ../ostp-flutter

flutter pub get
flutter build apk --release --target-platform android-arm64
```

A release build must be signed with your own key; without one the APK is debug-signed and cannot be updated over a previously installed release.

---

# OSTP для Android

Android-клиент на Flutter. Код протокола тот же, что и везде, на Rust: он собирается в нативную библиотеку через [`ostp-jni`](../ostp-jni) и
подгружается приложением.

Готовые APK (arm64 и armv7) — на странице [Releases](https://github.com/ospab/ostp/releases). Документация для пользователя:
[docs/ru/client.md](../docs/ru/client.md).

Сборка — командами выше. Нужны Rust с Android-таргетами, [cargo-ndk](https://github.com/bbqsrc/cargo-ndk), Android NDK и Flutter. Релизную
сборку нужно подписать своим ключом: APK с отладочной подписью не установится поверх ранее установленного релиза.
