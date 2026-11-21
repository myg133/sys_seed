/**
 * 通知前端封装的单测（REQ-015 / AC-S-167 · 169 · 172 · 173 —— 前端侧）。
 *
 * ⚠️ **不测后端语义**：模板渲染、退订判定、重投守卫都在后端有真测试。
 * 这里只钉三件前端**自己**的事：
 *
 * 1. 字段名是 camelCase（契约 C13）—— 拼错就是「静默 0 条」；
 * 2. 形状不对时**不抛错**（列表页不白屏）；
 * 3. `{{variable}}` 白名单与后端**一致**（`^[a-z][a-z0-9_]*$`）。
 */

import { describe, expect, it } from 'vitest';
import type { ApiClient } from '@base/sdk';

import {
  NOTIFY_CATEGORIES,
  NOTIFY_CHANNELS,
  NOTIFY_DELIVERY_STATUSES,
  NOTIFY_ERROR_CODES,
  NOTIFY_IMPLEMENTED_CHANNELS,
  NOTIFY_OPTIONAL_CATEGORIES,
  NotifyApi,
  extractTemplateVariables,
  hasInvalidPlaceholders,
  isCategoryOptional,
  isChannelImplemented,
  isKnownNotifyErrorCode,
  isResendable,
  notifyErrorKey,
  notifyResendPath,
  notifyTemplatePath,
  notifyTemplatePreviewPath,
  parseDeliveries,
  parsePreferences,
  parsePreview,
  parseTemplates,
} from '../src/lib/notify-api';

/** 最小的假 `ApiClient`（只实现被用到的快捷方式）。 */
function fakeClient(handler: (path: string, body?: unknown) => unknown): ApiClient {
  const call = async (path: string, body?: unknown): Promise<unknown> => handler(path, body);
  return {
    baseUrl: '',
    request: async <T>(options: { path: string; body?: unknown }) =>
      (await call(options.path, options.body)) as T,
    get: async <T>(path: string) => (await call(path)) as T,
    post: async <T>(path: string, body?: unknown) => (await call(path, body)) as T,
    put: async <T>(path: string, body?: unknown) => (await call(path, body)) as T,
    delete: async <T>(path: string) => (await call(path)) as T,
  } as unknown as ApiClient;
}

describe('路径常量', () => {
  it('模板路径逐字等于后端路由', () => {
    expect(notifyTemplatePath('t-1')).toBe('/api/notify/templates/t-1');
    expect(notifyTemplatePreviewPath('t-1')).toBe('/api/notify/templates/t-1/preview');
    expect(notifyResendPath('d-1')).toBe('/api/notify/deliveries/d-1/resend');
  });

  it('路径里的 ID 会被 URL 编码（防止路径穿越）', () => {
    expect(notifyTemplatePath('../etc/passwd')).toBe(
      '/api/notify/templates/..%2Fetc%2Fpasswd',
    );
  });
});

describe('白名单', () => {
  it('通道恰好 7 个（3 真实现 + 4 未配置）', () => {
    expect(NOTIFY_CHANNELS).toHaveLength(7);
    expect(NOTIFY_IMPLEMENTED_CHANNELS).toHaveLength(3);
  });

  it('真实现集合是全部通道的子集且不重复', () => {
    for (const channel of NOTIFY_IMPLEMENTED_CHANNELS) {
      expect(NOTIFY_CHANNELS).toContain(channel);
    }
    expect(new Set(NOTIFY_CHANNELS).size).toBe(NOTIFY_CHANNELS.length);
  });

  it('分类恰好 3 个，可退订的恰好 2 个（system 不可退订，D295）', () => {
    expect(NOTIFY_CATEGORIES).toHaveLength(3);
    expect(NOTIFY_OPTIONAL_CATEGORIES).toHaveLength(2);
    expect(NOTIFY_OPTIONAL_CATEGORIES).not.toContain('system');
  });

  it('投递状态恰好 4 个', () => {
    expect(NOTIFY_DELIVERY_STATUSES).toHaveLength(4);
  });

  it('错误码恰好 8 个（D303）', () => {
    expect(NOTIFY_ERROR_CODES).toHaveLength(8);
    expect(new Set(NOTIFY_ERROR_CODES).size).toBe(8);
  });

  it('4 个 notify.* 审计事件码**不在**错误码里（D223）', () => {
    for (const code of NOTIFY_ERROR_CODES) {
      expect(code.startsWith('notify.')).toBe(false);
    }
  });
});

describe('纯函数判定', () => {
  it('isCategoryOptional：system 不可退订', () => {
    expect(isCategoryOptional('system')).toBe(false);
    expect(isCategoryOptional('marketing')).toBe(true);
    expect(isCategoryOptional('transactional')).toBe(true);
    expect(isCategoryOptional('unknown')).toBe(false);
  });

  it('isChannelImplemented：只有 3 个有真实现', () => {
    expect(isChannelImplemented('in_app')).toBe(true);
    expect(isChannelImplemented('smtp')).toBe(true);
    expect(isChannelImplemented('webhook')).toBe(true);
    for (const channel of ['sms', 'dingtalk', 'wecom', 'feishu']) {
      expect(isChannelImplemented(channel)).toBe(false);
    }
  });

  it('isResendable：只有 dead_letter 能重投', () => {
    expect(isResendable('dead_letter')).toBe(true);
    for (const status of ['pending', 'delivering', 'delivered']) {
      expect(isResendable(status)).toBe(false);
    }
  });

  it('notifyErrorKey：错误码 → error.<CODE>', () => {
    expect(notifyErrorKey('NOTIFY_TEMPLATE_NOT_FOUND')).toBe(
      'error.NOTIFY_TEMPLATE_NOT_FOUND',
    );
  });

  it('isKnownNotifyErrorCode：8 个码识别为已知，其余不是', () => {
    for (const code of NOTIFY_ERROR_CODES) {
      expect(isKnownNotifyErrorCode(code)).toBe(true);
    }
    expect(isKnownNotifyErrorCode('APP_NOT_FOUND')).toBe(false);
    expect(isKnownNotifyErrorCode('notify.template.created')).toBe(false);
  });
});

