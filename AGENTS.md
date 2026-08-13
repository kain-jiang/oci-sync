# oci-sync AI 编码规范（Rust 版）

AI 工具（Cursor、Copilot、OpenCode、Claude Code 等）协助开发此项目时，必须严格遵守以下规范。

## 🚫 Git 工作流（关键）

**⚠️ 禁止自动提交和推送（绝对规则）**

AI 绝对不能自动执行以下操作，除非用户明确、清晰地用中文或英文说"提交"、"push"、"commit"、"提交代码"等明确指令：

- ❌ `git commit` / `git commit --amend`
- ❌ `git push` / `git push --force`
- ❌ 任何其他强制/破坏性操作

**工作流程：**
1. AI 完成代码修改后，必须等待用户明确指示才能提交
2. 提交前必须验证所有检查通过（见下文）
3. 如果不确定，必须先询问用户
4. 永远不要假设用户想要提交

**提交前验证（必须全部通过）：**
```bash
cargo fmt --check          # 格式
cargo clippy --all-targets -- -D warnings   # lint（零警告）
cargo test --all-features   # 测试
cargo build --release       # 构建
```

## 📋 开发命令

**构建和测试工作流：**
```bash
cargo build                       # 开发构建
cargo build --release             # 发布构建
cargo run -- <args>               # 运行
cargo test                        # 单元测试
cargo clippy --all-targets -- -D warnings
cargo fmt
```

**集成测试：**
- 完整运行时检查：`cd e2e && uv run -m e2e`（Python e2e 项目，独立 uv 工程）
  - 必须：`OCI_SYNC_TEST_REPO` 环境变量（如 `registry.example.com/test/repo`）
  - 可选：`OCI_SYNC_TEST_TAG_BASE`、`OCI_SYNC_TEST_PASSPHRASE`
  - 覆盖 push/list/pull/delete/label 完整流程

**临时文件和测试数据：**
- Rust 构建产物：`target/` 目录（.gitignore）
- e2e 测试产物：`e2e/runtime-check/` 子目录
- 禁止在项目根目录创建临时文件

## 🔧 代码规范

**日志：** 只使用 `tracing`（`tracing::info!` / `warn!` / `error!` / `debug!`）。禁止用 `println!`/`eprintln!` 做日志（表格/JSON/YAML 等"结果输出"除外，见 `src/output.rs` 约定）。

**CLI/错误：** 所有命令描述（clap doc 注释）、错误消息和日志必须使用英文（即使需求是中文）。错误消息必须**可操作**：说明发生了什么 + 建议如何修复。

**安全：**
- 文件解包必须做路径穿越检查（解包目标必须落在 dest 目录内），见 `src/archive`
- 加密/解密格式必须与 Go 版字节级兼容（`docs/design.md` §数据格式）
- 认证凭据禁止写入日志

**依赖（已锁定，见 `docs/implementation.md` §Cargo 依赖）：**
- CLI：`clap` v4（derive）
- OCI：`oci-distribution` v0.11（必需且唯一；catalog/delete 用 `reqwest` 直连补足）
- TUI：`ratatui` v0.30 + `crossterm`
- 表格：`tabled`；进度条：`indicatif`
- 加密：`aes-gcm` + `scrypt`（RustCrypto）
- 配置：`serde` + `serde_yml`；JSON：`serde_json`
- 异步：`tokio`；错误：`anyhow` + `thiserror`
- Docker 凭据：`docker_credential`
- 日志：`tracing` + `tracing-subscriber`

禁止添加新依赖，除非有充分理由并在文档中说明。

## 📚 文档更新

**架构或 CLI 参数变更** 必须同步更新：
- `docs/design.md`（架构、数据流、模块 API、数据格式）
- `docs/interaction.md`（CLI 标志、TUI 键位、输出格式）
- `README.md`（用户端示例和工作流）

**功能添加或边界情况** 应更新 `docs/features.md`。

## 📁 项目结构（目标形态）

```
src/
├── main.rs            # 入口（tokio main）
├── lib.rs             # 模块声明
├── cli/               # clap 参数定义 + 分发 + 日志初始化
│   ├── mod.rs
│   ├── args.rs        # 全部 CLI 参数结构（单一事实来源）
│   ├── dispatch.rs    # 命令分发（含动态 shortcut 解析）
│   └── logging.rs     # tracing 初始化
├── app/               # 业务编排（薄层：校验→调用核心→记活动）
│   ├── mod.rs
│   ├── push.rs pull.rs delete.rs list.rs label.rs alias.rs recent.rs
│   ├── completion.rs  # shell 补全
│   └── tui.rs         # TUI 入口
├── archive/           # tar.gz 打包/解包
├── crypto/            # AES-256-GCM + scrypt
├── oci/               # OCI 仓库交互
│   ├── mod.rs         # 类型与常量
│   └── client.rs      # OciClient
├── config/            # YAML 配置
├── cache/             # 活动历史
├── output.rs          # 表格/JSON/YAML 输出、确认提示、格式化
├── tui/               # ratatui 界面（布局/键位/弹窗）
└── xdg/               # XDG 目录
```

## 🎯 关键实现注意事项

- **无 OCI 单元测试**：`src/oci` 依赖真实仓库，只做集成测试（e2e）。
- **配置发现**：cwd `./oci-sync.yaml` → `~/.config/oci-sync/oci-sync.yaml`。
- **动态命令**（`oci-sync <name> push|pull|list|delete`）：clap `external_subcommand` 捕获原始参数后二次解析，仓库来自 `shortcuts.<name>.repo`（必须无 tag/digest）。
- **兼容性**：manifest annotations（`io.oci-sync.*`）与加密字节格式必须与 Go 版一致，保证新旧 artifact 互通。
- **参考实现**：原 Go 版代码在 `/root/oci-sync-go-backup`（仅本机参考，勿提交）。
