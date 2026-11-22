/**
 * 文件存储前端封装的单测（REQ-018 —— 前端侧）。
 *
 * ⚠️ **不测后端语义**：CSV 解析、病毒扫描、分片合并都在后端有真测试。
 * 这里只钉四件前端**自己**的事：
 *
 * 1. 路径 / 错误码 / 枚举 / 配置默认值与后端**逐字一致**；
 * 2. 等宽字段是 camelCase（契约 C13 / D258）—— 拼错就是「静默 0 条」；
 * 3. 形状不对时**不抛错**（列表页不白屏）；
 * 4. 界面判据的边界（进度百分比夹取、`skipped` ≠ 干净，D326）。
 */

import { describe, expect, it } from 'vitest';
import type { ApiClient, RequestOptions } from '@base/sdk';

import {
  STORAGE_CONFIG_KEYS,
  STORAGE_DEFAULT_IMPORT_BATCH_SIZE,
  STORAGE_DEFAULT_MAX_ROW_BYTES,
  STORAGE_DEFAULT_MAX_UPLOAD_BYTES,
  STORAGE_DEFAULT_PART_SIZE_BYTES,
  STORAGE_DEFAULT_PRESIGN_TTL_SECONDS,
  STORAGE_DEFAULT_UPLOAD_SESSION_TTL_SECONDS,
  STORAGE_ERROR_CODES,
  STORAGE_EXPORTS_PATH,
  STORAGE_IMPORTS_PATH,
  STORAGE_JOBS_PATH,
  STORAGE_JOB_KINDS,
  STORAGE_JOB_STATUSES,
  STORAGE_OBJECTS_PATH,
  STORAGE_SCAN_VERDICTS,
  STORAGE_UPLOADS_PATH,
  STORAGE_UPLOAD_STATUSES,
  StorageApi,
  isFileScanned,
  isJobActive,
  isJobDownloadable,
  isKnownStorageErrorCode,
  jobProgressPercent,
  parseJobErrors,
  parseJobs,
  parseObjects,
  parsePresign,
  storageErrorKey,
  storageJobErrorsPath,
  storageJobPath,
  storageObjectPath,
  storagePresignPath,
  storageUploadCompletePath,
  storageUploadPartPath,
  storageUploadPath,
} from '../src/lib/storage-api';
import type { StorageJobView, StorageObjectView } from '../src/lib/storage-api';

/** 一次被记录下来的调用。 */
interface RecordedCall {
  method: string;
  path: string;
  body?: unknown;
}

/**
 * 记录全部调用的假 `ApiClient`。
 *
 * `request` 读 `options.method`，因此 `uploadObject` / `uploadPart` 用 `PUT`
 * 的真实行为（而不是「碰巧调了 put 快捷方式」）也被钉住。
 */
function fakeClient(respond: (call: RecordedCall) => unknown): {
  client: ApiClient;
  calls: RecordedCall[];
} {
  const calls: RecordedCall[] = [];
  const answer = (call: RecordedCall): unknown => {
    calls.push(call);
    return respond(call);
  };
  const client = {
    baseUrl: '',
    request: async <T>(options: RequestOptions) =>
      answer({
        method: options.method ?? 'GET',
        path: options.path,
        body: options.body,
      }) as T,
    get: async <T>(path: string) => answer({ method: 'GET', path }) as T,
    post: async <T>(path: string, body?: unknown) => answer({ method: 'POST', path, body }) as T,
    put: async <T>(path: string, body?: unknown) => answer({ method: 'PUT', path, body }) as T,
    delete: async <T>(path: string) => answer({ method: 'DELETE', path }) as T,
  } as unknown as ApiClient;
  return { client, calls };
}

/** 造一个任务视图（只覆盖被断言用到的字段）。 */
function job(overrides: Partial<StorageJobView> = {}): StorageJobView {
  return {
    jobId: 'j-1',
    kind: 'export',
    entity: 'customer',
    status: 'completed',
    totalRows: 100,
    processedRows: 100,
    failedRows: 0,
    downloadUrl: 'https://files.example/x.csv',
    error: null,
    createdAt: '2026-01-01T00:00:00Z',
    finishedAt: '2026-01-01T00:01:00Z',
    ...overrides,
  };
}

