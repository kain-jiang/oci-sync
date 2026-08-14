# AI 落地实现指南（Implementation Guide）

> 本文档是 AI 实现 oci-sync Rust 版的落地依据。**代码已全部实现并发布**（提交 `2ee2d63` 起），本文档同步为"实现后的权威描述"，供后续维护/扩展参考。
> 配套：`design.md`（架构）、`interaction.md`（交互）、`features.md`（功能）、`testing.md`（测试）。
> 参考实现（Go 版）：`/root/oci-sync-go-backup`。

## 1. 起步

```bash
cd /root/projects/oci-sync
cargo build --release    # 构建
cargo test --all-features  # 测试
```

提交前验证（见 AGENTS.md）：`cargo fmt --check`、`cargo clippy --all-targets -- -D warnings`、`cargo test --all-features`、`cargo build --release`。

## 2. Cargo.toml（完整依赖清单）

```toml
[package]
name = "oci-sync"
version = "0.7.0"
edition = "2024"
rust-version = "1.85"
description = "Sync local files or directories to OCI-compatible image registries"
license = "MIT"

[dependencies]
# CLI
clap = { version = "4", features = ["derive", "wrap_help"] }
clap_complete = "4"
# 异步 & HTTP
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
futures = "0.3"
# OCI（唯一 OCI 客户端 crate）
oci-distribution = { version = "0.11", default-features = false, features = ["rustls-tls"] }
# Docker credential store/helpers（docker login 兼容）
docker_credential = "1"
# 序列化
serde = { version = "1", features = ["derive"] }
serde_json = "1"
serde_yml = "0.0.13"
# 打包
tar = "0.4"
flate2 = "1"
# 加密（RustCrypto）
aes-gcm = "0.11"
scrypt = "0.12"
rand = "0.8"
# 杂项
sha2 = "0.10"
hex = "0.4"
base64 = "0.22"
chrono = { version = "0.4", features = ["serde"] }
dirs = "5"
regex = "1"
anyhow = "1"
thiserror = "2"
# 日志
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
# UI
ratatui = { version = "0.30", features = ["crossterm"] }
crossterm = "0.28"
tabled = "0.21"
indicatif = "0.18"

[profile.release]
lto = true
codegen-units = 1
strip = true
```

> 已确认可用版本：oci-distribution 0.11.0、ratatui 0.30.2、serde_yml 0.0.13、tabled 0.21.0、indicatif 0.18.6、docker_credential 1.4.0、aes-gcm 0.11.0、scrypt 0.12.0、tar 0.4.46、flate2 1.1.9、thiserror 2.0.20、anyhow 1.0.104、tracing-subscriber 0.3.23、base64 0.22。
> 国内镜像：cargo 已配置 TUNA（`~/.cargo/config.toml`）；rustup 已配置 rsproxy.cn（`RUSTUP_DIST_SERVER`）。

## 3. 模块结构（目标形态）

```
src/
├── main.rs            # tokio main；cli::run() 错误 → eprintln + exit(1)
├── lib.rs             # pub mod 声明：app archive cache cli config crypto oci output tui xdg
├── cli/
│   ├── mod.rs         # pub async fn run() -> Result<()>：parse → logging → dispatch
│   ├── args.rs        # clap 结构体（§4 全文契约）
│   ├── dispatch.rs    # 分发 + shortcut 二次解析（§4.2）
│   └── logging.rs     # tracing-subscriber，RUST_LOG 覆盖，--quiet→error
├── app/
│   ├── mod.rs
│   ├── push.rs pull.rs delete.rs list.rs label.rs alias.rs recent.rs completion.rs tui.rs
├── archive/mod.rs
├── crypto/mod.rs
├── oci/mod.rs         # 常量/类型 + parse_ref
├── oci/client.rs      # OciClient
├── config/mod.rs
├── cache/mod.rs
├── output.rs
├── progress.rs        # 阶段进度指示器（indicatif，非 TTY/--quiet 静默）
├── tui/mod.rs         # 单文件：状态机 + 布局 + 弹窗/搜索/详情（app.rs/ui.rs 已合并）
└── xdg/mod.rs
```

## 4. CLI 契约（args.rs 必须实现的标志）

```
全局: --quiet/-q (global=true)
push:    -l/--local (必填)  -r/--remote (必填)  --passphrase  --label k=v (可重复)  --verify
pull:    -r/--remote (必填)  -l/--local (必填)  --passphrase  -f/--force
delete:  -r/--remote (必填)  -y/--yes
list:    -r/--remote (必填)  -f/--format (table|json|yaml, 默认 table)  --label k[=v] (可重复)  -t/--tag (可重复)
label:   set -r <ref> k=v... | unset -r <ref> k...
alias:   list | add NAME --repo <ref> | remove NAME
recent:  -n/--limit (默认20)  -f/--format  --clear  --stats
tui
completion: bash|zsh|fish|powershell
shortcut(external_subcommand): <name> push|pull|list|delete
```

### 4.1 动态 shortcut 的实现方式

