# 功能规格（Features）

> 对应原 Go 版 FEATURE.md；功能保持一致，交互按 `docs/interaction.md` 重新设计。
> 版本：0.7.0（Rust 重写）

## 1. 核心功能

### 1.1 push

将本地文件或目录同步到 OCI 兼容镜像仓库。

```
oci-sync push --local <path> --remote <registry>/<repository>:<tag> [--passphrase <pwd>] [--label k=v]... [--verify]
```

- `--local`：文件或目录（必填）
- `--remote`：`<registry>/<repository>:<tag>`（必填）
- `--passphrase`：提供则加密（AES-256-GCM，scrypt KDF）
- `--label`：可重复，`key=value`，value 可为空字符串
- `--verify`：推送完成后拉取回本地校验（新增交互增强）

### 1.2 pull

```
oci-sync pull --remote <ref> --local <dir> [--passphrase <pwd>] [--force]
```

- 先拉 manifest 检查加密状态：已加密但缺 `--passphrase` → **快速失败**（不下载数据）
- 未加密但提供了 `--passphrase` → 警告并忽略
- 目标目录已存在时默认**拒绝覆盖**并报错（安全默认，与 delete 确认同理）；`--force` 允许覆盖（对 Go 版"静默覆盖"的改进）

### 1.3 delete

```
oci-sync delete --remote <ref> [--yes]
```

- **默认要求确认**（TTY 下交互确认，非 TTY 必须 `--yes`）——对 Go 版"直接删除"的安全改进
- 确认前显示解析出的 digest

### 1.4 list

```
oci-sync list --remote <repo|registry> [--format table|json|yaml] [--label k[=v]]... [--tag t]...
```

- `<registry>/<repository>` → 列出该仓库所有 tag；裸 `<registry>` → catalog 扫描全注册表
- 只列出带 `io.oci-sync.version` annotation 的 artifact（即本工具上传的）
- `--label key=value` 精确匹配；裸 `--label key` 检查 key 存在
- `--tag`：只显示指定 tag（新增交互增强）

### 1.5 label

```
oci-sync label set --remote <ref> <key=value>...
oci-sync label unset --remote <ref> <key>...
```

- 修改 manifest annotations 后推送新 manifest 并重新指向 tag
- set 时 value 可为空字符串

### 1.6 alias（shortcuts 管理）

```
oci-sync alias list
oci-sync alias add <name> --repo <registry>/<repository>
oci-sync alias remove <name>
```

- 读写 `~/.config/oci-sync/oci-sync.yaml`（或 cwd 配置，若可写）
- 配置不可写时输出警告但不报错（与 Go 版一致）

### 1.7 recent

```
oci-sync recent [--limit n] [--format table|json|yaml] [--clear] [--stats]
```

- 记录 push/pull/delete/label 操作：类型、时间、远程引用、本地路径、标签、结果
- 存储：`~/.cache/oci-sync/activity.json`（XDG 兼容），上限 100 条，最新在前
- `--stats`：按类型统计（新增）

### 1.8 tui

全屏交互界面，管理 shortcuts 的 artifacts。见 `docs/interaction.md` §TUI。

### 1.9 shortcut 动态命令

```
oci-sync <name> push -l <path> -t <tag> [--passphrase] [--label k=v] [--verify]
oci-sync <name> pull -t <tag> -l <dir> [--passphrase] [--force]
oci-sync <name> list [--format] [--label] [--tag]
oci-sync <name> delete -t <tag> [--yes]
```

- 仓库来自 `shortcuts.<name>.repo`，`--tag` 组合出完整引用
- repo 校验：不允许含 `@`（digest）或 `:` 在最后一个 `/` 之后（tag）
- `--tag` 增加 `-t` 短标志（Go 版只有长标志）

### 1.10 completion（新增）

```
oci-sync completion bash|zsh|fish|powershell
```

输出补全脚本到 stdout（clap_complete）。

## 2. 认证

| 优先级 | 来源 | 说明 |
|---|---|---|
| 1 | 配置 `auths.<registry>.username/password` | 每仓库独立凭据 |
| 2 | Docker credential store | `~/.docker/config.json`，支持 credsStore/credHelpers（macOS keychain、Windows Credential Manager、Linux secret service） |

## 3. 配置文件

搜索路径：`./oci-sync.yaml` → `~/.config/oci-sync/oci-sync.yaml`（第一个存在者生效）

```yaml
shortcuts:
  x:
    repo: registry.example.com/myteam/files
auths:
  registry.example.com:
    username: myuser
    password: mytoken
```

## 4. 兼容性要求

- manifest annotations 键名与 Go 版一致：`io.oci-sync.encrypted`（"true"/"false"）、`io.oci-sync.version`
- 加密字节格式与 Go 版一致：`[salt(32B)][nonce(12B)][ciphertext+GCM tag]`，scrypt(N=32768, r=8, p=1)
- 打包格式一致：目录以顶层目录名为根，单文件以 basename 为根
- 即：Rust 版必须能拉取/列出 Go 版推送的 artifact，反之亦然
