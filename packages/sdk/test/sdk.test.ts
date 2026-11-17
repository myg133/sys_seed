import { describe, expect, it } from 'vitest';

import { createApiClient } from '../src/index';

describe('@base/sdk', () => {
  it('去掉结尾斜杠', () => {
    expect(createApiClient({ baseUrl: 'https://api.base.example/' }).baseUrl).toBe(
      'https://api.base.example',
    );
  });

  it('拼接路径时补上分隔斜杠', () => {
    const client = createApiClient({ baseUrl: 'https://api.base.example' });
    expect(client.url('/api/iam/users')).toBe('https://api.base.example/api/iam/users');
    expect(client.url('api/iam/users')).toBe('https://api.base.example/api/iam/users');
  });
});