/** 造一个对象视图。 */
function objectView(): StorageObjectView {
  return {
    objectKey: 'tenant/a.csv',
    sizeBytes: 10,
    sha256: 'abc',
    contentType: 'text/csv',
    scanVerdict: 'clean',
    createdAt: '2026-01-01T00:00:00Z',
  };
}

describe('路径常量', () => {
  it('5 个基路径逐字等于后端路由', () => {
    expect(STORAGE_OBJECTS_PATH).toBe('/api/storage/objects');
    expect(STORAGE_UPLOADS_PATH).toBe('/api/storage/uploads');
    expect(STORAGE_IMPORTS_PATH).toBe('/api/storage/imports');
    expect(STORAGE_EXPORTS_PATH).toBe('/api/storage/exports');
    expect(STORAGE_JOBS_PATH).toBe('/api/storage/jobs');
  });

  it('派生路径逐字（对象 / 预签名 / 分片 / 完成 / 任务 / 错误回执）', () => {
    expect(storageObjectPath('a.csv')).toBe('/api/storage/objects/a.csv');
    expect(storagePresignPath('a.csv')).toBe('/api/storage/objects/a.csv/presign');
    expect(storageUploadPath('u-1')).toBe('/api/storage/uploads/u-1');
    expect(storageUploadPartPath('u-1', 3)).toBe('/api/storage/uploads/u-1/parts/3');
    expect(storageUploadCompletePath('u-1')).toBe('/api/storage/uploads/u-1/complete');
    expect(storageJobPath('j-1')).toBe('/api/storage/jobs/j-1');
    expect(storageJobErrorsPath('j-1')).toBe('/api/storage/jobs/j-1/errors');
  });

  it('key / id 会被 URL 编码（防止路径穿越与空格）', () => {
    expect(storageObjectPath('a/b c.csv')).toBe('/api/storage/objects/a%2Fb%20c.csv');
    expect(storagePresignPath('a/b c.csv')).toBe(
      '/api/storage/objects/a%2Fb%20c.csv/presign',
    );
    expect(storageUploadPath('../x')).toBe('/api/storage/uploads/..%2Fx');
  });
});

describe('错误码白名单', () => {
  it('恰好 10 个且逐字等于后端 ALL_CODES', () => {
    // ⚠️ 10 = D330 基线 9 + R-05 新增的 STORAGE_UNKNOWN_ENTITY。
    expect(STORAGE_ERROR_CODES).toHaveLength(10);
    expect([...STORAGE_ERROR_CODES]).toEqual([
      'STORAGE_OBJECT_NOT_FOUND',
      'STORAGE_PRESIGN_EXPIRED',
      'STORAGE_PRESIGN_INVALID',
      'STORAGE_PART_MISSING',
      'STORAGE_CHECKSUM_MISMATCH',
      'STORAGE_UPLOAD_TOO_LARGE',
      'STORAGE_UNSUPPORTED_FORMAT',
      'STORAGE_FILE_INFECTED',
      'STORAGE_TTL_OUT_OF_RANGE',
      'STORAGE_UNKNOWN_ENTITY',
    ]);
  });

  it('无重复', () => {
    expect(new Set(STORAGE_ERROR_CODES).size).toBe(STORAGE_ERROR_CODES.length);
  });

  it('不含 storage.* 事件码（D223）', () => {
    for (const code of STORAGE_ERROR_CODES) {
      expect(code.startsWith('storage.')).toBe(false);
      expect(code.startsWith('STORAGE_')).toBe(true);
    }
  });

  it('storageErrorKey 生成 error.<CODE>，isKnownStorageErrorCode 只认白名单', () => {
    expect(storageErrorKey('STORAGE_PART_MISSING')).toBe('error.STORAGE_PART_MISSING');
    expect(isKnownStorageErrorCode('STORAGE_FILE_INFECTED')).toBe(true);
    expect(isKnownStorageErrorCode('storage.upload.started')).toBe(false);
    expect(isKnownStorageErrorCode('NOTIFY_TEMPLATE_NOT_FOUND')).toBe(false);
  });
});