```rust
#[derive(clap::Subcommand)]
pub enum Command {
    // ... 内置命令 ...
    /// Dynamic shortcut commands (from config shortcuts.<name>.repo)
    #[command(external_subcommand)]
    Shortcut(Vec<String>),   // e.g. ["x", "push", "-l", "./dir", "-t", "latest"]
}
```

分发（dispatch.rs）：
```rust
async fn dispatch_shortcut(cfg: &Config, raw: Vec<String>) -> Result<()> {
    let name = raw.first().context("shortcut requires a name")?;
    let sub  = raw.get(1).context("shortcut requires push|pull|list|delete")?;
    let rest = raw[2..].to_vec();
    match sub.as_str() {
        "push"   => { let a = ShortcutPushArgs::try_parse_from(once("push").chain(rest))?;
                      let remote = cfg.shortcut_remote_ref(name, &a.tag)?;
                      app::push::run_ref(cfg, &remote, &a.local, a.passphrase.as_deref(), &a.labels, a.verify).await }
        // pull / list / delete 同理
        other => bail!("shortcut {name:?}: unknown subcommand {other:?}"),
    }
}
```

每个 shortcut 子命令的独立 arg 结构体（`ShortcutPushArgs` 等）用 `clap::Args` 派生 + `try_parse_from` 二次解析。

## 5. 核心算法规格

### 5.1 archive（与 Go 版行为一致）

```rust
pub fn pack(src: &Path) -> Result<Vec<u8>> {
    // stat src；目录 → 递归遍历，根条目名 = src 的 basename（目录名末尾不加 / 但 typeflag=Dir）
    // 单文件 → 根条目名 = basename
    // gzip(flate2::write::GzEncoder, Compression::default) + tar::Builder
    // 文件头用 tar::Header::new_gnu()，设 mode（保留权限位）、size、mtime
}

pub fn unpack(data: &[u8], dest: &Path) -> Result<()> {
    // dest = dest.absolutize；MkdirAll(dest, 0755)
    // GzDecoder + tar::Archive::entries()
    // 每个 entry：target = dest.join(name)；校验 target.starts_with(dest) 否则 Err("illegal file path in archive: <name>")
    // TypeDir → create_dir_all；TypeFile → 建父目录 + 写文件（保留 mode）
    // 其他类型（symlink 等）→ 跳过
}
```

### 5.2 crypto（字节级兼容 Go 版）

```rust
pub const SALT_SIZE: usize = 32;
pub const NONCE_SIZE: usize = 12;
pub const SCRYPT_N_LOG: u8 = 15; // N = 2^15 = 32768
pub const SCRYPT_R: u32 = 8;
pub const SCRYPT_P: u32 = 1;

pub fn encrypt(data: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    // salt = rand 32B；key：Params::new(15, 8, 1) + scrypt::scrypt(passphrase, salt, &params, &mut key[..32])
    // nonce = rand 12B；Aes256Gcm::new(key.into())；cipher.encrypt(&nonce_from_slice(&nonce)?, data)
    // 返回 salt ‖ nonce ‖ ciphertext
}

pub fn decrypt(data: &[u8], passphrase: &str) -> Result<Vec<u8>> {
    // len < 32+12+16 → Err("ciphertext too short")
    // salt=data[..32], nonce=data[32..44], ct=data[44..]
    // cipher.decrypt(&nonce_from_slice(nonce)?, ct) → Err 时: Err("decryption failed: wrong passphrase?")
}
```

> scrypt 0.12：`Params::new(log_n: u8, r: u32, p: u32)`（无输出长度参数，长度由输出缓冲决定）；AES-GCM nonce 用 `Nonce::<U12>::try_from(bytes)` 构造（`aes_gcm::aead::common::array::typenum::consts::U12`）。

### 5.3 oci（oci-distribution + reqwest 补足）

```rust
pub struct OciClient {
    host: String,
    client: oci_distribution::Client,   // 0.11 的 Client（非旧版 Registry）
    auth: RegistryAuth,                  // Anonymous | Basic(user, pass)
    http: reqwest::Client,               // catalog/delete 直连
    scheme: &'static str,                // localhost/127.0.0.1 → http，其余 https
}
```

