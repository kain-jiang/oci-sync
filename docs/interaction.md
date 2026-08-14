# 交互设计（Interaction Design）

> 功能与 Go 版一致，但交互与 UX 全部重新设计。
> 原则：**安全默认**（破坏性操作要确认）、**清晰反馈**（进度可见、错误可行动）、**一致体验**（CLI/TUI 同一套心智模型）。

## 1. 全局设计原则

1. **安全默认**：`delete` 必须确认；`--yes` 显式跳过。宁可多一次回车，不可误删数据。
2. **错误可行动**：错误消息 = 发生了什么 + 怎么修。例如：
   - `content is encrypted, provide --passphrase`（而不是裸的 `401 Unauthorized`）
   - `shortcut "x" not found (add shortcuts.x.repo to config)`
3. **结果与日志分离**：
   - 日志（tracing）→ stderr，记录过程
   - 结果（表格/JSON/YAML）→ stdout，机器可解析
   - 管道场景（`| jq`）不受日志污染
4. **颜色与符号**：仅 TTY 时启用颜色；成功 `✓`、失败 `✗`、警告 `!`（非 TTY 用纯文本 ok/failed/warn）。
5. **进度可见**：push/pull 显示进度条（indicatif）；大文件打包/加密也显示阶段进度。
6. **短标志直觉化**：`-l` local、`-r` remote、`-t` tag、`-f` format、`-n` limit、`-q` quiet、`-y` yes。

## 2. CLI 交互

### 2.1 命令树

```
oci-sync [--quiet]
├── push    -l <path> -r <ref> [--passphrase] [--label k=v]* [--verify]
├── pull    -r <ref> -l <dir>  [--passphrase] [--force]
├── delete  -r <ref> [--yes]
├── list    -r <repo|registry> [-f table|json|yaml] [--label k[=v]]* [--tag t]*
├── label   set -r <ref> k=v... | unset -r <ref> k...
├── alias   list | add <name> --repo <ref> | remove <name>
├── recent  [-n 20] [-f table|json|yaml] [--clear] [--stats]
├── tui
├── completion <bash|zsh|fish|powershell>
└── <name>  push|pull|list|delete   （动态 shortcut）
```

### 2.2 输出风格

- **push 成功**：
  ```
  ✓ pushed registry.example.com/myrepo:latest (1.5 MiB, encrypted: no) in 3.2s
  ```
- **pull 成功**：
  ```
  ✓ pulled registry.example.com/myrepo:latest → ./output (1.5 MiB)
  ```
- **list 表格**（tabled，rounded 风格，对齐/截断友好）：
  ```
  REPO   TAG      ENCRYPTED  VERSION  SIZE     DIGEST                          LABELS
  myrepo latest   no         0.7.0    1.5 MiB  sha256:8f3a…c1e2                app=web,env=prod
  ```
- **JSON/YAML**：完整字段，不截断（机器消费）。

### 2.3 进度反馈（push 示例）

每个阶段由 spinner 指示器驱动（indicatif），阶段名 + 实时消息（当前大小/引用）：

```
⠹ Packing files...          →  ⠹ Packed ./mydir (12.3 MiB)
⠹ Encrypting...             →  ⠹ Encrypted (12.3 MiB)
⠹ Pushing registry.../repo  →  ✓ pushed registry.example.com/myrepo:latest (12.3 MiB) in 3.2s
```

- TTY：spinner + 阶段消息（stderr），完成时保留 `✓ ...` 结果行
- 非 TTY：进度条静默，退化为行式 INFO 日志（`Packing files...` / `Push successful ✓`）
- `--quiet`：无进度条、无日志，只显示错误

### 2.4 确认交互

```
$ oci-sync delete -r registry.example.com/myrepo:old
  Will delete: registry.example.com/myrepo:old (sha256:8f3a…c1e2)
  Continue? [y/N] _
```

- TTY：交互确认；非 TTY：报错提示加 `--yes`
- `delete --yes` 静默删除

### 2.5 动态 shortcut 命令

`oci-sync x push -l ./dir -t latest` → 解析为 `push -l ./dir -r <repo>:latest`

