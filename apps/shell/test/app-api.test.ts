import { describe, expect, it } from 'vitest';

import {
  APP_ALLOWED_URL_SCHEMES,
  APP_EMPTY_VALUE,
  APP_ERROR_CODES,
  APP_WRITABLE_FIELDS,
  appAccessPath,
  appLaunchUrlPath,
  appOpenAction,
  appPath,
  appStatusKey,
  buildCreateBody,
  buildPatchBody,
  navigationClickTarget,
  parseAppList,
  parseNavigation,
  parseWorkspace,
  safeLaunchUrl,
  sortApps,
  sortWorkspace,
  toAppRows,
  toAppView,
  toNavigationItem,
  type AppView,
} from '../src/lib/app-api';

/** 造一条应用（只覆盖断言用到的字段）。 */
function app(overrides: Partial<AppView> = {}): AppView {
  return {
    appId: 'app-1',
    clientId: 'client-A',
    displayName: '差旅报销',
    description: null,
    category: 'finance',
    iconUrl: null,
    launchUrl: 'https://a.example/',
    sortOrder: 0,
    status: 'active',
    scopes: [],
    createdAt: '2026-11-21T00:00:00.000000Z',
    createdBy: 'u-admin',
    ...overrides,
  };
}

describe('端点路径与白名单（与棒 1 逐字对照）', () => {
  it('应用路径逐字等于 /api/apps/{appId}（appId 做 URL 编码）', () => {
    expect(appPath('app-1')).toBe('/api/apps/app-1');
    expect(appPath('a/b c')).toBe('/api/apps/a%2Fb%20c');
  });

  it('访问记录路径以 /access 结尾；清空入口以 /launch-url 结尾（D283）', () => {
    expect(appAccessPath('app-1')).toBe('/api/apps/app-1/access');
    expect(appLaunchUrlPath('app-1')).toBe('/api/apps/app-1/launch-url');
  });

  it('错误码白名单恰好 6 个，且不含死码', () => {
    expect(APP_ERROR_CODES).toHaveLength(6);
    expect([...APP_ERROR_CODES]).toEqual([
      'APP_NOT_FOUND',
      'APP_DUPLICATED',
      'APP_CLIENT_INVALID',
      'APP_LAUNCH_URL_INVALID',
      'APP_ICON_URL_INVALID',
      'APP_INVALID_REQUEST',
    ]);
    // 死码：`APP_REGISTRY_MISMATCH` 是启动对账失败、`APP_INVALID_SCOPE_PROJECTION` 构造不出来。
    expect(APP_ERROR_CODES).not.toContain('APP_REGISTRY_MISMATCH');
    expect(APP_ERROR_CODES).not.toContain('APP_INVALID_SCOPE_PROJECTION');
  });

  it('可写字段白名单含 clientId/displayName/launchUrl，且不含响应侧字段（scopes/appId/status）', () => {
    expect([...APP_WRITABLE_FIELDS]).toEqual([
      'clientId',
      'displayName',
      'description',
      'category',
      'iconUrl',
      'launchUrl',
      'sortOrder',
    ]);
    for (const forbidden of ['appId', 'status', 'scopes', 'createdAt', 'createdBy']) {
      expect(APP_WRITABLE_FIELDS).not.toContain(forbidden);
    }
  });
});

describe('响应规整：三类形状不能混用', () => {
  it('裸数组型端点解析成数组（GET /api/apps · recent · frequent）', () => {
    const list = parseAppList([{ appId: 'a' }, { appId: 'b' }]);
    expect(list.map((entry) => entry.appId)).toEqual(['a', 'b']);
  });

  it('把裸数组端点的响应错包成对象 → 空列表（不假装有数据、也不抛错）', () => {
    expect(parseAppList({ apps: [{ appId: 'a' }] })).toEqual([]);
    expect(parseAppList(undefined)).toEqual([]);
  });

  it('工作台从 { groups } 里取分组，嵌套的 apps 走同一条规整', () => {
    const workspace = parseWorkspace({
      groups: [
        { category: 'finance', apps: [{ appId: 'a', sortOrder: '10' }] },
        { category: 'general', apps: [] },
      ],
    });
    expect(workspace.groups.map((group) => group.category)).toEqual(['finance', 'general']);
    expect(workspace.groups[0]?.apps[0]?.appId).toBe('a');
    // 字符串型 sortOrder 也接受（后端若给字符串不至于变 0）。
    expect(workspace.groups[0]?.apps[0]?.sortOrder).toBe(10);
  });

  it('工作台响应不是对象 / 没有 groups → 0 组', () => {
    expect(parseWorkspace(undefined).groups).toEqual([]);
    expect(parseWorkspace([]).groups).toEqual([]);
  });

  it('AppView 的字段名逐字是 camelCase（契约 C13）：缺 description 时是 null 而不是空串', () => {
    const view = toAppView({
      appId: 'a',
      clientId: 'c',
      displayName: 'n',
      category: 'general',
      sortOrder: 3,
      status: 'active',
      scopes: ['rbac:role:read'],
      createdAt: 't',
      createdBy: 'u',
    });
    expect(view.description).toBeNull();
    expect(view.iconUrl).toBeNull();
    expect(view.launchUrl).toBeNull();
    expect(view.scopes).toEqual(['rbac:role:read']);
    // 驼峰字段能被读到（错写成 snap_case 的键会读不到）。
    expect(view.createdAt).toBe('t');
    expect(view.createdBy).toBe('u');
  });
});

