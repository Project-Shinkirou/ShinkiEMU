# Security Policy / 安全政策

[English](#english) · [简体中文](#简体中文)

---

<a id="english"></a>

## English

### Reporting a Vulnerability

Please **do not** report security vulnerabilities through public GitHub issues or discussions.

Use **[GitHub Private Vulnerability Reporting](https://github.com/Project-Shinkirou/ShinkiEMU/security/advisories/new)** to submit a private advisory directly to the maintainers.

### Areas of Particular Interest

- **Luau patch sandbox** — path traversal, symlink escapes, unauthorized file writes/EXE modifications, or bypasses of declared HTTPS/SHA-256 download restrictions.
- **Store authentication & secret storage** — credential leakage into logs, install receipts, webview cookies, or disk outside the OS secret store.
- **Core package & metadata verification** — archive path traversal, signature/hash bypasses, or unsafe loading of untrusted metadata snapshots.
- **Memory safety across the C ABI v3 boundary** — out-of-bounds access or frame-lease use-after-free in `shinki-emu-abi` or in-repo cores.

*(Vulnerabilities originating strictly in an upstream third-party engine should also be coordinated with the corresponding upstream project.)*

### What to Include

- Affected component, version (e.g. `Manager 0.1.0-beta.8` or core version), and platform/architecture.
- Clear reproduction steps or a minimal synthetic proof of concept (no commercial game files or real account tokens).

We aim to acknowledge private reports promptly and coordinate disclosure once a fix is verified.

---

<a id="简体中文"></a>

## 简体中文

### 报告安全漏洞

请**不要**通过公开的 GitHub Issue 或 Discussions 披露安全漏洞。

请直接通过 **[GitHub Private Vulnerability Reporting](https://github.com/Project-Shinkirou/ShinkiEMU/security/advisories/new)** 私密提交安全通告给项目维护者。

### 重点关注范围

- **Luau 补丁沙箱**：路径穿越、符号链接逃逸、绕过限制写入/修改 EXE、绕过 HTTPS 或 SHA-256 下载声明校验；
- **商店登录与凭据存储**：账号 Token 或 Cookie 泄露到日志、安装回执、WebView 持久化目录或系统密钥库之外；
- **核心包与元数据校验**：压缩包路径穿越、哈希校验绕过或不可信快照加载风险；
- **C ABI v3 边界与内存安全**：`shinki-emu-abi` 或自研核心中的越界读写与帧租约生命周期问题。

*（若漏洞纯粹位于外部上游引擎本体，建议同步协调对应上游项目。）*

### 提交建议

- 说明受影响组件、版本号（如 `Manager 0.1.0-beta.8` 或具体核心版本）及平台/架构；
- 提供清晰的复现步骤或最小合成 PoC（切勿附带商业游戏文件或真实账号凭据）。

维护者会在收到私密通告后尽快响应，并在修复验证后协调公开披露。
