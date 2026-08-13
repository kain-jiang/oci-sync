# 架构设计（Architecture Design）

> 版本：0.7.0（Rust 重写）| 更新：2026-08-13
> 功能对齐 Go 版 v0.6.0；交互见 `interaction.md`；实现细节见 `implementation.md`。

## 1. 概述

`oci-sync` 将本地文件/目录以 OCI artifact 形式同步到任意兼容 OCI Distribution Spec 的镜像仓库，支持可选 AES-256-GCM 加密、配置文件/Docker credential store 认证、本地操作历史。

## 2. 整体架构

```
┌──────────────────────────────────────────────────────────┐
│                       CLI 层 (src/cli)                    │
│  args.rs(clap) → dispatch.rs → logging.rs                 │
└───────────────┬──────────────────────────────────────────┘
                │
        ┌───────▼────────┐      ┌─────────────────────────┐
        │   app 层         │      │  output.rs              │
        │  push/pull/...  │─────▶│  表格/JSON/YAML/确认提示  │
        │  业务编排+活动记录 │      └─────────────────────────┘
        └───┬────────┬───┘
            │        │
   ┌────────▼──┐  ┌──▼─────────┐
   │  archive  │  │    oci      │
   │  tar.gz   │  │ oci-distro │──▶ reqwest(直连补足:
   │ 打包/解包  │  │ + reqwest  │    catalog/delete)
   └───────────┘  └──┬─────────┘
            │        │
   ┌────────▼──┐  ┌──▼──────────────────┐
   │  crypto   │  │  Docker Credential   │
   │ AES-256-  │  │  Store + config      │
   │ GCM+scrypt│  │  auths 解析          │
   └───────────┘  └─────────────────────┘
            │
   ┌────────▼─────────┐   ┌───────────┐
   │  cache            │   │   tui     │
   │ activity.json    │   │ ratatui   │
   └──────────────────┘   └───────────┘
```

### 数据流

**push**：
```
本地路径 → [archive::pack] → tar.gz bytes
        → [crypto::encrypt]（可选）→ 加密 bytes
        → [oci::push] → OCI manifest + layer → Registry
        → [可选 --verify] 拉回校验 → [cache::add]
```

**pull**：
```
Registry → [oci::is_encrypted] 检查加密（仅 manifest，快速失败）
        → 校验 --passphrase → [oci::pull] → layer bytes + 加密标记
        → [crypto::decrypt]（若加密）→ tar.gz bytes
        → [archive::unpack] → 本地（防路径穿越）
        → [cache::add]
```

**list**：`<registry>/<repo>` → repo.Tags 遍历；裸 `<registry>` → catalog（`/v2/_catalog`）→ 各 repo 遍历 → 只收 `io.oci-sync.version` 标记的 → 输出层筛选/格式化。

**label set/unset**：拉 manifest → 改 annotations → 推新 manifest → 重新打 tag。

**delete**：解析 tag → 确认 → HTTP DELETE manifest。

**alias**：读配置 → 改 shortcuts → 写回。

**recent**：读 `~/.cache/oci-sync/activity.json`。

## 3. 模块设计

### 3.1 `src/archive` — tar.gz 打包/解包

| 函数 | 签名 | 说明 |
|---|---|---|
| `pack` | `(src: &Path) -> Result<Vec<u8>>` | 文件或目录 → tar.gz 字节 |
| `unpack` | `(data: &[u8], dest: &Path) -> Result<()>` | 解包到 dest，含路径穿越防护 |

- 目录打包：根条目为目录 basename，保留完整子结构（`filepath.Walk` 语义 → `walkdir` 或手写递归）
- 单文件：根条目为 basename
- 解包：`dest.join(entry)` 解析后必须 `starts_with(dest)`，否则报错中止；跳过 symlink/设备等特殊条目

### 3.2 `src/crypto` — 加密/解密

| 函数 | 签名 | 说明 |
|---|---|---|
| `encrypt` | `(data: &[u8], passphrase: &str) -> Result<Vec<u8>>` | 加密 |
| `decrypt` | `(data: &[u8], passphrase: &str) -> Result<Vec<u8>>` | 解密 |

- KDF：scrypt(N=32768=2^15, r=8, p=1) → 32B 密钥
- 加密：AES-256-GCM（认证加密）
- 布局：`[salt(32B)][nonce(12B)][ciphertext+16B tag]`
- 每次加密新随机 salt + nonce；解密长度 < 60B 报错；GCM 认证失败 → "wrong passphrase?"

### 3.3 `src/oci` — OCI 交互

