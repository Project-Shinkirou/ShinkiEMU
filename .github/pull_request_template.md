## Summary / 变更说明

<!-- Briefly describe what changed and why. / 简述改动内容与原因。 -->

## Related Issues or Discussions / 关联 Issue 或讨论

<!-- Closes #... / Related to #... -->

## Verification / 验证记录

<!-- List the exact commands and results. For core changes, build and package on the declared Windows x64 and Android ARM64 targets. For Manager changes, build and package Windows x64; see CONTRIBUTING.md for Android QA. Use the x64 Visual Studio developer shell for Windows packaging. Do not commit logs, scratch captures or build records.
列出实际运行的命令与结果。核心改动需在已声明支持的 Windows x64 和 Android ARM64 上构建并打包。Manager 改动需在 Windows x64 上构建并打包；Android 验收见 CONTRIBUTING.md。Windows 打包使用 x64 Visual Studio 开发环境。请勿提交日志、临时截图或构建记录。 -->

```sh
# e.g.
# cargo build --locked --release -p shinki-emu-manager
# cargo xtask package --target x86_64-pc-windows-msvc --manager --version 0.1.0-qa.1 --out .work/manager-qa
# cargo test --locked -p xtask -p shinki-emu-build
# cargo xtask metadata validate
```

## Checklist / 提交检查清单

- [ ] No commercial game assets, scripts, saves, keys, or unredacted logs are included / 不含商业游戏素材、正文、存档、密钥或未脱敏日志
- [ ] Existing behavior regressions pass and new behavior changes are tested / 既有回归测试通过，行为变化已补充验证
- [ ] Updated trilingual docs (`docs/{zh-cn,en,ja}/`) or UI translations (`.po`) if user-facing behavior changed / 若涉及用户可见改动，已同步更新三语文档或 `.po` 界面翻译
- [ ] External-core changes (if any) were reviewed on the adapter branch declared in `build/cores.toml` and pinned in `Cargo.toml` and `Cargo.lock` / 外部核心改动（如有）已在 `build/cores.toml` 声明的适配分支审查，并在 `Cargo.toml` 和 `Cargo.lock` 锁定提交
