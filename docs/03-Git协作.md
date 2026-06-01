# Git 协作

工具不做本地并发锁，完全依赖 Git 解决冲突。文本化 + 一行一记录的格式让大多数情况下能自动 merge。

## 1. 冲突最小化策略

| 策略 | 效果 |
|------|------|
| 一行一记录 | 不同策划改不同行，Git 自动 merge |
| 按 Group 分文件夹 | 不同策划改不同表，无冲突 |
| 主键排序 | 行顺序稳定，不因排序产生 diff |
| schema 与数据同文件 | 减少文件数，简化管理 |
| 多 Project 物理隔离 | 不同 Project 互不影响，跨 Project 改动**永远**不会冲突 |

## 2. .gitignore

```gitignore
# Excel 桥接缓存：每个 Project 自带一份，全部忽略
projects/*/.tbl-cache/

# Excel 临时文件
*.xlsx~
~$*.xlsx

# 生成产物
gen/

# 进程锁
.tbl-tool.lock
```

注意：`project.toml` / `project.tblschema` / `config/**/*.tbl` 都需要进版本控制——它们是 Project 的本体。

## 3. 仓库结构（多 Project）

```
<repo-root>/
├── tbl-tool.toml                 # 全局配置（进 git）
├── projects/                     # 全部 Project（进 git，除 .tbl-cache/）
│   ├── slg-test/
│   │   ├── project.toml
│   │   ├── project.tblschema
│   │   ├── config/
│   │   └── .tbl-cache/           # ← .gitignore
│   └── slg-prod/...
├── tblschema/                    # 本地模板库（可选进 git，按团队约定）
└── gen/                          # 生成产物（.gitignore）
```

是否把本地模板（`tblschema/`）进 git 由团队决定：
- 如果想让团队成员共享自定义模板 → 进 git
- 如果只是个人临时模板 → .gitignore

## 4. 冲突场景

| 场景 | 概率 | 解决方式 |
|------|------|----------|
| 不同策划改不同 Project | 最常见 | 完全无冲突 |
| 同 Project 不同策划改不同表 | 常见 | 无冲突 |
| 同 Project 不同策划改同表不同行 | 常见 | Git 自动 merge |
| 同 Project 不同策划改同一行 | 极少 | Git 标记冲突，手动解决（文本格式易读） |
| schema 变更 | 极少 | 需协调，一人改 schema 后通知其他人 |
| 同时新建同名 Project | 极少 | 目录冲突，约定 project id 命名规则规避 |

