# 提示词 02：实现接入

## 角色
你是接入方的实现 agent。按已确认的方案落地，不要重新设计形态。

## 必读
- 已确认的接入方案（上一轮产出）
- `CONTRACT.md` §4（签名）、§5（HTTP）、§6（权限）、§8（审计）、§9（配置，含 §9.10 事件通道）、§13（admin）
- `examples/go-hello-module/`（骨架）
- `conformance/run.sh`（验收）

## 顺序（不要跳步；只实现已声明的能力）
1. 骨架：验签中间件、`/healthz`、`/readyz`、配置读取；跑通一次带签名的 GET
2. manifest：`runtime`（含 `topology`）+ `permissions` + `config` + `admin`；按选定方式暴露（`GET /internal/manifest` 或 `PUT /api/apps/{id}/manifest`）；跑 `contract-verify --manifest`
3. 只读页面：`stats` + `table`（proxy 数据源）
4. 配置页面：`form`（config 数据源）+ 生效验证
5. 配置读取：调用用户级/租户级查询接口；本地缓存 TTL ≤60s；有事件时按 scope 失效
6. 写动作与审计：`actions` + 幂等 + outbox（首批无动作类可跳过）
7. 事件端点 `/internal/events`：验签、去重、未知类型 2xx、失败不阻塞轮询
8. 已声明能力对应的测试组全绿（§12.1）

## 每步都要给
- 改了哪些文件（路径）
- 跑了什么命令（原样）
- 实际输出（不要只说"通过"）
- 没做完的项与原因

## 红线
- 验签不许有"开发模式跳过"的开关
- 分页必须是 `{items, nextCursor}`，不要自造 `{data, total}`
- 403（动作）与 404（资源）不能混用
- 配置不许写死在 env 后假装已托管
- 审计事件必须在业务事务内写 outbox，不能异步补写

## 验收
```bash
contract-verify --manifest ./manifest.json --contract 1.0 \
  --conformance ./conformance --report ./contract-report.json
```
退出码非 0 时，先修报告里的第一项，再继续。