describe('模板变量抽取（与后端白名单一致）', () => {
  it('抽出合法变量并去重，保持出现顺序', () => {
    expect(extractTemplateVariables('你好 {{name}}，订单 {{order_id}} 已发货 {{name}}')).toEqual([
      'name',
      'order_id',
    ]);
  });

  it('非法变量名被跳过（不报错）', () => {
    expect(extractTemplateVariables('{{Name}} {{1a}} {{a-b}} {{ok}}')).toEqual(['ok']);
  });

  it('没有占位符 → 空数组', () => {
    expect(extractTemplateVariables('纯文本')).toEqual([]);
  });

  it('hasInvalidPlaceholders：4 个 SSTI 载荷都判为非法', () => {
    for (const payload of [
      '{{#if x}}y{{/if}}',
      '{{x.y}}',
      '{{> partial}}',
      "{{constructor.constructor('x')()}}",
    ]) {
      expect(hasInvalidPlaceholders(payload)).toBe(true);
    }
  });

  it('hasInvalidPlaceholders：合法模板为 false（阳性对照）', () => {
    expect(hasInvalidPlaceholders('你好 {{name}}')).toBe(false);
    expect(hasInvalidPlaceholders('纯文本')).toBe(false);
  });

  it('两个函数对同一输入**不矛盾**：有非法时 extract 仍返回合法的那部分', () => {
    const template = '{{name}} {{#if x}}y{{/if}}';
    expect(hasInvalidPlaceholders(template)).toBe(true);
    expect(extractTemplateVariables(template)).toEqual(['name']);
  });
});

describe('解析函数（形状不对不抛错）', () => {
  it('parseTemplates：数组 → 原样；非数组 → 空数组', () => {
    expect(parseTemplates([{ templateId: 't1' }])).toHaveLength(1);
    expect(parseTemplates({ items: [] })).toEqual([]);
    expect(parseTemplates(null)).toEqual([]);
    expect(parseTemplates('nope')).toEqual([]);
  });

  it('parseDeliveries / parsePreferences 同款', () => {
    expect(parseDeliveries({ items: [] })).toEqual([]);
    expect(parsePreferences(undefined)).toEqual([]);
  });

  it('parsePreview：形状正确 → 对象；形状不对 → null', () => {
    expect(parsePreview({ title: 'a', body: 'b' })).toEqual({ title: 'a', body: 'b' });
    expect(parsePreview({ title: 'a' })).toBeNull();
    expect(parsePreview(null)).toBeNull();
    expect(parsePreview({ title: 1, body: 'b' })).toBeNull();
  });
});

describe('NotifyApi', () => {
  it('listTemplates 走 GET /api/notify/templates', async () => {
    const seen: string[] = [];
    const api = new NotifyApi(
      fakeClient((path) => {
        seen.push(path);
        return [{ templateId: 't1' }];
      }),
    );
    const rows = await api.listTemplates();
    expect(seen).toEqual(['/api/notify/templates']);
    expect(rows[0]?.templateId).toBe('t1');
  });

  it('updateTemplate 用 PATCH（ApiClient 没有 patch 快捷方式）', async () => {
    const seen: Array<{ path: string; method?: string }> = [];
    const client = {
      baseUrl: '',
      request: async <T>(options: { path: string; method?: string }) => {
        seen.push(options);
        return { templateId: 't1' } as T;
      },
      get: async () => ({}),
      post: async () => ({}),
      put: async () => ({}),
      delete: async () => ({}),
    } as unknown as ApiClient;
    const api = new NotifyApi(client);
    await api.updateTemplate('t1', { titleTemplate: 'x' });
    expect(seen[0]?.method).toBe('PATCH');
    expect(seen[0]?.path).toBe('/api/notify/templates/t1');
  });

  it('resendDelivery 走 POST .../resend', async () => {
    const seen: string[] = [];
    const api = new NotifyApi(
      fakeClient((path) => {
        seen.push(path);
        return { deliveryId: 'd1', status: 'pending' };
      }),
    );
    const row = await api.resendDelivery('d1');
    expect(seen).toEqual(['/api/notify/deliveries/d1/resend']);
    expect(row.status).toBe('pending');
  });

  it('previewTemplate 发 { payload } 且返回解析结果', async () => {
    const bodies: unknown[] = [];
    const api = new NotifyApi(
      fakeClient((_path, body) => {
        bodies.push(body);
        return { title: '你好 张三', body: '正文' };
      }),
    );
    const result = await api.previewTemplate('t1', { name: '张三' });
    expect(bodies[0]).toEqual({ payload: { name: '张三' } });
    expect(result).toEqual({ title: '你好 张三', body: '正文' });
  });

  it('setPreference 走 PUT 且发 camelCase underscoredOut?', async () => {
    const bodies: unknown[] = [];
    const api = new NotifyApi(
      fakeClient((_path, body) => {
        bodies.push(body);
        return { category: 'marketing', optedOut: true, updatedAt: 'x' };
      }),
    );
    await api.setPreference({ category: 'marketing', optedOut: true });
    // ⚠️ 字段名必须是 `optedOut`（camelCase），不是 `opted_out`。
    expect(bodies[0]).toEqual({ category: 'marketing', optedOut: true });
  });
});
