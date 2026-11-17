import { describe, expect, it } from 'vitest';

import { createToggle } from '../src/index';

describe('@base/hooks', () => {
  it('初始值可指定', () => {
    expect(createToggle().value).toBe(false);
    expect(createToggle(true).value).toBe(true);
  });

  it('toggle 取反当前值', () => {
    const toggle = createToggle();
    toggle.toggle();
    expect(toggle.value).toBe(true);
    toggle.toggle();
    expect(toggle.value).toBe(false);
  });

  it('set 写入指定值', () => {
    const toggle = createToggle();
    toggle.set(true);
    expect(toggle.value).toBe(true);
  });
});
