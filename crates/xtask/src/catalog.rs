//! 底座能力目录：底座**已提供**什么、**尚未**提供什么。
//!
//! 这份清单是 `capabilities/index.json` 的 `already_provided` / `not_yet_provided`
//! 两段的**唯一来源**。仓库级 `AGENTS.md` 的「已有能力」段必须与它逐行一致 ——
//! 由 [`agents_doc_matches_catalog`] 测试守住，避免出现「两套清单」。
//!
//! # 为什么分「已提供」和「未提供」两段
//!
//! 只列「已提供」是不够的：AI 会把「平台迟早会有」误当成「平台现在就有」，
//! 于是写出引用不存在 API 的代码。两段都列，才既防重造、也防幻觉。
//!
//! # 每条说明的写法
//!
//! 一句话讲清三件事：**已有什么**、**怎么用**、**不要重写什么**。
//! 只写「有 XXX」不够 —— AI 不知道该用还是该自己写。

/// 一条能力说明。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProvidedCapability {
    /// 能力键：AI 用它检索。
    pub key: &'static str,
    /// 一句话说明。
    pub summary: &'static str,
}

/// 底座**已提供**的能力 —— 业务模块直接用，不要重写。
///
/// 排序即展示顺序（按「后端接缝 → 前端包」排列），生成器按此顺序写入索引。
pub const ALREADY_PROVIDED: &[ProvidedCapability] = &[
    ProvidedCapability {
        key: "request_context",
        summary: "RequestContext 携带租户/空间/追踪 ID；Repository 每个方法强制传 ctx，租户隔离是编译期保证，勿自建租户上下文",
    },
    ProvidedCapability {
        key: "repository",
        summary: "Repository<T> 提供 find/list/insert/update/delete，业务模块实现它即可，勿另造 DAO 基类或数据访问框架",
    },
    ProvidedCapability {
        key: "tenant_scoped_db",
        summary: "platform-db 的 TenantDb 提供带 RequestContext 的租户作用域数据访问（事务内 SET LOCAL app.tenant_id + RLS 兜底），公开 API 拿不到裸连接，勿自建连接池或直连数据库",
    },
    ProvidedCapability {
        key: "list_query",
        summary: "ListQuery/SortSpec/FilterSpec 已定义游标分页（limit 默认 20、上限 200）与过滤算子白名单，勿自造分页参数",
    },
    ProvidedCapability {
        key: "module_registry",
        summary: "register_platform_module! + discover()/discover_enabled() 做编译期模块注册，勿自建插件或动态加载",
    },
    ProvidedCapability {
        key: "manifest",
        summary: "Manifest 解析与字段校验（含 schema 实体元数据段：两阶段校验、字段类型只有 text/number/boolean/date/reference）+ capabilities/schema/manifest.schema.json 与 schema/entity.schema.json 已提供，勿手写 manifest 校验",
    },
    ProvidedCapability {
        key: "contract_suite",
        summary: "#[platform_module_contract] 自动生成 C-1~C-6 六项契约测试（C-5：已登记权限点必须有鉴权调用点；C-6：声明了字段级权限的模块必须有 SchemaProjector::project 调用点），勿自写重复的契约检查",
    },
    ProvidedCapability {
        key: "schema_metadata",
        summary: "platform-schema 提供 GET /api/schema/<module>/<entity> 元数据接口（已认证即可读；无权字段在元数据里就被剔除，不是 hidden 标记），前端据此渲染，勿自造元数据接口或另立形状",
    },
    ProvidedCapability {
        key: "schema_projector",
        summary: "SchemaProjector::project 是字段级权限的唯一序列化入口（白名单语义：元数据未声明的字段默认不输出，列表要逐条投影、信封不动），返回实体的 handler 必须经它，勿直接 Json(entity)",
    },
    ProvidedCapability {
        key: "audit_declaration",
        summary: "Auditable + AuditEventSpec 提供审计事件声明面，平台据此自动埋点，勿自建审计表",
    },
    ProvidedCapability {
        key: "audit_sink",
        summary: "platform-audit 提供审计落库与查询（PgAuditSink 写哈希链、GET /api/audit/events 查询、/api/audit/verify 链校验与归档）；模块用 core 的 AuditSink 埋点，勿自建审计表或自写哈希链",
    },
    ProvidedCapability {
        key: "notification_sender",
        summary: "NotificationSender + NotificationMessage/DeliveryResult 提供通知投递接缝，勿自建消息中心",
    },
    ProvidedCapability {
        key: "workflow_handler",
        summary: "WorkflowHandler + TransitionEvent 提供工作流接入接缝，勿自建流程引擎",
    },
    ProvidedCapability {
        key: "transaction_boundary",
        summary: "TransactionalWrite + WriteBoundary 提供写边界声明，跨表写必须显式开平台事务（C-4 判定依据）",
    },
    ProvidedCapability {
        key: "config",
        summary: "ConfigScope/ConfigResolver（platform-capability，不是扩展点）/AppConfig + manifest 的 config 段提供三层配置与 Feature Flag（4 个权限点 config:entry:write / config:flag:write / config:platform:write / config:platform:read），勿自建配置读取",
    },
    ProvidedCapability {
        key: "error_code_i18n",
        summary: "@base/i18n 的 translate()/hasMessage() 提供错误码到文案映射，勿在后端拼中文句子",
    },
    ProvidedCapability {
        key: "theme",
        summary: "@base/theme 的 applyTheme()/toggleTheme() 提供 CSS 变量令牌与暗色模式，勿另造主题变量或硬编码颜色",
    },
    ProvidedCapability {
        key: "ui_token",
        summary: "@base/ui 的 toneClass()/TONES/defaultRadius() 提供色调与圆角 token，勿另造样式变量",
    },
    ProvidedCapability {
        key: "ui_table_component",
        summary: "@base/ui 的 Table 是页码分页（依赖 total）的只读表格；游标分页没有 total、且空态不能渲染表头空壳，请用 apps/shell 的 SchemaTable，勿自造表格渲染",
    },
    ProvidedCapability {
        key: "schema_field_meta",
        summary: "@base/schema 的 FieldMeta/describeField()/findField() 提供字段元数据形状，勿另造元数据结构",
    },
    ProvidedCapability {
        key: "sdk_client",
        summary: "@base/sdk 的 createApiClient() 提供类型安全请求、RFC 9457 错误码、token 自动刷新与幂等键注入，勿另造 HTTP 基类",
    },
    ProvidedCapability {
        key: "auth_state",
        summary: "@base/auth 的 isAuthenticated()/tenantId() 提供登录态形状，勿另造登录态模型",
    },
    ProvidedCapability {
        key: "permission_can",
        summary: "@base/auth 的 can()/guard() 提供权限点判定与路由守卫（权限点来自 GET /api/rbac/me/permissions），前端别自己写一套；服务端鉴权入口仍是 require_permission",
    },
    ProvidedCapability {
        key: "hooks_toggle",
        summary: "@base/hooks 的 createToggle()/createPagination()/createTableState() 提供通用状态容器，勿另造状态容器",
    },
    ProvidedCapability {
        key: "iam_auth",
        summary: "platform-iam 提供 /api/iam/* 认证接口（本地账号 + JWT access / 不透明 refresh + Argon2id + TOTP）与 /.well-known/jwks.json；业务模块要鉴权请用这些接口，勿自造认证",
    },
    ProvidedCapability {
        key: "org_structure",
        summary: "platform-org 提供 /api/org/* 组织架构接口（组织树用邻接表 + 递归 CTE、用户↔节点成员关系、岗位与汇报关系）、SCIM 2.0 出站与 LDAP/AD 入站、字段级同步冲突队列与人工裁决；数据范围的输入是它，勿自造组织树",
    },
    ProvidedCapability {
        key: "rbac_authorization",
        summary: "platform-core 的 require_permission 是平台唯一鉴权入口，platform-rbac 提供 /api/rbac/* 角色授权接口、5 种数据范围与 DB epoch 主动失效；业务模块用 require_permission + TenantQuery::with_scope，勿自写鉴权或数据范围",
    },
];