describe('状态 / 判定枚举', () => {
  it('扫描结论恰好 3 个', () => {
    expect([...STORAGE_SCAN_VERDICTS]).toEqual(['skipped', 'clean', 'infected']);
  });

  it('任务状态恰好 4 个', () => {
    expect([...STORAGE_JOB_STATUSES]).toEqual(['pending', 'running', 'completed', 'failed']);
  });

  it('任务类型恰好 2 个', () => {
    expect([...STORAGE_JOB_KINDS]).toEqual(['import', 'export']);
  });

  it('上传会话状态恰好 4 个', () => {
    expect([...STORAGE_UPLOAD_STATUSES]).toEqual([
      'pending',
      'completing',
      'completed',
      'aborted',
    ]);
  });
});

describe('配置默认值', () => {
  it('6 个配置键逐字', () => {
    expect([...STORAGE_CONFIG_KEYS]).toEqual([
      'storage.maxUploadBytes',
      'storage.partSizeBytes',
      'storage.presignTtlSeconds',
      'storage.importBatchSize',
      'storage.maxRowBytes',
      'storage.uploadSessionTtlSeconds',
    ]);
  });

  it('6 个默认值逐字（前端只做常量，不读值）', () => {
    expect(STORAGE_DEFAULT_MAX_UPLOAD_BYTES).toBe(104857600);
    expect(STORAGE_DEFAULT_PART_SIZE_BYTES).toBe(8388608);
    expect(STORAGE_DEFAULT_PRESIGN_TTL_SECONDS).toBe(900);
    expect(STORAGE_DEFAULT_IMPORT_BATCH_SIZE).toBe(1000);
    expect(STORAGE_DEFAULT_MAX_ROW_BYTES).toBe(65536);
    expect(STORAGE_DEFAULT_UPLOAD_SESSION_TTL_SECONDS).toBe(86400);
  });
});

describe('解析（形状容错）', () => {
  it('parseObjects：数组原样返回，非数组 → []', () => {
    const view = objectView();
    expect(parseObjects([view])).toEqual([view]);
    expect(parseObjects({ items: [view] })).toEqual([]);
    expect(parseObjects(null)).toEqual([]);
    expect(parseObjects('nope')).toEqual([]);
  });

  it('parseJobs：数组原样返回，非数组 → []', () => {
    const view = job();
    expect(parseJobs([view])).toEqual([view]);
    expect(parseJobs({})).toEqual([]);
    expect(parseJobs(undefined)).toEqual([]);
  });

  it('parseJobErrors：数组原样返回，非数组 → []', () => {
    const errors = [{ rowNumber: 2, column: 'email', reason: '格式非法' }];
    expect(parseJobErrors(errors)).toEqual(errors);
    expect(parseJobErrors({ errors })).toEqual([]);
    expect(parseJobErrors(0)).toEqual([]);
  });

  it('parsePresign：字段齐全才返回，缺字段 / 非对象 → null', () => {
    expect(parsePresign({ url: 'https://x/y', expiresAt: '2026-01-01T00:15:00Z' })).toEqual({
      url: 'https://x/y',
      expiresAt: '2026-01-01T00:15:00Z',
    });
    expect(parsePresign({ url: 'https://x/y' })).toBeNull();
    expect(parsePresign({ expiresAt: '2026-01-01T00:15:00Z' })).toBeNull();
    expect(parsePresign(null)).toBeNull();
  });
});

