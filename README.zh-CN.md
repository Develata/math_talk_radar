# math_talk_radar — 数学演讲雷达

用于发现公开数学会议、演讲、讲座系列、录像、讲义及相关资源的雷达。
执行确定性的采集与粗排；解释与摘要留给下游人类或 AI 代理。

v0.1 是纯 Rust CLI —— 无 LLM、无浏览器自动化、无 JS 运行时。

## 状态

v0.1.0 发布候选。65/65 验收用例通过；发布门槛覆盖格式、lint、workspace
测试、验收检查、依赖策略、覆盖率、MSRV、完整 synthetic pipeline baseline、
静态 musl 构建以及干净 Ubuntu 环境运行验证。详见
`docs/report/implementation-status.md`。

## 安装

v0.1 的预构建资产**仅支持 `x86_64-unknown-linux-musl`**。从
[releases 页面](https://github.com/Develata/math_talk_radar/releases) 下载，
校验随附的 SHA-256 与 GitHub build-provenance attestation 后置于 `PATH`。
其它目标请从源码构建：

```bash
cargo build --release
```

## 快速上手

```bash
math_talk_radar scan --after 180 | jq
math_talk_radar sources list
math_talk_radar doctor
math_talk_radar schema
math_talk_radar update --check
math_talk_radar uninstall --keep-data --dry-run
```

`stdout` 为结构化 JSON（schema `"1.0"`）；`stderr` 为日志。

## 配置

二进制内置 source、scholar 与 topic registry。`scan` 可通过显式 CLI 路径
覆盖 source/scholar registry，并可选加载 interest weights；需要持久化的
应用自有配置/数据遵循 XDG 路径。`config/` 包含内置 registry 与示例：

- `sources.toml` —— 来源定义（16 个已审计且启用的来源：5 RSS + 11 HTML-config）。
- `scholars.toml` —— 学者别名（与任何解析器解耦）。
- `topics.toml` —— 规范主题 + 别名。
- `interests.example.toml` —— 兴趣权重，仅调整排序、不删除事件。

source 配置采用 fail-closed：未知字段、重复 ID、无效 enabled entrypoint、
零 budget/depth 与不支持的 media strategy 会在扫描前直接拒绝。v0.1 的
`media_strategy` 仅支持 RSS source 上的 `youtube_channel`。

详见 `docs/reference/config-schema.md` 与 `docs/reference/cli.md`。

## 自更新与卸载

```bash
math_talk_radar update --check
math_talk_radar update
math_talk_radar uninstall --keep-data --dry-run
math_talk_radar uninstall --keep-data --yes
```

自更新校验 SHA-256、保留回滚副本，失败时绝不删除可用二进制；不支持的
预构建 target 会在任何网络/文件系统副作用之前拒绝。卸载仅删除已知应用
自有路径，除非显式 `--force-unmanaged`，否则保护 `cargo run` 开发二进制。

## 开发

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo xtask check          # source-registry + acceptance-matrix + doc coverage
cargo xtask check-matrix   # acceptance-matrix 结构校验
cargo xtask baseline       # tests + quality + RSS memory + 1k/5k/10k pipeline
cargo deny check           # licenses + advisories + bans + sources
```

完整 baseline 记录 wall time、peak RSS、redb 大小、JSON/JSONL streaming、
10k high-collision dedup、release binary 大小和 CLI startup。已有 RSS parser
内存 hard gate 仍为 ≤128 MiB；规模测试用于记录增长行为，不人为设置微型 SLA。

## 文档

- 工程契约：`docs/plan/00_engineering_constitution.md`
- 路线图：`docs/tasks/implementation-roadmap.md`
- 验收矩阵：`docs/registry/acceptance-matrix.tsv`
- 来源注册表：`docs/registry/source-registry.tsv`
- 运行手册：`docs/runbook.md`
- ADR：`docs/adr/`

## 许可证

MIT（`LICENSE`）。
