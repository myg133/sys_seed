import { describe, expect, it } from 'vitest';

import { TONES, defaultRadius, toneClass } from '../src/index';

describe('@base/ui', () => {
  it('色调类名可预期', () => {
    expect(toneClass('primary')).toBe('base-tone base-tone--primary');
    expect(toneClass('danger')).toBe('base-tone base-tone--danger');
  });

  it('默认圆角 token 递增', () => {
    const radius = defaultRadius();
    expect(radius.small).toBeLessThan(radius.medium);
    expect(radius.medium).toBeLessThan(radius.large);
  });

  it('色调清单包含全部可选值', () => {
    expect(TONES).toEqual(['default', 'primary', 'danger']);
  });
});