describe('StorageApi 走对路径与方法', () => {
  it('listObjects → GET /api/storage/objects', async () => {
    const { client, calls } = fakeClient(() => [objectView()]);
    const views = await new StorageApi(client).listObjects();
    expect(views).toHaveLength(1);
    expect(calls).toEqual([{ method: 'GET', path: '/api/storage/objects', body: undefined }]);
  });

  it('uploadObject → PUT（走 request，因为 put() 只收 JSON body）', async () => {
    const { client, calls } = fakeClient(() => ({ ok: true }));
    const blob = 'raw-bytes';
    await new StorageApi(client).uploadObject('a.csv', blob);
    expect(calls).toEqual([
      { method: 'PUT', path: '/api/storage/objects/a.csv', body: blob },
    ]);
  });

  it('presignObject → POST .../presign，带 ttlSeconds', async () => {
    const { client, calls } = fakeClient(() => ({
      url: 'https://x/y',
      expiresAt: '2026-01-01T00:15:00Z',
    }));
    const result = await new StorageApi(client).presignObject('a.csv', 900);
    expect(result?.url).toBe('https://x/y');
    expect(calls).toEqual([
      { method: 'POST', path: '/api/storage/objects/a.csv/presign', body: { ttlSeconds: 900 } },
    ]);
  });

  it('presignObject 形状不对 → null（不抛错）', async () => {
    const { client } = fakeClient(() => ({}));
    await expect(new StorageApi(client).presignObject('a.csv', 900)).resolves.toBeNull();
  });

  it('deleteObject → DELETE /api/storage/objects/{key}', async () => {
    const { client, calls } = fakeClient(() => undefined);
    await new StorageApi(client).deleteObject('a.csv');
    expect(calls).toEqual([{ method: 'DELETE', path: '/api/storage/objects/a.csv', body: undefined }]);
  });

  it('createUpload → POST /api/storage/uploads，带请求体', async () => {
    const body = { objectKey: 'a.csv', totalParts: 4, partSize: 8388608 };
    const { client, calls } = fakeClient(() => ({ uploadId: 'u-1' }));
    await new StorageApi(client).createUpload(body);
    expect(calls).toEqual([{ method: 'POST', path: '/api/storage/uploads', body }]);
  });

  it('uploadPart → PUT .../parts/{n}（走 request）', async () => {
    const { client, calls } = fakeClient(() => ({ ok: true }));
    await new StorageApi(client).uploadPart('u-1', 2, 'chunk');
    expect(calls).toEqual([
      { method: 'PUT', path: '/api/storage/uploads/u-1/parts/2', body: 'chunk' },
    ]);
  });

  it('completeUpload → POST .../complete；给 contentType 才带上', async () => {
    const first = fakeClient(() => ({}));
    await new StorageApi(first.client).completeUpload('u-1');
    expect(first.calls).toEqual([
      { method: 'POST', path: '/api/storage/uploads/u-1/complete', body: {} },
    ]);

    const second = fakeClient(() => ({}));
    await new StorageApi(second.client).completeUpload('u-1', 'text/csv');
    expect(second.calls).toEqual([
      {
        method: 'POST',
        path: '/api/storage/uploads/u-1/complete',
        body: { contentType: 'text/csv' },
      },
    ]);
  });

  it('abortUpload → DELETE /api/storage/uploads/{id}', async () => {
    const { client, calls } = fakeClient(() => undefined);
    await new StorageApi(client).abortUpload('u-1');
    expect(calls).toEqual([{ method: 'DELETE', path: '/api/storage/uploads/u-1', body: undefined }]);
  });

  it('createImport → POST /api/storage/imports，带 entity + objectKey', async () => {
    const { client, calls } = fakeClient(() => ({ jobId: 'j-1' }));
    await new StorageApi(client).createImport('customer', 'a.csv');
    expect(calls).toEqual([
      { method: 'POST', path: '/api/storage/imports', body: { entity: 'customer', objectKey: 'a.csv' } },
    ]);
  });

  it('createExport → POST /api/storage/exports，带 entity', async () => {
    const { client, calls } = fakeClient(() => ({ jobId: 'j-2' }));
    await new StorageApi(client).createExport('customer');
    expect(calls).toEqual([
      { method: 'POST', path: '/api/storage/exports', body: { entity: 'customer' } },
    ]);
  });

  it('listJobs / getJob / listJobErrors 走对路径', async () => {
    const { client, calls } = fakeClient((call) => {
      if (call.path.endsWith('/errors')) {
        return [{ rowNumber: 1, column: 'id', reason: '重复' }];
      }
      if (call.path === '/api/storage/jobs') {
        return [job()];
      }
      return job();
    });
    const api = new StorageApi(client);

    expect(await api.listJobs()).toHaveLength(1);
    expect((await api.getJob('j-1'))?.jobId).toBe('j-1');
    expect(await api.listJobErrors('j-1')).toHaveLength(1);

    expect(calls).toEqual([
      { method: 'GET', path: '/api/storage/jobs', body: undefined },
      { method: 'GET', path: '/api/storage/jobs/j-1', body: undefined },
      { method: 'GET', path: '/api/storage/jobs/j-1/errors', body: undefined },
    ]);
  });

  it('getJob 形状不对 → null（不抛错）', async () => {
    const { client } = fakeClient(() => 'nope');
    await expect(new StorageApi(client).getJob('j-1')).resolves.toBeNull();
  });
});

