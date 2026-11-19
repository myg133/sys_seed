import { describe, expect, it, vi } from 'vitest';

import { BRANDING_ENDPOINT, applyBranding, fetchBranding } from '../src/lib/branding';

function fakeDocument() {
  const properties = new Map<string, string>();
  const attributes = new Map<string, string>();
  const doc = {
    title: '',
    documentElement: {
      style: {
        setProperty: (name: string, value: string) => properties.set(name, value),
      },
      setAttribute: (name: string, value: string) => attributes.set(name, value),
      getAttribute: (name: string) => attributes.get(name) ?? null,
    },
  } as unknown as Document;
  return { doc, properties, attributes };
}

describe('品牌配置', () => {
  it('把接口给的标题写进 document.title', () => {
    const { doc } = fakeDocument();
    const applied = applyBranding({ title: '运行时标题' }, doc);
    expect(applied.title).toBe('运行时标题');
    expect(doc.title).toBe('运行时标题');
  });

  it('合法主色写进 CSS 变量', () => {
    const { doc, properties } = fakeDocument();
    const applied = applyBranding({ title: 'x', primaryColor: '#123456' }, doc);
    expect(applied.primaryColor).toBe('#123456');
    expect(properties.get('--base-primary')).toBe('#123456');
  });

  it('非法主色由 @base/theme 写中性兜底 currentColor', () => {
    // REQ-011 起，shell 的品牌实现**委托** `@base/theme`：
    // 兜底值由主题包**自己**写到根元素上（不依赖宿主样式表）——
    // 否则宿主没有 `:root{--base-primary:currentColor}` 时读到的是空串。
    const { doc, properties } = fakeDocument();
    const rejected = applyBranding(
      { title: 'x', primaryColor: 'url(https://evil.example/x)' },
      doc,
    );
    expect(rejected.primaryColor).toBe('currentColor');
    expect(properties.get('--base-primary')).toBe('currentColor');
  });

  it('缺字段时留空，不回落任何旧品牌值', () => {
    const { doc, properties } = fakeDocument();
    const applied = applyBranding({}, doc);
    expect(applied.title).toBe('');
    expect(doc.title).toBe('');
    // 主色同样走中性兜底。
    expect(properties.get('--base-primary')).toBe('currentColor');
  });

  it('从同源相对路径读取品牌配置', async () => {
    const fetcher = vi.fn(
      async () => new Response(JSON.stringify({ title: '示例' }), { status: 200 }),
    ) as unknown as typeof fetch;
    const branding = await fetchBranding(fetcher);
    expect(branding.title).toBe('示例');
    expect(fetcher).toHaveBeenCalledWith(BRANDING_ENDPOINT, {
      headers: { accept: 'application/json' },
    });
    expect(BRANDING_ENDPOINT.startsWith('/')).toBe(true);
  });

  it('接口失败时抛错（由调用方降级）', async () => {
    const fetcher = vi.fn(
      async () => new Response('{}', { status: 500 }),
    ) as unknown as typeof fetch;
    await expect(fetchBranding(fetcher)).rejects.toThrow('status 500');
  });
});