/// 底座**尚未**提供的能力 —— 别当成已有，别照着不存在的 API 写代码。
pub const NOT_YET_PROVIDED: &[ProvidedCapability] = &[
    ProvidedCapability {
        key: "http_api_layer",
        summary: "HTTP 接口层尚未就绪（REQ-011/012 补），capabilities/api.json 目前为空",
    },
    ProvidedCapability {
        key: "crud_scaffolding",
        summary: "平台没有通用 CRUD/分页脚手架（模块的仓储要自己实现），但数据访问必须走 platform-db 的租户作用域入口",
    },
    ProvidedCapability {
        key: "notification_dispatch",
        summary: "通知只到 trait，通道投递与模板待 REQ-015",
    },
    ProvidedCapability {
        key: "workflow_engine",
        summary: "工作流只到 trait，流程定义与实例待 REQ-017",
    },
];

/// 前端包的用法示例（与 `packages/<name>/src/index.ts` 的真实导出对应）。
///
/// 导出清单是**扫出来的**，示例是**人工 curate 的** —— 示例会过期，所以
/// `sdk_examples_cover_every_package` 测试守住「每个包都要有示例」。
pub struct SdkUsage {
    /// 包名（`@base/xxx`）。
    pub package: &'static str,
    /// 用法示例代码。
    pub example: &'static str,
}

