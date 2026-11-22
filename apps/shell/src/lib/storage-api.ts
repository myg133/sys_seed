/**
 * 文件存储接口封装（REQ-018 —— 前端侧）。
 *
 * # 字段名 = camelCase（契约 C13 / D258）
 *
 * `/api/storage*` 的 JSON 字段**一律 camelCase**：`objectKey` / `sizeBytes` / `sha256` /
 * `contentType` / `scanVerdict` / `uploadId` / `totalParts` / `partSize` / `jobId` / `kind` /
 * `totalRows` / `processedRows` / `failedRows` / `downloadUrl` / `rowNumber` / `column` /
 * `reason`。逐字对照 `design-summary.md` §3.1 的字段表 —— 改一个要改两处。
 *
 * # 响应形状，不要混着用
 *
 * | 端点 | 形状 |
 * |---|---|
 * | `GET /api/storage/objects` | **数组** `[StorageObjectView]` |
 * | `POST /api/storage/objects/{key}/presign` | `{ url, expiresAt }` |
 * | `GET /api/storage/jobs` | **数组** `[StorageJobView]` |
 * | `GET /api/storage/jobs/{id}/errors` | **数组** `[StorageJobErrorView]` |
 *
 * 把数组当 `{ items: [] }` 读会**静默**得到「0 条」而不是报错 ——
 * 所以每个解析函数都独立、且形状不对时**不抛错**（列表页不白屏）。
 *
 * # ⚠️ `scanVerdict = 'skipped'` 表示「没扫」，不是「干净」（D326）
 *
 * 判据用 `isFileScanned()`：只有 `clean` / `infected` 算扫过。
 * 界面上不能把 `skipped` 画成绿色对勾。
 *
 * # ⚠️ `ApiClient` 没有 `patch` 快捷方式
 *
 * 上传与分片用 `client.request({ method: 'PUT', ... })` —— `put()` 只接受 JSON body，
 * 这里要传 `BodyInit`。改/写方法同样走 `request`。
 */

import type { ApiClient } from '@base/sdk';

/* -------------------------------------------------------------------------- */
/* 路径常量（逐字 —— 与后端 `src/http.rs` 对照用）                              */
/* -------------------------------------------------------------------------- */

/** 对象列表 / 直接上传。 */
export const STORAGE_OBJECTS_PATH = '/api/storage/objects';

/** 分片上传会话。 */
export const STORAGE_UPLOADS_PATH = '/api/storage/uploads';

/** 异步导入（CSV）。 */
export const STORAGE_IMPORTS_PATH = '/api/storage/imports';

/** 异步导出。 */
export const STORAGE_EXPORTS_PATH = '/api/storage/exports';

/** 导入导出任务。 */
export const STORAGE_JOBS_PATH = '/api/storage/jobs';

/** 单个对象的元数据 / 删除。 */
export function storageObjectPath(key: string): string {
  return `${STORAGE_OBJECTS_PATH}/${encodeURIComponent(key)}`;
}

/** 预签名下载路径。 */
export function storagePresignPath(key: string): string {
  return `${storageObjectPath(key)}/presign`;
}

/** 单个上传会话。 */
export function storageUploadPath(id: string): string {
  return `${STORAGE_UPLOADS_PATH}/${encodeURIComponent(id)}`;
}

/** 单个分片。 */
export function storageUploadPartPath(id: string, n: number): string {
  return `${storageUploadPath(id)}/parts/${n}`;
}

/** 完成分片上传。 */
export function storageUploadCompletePath(id: string): string {
  return `${storageUploadPath(id)}/complete`;
}

/** 单个任务。 */
export function storageJobPath(id: string): string {
  return `${STORAGE_JOBS_PATH}/${encodeURIComponent(id)}`;
}

/** 任务的行级错误回执。 */
export function storageJobErrorsPath(id: string): string {
  return `${storageJobPath(id)}/errors`;
}

/* -------------------------------------------------------------------------- */
/* 权限点（契约 C4：`module:resource:action`）                                   */
/* -------------------------------------------------------------------------- */

/** 查看与下载文件（对象列表 / 预签名）。 */
export const STORAGE_FILE_READ_PERMISSION = 'storage:file:read';

/** 上传与删除文件。 */
export const STORAGE_FILE_WRITE_PERMISSION = 'storage:file:write';

/** 查看导入导出任务。 */
export const STORAGE_JOB_READ_PERMISSION = 'storage:job:read';

/* -------------------------------------------------------------------------- */
/* 白名单                                                                      */
/* -------------------------------------------------------------------------- */

/**
 * 平台错误码白名单（**恰好 9 个**，`design-summary.md` §5）。
 *
 * 码逐字等于后端 `crates/platform-storage/src/error.rs` 的 `ALL_CODES` —— 改一个要改两处。
 * ⚠️ 4 个 `storage.*` 审计**事件码不在这里**（D223：事件码不是 HTTP 错误码）。
 */
export const STORAGE_ERROR_CODES = [
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
] as const;

/** 平台错误码。 */
export type StorageErrorCode = (typeof STORAGE_ERROR_CODES)[number];