describe('导航项：按来源分别定义（D284 / S-09）', () => {
  it('module 项：kind 由来源推出为 internal，字段恰好 6 个（无 labelKey / category / sortOrder / key）', () => {
    const item = toNavigationItem({
      source: 'module',
      label: '审计日志',
      icon: null,
      target: '/admin/audit',
      kind: 'internal',
      requiredPermission: 'audit:events:read',
    });
    expect(item).toBeDefined();
    expect(item && 'kind' in item ? item.kind : undefined).toBe('internal');
    expect(Object.keys(item ?? {}).sort()).toEqual([
      'icon',
      'kind',
      'label',
      'requiredPermission',
      'source',
      'target',
    ]);
  });

  it('app 项：kind 推出为 external，appId 必填，requiredPermission 恒为 null', () => {
    const item = toNavigationItem({
      source: 'app',
      appId: 'app-1',
      label: '第三方报销',
      icon: 'https://i.example/x.png',
      target: 'https://expense.example/app',
      kind: 'external',
      requiredPermission: 'should:be:ignored',
    });
    expect(item).toEqual({
      source: 'app',
      appId: 'app-1',
      label: '第三方报销',
      icon: 'https://i.example/x.png',
      target: 'https://expense.example/app',
      kind: 'external',
      requiredPermission: null,
    });
  });

  it('kind 不采信响应里的值：module 项声称 external 仍被规整成 internal', () => {
    // 后端若把 kind 发错，外部应用可能被当成站内路径 → 必须在规整层纠正。
    const item = toNavigationItem({
      source: 'module',
      label: '审计日志',
      icon: null,
      target: '/admin/audit',
      kind: 'external',
    });
    expect(item?.kind).toBe('internal');
  });

  it('不可用的项被跳过（缺 target / 来源不认识 / app 项缺 appId）', () => {
    expect(
      parseNavigation({
        items: [
          { source: 'module', label: 'x', target: '' },
          { source: 'unknown', label: 'y', target: '/y' },
          { source: 'app', label: 'z', target: 'https://z.example/' },
          { source: 'module', label: '好', target: '/good' },
        ],
      }),
    ).toHaveLength(1);
  });

  it('导航响应不是对象 / 没有 items → 空数组', () => {
    expect(parseNavigation(undefined)).toEqual([]);
    expect(parseNavigation({ items: 'x' })).toEqual([]);
  });
});