| 类型/函数 | 说明 |
|---|---|
| `OciClient::new(host, cfg)` | 按 host 解析凭据（config auths → docker credential store）|
| `push(repo, tag, data, encrypted, labels)` | 推 artifact |
| `is_encrypted(repo, tag)` | 仅拉 manifest 判断 |
| `pull(repo, tag)` → `PullResult{data, encrypted}` | 拉 layer |
| `delete(repo, tag)` | HTTP DELETE（oci-distribution 未提供）|
| `list_repo(repo)` → `Vec<ArtifactInfo>` | 仓库级列表 |
| `list_registry()` → `Vec<ArtifactInfo>` | catalog 级列表（reqwest 直连）|
| `update_annotations(repo, tag, updates, removes)` | label set/unset |

**Artifact 结构**（与 Go 版一致）：

```json
{
  "schemaVersion": 2,
  "mediaType": "application/vnd.oci.image.manifest.v1+json",
  "config": {
    "mediaType": "application/vnd.oci.image.config.v1+json",
    "digest": "sha256:<空配置{}>",
    "size": 2
  },
  "layers": [{ "mediaType": "application/octet-stream", "digest": "...", "size": N }],
  "annotations": {
    "io.oci-sync.encrypted": "true|false",
    "io.oci-sync.version": "0.7.0",
    "<user label k>": "<v>"
  }
}
```

**ArtifactInfo**：`full_name / repo / tag / digest / encrypted / version / size / labels`（labels 为 annotations 中非 `io.oci-sync.` 前缀的键）。

**认证**：config `auths.<host>` 优先；否则 `docker_credential` crate 读 `~/.docker/config.json`（支持 credsStore/credHelpers）。

### 3.4 `src/config` — 配置

| 函数 | 说明 |
|---|---|
| `load()` | cwd `./oci-sync.yaml` → `~/.config/oci-sync/oci-sync.yaml`，缺省返回空配置 |
| `save_to(cfg, path)` / `save_user(cfg)` | 写配置 |
| `registry_auth(host)` | 取凭据 |
| `shortcut_repo(name)` | 校验（无 tag/digest）后返回 repo |
| `shortcut_remote_ref(name, tag)` | `repo:tag` 拼接 |
| `all_shortcuts()` | 排序后的 (name, repo) 列表 |

### 3.5 `src/cache` — 活动历史

`Activity{kind: push|pull|delete|label, timestamp, remote_ref, local_path?, labels?, success, error?}`；JSON 持久化到 `~/.cache/oci-sync/activity.json`；上限 100 条，最新在前；`add/recent/stats/clear`。

### 3.6 `src/output` — 输出与交互原语

- `render_artifacts(arts, label_rules, format)`：表格（tabled rounded）/ JSON / YAML
- `filter_by_labels` / `filter_by_tags`
- `confirm(question) -> bool`：TTY 交互确认（非 TTY 报错提示 `--yes`）
- `format_bytes(n)`：人类可读大小

### 3.7 `src/tui` — 全屏界面

ratatui + crossterm 双栏（shortcuts | artifacts）+ 详情区 + 状态栏 + 弹窗。完整键位见 `interaction.md` §3。

### 3.8 `src/xdg` — 目录解析

`config_dir / cache_dir / data_dir`（`dirs` crate）。

## 4. 依赖

见 `implementation.md` §2（Cargo.toml 全文）。

## 5. 安全考量

| 风险 | 缓解 |
|---|---|
| 密码暴力破解 | scrypt N=32768 高内存成本 |
| 篡改/重放 | AES-GCM 认证标签，解密失败即报错 |
| nonce 重用 | 每次随机 |
| 路径穿越 | unpack 严格 `starts_with(dest)` 校验 |
| 凭据泄露 | 凭据不入日志；优先系统 credential store |
| 误删远程 artifact | delete 强制确认（TTY）或 `--yes` |

## 6. 兼容性矩阵

| 项 | Go 版 | Rust 版 |
|---|---|---|
| manifest annotation 键 | `io.oci-sync.encrypted/version` | 同（必须）|
| 加密布局 | salt32+nonce12+ct | 同（必须）|
| scrypt 参数 | N=32768, r=8, p=1 | 同（必须）|
| tar 根条目 | 目录 basename / 文件 basename | 同（必须）|
| 配置路径/格式 | `oci-sync.yaml`，shortcuts+auths | 同（必须）|
| 活动缓存 | `~/.cache/oci-sync/activity.json` | 同（必须）|
| CLI 标志 | `--tag` 仅长标志 | 增加 `-t`；新增 `--verify/--yes/--force/-t/--stats/completion` |
| delete | 直接删 | 确认 + `--yes` |
