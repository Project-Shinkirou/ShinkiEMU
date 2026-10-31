# ShinkiEMU Manager 0.1.0-beta.6

Musica uses the original eden/Perseus `.sav` slots and matching `.png` thumbnails for manual saves, quick saves and autosaves. Old beta `.asav` files are incompatible and remain untouched in their old directories. There is no conversion or fallback reader.

Musica is also statically embedded in the iOS Manager for the device and both simulator architectures, alongside its Android and experimental OHOS integrations. iOS does not support installing a separate dynamic core. The integrated release requires its own exact-head build, package, signature, native-dependency and corresponding-source gates; isolated-port results do not replace those gates.

The OHOS HAP remains experimental and signed with the OpenHarmony SDK test PKI. Device/emulator/game/account launch checks are waived; retail-device installability is unverified. Corresponding source and dependency notices accompany every published platform package. Published Manager beta4 and beta5 assets remain unchanged.