- `-t/--tag` 是 shortcut 命令的远程引用补充
- list 不需要 tag；delete 用 `-t` + `--yes`

## 3. TUI 设计（ratatui）

### 3.1 布局（三区 + 状态栏）

```
┌────────────────────────────────────────────────────────────────┐
│ oci-sync  v0.7.0                    [registry.example.com]  Q退出 │ ← 顶栏
├───────────────┬────────────────────────────────────────────────┤
│  SHORTCUTS    │  ARTIFACTS (myrepo)          搜索: /            │
│  ▸ x          │  TAG      ENCRYPTED  SIZE     VERSION  LABELS   │
│    y          │  latest   🔒 yes    1.5 MiB   0.7.0    app=web  │
│               │  v1.0     no        800 KiB   0.7.0             │
│               │                                                  │
├───────────────┴────────────────────────────────────────────────┤
│  DETAILS: latest                                                │
│  Full name: registry.example.com/myrepo:latest                  │
│  Digest: sha256:8f3a…c1e2   Version: 0.7.0   Size: 1.5 MiB      │
│  Encrypted: yes (AES-256-GCM)   Labels: app=web, env=prod       │
├────────────────────────────────────────────────────────────────┤
│  Tab/←→ 切换栏  ↑↓/jk 移动  / 搜索  p 拉取  d 删除  r 刷新  ? 帮助  q 退出 │ ← 快捷键提示
└────────────────────────────────────────────────────────────────┘
```

### 3.2 键位表

| 键 | 作用 |
|---|---|
| `Tab` / `←` `→` / `h` `l` | 切换焦点栏（shortcuts ↔ artifacts）|
| `↑` `↓` / `j` `k` | 焦点栏内移动 |
| `Enter` | shortcuts 栏：加载该仓库 tags；artifacts 栏：无操作（或展开详情）|
| `/` | 打开搜索输入框，实时过滤 tags（支持子串匹配）|
| `p` | 拉取选中 artifact：弹窗输入本地路径（预填当前目录）与 passphrase（掩码输入）|
| `d` | 删除选中 artifact：弹窗显示 tag+digest，需输入 `y` 或回车确认（红色警告样式）|
| `r` | 手动刷新当前 tags |
| `s` | 按大小排序（再按切换升降序）|
| `?` | 帮助视图（全键位说明）|
| `Esc` | 关闭弹窗 / 退出搜索 / 返回上一级 |
| `q` / `Ctrl+C` | 退出 TUI |

### 3.3 状态与反馈

- 加密 artifact 用 🔒 标记；解密后提示 `✓ decrypted`
- 操作结果用**底部 toast 栏**展示（4 秒后消失），不打断当前界面
- 网络错误：toast 显示错误摘要
- 加载中：artifacts 面板标题显示 spinner 动画 + 面板内 `⠹ Loading tags...`

### 3.4 弹窗规范

- **输入弹窗**（pull）：路径输入框 + passphrase 掩码输入框 + 确认/取消按钮
- **确认弹窗**（delete）：红色边框，显示 `tag (digest)`，按钮 `[Delete] [Cancel]`，默认焦点在 Cancel
- **帮助弹窗**：全屏滚动文本

## 4. 错误体验

| 场景 | 交互 |
|---|---|
| 内容加密但没给 passphrase | 快速失败：`content is encrypted, provide a decryption passphrase via --passphrase`（不下载数据）|
| 密码错误（GCM 认证失败） | `decryption failed: wrong passphrase?` |
| 引用格式错误 | `invalid reference "<ref>": <原因>` + 期望格式示例 |
| 未登录/无权限 | `authentication required for <host> (docker login <host> or set auths.<host> in config)` |
| shortcut 不存在 | `shortcut "x" not found (add shortcuts.x.repo to config)` |
| 非 TTY 下 delete | `confirmation required but stdin is not a TTY (use --yes to skip)` |
| 目标目录已存在（pull）| `destination <path> already exists (use --force to overwrite)` |
| 顶层错误 | TTY 红色 `✗ error: <msg>`；非 TTY 纯文本 `error: <msg>` |
