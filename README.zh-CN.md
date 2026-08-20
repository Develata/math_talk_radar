# math_talk_radar — 数学演讲雷达

用于发现公开数学会议、演讲、讲座系列、录像、讲义及相关资源的雷达。
执行确定性的采集与粗排；解释与摘要留给下游人类或 AI 代理。

v0.1 是纯 Rust CLI —— 无 LLM、无浏览器自动化、无 JS 运行时。

## 状态

v0.1.0 就绪。M0–M7 完成；65/65 验收用例全绿。详见
`docs/report/implementation-status.md`。

## 安装

每个 release 附带静态 musl 二进制。从
[releases 页面](https://github.com/Develata/math_talk_radar/releases) 下载，
校验 SHA-256 后置于 `PATH`。或从源码构建：

```bash
cargo build --release
```

## 快速上手

```bash
math_talk_radar scan --after 180 | jq
math_talk_radar sources list
math_talk_radar doctor
math_talk_radar schema
```

`stdout` 为结构化 JSON（schema `"1.0"`）；`stderr` 为日志。

## 配置

用户配置位于 `$XDG_CONFIG_HOME/math_talk_radar/`（默认
`~/.config/math_talk_radar/`）。示例见 `config/`：

- `sources.toml` —— 来源定义（M6 推广已审计条目）。
- `scholars.toml` —— 学者别名（与任何解析器解耦）。
- `topics.toml` —— 规范主题 + 别名。
- `interests.example.toml` —— 兴趣权重，仅调整排序、不删除事件。

详见 `docs/reference/config-schema.md`。

## 自更新与卸载

```bash
math_talk_radar update --check
math_talk_radar update
math_talk_radar uninstall --keep-data --dry-run
math_talk_radar uninstall --keep-data --yes
```

自更新校验 SHA-256、保留回滚副本，失败时绝不删除可用二进制。卸载仅
删除已知应用自有路径，除非显式 `--force-unmanaged`，否则保护
`cargo run` 开发二进制。

## 开发

```bash
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo xtask check          # source-registry + acceptance-matrix + doc coverage
cargo xtask check-matrix   # acceptance-matrix 结构校验
```

在 `main` 上工作，每个已验证里程碑（M0–M8）一个原子提交。无特性分支，
无 PR。

## 文档

- 工程契约：`docs/plan/00_engineering_constitution.md`
- 路线图：`docs/tasks/implementation-roadmap.md`
- 验收矩阵：`docs/registry/acceptance-matrix.tsv`
- 运行手册：`docs/runbook.md`
- ADR：`docs/adr/`

## 许可证

MIT（`LICENSE`）。