/** 错误码 → i18n 键（`error.<CODE>`，与 `packages/i18n` 的扁平键一致）。 */
export function storageErrorKey(code: string): string {
  return `error.${code}`;
}

/** 该错误码是否已登记（用于「未知码也要显示得出来」的兜底）。 */
export function isKnownStorageErrorCode(code: string): code is StorageErrorCode {
  return (STORAGE_ERROR_CODES as readonly string[]).includes(code);
}

/* -------------------------------------------------------------------------- */
/* 状态 / 判定枚举                                                             */
/* -------------------------------------------------------------------------- */

/** 病毒扫描结论（`skipped` = **没扫**，不是「干净」，D326）。 */
export const STORAGE_SCAN_VERDICTS = ['skipped', 'clean', 'infected'] as const;

/** 任务状态。 */
export const STORAGE_JOB_STATUSES = ['pending', 'running', 'completed', 'failed'] as const;

/** 任务类型。 */
export const STORAGE_JOB_KINDS = ['import', 'export'] as const;

/** 上传会话状态。 */
export const STORAGE_UPLOAD_STATUSES = ['pending', 'completing', 'completed', 'aborted'] as const;

/* -------------------------------------------------------------------------- */
/* 配置默认值（前端只做常量，**不读值**）                                       */
/* -------------------------------------------------------------------------- */

/** 配置键（后端读值；前端只用于展示/校验提示）。 */
export const STORAGE_CONFIG_KEYS = [
  'storage.maxUploadBytes',
  'storage.partSizeBytes',
  'storage.presignTtlSeconds',
  'storage.importBatchSize',
  'storage.maxRowBytes',
  'storage.uploadSessionTtlSeconds',
] as const;

/** `storage.maxUploadBytes` 默认值（100 MiB）。 */
export const STORAGE_DEFAULT_MAX_UPLOAD_BYTES = 104857600;

/** `storage.partSizeBytes` 默认值（8 MiB）。 */
export const STORAGE_DEFAULT_PART_SIZE_BYTES = 8388608;

/** `storage.presignTtlSeconds` 默认值（15 分钟）。 */
export const STORAGE_DEFAULT_PRESIGN_TTL_SECONDS = 900;

/** `storage.importBatchSize` 默认值。 */
export const STORAGE_DEFAULT_IMPORT_BATCH_SIZE = 1000;

/** `storage.maxRowBytes` 默认值（64 KiB）。 */
export const STORAGE_DEFAULT_MAX_ROW_BYTES = 65536;

/** `storage.uploadSessionTtlSeconds` 默认值（24 小时）。 */
export const STORAGE_DEFAULT_UPLOAD_SESSION_TTL_SECONDS = 86400;

/* -------------------------------------------------------------------------- */
/* 视图类型                                                                    */
/* -------------------------------------------------------------------------- */

/** 对象元数据（camelCase）。 */
export interface StorageObjectView {
  objectKey: string;
  sizeBytes: number;
  sha256: string;
  contentType: string;
  scanVerdict: string;
  createdAt: string;
}

/** 预签名结果。 */
export interface PresignView {
  url: string;
  expiresAt: string;
}

/** 导入导出任务（camelCase）。 */
export interface StorageJobView {
  jobId: string;
  kind: string;
  entity: string;
  status: string;
  totalRows: number;
  processedRows: number;
  failedRows: number;
  downloadUrl: string | null;
  error: string | null;
  createdAt: string;
  finishedAt: string | null;
}

/** 任务的行级错误回执（camelCase）。 */
export interface StorageJobErrorView {
  rowNumber: number;
  column: string;
  reason: string;
}

/** 建上传会话的请求体。 */
export interface CreateUploadBody {
  objectKey: string;
  totalParts: number;
  partSize: number;
  checksum?: string;
}

/* -------------------------------------------------------------------------- */
/* 解析（形状不对时返回空，不抛错）                                             */
/* -------------------------------------------------------------------------- */

function asArray<T>(value: unknown): T[] {
  return Array.isArray(value) ? (value as T[]) : [];
}

/** 解析对象列表（非数组 → `[]`）。 */
export function parseObjects(payload: unknown): StorageObjectView[] {
  return asArray<StorageObjectView>(payload);
}

/** 解析任务列表（非数组 → `[]`）。 */
export function parseJobs(payload: unknown): StorageJobView[] {
  return asArray<StorageJobView>(payload);
}

/** 解析行级错误回执（非数组 → `[]`）。 */
export function parseJobErrors(payload: unknown): StorageJobErrorView[] {
  return asArray<StorageJobErrorView>(payload);
}

/** 解析预签名结果（缺字段 → `null`）。 */
export function parsePresign(payload: unknown): PresignView | null {
  if (typeof payload !== 'object' || payload === null) {
    return null;
  }
  const record = payload as Record<string, unknown>;
  const url = record['url'];
  const expiresAt = record['expiresAt'];
  if (typeof url !== 'string' || typeof expiresAt !== 'string') {
    return null;
  }
  return { url, expiresAt };
}

