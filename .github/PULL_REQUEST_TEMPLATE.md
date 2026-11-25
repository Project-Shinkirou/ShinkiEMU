## Summary / 变更说明

<!-- Briefly describe what changed and why. / 简述改动内容与原因。 -->

## Related Issues or Discussions / 关联 Issue 或讨论

<!-- Closes #... / Related to #... -->

## Verification / 验证记录

<!-- List the exact commands and checks you ran. / 列出实际执行的检查与测试命令。 -->

```sh
# e.g.
# cargo xtask check --target x86_64-pc-windows-msvc --manager
# cargo xtask test
# cargo xtask metadata validate
```

## Checklist / 提交检查清单

- [ ] No commercial game assets, scripts, saves, keys, or unredacted logs are included / 不含商业游戏素材、正文、存档、密钥或未脱敏日志
- [ ] Existing behavior regressions pass and new behavior changes are tested / 既有回归测试通过，行为变化已补充验证
- [ ] Updated trilingual docs (`docs/{zh-cn,en,ja}/`) or UI translations (`.po`) if user-facing behavior changed / 若涉及用户可见改动，已同步更新三语文档或 `.po` 界面翻译
- [ ] External-core changes (if any) were reviewed on the declared adapter branch and pinned in `build/cores.toml` / 外部核心改动（如有）已在声明的适配分支审查并在 `build/cores.toml` 固定提交
