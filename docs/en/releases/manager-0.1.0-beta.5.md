# ShinkiEMU Manager 0.1.0-beta.5

This prerelease adds an experimental OHOS arm64 HAP to the existing Manager platforms. The HAP embeds the Slint Manager interface and uses the OpenHarmony native window, input, audio and WebView host integration.

The HAP is signed with the OpenHarmony SDK test PKI, not a production certificate. Local DevEco packaging verifies its signature and native-library dependency closure. The hosted release handoff checks the actual HAP and corresponding-source hashes, package layout, native closure and source revision. Device/emulator launch acceptance is waived; retail-device installability is unverified.

Hosted platform binaries retain their existing release gates. The Android universal APK uses the release signing key. Each published platform includes corresponding source and dependency notices. The beta4 release and its assets remain unchanged.
