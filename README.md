# oci-sync

将本地文件或目录同步到 OCI 兼容镜像仓库的 CLI 工具（Rust 重写版）。

> 功能与原 Go 版（v0.6.0）保持一致，交互与 UX 全新设计。原 Go 参考实现见 `/root/oci-sync-go-backup`。

## 功能一览

- `push` — 本地文件/目录 → tar.gz →（可选 AES-256-GCM 加密）→ OCI artifact → 镜像仓库
- `pull` — 从镜像仓库拉取 →（自动检测加密并解密）→ 解包到本地
- `delete` — 删除远程 artifact（带确认提示）
- `list` — 列出仓库或整个注册表中的 oci-sync artifact（支持标签筛选、多格式输出）
- `label set/unset` — 管理 artifact 标签
- `alias` — 管理快捷仓库（shortcuts）
- `recent` — 查看操作历史（本地缓存）
- `tui` — 全屏双栏交互界面
- `completion` — 生成 shell 补全脚本（bash/zsh/fish/powershell）
- `<name> push/pull/list/delete` — 动态快捷命令

## 使用

### push

```bash
# 推送目录（不加密）
oci-sync push --local ./mydir --remote registry.example.com/myrepo:latest

# 推送文件并加密
oci-sync push -l ./secret.txt -r registry.example.com/myrepo:encrypted --passphrase mypassword

# 推送并设置标签，推送后自动校验
oci-sync push -l ./data -r registry.example.com/myrepo:latest --label app=myapp --label env=prod --verify
```

### pull

```bash
# 拉取（内容加密时自动提示缺少 --passphrase）
oci-sync pull -r registry.example.com/myrepo:latest -l ./output

# 拉取并解密
oci-sync pull -r registry.example.com/myrepo:encrypted -l ./output --passphrase mypassword
```

### delete（带确认）

```bash
oci-sync delete -r registry.example.com/myrepo:old-tag     # 会先询问确认
oci-sync delete -r registry.example.com/myrepo:old-tag --yes   # 跳过确认
```

### list

```bash
oci-sync list -r registry.example.com/myrepo            # 仓库级
oci-sync list -r registry.example.com                   # 注册表级（catalog）
oci-sync list -r registry.example.com/myrepo --format json
oci-sync list -r registry.example.com/myrepo --label app=myapp
oci-sync list -r registry.example.com/myrepo --tag v1.0
```

### label

```bash
oci-sync label set -r registry.example.com/myrepo:tag app=web env=
oci-sync label unset -r registry.example.com/myrepo:tag app
```

### alias（快捷仓库）

```bash
oci-sync alias add x --repo registry.example.com/myteam/files
oci-sync alias list
oci-sync x push -l ./mydir -t latest
oci-sync x pull -t latest -l ./output
oci-sync x list --format json
oci-sync x delete -t old-release --yes
```

### recent

```bash
oci-sync recent                  # 最近 20 条
oci-sync recent -n 5 -f json
oci-sync recent --stats          # 各操作类型统计
oci-sync recent --clear
```

### tui

```bash
oci-sync tui
```

### 补全

```bash
oci-sync completion bash > /etc/bash_completion.d/oci-sync
oci-sync completion zsh > "${fpath[1]}/_oci-sync"
```

## 配置文件

搜索顺序：`./oci-sync.yaml` → `~/.config/oci-sync/oci-sync.yaml`

```yaml
shortcuts:
  x:
    repo: registry.example.com/myteam/files

auths:
  registry.example.com:
    username: myuser
    password: mytoken
```

认证优先级：**配置 `auths` > Docker credential store**（`docker login` 兼容）。

## 安装

```bash
cargo install --git https://github.com/kain-jiang/oci-sync.git
# 或本地构建
cargo build --release && install -m755 target/release/oci-sync /usr/local/bin/
```

## 设计文档

- [功能规格](docs/features.md)
- [交互设计](docs/interaction.md)
- [架构设计](docs/design.md)
- [AI 落地实现指南](docs/implementation.md)
- [测试策略](docs/testing.md)