describe('确定性排序（D267）', () => {
  it('组内：sortOrder 升序，并列时按 displayName（不是按 id / 插入顺序）', () => {
    const sorted = sortApps([
      app({ appId: 'A', displayName: '差旅报销', sortOrder: 20 }),
      app({ appId: 'B', displayName: '费用申请', sortOrder: 10 }),
      app({ appId: 'D', displayName: 'D 应用', sortOrder: 0 }),
      app({ appId: 'C', displayName: 'C 应用', sortOrder: 0 }),
    ]);
    expect(sorted.map((entry) => entry.appId)).toEqual(['C', 'D', 'B', 'A']);
  });

  it('组内并列 sortOrder 且 displayName 相同时顺序稳定（相等比较返回 0）', () => {
    const left = app({ appId: 'x1', displayName: '同名', sortOrder: 5 });
    const right = app({ appId: 'x2', displayName: '同名', sortOrder: 5 });
    const sorted = sortApps([left, right]);
    expect(sorted).toHaveLength(2);
    expect(sorted.map((entry) => entry.appId)).toEqual(['x1', 'x2']);
  });

  it('组：按 category 码点升序；空 category 排最前', () => {
    const sorted = sortWorkspace([
      { category: 'general', apps: [app({ appId: 'C', displayName: 'C', sortOrder: 0 })] },
      { category: 'finance', apps: [app({ appId: 'B', displayName: 'B', sortOrder: 10 })] },
      { category: '', apps: [] },
    ]);
    expect(sorted.map((group) => group.category)).toEqual(['', 'finance', 'general']);
  });

  it('排序不改原数组（纯函数）', () => {
    const input = [app({ appId: 'A', sortOrder: 2 }), app({ appId: 'B', sortOrder: 1 })];
    const copy = [...input];
    sortApps(input);
    expect(input).toEqual(copy);
  });

  /**
   * 后端已错序时，前端仍纠正 —— 且**不改输入**（D286）。
   *
   * # 这条判据断的是什么（读清楚，别当成「后端排对了」）
   *
   * 它证明的是「**即使后端错序，前端也纠正**」，**不是**「后端排对了」。
   * AC-S-160 步骤 3 的**真判据**落在 `GET /api/apps/workspace` 的**原始 JSON** 上，
   * 由后端（棒 1）的测试承担 —— 前端这里没有 HTTP、也**不能**替代它。
   *
   * # 为什么输入要「反着给」
   *
   * 输入顺序若恰好等于期望顺序，稳定排序会原样保留 → 去掉排序实现也照样绿
   * （本需求开发时真踩过：变异「去掉 displayName 次级排序」当时只红 1 条）。
   * 所以这里刻意给 **general 组在前、组内 D→C→B→A**。
   */
  it('后端已错序 → 前端纠正（组 general 在前、组内 D→C→B→A），且不修改入参', () => {
    const make = (name: string, sortOrder = 0): AppView =>
      app({ appId: name, displayName: `${name} 应用`, sortOrder });

    // 后端「错序」的输入：general 组在前；general 组内 D→C→B→A（期望 A→B→C→D）。
    const input = [
      { category: 'general', apps: [make('D'), make('C'), make('B'), make('A')] },
      { category: 'finance', apps: [make('B', 10), make('A', 20)] },
    ];
    // 入参快照（组顺序 + 组内顺序 + 组内成员），用于断言「没有被原地改」。
    const before = input.map((group) => ({
      category: group.category,
      apps: group.apps.map((entry) => entry.appId),
    }));

    const sorted = sortWorkspace(input);

    // ① 组按 category 码点升序：finance 在前。
    expect(sorted.map((group) => group.category)).toEqual(['finance', 'general']);
    // ② 组内：finance 按 sortOrder（10 < 20）；general 并列 0 → 按 displayName。
    expect(sorted[0]?.apps.map((entry) => entry.appId)).toEqual(['B', 'A']);
    expect(sorted[1]?.apps.map((entry) => entry.appId)).toEqual(['A', 'B', 'C', 'D']);

    // ③ **不可变性**：入参的组顺序与组内顺序一字未动（`[...groups]` / `[...apps]` 钉住）。
    expect(
      input.map((group) => ({
        category: group.category,
        apps: group.apps.map((entry) => entry.appId),
      })),
    ).toEqual(before);

    // ④ 返回的是**新对象**（不是把入参数组/组对象原样返回 —— 否则调用方拿到的是同一引用，
    //    下次再排就会叠在上一次结果上）。
    expect(sorted).not.toBe(input);
    expect(sorted[0]).not.toBe(input[1]);
    expect(sorted[1]).not.toBe(input[0]);
    expect(sorted[0]?.apps).not.toBe(input[1]?.apps);
  });
});

describe('入口地址的安全渲染（AC-S-156 步骤 9 的前端对应面）', () => {
  it('https / http 放行', () => {
    expect(safeLaunchUrl('https://x.example/a')).toBe('https://x.example/a');
    expect(safeLaunchUrl('http://x.example/a')).toBe('http://x.example/a');
  });

  it('javascript: 与 data: 被挡（打开即 XSS）', () => {
    expect(safeLaunchUrl('javascript:alert(1)')).toBeUndefined();
    expect(safeLaunchUrl('data:text/html,<script>alert(1)</script>')).toBeUndefined();
    // 大小写与空白变体同样被挡（URL 解析器会规范化协议）。
    expect(safeLaunchUrl('JaVaScRiPt:alert(1)')).toBeUndefined();
    expect(safeLaunchUrl(' javascript:alert(1) ')).toBeUndefined();
  });

  it('ftp: / 非 URL / 空值被挡', () => {
    expect(safeLaunchUrl('ftp://x.example/a')).toBeUndefined();
    expect(safeLaunchUrl('not-a-url')).toBeUndefined();
    expect(safeLaunchUrl('')).toBeUndefined();
    expect(safeLaunchUrl(null)).toBeUndefined();
    expect(safeLaunchUrl(undefined)).toBeUndefined();
  });

  it('允许的 scheme 只有 http/https（白名单本身的判据）', () => {
    expect([...APP_ALLOWED_URL_SCHEMES]).toEqual(['https:', 'http:']);
  });
});

