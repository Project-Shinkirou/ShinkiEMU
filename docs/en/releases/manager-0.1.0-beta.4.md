---
title: Manager 0.1.0-beta.4
description: Manager language selection, Android embedded cores and native release packages.
---

# Manager 0.1.0-beta.4

Manager adds English, Simplified Chinese and Japanese interface translations, including persistent language selection.

Android Manager now embeds Artemis, Siglus and RFVP together. Artemis uses Luau; the embedded cores share the Manager's graphics and VM dependency graph. Android downloads are direct APKs. Packages carry the ShinkiEMU application icon, and package names no longer contain `adhoc`.

Core release catalog PRs are validated by the trusted metadata workflow and merged automatically after validation. This release publishes Manager only; existing beta.3 downloadable cores retain native ABI v3 compatibility. The Android core revisions are included inside the Manager APK and are not separate core releases.

HarmonyOS is excluded because its host acceptance gates had not passed at the integration cutoff.