describe('纯函数边界', () => {
  it('jobProgressPercent：totalRows=0 → 0（不是 NaN / Infinity）', () => {
    const result = jobProgressPercent(job({ totalRows: 0, processedRows: 0 }));
    expect(result).toBe(0);
    expect(Number.isFinite(result)).toBe(true);
    expect(Number.isInteger(result)).toBe(true);
  });

  it('jobProgressPercent：50/100 → 50', () => {
    expect(jobProgressPercent(job({ totalRows: 100, processedRows: 50 }))).toBe(50);
  });

  it('jobProgressPercent：200/100 → 夹到 100', () => {
    expect(jobProgressPercent(job({ totalRows: 100, processedRows: 200 }))).toBe(100);
  });

  it('jobProgressPercent：结果永远是 [0,100] 的有限整数', () => {
    const samples = [
      job({ totalRows: 3, processedRows: 1 }),
      job({ totalRows: 7, processedRows: 0 }),
      job({ totalRows: 1, processedRows: 1 }),
      job({ totalRows: 0, processedRows: 5 }),
    ];
    for (const sample of samples) {
      const value = jobProgressPercent(sample);
      expect(Number.isInteger(value)).toBe(true);
      expect(Number.isFinite(value)).toBe(true);
      expect(value).toBeGreaterThanOrEqual(0);
      expect(value).toBeLessThanOrEqual(100);
    }
  });

  it('isJobDownloadable：export + completed + 有 url = true', () => {
    expect(isJobDownloadable(job())).toBe(true);
  });

  it('isJobDownloadable：非 export / 未完成 / 无 url 都为 false', () => {
    expect(isJobDownloadable(job({ kind: 'import' }))).toBe(false);
    expect(isJobDownloadable(job({ status: 'running', finishedAt: null }))).toBe(false);
    expect(isJobDownloadable(job({ downloadUrl: null }))).toBe(false);
    expect(isJobDownloadable(job({ downloadUrl: '' }))).toBe(false);
  });

  it('isJobActive：pending / running = true，completed / failed = false', () => {
    expect(isJobActive(job({ status: 'pending' }))).toBe(true);
    expect(isJobActive(job({ status: 'running' }))).toBe(true);
    expect(isJobActive(job({ status: 'completed' }))).toBe(false);
    expect(isJobActive(job({ status: 'failed' }))).toBe(false);
  });

  it('isFileScanned：skipped = false（没扫 ≠ 干净，D326），clean / infected = true', () => {
    expect(isFileScanned({ scanVerdict: 'skipped' })).toBe(false);
    expect(isFileScanned({ scanVerdict: 'clean' })).toBe(true);
    expect(isFileScanned({ scanVerdict: 'infected' })).toBe(true);
  });
});