/// 各前端包的用法示例。
pub const SDK_USAGE: &[SdkUsage] = &[
    SdkUsage {
        package: "@base/auth",
        example: "import { isAuthenticated, tenantId } from '@base/auth';\n\nif (isAuthenticated(state)) {\n  const tenant = tenantId(state); // string | undefined\n}",
    },
    SdkUsage {
        package: "@base/hooks",
        example: "import { createToggle } from '@base/hooks';\n\nconst toggle = createToggle(false);\ntoggle.toggle();",
    },
    SdkUsage {
        package: "@base/i18n",
        example: "import { translate } from '@base/i18n';\n\nconst text = translate({ INSUFFICIENT_PERMISSION: '权限不足' }, 'INSUFFICIENT_PERMISSION');",
    },
    SdkUsage {
        package: "@base/schema",
        example: "import { describeField, findField } from '@base/schema';\n\nconst field = findField(fields, 'title');\nconst label = field ? describeField(field) : '';",
    },
    SdkUsage {
        package: "@base/sdk",
        example: "import { createApiClient } from '@base/sdk';\n\nconst client = createApiClient({ baseUrl: 'https://api.base.example' });\nclient.url('/api/order/invoice'); // 'https://api.base.example/api/order/invoice'",
    },
    SdkUsage {
        package: "@base/theme",
        example: "import { mergeTheme } from '@base/theme';\n\nconst theme = mergeTheme({ primaryColor: '#ff0000' });",
    },
    SdkUsage {
        package: "@base/ui",
        example: "import { defaultRadius, toneClass } from '@base/ui';\n\nconst className = toneClass('primary'); // 'base-tone base-tone--primary'\nconst radius = defaultRadius().medium; // 4",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// 读取仓库级 `AGENTS.md`。
    fn agents_doc() -> String {
        let path = crate::workspace_root().join("AGENTS.md");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("读取 {} 失败：{err}", path.display()))
    }

    /// 取出 `AGENTS.md` 里某个小节的内容（到下一个 `## ` 为止）。
    fn section(doc: &str, heading: &str) -> String {
        let start = doc
            .find(heading)
            .unwrap_or_else(|| panic!("AGENTS.md 缺少小节 {heading}"));
        let rest = &doc[start + heading.len()..];
        let end = rest.find("\n## ").unwrap_or(rest.len());
        rest[..end].to_string()
    }

    /// 解析 `| \`key\` | summary |` 形式的表格行。
    fn table_rows(section: &str) -> Vec<(String, String)> {
        section
            .lines()
            .filter_map(|line| {
                let trimmed = line.trim();
                if !trimmed.starts_with("| `") {
                    return None;
                }
                let cells: Vec<&str> = trimmed
                    .trim_matches('|')
                    .split('|')
                    .map(str::trim)
                    .collect();
                let key = cells.first()?.trim_matches('`');
                let summary = cells.get(1)?;
                Some((key.to_string(), summary.to_string()))
            })
            .collect()
    }

    /// 目录里的键必须唯一，否则索引里会互相覆盖。
    fn assert_unique_keys(catalog: &[ProvidedCapability]) {
        let mut keys: Vec<&str> = catalog.iter().map(|item| item.key).collect();
        keys.sort_unstable();
        let count = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), count, "能力键重复：{keys:?}");
    }

    #[test]
    fn catalog_keys_are_unique() {
        assert_unique_keys(ALREADY_PROVIDED);
        assert_unique_keys(NOT_YET_PROVIDED);
    }

    #[test]
    fn already_provided_and_not_yet_provided_do_not_overlap() {
        let already: Vec<&str> = ALREADY_PROVIDED.iter().map(|item| item.key).collect();
        for item in NOT_YET_PROVIDED {
            assert!(
                !already.contains(&item.key),
                "`{}` 同时出现在已提供与未提供两段里",
                item.key
            );
        }
    }

    /// 「已有能力」段必须与目录逐行一致 —— 这条约束就是「别写两套」的机器化保障。
    #[test]
    fn agents_doc_matches_catalog() {
        let doc = agents_doc();
        let provided = table_rows(&section(&doc, "## 已有能力"));
        let expected: Vec<(String, String)> = ALREADY_PROVIDED
            .iter()
            .map(|item| (item.key.to_string(), item.summary.to_string()))
            .collect();
        assert_eq!(
            provided, expected,
            "AGENTS.md 的「已有能力」段与能力目录不一致"
        );

        let missing = table_rows(&section(&doc, "## 尚未提供"));
        let expected_missing: Vec<(String, String)> = NOT_YET_PROVIDED
            .iter()
            .map(|item| (item.key.to_string(), item.summary.to_string()))
            .collect();
        assert_eq!(
            missing, expected_missing,
            "AGENTS.md 的「尚未提供」段与能力目录不一致"
        );
    }

    /// 每个前端包都要有用法示例，否则 SDK 索引里会出现空示例。
    #[test]
    fn sdk_examples_cover_every_package() {
        let packages_dir = crate::workspace_root().join("packages");
        let mut names: Vec<String> = std::fs::read_dir(&packages_dir)
            .expect("packages 目录应可读")
            .flatten()
            .filter(|entry| entry.path().is_dir())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();

        for name in names {
            let manifest_path = packages_dir.join(&name).join("package.json");
            let raw = std::fs::read_to_string(&manifest_path).expect("package.json 应可读");
            let package: serde_json::Value =
                serde_json::from_str(&raw).expect("package.json 应是合法 JSON");
            let package_name = package["name"].as_str().expect("name 应是字符串");
            assert!(
                SDK_USAGE.iter().any(|usage| usage.package == package_name),
                "包 {package_name} 缺少用法示例（SDK_USAGE）"
            );
        }
    }
}
