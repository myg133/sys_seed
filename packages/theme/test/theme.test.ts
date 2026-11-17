import { describe, expect, it } from 'vitest';

import { defaultTheme, mergeTheme } from '../src/index';

describe('@base/theme', () => {
  it('默认主题 token 齐全', () => {
    const theme = defaultTheme();
    expect(theme.primaryColor).toMatch(/^#[0-9a-f]{6}$/i);
    expect(theme.radius).toBeGreaterThan(0);
    expect(theme.fontFamily.length).toBeGreaterThan(0);
  });

  it('覆盖只影响指定项', () => {
    const theme = mergeTheme({ radius: 8 });
    expect(theme.radius).toBe(8);
    expect(theme.primaryColor).toBe(defaultTheme().primaryColor);
  });
});