/* -------------------------------------------------------------------------- */
/* 纯函数（界面判据）                                                          */
/* -------------------------------------------------------------------------- */

/**
 * 该任务是否可下载结果：**只有** `export` + `completed` + 有 `downloadUrl`。
 *
 * `import` 任务即便完成也没有可下载产物 —— 返回 `false`。
 */
export function isJobDownloadable(job: StorageJobView): boolean {
  return (
    job.kind === 'export' &&
    job.status === 'completed' &&
    typeof job.downloadUrl === 'string' &&
    job.downloadUrl.length > 0
  );
}

/** 该任务是否还在跑（`pending` / `running`）—— 决定要不要显示进度条。 */
export function isJobActive(job: StorageJobView): boolean {
  return job.status === 'pending' || job.status === 'running';
}

/**
 * 进度百分比（`0~100` 的整数；`totalRows` 为 0 → `0`）。
 *
 * ⚠️ `totalRows` 为 0 时**不能**算出 `NaN` / `Infinity`；`processedRows` 越界时夹到 `100`。
 */
export function jobProgressPercent(job: StorageJobView): number {
  if (!Number.isFinite(job.totalRows) || job.totalRows <= 0) {
    return 0;
  }
  if (!Number.isFinite(job.processedRows) || job.processedRows <= 0) {
    return 0;
  }
  const raw = (job.processedRows / job.totalRows) * 100;
  const clamped = Math.min(100, Math.max(0, Math.floor(raw)));
  return Number.isFinite(clamped) ? clamped : 0;
}

/**
 * 该文件是否被扫描过（`skipped` 表示**没扫**，不是「干净」——D326）。
 *
 * 只有 `clean` / `infected` 算扫过。
 */
export function isFileScanned(job: { scanVerdict: string }): boolean {
  return job.scanVerdict === 'clean' || job.scanVerdict === 'infected';
}

/* -------------------------------------------------------------------------- */
/* 客户端                                                                      */
/* -------------------------------------------------------------------------- */

/** 文件存储的接口客户端。 */
export class StorageApi {
  constructor(private readonly client: ApiClient) {}

  /** 列出对象（`storage:file:read`）。 */
  async listObjects(): Promise<StorageObjectView[]> {
    return parseObjects(await this.client.get(STORAGE_OBJECTS_PATH));
  }

  /**
   * 直接上传对象（`PUT`）。
   *
   * ⚠️ `put()` 只接受 JSON body；这里要传 `BodyInit`，故走 `request` 显式指定方法。
   */
  async uploadObject(key: string, body: BodyInit): Promise<unknown> {
    return this.client.request({
      method: 'PUT',
      path: storageObjectPath(key),
      body,
    });
  }

  /** 生成预签名下载链接（`storage:file:read`）。 */
  async presignObject(key: string, ttlSeconds: number): Promise<PresignView | null> {
    const raw = await this.client.post(storagePresignPath(key), { ttlSeconds });
    return parsePresign(raw);
  }

  /** 删除对象（`204`）。 */
  async deleteObject(key: string): Promise<void> {
    await this.client.delete(storageObjectPath(key));
  }

  /** 建分片上传会话。 */
  async createUpload(body: CreateUploadBody): Promise<unknown> {
    return this.client.post(STORAGE_UPLOADS_PATH, body);
  }

  /** 上传单个分片（`PUT`；`BodyInit` 走 `request`）。 */
  async uploadPart(uploadId: string, n: number, body: BodyInit): Promise<unknown> {
    return this.client.request({
      method: 'PUT',
      path: storageUploadPartPath(uploadId, n),
      body,
    });
  }

  /** 完成分片上传。 */
  async completeUpload(uploadId: string, contentType?: string): Promise<unknown> {
    const body = contentType === undefined ? {} : { contentType };
    return this.client.post(storageUploadCompletePath(uploadId), body);
  }

  /** 中止分片上传（`204`）。 */
  async abortUpload(uploadId: string): Promise<void> {
    await this.client.delete(storageUploadPath(uploadId));
  }

  /** 发起异步导入（CSV）。 */
  async createImport(entity: string, objectKey: string): Promise<unknown> {
    return this.client.post(STORAGE_IMPORTS_PATH, { entity, objectKey });
  }

  /** 发起异步导出。 */
  async createExport(entity: string): Promise<unknown> {
    return this.client.post(STORAGE_EXPORTS_PATH, { entity });
  }

  /** 列出导入导出任务（`storage:job:read`）。 */
  async listJobs(): Promise<StorageJobView[]> {
    return parseJobs(await this.client.get(STORAGE_JOBS_PATH));
  }

  /** 读单个任务。 */
  async getJob(id: string): Promise<StorageJobView | null> {
    const payload = await this.client.get(storageJobPath(id));
    if (typeof payload !== 'object' || payload === null) {
      return null;
    }
    return payload as StorageJobView;
  }

  /** 读任务的行级错误回执。 */
  async listJobErrors(id: string): Promise<StorageJobErrorView[]> {
    return parseJobErrors(await this.client.get(storageJobErrorsPath(id)));
  }
}
