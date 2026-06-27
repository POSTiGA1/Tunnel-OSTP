$ErrorActionPreference = "Stop"

Write-Host "Building OSTP JNI for Android (arm64-v8a and armeabi-v7a)..."

$jniLibs = "$PSScriptRoot\android\app\src\main\jniLibs"
New-Item -ItemType Directory -Force -Path "$jniLibs\arm64-v8a" | Out-Null
New-Item -ItemType Directory -Force -Path "$jniLibs\armeabi-v7a" | Out-Null

Push-Location "$PSScriptRoot\..\ostp-jni"

Write-Host "Compiling for aarch64-linux-android and armv7-linux-androideabi..."
cargo ndk -t arm64-v8a -t armeabi-v7a -o "$jniLibs" build --release

# tun2socks removed in 0.4.0 — the native OSTP TUN stack is the only path,
# so no external tun2socks binary is downloaded or bundled.

Pop-Location

Write-Host "Done! The .so files have been copied to $jniLibs"
