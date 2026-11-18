# 契约快照

> **这里的文件是快照，不是正本。**

## 正本在哪

契约由 BA 侧维护，**正本只有一份**：BA 分支（`demand`）上的
`contracts/<name>/<version>.md`。

在 workspace 里，`BA/` 目录就是这个分支的 worktree，正本路径是
`BA/contracts/<name>/<version>.md`。

只拿到代码仓库（没有 BA worktree）的人，用 git 直接读正本：

```bash
git fetch origin demand            # 本地还没有 demand 分支时先拉一次
git show demand:contracts/api-conventions/v1.md
git show demand:contracts/extension-points/v1.md
```

## 这里的快照是什么

`docs/contracts/<name>/<version>.md` 是**随版本发布的快照**，内容与正本逐字节相同，
只为「只 clone 了代码仓库」的人留一个可直接打开的入口。

## 谁说了算，怎么防漂移

| 项 | 约定 |
|---|---|
| 正本 | BA 分支 `demand` 的 `contracts/` |
| 快照 | 本目录，由 `scripts/sync-contracts.sh --sync` 从正本生成 |
| 一致性 | `scripts/sync-contracts.sh` 校验，CI 强制执行 |

**不要手改本目录下的文件** —— 下次同步会被覆盖，而且 CI 会因快照与正本不一致而失败。
改了契约请改 BA 分支上的正本，然后运行：

```bash
scripts/sync-contracts.sh --sync
```

契约清单来自 `feature-meta/manifest.json` 里 pin 的版本（当前：`api-conventions v1`、
`extension-points v1`），pin 升级时同样用上面的命令补齐对应版本的快照。
