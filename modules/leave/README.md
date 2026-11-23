# 请假审批（REQ-020 业务消费方）

`enable` 它之后，服务提供 `/api/leave/*` 四个端点：

| 方法 | 路径 | 权限点 |
|---|---|---|
| `GET` | `/api/leave/requests` | `leave:request:read` |
| `POST` | `/api/leave/requests` | `leave:request:write` |
| `POST` | `/api/leave/requests/{id}/submit` | `leave:request:write` |
| `POST` | `/api/leave/requests/{id}/decide` | `leave:request:approve` |

## 状态机（业务层自实现，D347）

```text
draft ──submit──► pending ──approve──► approved（终态）
                    └──reject──► rejected（终态）
```

非法流转返回 **409** + `code == "LEAVE_INVALID_TRANSITION"`。

## 复用了什么

| 能力 | 入口 | 本模块**不**做的事 |
|---|---|---|
| 租户隔离 | `platform_db::TenantDb` + `TenantQuery` | 裸 `sqlx::query` |
| 鉴权 | `require_permission("leave:...")` | 自己解析 JWT |
| 审计 | `AuditEvent` + 注入的 `AuditSink` | 写审计表 SQL |
| 通知 | 注入的 `NotificationSender` | 依赖 `platform-notify` |

## 跑测试

```bash
cargo test -p leave --all-targets -- --test-threads=1
```