- **凭据解析**（new）：`cfg.registry_auth(host)` 有 username+password → `RegistryAuth::Basic`；否则 `docker_credential::get_credential(host)` → Basic；都没有 → `Anonymous`（匿名仓库也可用）。
- **push**：构造 manifest（§design 3.3 的 JSON 结构，显式设置 `media_type = "application/vnd.oci.image.manifest.v1+json"`，与 Go 版一致），以 `client.push(&reference, &[layer], config, &auth, Some(manifest))` 推送。
- **pull**：`client.pull_manifest(&reference, &auth)` 拿 annotations（判断 encrypted）→ `client.pull(&reference, &auth, accepted_media_types)` 拿 layer blob。
- **delete**：`oci-distribution` **不提供** → `fetch_manifest_digest` 解析 digest 后直连 `DELETE {scheme}://{host}/v2/{repo}/manifests/{digest}`；支持 Basic 与 Bearer token challenge（`WWW-Authenticate` → token endpoint）。
- **list_repo**：`client.list_tags(&reference, &auth, None, None)` 拿全部 tags → 每个 tag `pull_manifest` → 有 `io.oci-sync.version` 才收录 → 组装 `ArtifactInfo`（labels = annotations 去掉 `io.oci-sync.` 前缀）。
- **list_registry**：`GET {scheme}://{host}/v2/_catalog?n=100`（带认证头，Bearer 回退）分页遍历 → 每 repo 走 list_repo 逻辑。
- **update_annotations**：pull_manifest → 改 annotations → `client.push_manifest(&reference, &OciManifest::Image(...))` 重新打 tag。
- **reference 解析**：`parse_ref` 规则：
  - 含 `/` → host=第一段，repo=其余（可能带 `:tag`）
  - 无 `/` → 视为裸 registry host（list 用，`localhost:5000` 不会被误拆成 tag）
  - tag 判定：最后一个 `:` 且其后无 `/` 才视为 tag（`localhost:5000/repo` 的端口不受影响）
  - 注：未实现 docker.io 的 `library/` 隐式前缀（与 Go 版行为一致，显式写全路径即可）

> `oci-distribution` 0.11 的 `RegistryAuth` 位于 `oci_distribution::secrets::RegistryAuth`，`Reference` 用 `oci_distribution::Reference::with_tag(registry, repo, tag)` 构造。若 API 与本文描述有出入，以 crate 文档为准并同步更新本文档。

### 5.4 config / cache / output

- config：serde_yml 反序列化（字段 `auths`、`shortcuts`）；路径搜索 cwd → xdg；`shortcut_repo` 校验规则（`@` 拒绝、`:` 在最后 `/` 之后拒绝）。
- cache：serde_json 序列化；`insert(0)` + `truncate(100)`；`chrono::Local` 时间戳。
- output：tabled `Builder` + `Style::rounded()`；JSON/YAML 全字段；`confirm()` 用 `std::io::IsTerminal`。

## 6. 关键风险与对策（实现后确认）

| 风险 | 对策（已落地） |
|---|---|
| `oci-distribution` 无 catalog/delete | reqwest 直连 `/v2/_catalog` 与 `DELETE /v2/...`，含 Bearer token challenge |
| `oci-distribution` 版本 API 漂移 | 0.11 实际 API（Client/RegistryAuth/with_tag）已核实并写入 §5.3 |
| scrypt/aes-gcm 新版本 API 变化 | scrypt 0.12 用底层 `scrypt::scrypt` 函数；nonce 用 `TryFrom` 构造（见 §5.2） |
| docker credential helper 平台差异 | `docker_credential` crate 处理；helper 缺失时回退 anonymous |
| 大文件内存占用 | v0.7 与 Go 版一致：整体入内存（`Vec<u8>`）；流式优化为后续规划（见 features.md 待办） |
| Go/Rust artifact 互通 | 按 §design 兼容矩阵实现；manifest 显式带 mediaType 字段与 Go 版一致；e2e 互通用例规划中（testing.md） |
| 进度/日志干扰 | indicatif 阶段指示器 + `stage_log!` 宏：TTY 用进度条、非 TTY 退化 INFO 日志、`--quiet` 全静默 |

## 7. 实现里程碑（全部完成 ✓）

1. **M1 骨架**：cargo init；模块目录；main/lib；`cli --help` 全命令树可解析（args.rs + dispatch.rs + todo!()）✓
2. **M2 基础模块**：xdg、config（含单测）、cache（含单测）、output（含单测）✓
3. **M3 archive + crypto**（含单测，纯函数，无网络）✓
4. **M4 oci 层**：client + push/pull/is_encrypted/list_repo（e2e 验证）✓
5. **M5 命令补全**：delete（含确认）、label、alias、list（含 catalog）、recent、completion ✓
6. **M6 交互增强**：进度条（indicatif 阶段指示器）、--verify、--force、错误消息润色 ✓
7. **M7 TUI**：双栏 + 详情 + 搜索 + 弹窗（按钮式 Pull/Delete、默认焦点 Cancel）+ toast + 顶栏 + spinner ✓
8. **M8 打磨**：clippy 零警告、文档同步、e2e 全绿、README 示例验证 ✓

后续扩展方向（未实现）：流式大文件传输、e2e 补充用例（重复 push/空目录/Go 互通，见 testing.md）、TUI 批量操作。

## 8. 定义完成（Definition of Done — 已达成）

- [x] `cargo build --release` 通过
- [x] `cargo test --all-features` 全绿（42 项单测）
- [x] `cargo clippy --all-targets -- -D warnings` 零警告
- [x] `cargo fmt --check` 通过
- [x] e2e（zot 本地仓库 + CI services）push/list/pull/delete/label 全流程通过（20 项断言）
- [ ] 与 Go 版 artifact 互通验证（规划中，需可访问旧仓库）
- [x] design/interaction/features/README 与实现一致（2026-08-14 审计后同步）