describe('导航项点击的处置：整页跳 vs SPA 内跳（AC-S-163 步骤 3 / D278）', () => {
  it('external 项 → 整页跳转（url 逐字等于 target，不加工、不当作站内路径）', () => {
    const target = navigationClickTarget({
      source: 'app',
      appId: 'app-1',
      label: '第三方报销',
      icon: null,
      target: 'https://expense.example/app',
      kind: 'external',
      requiredPermission: null,
    });
    expect(target).toEqual({ kind: 'external', url: 'https://expense.example/app' });
  });

  it('internal 项 → 站内路径（不走整页跳转）', () => {
    const target = navigationClickTarget({
      source: 'module',
      label: '审计日志',
      icon: null,
      target: '/admin/audit',
      kind: 'internal',
      requiredPermission: null,
    });
    expect(target).toEqual({ kind: 'internal', path: '/admin/audit' });
  });

  it('两类形态互斥：external 的产物里没有 path，internal 的产物里没有 url', () => {
    const external = navigationClickTarget({
      source: 'app',
      appId: 'a',
      label: 'x',
      icon: null,
      target: 'https://x.example/',
      kind: 'external',
      requiredPermission: null,
    });
    const internal = navigationClickTarget({
      source: 'module',
      label: 'y',
      icon: null,
      target: '/y',
      kind: 'internal',
      requiredPermission: null,
    });
    expect(external).not.toHaveProperty('path');
    expect(internal).not.toHaveProperty('url');
  });
});

describe('「打开应用」的决策（AC-S-162 步骤 6 的前端对应面）', () => {
  it('有合法入口 → link，url 逐字等于 launchUrl', () => {
    const action = appOpenAction(app({ launchUrl: 'https://expense.example/app' }));
    expect(action).toEqual({ kind: 'link', url: 'https://expense.example/app' });
  });

  it('launch_url 为空 → none（工作台仍显示卡片，但没有可点入口 —— D279）', () => {
    expect(appOpenAction(app({ launchUrl: null }))).toEqual({ kind: 'none' });
  });

  it('scheme 非法 → none（不把 javascript: 当入口）', () => {
    expect(appOpenAction(app({ launchUrl: 'javascript:alert(1)' }))).toEqual({ kind: 'none' });
  });
});

describe('请求体只带白名单 + 有值的字段', () => {
  it('空串不发（用户没填 ≠ 填了空串）；sortOrder 的 0 是合法值', () => {
    const body = buildCreateBody({
      clientId: 'client-A',
      displayName: '差旅报销',
      description: '',
      launchUrl: '',
      sortOrder: 0,
    });
    expect(body).toEqual({ clientId: 'client-A', displayName: '差旅报销', sortOrder: 0 });
  });

  it('PATCH 与 POST 同一白名单，且不发响应侧字段（scopes / appId / status 即使在输入里出现也不发）', () => {
    const input = {
      clientId: 'c',
      launchUrl: 'https://x.example/',
      // 故意塞响应侧字段（TS 层不该允许，运行时用来证明过滤有效）。
      ...({ appId: 'a', status: 'active', scopes: ['x'] } as Record<string, unknown>),
    };
    const body = buildPatchBody(input);
    expect(Object.keys(body).sort()).toEqual(['clientId', 'launchUrl']);
  });

  it('sortOrder 非数字 / NaN 不发', () => {
    expect(buildCreateBody({ sortOrder: Number.NaN })).toEqual({});
  });
});

describe('展示行与状态文案', () => {
  it('没有 launchUrl 的行显示占位符；sortOrder 转成文本', () => {
    const rows = toAppRows([app({ launchUrl: null, sortOrder: 7 })]);
    expect(rows[0]?.launchUrl).toBe(APP_EMPTY_VALUE);
    expect(rows[0]?.sortOrder).toBe('7');
  });

  it('状态键：active / hidden 有键，未知值返回 undefined（调用方逐字显示）', () => {
    expect(appStatusKey('active')).toBe('app.registryStatusActive');
    expect(appStatusKey('hidden')).toBe('app.registryStatusHidden');
    expect(appStatusKey('archived')).toBeUndefined();
  });
});
