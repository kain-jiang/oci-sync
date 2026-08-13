# 测试策略（Testing Strategy）

> 分层：单元测试（纯逻辑）→ 集成测试（e2e，真实仓库）。`src/oci` 无单元测试（需真实仓库，遵守 AGENTS.md 约定）。

## 1. 单元测试（cargo test）

### 1.1 `src/archive`

| 用例 | 断言 |
|---|---|
| pack 单文件 | 解包后内容一致、文件名 = basename |
| pack 目录（嵌套） | 根条目为目录名，子文件/子目录结构完整保留 |
| pack 空目录 | 只含目录条目 |
| pack 空文件 | 正常打包解包 |
| unpack 路径穿越 `../evil` | 返回错误，目标目录无文件写出 |
| unpack 绝对路径条目 `/etc/passwd` | 返回错误 |
| unpack 非法 gzip/tar 数据 | 返回错误 |
| unpack 目标目录自动创建 | 不存在时创建成功 |
| 权限位保留 | 解包后 mode 与打包前一致 |

### 1.2 `src/crypto`

| 用例 | 断言 |
|---|---|
| 加解密往返 | `decrypt(encrypt(d, p), p) == d` |
| 错误口令 | 返回错误（wrong passphrase）|
| 相同明文两次加密结果不同 | salt/nonce 随机性 |
| 空数据 | 正常（tag 尺寸校验）|
| 密文过短（< 60B）| 返回错误 |
| 字节布局 | 前 32B salt、次 12B nonce、密文长度 = 明文 + 16 |
| 与 Go 版互操作（若可用）| Go 加密 → Rust 解密（可用 `oci-sync-go-backup` 编译验证）|

### 1.3 `src/config`

| 用例 | 断言 |
|---|---|
| 解析完整 YAML | auths/shortcuts 正确 |
| 缺省文件 | 空配置（不报错）|
| shortcut_repo 含 tag（`repo:tag`）| 报错 |
| shortcut_repo 含 digest（`repo@sha256:`）| 报错 |
| shortcut_remote_ref 拼接 | `repo:tag` |
| 搜索顺序 | cwd 优先于用户目录（用临时目录模拟）|

### 1.4 `src/cache`

| 用例 | 断言 |
|---|---|
| add → recent | 最新在前 |
| 超过 100 条 | 截断到 100 |
| clear | 清空 |
| 文件缺失 | 返回空缓存不报错 |
| 序列化字段 | JSON 键名与 Go 版一致（`type/timestamp/remote_ref/...`）|

### 1.5 `src/output`

| 用例 | 断言 |
|---|---|
| format_bytes | 边界（0/1023/1536/5MiB）|
| filter_by_labels | `k=v` 精确、裸 `k` 存在性、多规则 AND |
| filter_by_tags | 多 tag 匹配 |
| JSON/YAML 输出 | 可反序列化回 ArtifactInfo |

### 1.6 `src/cli`

- 每个命令的 clap 解析：必填缺失报错、短/长标志等价（`try_parse_from`）
- shortcut 二次解析：`["x","push","-l","./d","-t","latest"]` → 正确参数
- `dispatch_shortcut` 未知子命令 → 报错

### 1.7 `src/tui`

- 状态机转移（选中/搜索/弹窗/确认）不依赖终端渲染（纯逻辑抽离）
- 键位 → 动作映射表驱动测试

## 2. 集成测试（e2e，Python + uv）

独立工程 `e2e/`（pyproject + uv.lock），沿用 Go 版骨架：

```
e2e/
├── pyproject.toml
└── src/e2e/
    ├── __main__.py   # 入口
    ├── run.py        # 用例编排
    ├── case.py       # 用例基类
    ├── config.py     # 环境变量读取
    ├── env.py        # 临时目录/二进制路径
    └── state.py      # 断言工具
```

**环境变量：**
- 必填：`OCI_SYNC_TEST_REPO`（如 `registry.example.com/test/repo`）
- 可选：`OCI_SYNC_TEST_TAG_BASE`、`OCI_SYNC_TEST_PASSPHRASE`
- `OCI_SYNC_BIN`（二进制路径，默认 `target/release/oci-sync`）
- `OCI_SYNC_KEEP_WORKDIR=1` 保留测试产物（默认自动清理 `e2e/runtime-check/`）

**用例清单：**

| # | 场景 | 步骤与断言 |
|---|---|---|
| 1 | push 目录（明文）| push → list 可见 tag、encrypted=no → pull → 文件一致 |
| 2 | push 文件（加密）| 带 passphrase → list encrypted=yes → 无 passphrase pull 快速失败 → 带 passphrase pull 一致 |
| 3 | delete | push → delete（--yes）→ list 不含该 tag |
| 4 | label set/unset | push → label set → list --label 命中 → label unset → list 不再命中 |
| 5 | shortcut 命令 | 配置临时 `oci-sync.yaml` → `alias add` → `<name> push/pull/list/delete` |
| 6 | list 格式 | `--format json/yaml` 可解析 |
| 7 | 重复 push 覆盖 tag | 同 tag 两次 push，list 仍一条记录，digest 更新 |
| 8 | 空目录 / 空文件 push-pull | 往返一致 |
| 9 | 认证 | 私有仓库（若测试 repo 需认证）用配置 auths 或 docker login |
| 10 | 兼容性（可选）| Go 版推送 → Rust 版 pull；Rust 版推送 → Go 版 pull |

**运行：**
```bash
cargo build --release
OCI_SYNC_TEST_REPO=registry.example.com/test/repo uv run -m e2e
```

## 3. 手工验收清单（发布前）

- [ ] `oci-sync --help` / 各子命令 `--help` 完整且英文
- [ ] 错误场景均给出可行动提示（§interaction 4 的错误矩阵）
- [ ] delete 在 TTY 有确认、非 TTY 拒绝并提示 `--yes`
- [ ] `--quiet` 只输出错误
- [ ] 管道 `list -f json | jq` 输出纯净（无日志混入）
- [ ] TUI：双栏切换、搜索、pull/delete 弹窗、帮助视图、Ctrl+C 干净退出
- [ ] `completion bash` 输出可被 shell 加载
