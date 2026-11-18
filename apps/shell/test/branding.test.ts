import { describe, expect, it, vi } from 'vitest';

import { BRANDING_ENDPOINT, applyBranding, fetchBranding } from '../src/lib/branding';

function fakeDocument() {
  const properties = new Map<string, string>();
  const doc = {
    title: '',
    documentElement: {
      style: {
        setProperty: (name: string, value: string) => properties.set(name, value),
      },
    },
  } as unknown as Document;
  return { doc, properties };
}

describe('品牌配置', () => {
  it('把接口给的标题写进 document.title', () => {
    const { doc } = fakeDocument();
    const applied = applyBranding({ title: '运行时标题' }, doc);
    expect(applied.title).toBe('运行时标题');
    expect(doc.title).toBe('运行时标题');
  });

  it('合法主色写进 CSS 变量，非法值不写', () => {
    const { doc, properties } = fakeDocument();
    const applied = applyBranding({ title: 'x', primaryColor: '#123456' }, doc);
    expect(applied.primaryColor).toBe('#123456');
    expect(properties.get('--base-primary')).toBe('#123456');

    const { doc: second, properties: secondProperties } = fakeDocument();
    const rejected = applyBranding(
      { title: 'x', primaryColor: 'url(https://evil.example/x)' },
      second,
    );
    expect(rejected.primaryColor).toBeUndefined();
    expect(secondProperties.has('--base-primary')).toBe(false);
  });

  it('缺字段时留空，不回落任何旧品牌值', () => {
    const { doc } = fakeDocument();
    const applied = applyBranding({}, doc);
    expect(applied.title).toBe('');
    expect(doc.title).toBe('');
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
