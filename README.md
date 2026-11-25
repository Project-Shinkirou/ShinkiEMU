<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/readme-banner-dark.png">
  <img src="docs/assets/readme-banner-light.png" alt="ShinkiEMU — Cross-platform library manager and native cores for visual novels">
</picture>

<div align="center">

# ShinkiEMU

**One native home for your visual novel library — across desktop and mobile.**

[![CI](https://github.com/Project-Shinkirou/ShinkiEMU/actions/workflows/ci.yml/badge.svg)](https://github.com/Project-Shinkirou/ShinkiEMU/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/Project-Shinkirou/ShinkiEMU?include_prereleases&sort=date&label=release&color=0284c7)](https://github.com/Project-Shinkirou/ShinkiEMU/releases)
[![License](https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-38bdf8)](LICENSE-MIT)
[![Discussions](https://img.shields.io/github/discussions/Project-Shinkirou/ShinkiEMU?color=f59e0b)](https://github.com/Project-Shinkirou/ShinkiEMU/discussions)

English · [简体中文](README.zh-CN.md) · [日本語](README.ja.md)

[Portal](https://project-shinkirou.github.io/en/shinkiemu/) · [Download](https://project-shinkirou.github.io/en/shinkiemu/#download) · [Compatibility](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) · [Cores](https://project-shinkirou.github.io/en/shinkiemu/cores/) · [Patches](https://project-shinkirou.github.io/en/shinkiemu/patches/) · [Docs](docs/en/index.md) · [Discussions](https://github.com/Project-Shinkirou/ShinkiEMU/discussions)

</div>

---

ShinkiEMU is a cross-platform library manager for visual novels. Manager organizes games, installs native cores, looks up metadata, downloads owned PC titles and applies community patches.

> [!IMPORTANT]
> **Free & Open Source · Beta Software**
> - **ShinkiEMU is 100% free and open source.** Download it only from our [official portal](https://project-shinkirou.github.io/en/shinkiemu/#download) or [GitHub Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases). If you paid someone for this software, request a refund and report the seller.
> - Engine detection alone does not guarantee playability. Check the [Compatibility Catalog](https://project-shinkirou.github.io/en/shinkiemu/compatibility/) for per-game, per-platform reports.

## Highlights

- **Native Engine Cores** — Each core preserves the engine's own VM, rendering, audio, menus and saves. Desktop installs cores independently. The current Android source combines bundled Impacto with separately installed, verified cores; iOS and OpenHarmony use bundled cores.
- **Unified Library & Metadata** — Organize local game folders with automatic core detection, Bangumi & VNDB metadata lookup, and a trilingual UI (Simplified Chinese, English, Japanese) with light and dark themes.
- **Built-in Store Downloads** — Sign in directly to **Steam** or **DLsite** inside the Manager to download PC titles you own, with incremental updates and file verification for Steam. Credentials stay strictly in your OS secret store. *(DMM GAMES/FANZA currently supports sign-in only.)*
- **Sandboxed Luau Patches** — Apply reviewed localization or compatibility patches from the [Patch Library](https://project-shinkirou.github.io/en/shinkiemu/patches/) or load local `.luau`/ZIP scripts. By default, patches write to a new directory leaving your original game untouched; in-place mode keeps automatic hash-verified backups for one-click restore.

## Preview

<table>
  <tr>
    <td width="50%">
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="docs/assets/shots/dark/library-en.webp">
        <img src="docs/assets/shots/light/library-en.webp" alt="ShinkiEMU Manager library view">
      </picture>
    </td>
    <td width="50%">
      <picture>
        <source media="(prefers-color-scheme: dark)" srcset="docs/assets/shots/dark/cores-en.webp">
        <img src="docs/assets/shots/light/cores-en.webp" alt="ShinkiEMU Manager core catalog view">
      </picture>
    </td>
  </tr>
  <tr>
    <td align="center"><sub>Game Library &amp; Launch</sub></td>
    <td align="center"><sub>Modular Core Catalog</sub></td>
  </tr>
</table>

See more screenshots (Patches, Settings, and light/dark themes) on the [ShinkiEMU Portal](https://project-shinkirou.github.io/en/shinkiemu/).

## Quick Start

1. **Download** the Manager for your platform from the [Portal](https://project-shinkirou.github.io/en/shinkiemu/#download) or [GitHub Releases](https://github.com/Project-Shinkirou/ShinkiEMU/releases).
2. **Launch and add games** — Open Manager and use **Add folder** in the library header, or use **Download** to get an owned PC title after enabling networking. On Linux, install the [runtime libraries](docs/en/troubleshooting.md#linux-runtime-libraries).
3. **Install cores and play** — On desktop and on Android builds with a core catalog, install the required core from **Cores** if it is not already bundled. Select a game and choose **Play**.

Read the [3-Minute Quick Start](docs/en/quick-start.md) or full [User Guide](docs/en/user-guide.md) for details on saves, store logins, and patches.

## Supported Platforms & Cores

See the [build inventory](build/cores.toml) for current source targets, `android_dynamic` policies and publication blocks. On Android, the current source requires Android 9 (API 28) or later. It stages the KiriKiri Java hosts in Manager, but loads the admitted KiriKiri core packages only on activation. The external `cmvs-rs` core is publication-blocked pending real-game acceptance.

<details>
<summary><strong>Historical package inventory: Manager 0.1.0-beta.8</strong></summary>

Published cores at [`Manager 0.1.0-beta.8`](https://github.com/Project-Shinkirou/ShinkiEMU/releases/tag/manager-v0.1.0-beta.8):

| Engine Family | Core | Windows x64 | Linux x64 | macOS x64/arm64 | Android | iOS | OpenHarmony |
|---|---|---|---|---|---|---|---|
| FVP | [`rfvp`](docs/en/cores/rfvp.md) | ✓ | ✓ | ✓ | Built in | Built in | — |
| SiglusEngine | [`siglus_rs`](docs/en/cores/siglus-rs.md) | ✓ | ✓ | ✓ | Built in | Built in | — |
| Artemis | [`art3m1s-core`](docs/en/cores/art3m1s-core.md) | ✓ | ✓ | ✓ | Built in | — | — |
| Musica | [`shinki.emu.musica`](docs/en/cores/shinki-emu-musica.md) | ✓ | ✓ | ✓ | Built in | Built in | Built in |
| Musica | [`ReMinori`](docs/en/cores/reminori.md) | ✓ | ✓ | ✓ | — | Built in | — |
| CMVS | [`shinki.emu.cmvs`](docs/en/cores/shinki-emu-cmvs.md) | ✓ | ✓ | ✓ | Built in | — | — |
| KiriKiri | [`kirikiroid2-web`](docs/en/cores/kirikiroid2-web.md) | ✓ | ✓ | ✓ | Built in | — | — |
| MAGES. | [`impacto`](docs/en/cores/impacto.md) | ✓ | ✓ | ✓ | Built in | — | — |
| Ren'Py | [`renpy`](docs/en/cores/renpy.md) | ✓ | ✓ | ✓ | Built in | — | — |
| SoftPAL | [`sena-rs`](docs/en/cores/sena-rs.md) | ✓ | ✓ | ✓ | — | Built in | — |
| BGI / Ethornell | [`ethornell-rs`](docs/en/cores/ethornell-rs.md) | ✓ | ✓ | ✓ | — | Built in | — |

- **✓** = published in the desktop core catalog; **Built in** = statically linked into `Manager 0.1.0-beta.8` (8 cores on Android universal APK, 6 on iOS test builds, Musica on experimental OpenHarmony HAP).
- **Platform notes:** Android ships as one release-signed universal APK (`arm64-v8a` + `x86_64`); iOS ships ad-hoc device/simulator test builds; OpenHarmony ships an experimental SDK test-signed HAP (device launch and retail installation not yet verified).

</details>

<details>
<summary><strong>Technical format &amp; save details by core</strong></summary>

| Core | Engine Family | Primary Formats &amp; Scope |
|---|---|---|
| [`rfvp`](docs/en/cores/rfvp.md) | FVP | HCB bytecode |
| [`siglus_rs`](docs/en/cores/siglus-rs.md) | SiglusEngine | `Scene.pck` scene packages |
| [`sena-rs`](docs/en/cores/sena-rs.md) | SoftPAL | `SCRIPT.SRC` bytecode with its `.DAT` tables; scope is the native and Steam releases of Koikake |
| [`ethornell-rs`](docs/en/cores/ethornell-rs.md) | BGI / Ethornell | BCS scenarios, BP system programs and DSC-compressed archive entries |
| [`ReMinori`](docs/en/cores/reminori.md) | Musica | PAZ archives; built around eden* |
| [`shinki.emu.musica`](docs/en/cores/shinki-emu-musica.md) | Musica | PAZ archives and SC scripts; data-driven native saves and game-local generic persistence |
| [`art3m1s-core`](docs/en/cores/art3m1s-core.md) | Artemis | `system.ini`, PFS (PF6/PF8) archives and numbered `.pfs.NNN` patch volumes |
| [`renpy`](docs/en/cores/renpy.md) | Ren'Py | Runs the official Ren'Py 8.5.3 runtime |
| [`shinki.emu.cmvs`](docs/en/cores/shinki-emu-cmvs.md) | CMVS | CPZ5 archives and PS2A scripts |
| [`cmvs-rs`](docs/en/cores/cmvs-rs.md) | CMVS / CVNS | Only [upstream-registered releases](https://github.com/Shizuku-in/cmvs-rs/blob/b9fed68ba05f80444338331bb3999f14c8f4f976/crates/cmvs-core/profiles/games.txt); coexists with Shinki CMVS |
| [`kirikiroid2-web`](docs/en/cores/kirikiroid2-web.md) | KiriKiri | XP3 archives and KAG/TJS scripts |
| [`krkr2`](docs/en/cores/krkr2.md) | KiriKiri | Independent native C++ core; XP3 archives and KAG/TJS scripts |
| [`impacto`](docs/en/cores/impacto.md) | MAGES. | MAGES. SC3 scripts with per-game configuration |

External cores follow upstream save locations and formats. Files created by ShinkiEMU itself always stay inside `<game>/.shinki/`. Close any running game before backing up or exchanging saves.

See [Musica saves](docs/en/cores/shinki-emu-musica.md#saves) for the distinction between configured native layouts and game-local generic slots.

The [ADV32 core](docs/en/cores/shinki-emu-adv32.md) is under development.

</details>

## Community & How to Contribute

ShinkiEMU grows with its community — and you do **not** need to write Rust or C++ to make a real impact:

- **Report game compatibility** — Tested a title? Submit a [Compatibility Report](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=compatibility.yml) to help other players know what works on each platform.
- **Join the conversation** — Ask questions, share tips, or propose ideas in [GitHub Discussions](https://github.com/Project-Shinkirou/ShinkiEMU/discussions).
- **Translate & improve docs** — Help polish the Simplified Chinese, English, and Japanese UI (`.po`) or documentation.
- **Create Luau patches** — Write and submit reviewed localization/fix recipes via the [Patch Submission](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=patch.yml) form.
- **Develop Manager or native cores** — Check [CONTRIBUTING.md](CONTRIBUTING.md), our [Core ABI & SDK guide](docs/en/core-abi.md), and the built-in agent workflows in [`.agents/skills/`](.agents/skills/).

> [!NOTE]
> **Supporting the project:** Project-Shinkirou does **not** accept financial donations. The best ways to support ShinkiEMU are starring the repository, submitting accurate compatibility reports, helping newcomers in Discussions, and contributing patches, translations, or PRs.

### Reporting Issues

- Check the [FAQ](https://project-shinkirou.github.io/en/shinkiemu/faq/) and [Troubleshooting](docs/en/troubleshooting.md) first.
- [Bug Report](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=bug.yml) · [Compatibility Report](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=compatibility.yml) · [Luau Patch Submission](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=patch.yml) · [Feature / Engine Request](https://github.com/Project-Shinkirou/ShinkiEMU/issues/new?template=feature_request.yml)
- **Security vulnerabilities:** Please report privately via [GitHub Security Advisories](https://github.com/Project-Shinkirou/ShinkiEMU/security/advisories/new) (see [SECURITY.md](SECURITY.md)).

## Documentation

| Topic | English | 简体中文 | 日本語 |
|---|---|---|---|
| Overview | [index](docs/en/index.md) | [概览](docs/zh-cn/index.md) | [概要](docs/ja/index.md) |
| Quick start | [quick-start](docs/en/quick-start.md) | [快速上手](docs/zh-cn/quick-start.md) | [クイックスタート](docs/ja/quick-start.md) |
| User guide | [user-guide](docs/en/user-guide.md) | [用户指南](docs/zh-cn/user-guide.md) | [ユーザーガイド](docs/ja/user-guide.md) |
| Troubleshooting | [troubleshooting](docs/en/troubleshooting.md) | [故障排查](docs/zh-cn/troubleshooting.md) | [トラブルシューティング](docs/ja/troubleshooting.md) |
| Contributing | [contributing](docs/en/contributing.md) | [贡献指南](docs/zh-cn/contributing.md) | [コントリビュート](docs/ja/contributing.md) |
| Metadata & patches | [metadata](docs/en/metadata.md) | [元数据与补丁](docs/zh-cn/metadata.md) | [メタデータとパッチ](docs/ja/metadata.md) |
| Build from source | [build-and-release](docs/en/build-and-release.md) | [从源码构建](docs/zh-cn/build-and-release.md) | [ソースからのビルド](docs/ja/build-and-release.md) |
| Core ABI & SDK | [core-abi](docs/en/core-abi.md) | [核心 ABI 与 SDK](docs/zh-cn/core-abi.md) | [コア ABI と SDK](docs/ja/core-abi.md) |
| Headless runner | [headless](docs/en/headless.md) | [无窗口运行](docs/zh-cn/headless.md) | [ヘッドレス実行](docs/ja/headless.md) |
| Release runbook | [release-runbook](docs/en/release-runbook.md) | [发布维护手册](docs/zh-cn/release-runbook.md) | [リリース手順書](docs/ja/release-runbook.md) |

## Acknowledgements

The external cores adapt existing open-source engine implementations. Thank you to their authors and contributors. `shinki.emu.musica` and `shinki.emu.cmvs` are ShinkiEMU implementations.

| Core | Engine family | Upstream project | Author or organization | License |
|---|---|---|---|---|
| `rfvp` | FVP | [xmoezzz/rfvp](https://github.com/xmoezzz/rfvp) | xmoe ([@xmoezzz](https://github.com/xmoezzz)) | [MPL-2.0](https://github.com/xmoezzz/rfvp/blob/99435f22e94eaec27c0c1b9f22cc2a6b1910d7d1/LICENSE) |
| `siglus_rs` | SiglusEngine | [xmoezzz/siglus_rs](https://github.com/xmoezzz/siglus_rs) | xmoe ([@xmoezzz](https://github.com/xmoezzz)) | [MPL-2.0](https://github.com/xmoezzz/siglus_rs/blob/5f358ef1360b0439e92aed2b749e8f25d8721954/LICENSE-MPL-2.0) |
| `sena-rs` | SoftPAL | [xmoezzz/sena-rs](https://github.com/xmoezzz/sena-rs) | xmoe ([@xmoezzz](https://github.com/xmoezzz)) | [MPL-2.0](https://github.com/xmoezzz/sena-rs/blob/7f633dbb89408297827d6c1857eb8e97dc363dff/LICENSE-MPL-2.0) |
| `ethornell-rs` | BGI / Ethornell | [xmoezzz/ethornell-rs](https://github.com/xmoezzz/ethornell-rs) | xmoe ([@xmoezzz](https://github.com/xmoezzz)) | MPL-2.0 ([manifest](https://github.com/xmoezzz/ethornell-rs/blob/b5de417e4cff681bd2afb929e22e903d64602456/Cargo.toml)) |
| `ReMinori` | Musica | [luoyily/ReMinori](https://github.com/luoyily/ReMinori) | [@luoyily](https://github.com/luoyily) | [AGPL-3.0](https://github.com/luoyily/ReMinori/blob/79b00990324404bfbd496e7fa76c690cf040f744/LICENSE) |
| `art3m1s-core` | Artemis | [Alphaly2K/art3m1s-core](https://github.com/Alphaly2K/art3m1s-core) | Alphaly ([@Alphaly2K](https://github.com/Alphaly2K)) | [MPL-2.0](https://github.com/Alphaly2K/art3m1s-core/blob/ad90a62cb192b119b7cf8589aa7f87a94bd747c8/LICENSE); `crates/pf8` is [MIT](https://github.com/Alphaly2K/art3m1s-core/blob/ad90a62cb192b119b7cf8589aa7f87a94bd747c8/crates/pf8/LICENSE) |
| `renpy` | Ren'Py | [renpy/renpy](https://github.com/renpy/renpy) | The Ren'Py Visual Novel Engine Project ([@renpy](https://github.com/renpy)); source copyright Tom Rothamel | [MIT, with LGPL portions](https://github.com/renpy/renpy/blob/39895c1e017f0b36ffea2447d97eccd69d76ee1c/sphinx/source/license.rst) |
| `cmvs-rs` | CMVS / CVNS | [Shizuku-in/cmvs-rs](https://github.com/Shizuku-in/cmvs-rs) | [@Shizuku-in](https://github.com/Shizuku-in) and contributors | [MIT](https://github.com/Shizuku-in/cmvs-rs/blob/main/LICENSE-MIT) OR [Apache-2.0](https://github.com/Shizuku-in/cmvs-rs/blob/main/LICENSE-APACHE), plus upstream third-party notices |
| `kirikiroid2-web` | KiriKiri | [fenghengzhi/kirikiroid2-web](https://github.com/fenghengzhi/kirikiroid2-web), a fork of [2468785842/krkr2](https://github.com/2468785842/krkr2) | [@fenghengzhi](https://github.com/fenghengzhi); KiriKiri2 copyright W.Dee and contributors | [KiriKiri2 license and bundled component licenses](https://github.com/fenghengzhi/kirikiroid2-web/blob/2028cbccaa91498bd36f270d349ae711d0e8fed3/LICENSE) |
| `krkr2` | KiriKiri | [2468785842/krkr2](https://github.com/2468785842/krkr2) | KiriKiri copyright W.Dee and contributors; KrKr2 contributors | [KiriKiri and bundled component licenses](https://github.com/2468785842/krkr2/blob/dca7264572a63e753c1b07ca7053513d1751ce70/LICENSE) |
| `impacto` | MAGES. | [CommitteeOfZero/impacto](https://github.com/CommitteeOfZero/impacto) | Committee of Zero and contributors ([@CommitteeOfZero](https://github.com/CommitteeOfZero)) | [ISC](https://github.com/CommitteeOfZero/impacto/blob/3706ddd89d4ad4aed0e32e127355c4251f22e574/LICENSE) |

[`build/cores.toml`](build/cores.toml) records upstream attribution; [`Cargo.lock`](Cargo.lock) records the exact adapter fork revisions. See [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) for the full third-party notices.

## License & Brand

Project-owned code is dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. External cores, dependencies, and assets keep their respective licenses ([THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) · [ORIGINS.md](ORIGINS.md)). Distributors must meet applicable corresponding-source and relinking obligations. Brand assets and usage guidelines live in [Assets/Branding/BRANDING.md](Assets/Branding/BRANDING.md).

Shared runtime libraries come from our sister project, [RoukakuEngine](https://github.com/Project-Shinkirou/RoukakuEngine).
