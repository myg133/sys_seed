/**
 * @base/hooks —— 通用状态容器（REQ-011）。
 *
 * | 能力 | 约定 |
 * |---|---|
 * | 分页 / 表格状态 | **纯状态容器**，不绑框架（消费方自己接响应式） |
 * | SSE / WebSocket | **重连退避**；**关闭后不重连**（组件卸载后重连会泄漏） |
 * | 剪贴板 | 不可用时**返回失败**（不静默吞掉 —— 用户会以为复制成功了） |
 *
 * # 为什么分页状态在 hooks、表格组件在 ui
 *
 * `@base/ui` 的表格**只接受 props**（AC-S-075）——
 * 分页状态由消费方用本包的 `createPagination()` 组合后传进去，
 * 两个包因此**互不依赖**（`ui` 不引 `hooks`，`hooks` 不引 `ui`）。
 */

/** 布尔开关状态。 */
export interface Toggle {
  /** 当前值。 */
  readonly value: boolean;
  /** 取反。 */
  toggle(): void;
  /** 设为指定值。 */
  set(next: boolean): void;
}

/** 创建布尔开关状态。 */
export function createToggle(initial = false): Toggle {
  let current = initial;
  return {
    get value(): boolean {
      return current;
    },
    toggle(): void {
      current = !current;
    },
    set(next: boolean): void {
      current = next;
    },
  };
}

/** 分页状态。 */
export interface PaginationState {
  /** 当前页码（从 1 开始）。 */
  readonly page: number;
  /** 每页行数。 */
  readonly pageSize: number;
  /** 总行数。 */
  readonly total: number;
  /** 总页数（至少 1）。 */
  readonly pageCount: number;
  /** 当前页第一行的序号（从 1 开始；空数据时为 0）。 */
  readonly from: number;
  /** 当前页最后一行的序号（从 1 开始；空数据时为 0）。 */
  readonly to: number;
  /** 是否有上一页。 */
  readonly canPrev: boolean;
  /** 是否有下一页。 */
  readonly canNext: boolean;
  /** 跳到指定页（越界会被夹到有效范围）。 */
  setPage(page: number): void;
  /** 改每页行数（页码回到第 1 页）。 */
  setPageSize(pageSize: number): void;
  /** 改总行数。 */
  setTotal(total: number): void;
  /** 下一页（没有下一页时不动）。 */
  next(): void;
  /** 上一页（没有上一页时不动）。 */
  prev(): void;
  /** 回到第一页。 */
  first(): void;
  /** 跳到最后一页。 */
  last(): void;
}

/** 分页状态的初始值。 */
export interface PaginationOptions {
  /** 初始页码（默认 1）。 */
  readonly page?: number;
  /** 初始每页行数（默认 20）。 */
  readonly pageSize?: number;
  /** 初始总行数（默认 0）。 */
  readonly total?: number;
}

/** 创建分页状态容器。 */
export function createPagination(options: PaginationOptions = {}): PaginationState {
  let pageSize = Math.max(Math.floor(options.pageSize ?? 20) || 20, 1);
  let total = Math.max(Math.floor(options.total ?? 0) || 0, 0);
  let page = Math.floor(options.page ?? 1) || 1;

  const pageCount = (): number => (total === 0 ? 1 : Math.ceil(total / pageSize));
  const clamp = (value: number): number =>
    Math.min(Math.max(Math.floor(value) || 1, 1), pageCount());
  // 初始页码也要夹住（总数还没到齐时给的页码可能越界）。
  page = clamp(page);

  return {
    get page(): number {
      return page;
    },
    get pageSize(): number {
      return pageSize;
    },
    get total(): number {
      return total;
    },
    get pageCount(): number {
      return pageCount();
    },
    get from(): number {
      return total === 0 ? 0 : (page - 1) * pageSize + 1;
    },
    get to(): number {
      return total === 0 ? 0 : Math.min(page * pageSize, total);
    },
    get canPrev(): boolean {
      return page > 1;
    },
    get canNext(): boolean {
      return page < pageCount();
    },
    setPage(next: number): void {
      page = clamp(next);
    },
    setPageSize(next: number): void {
      pageSize = Math.max(Math.floor(next) || 1, 1);
      page = clamp(page);
    },
    setTotal(next: number): void {
      total = Math.max(Math.floor(next) || 0, 0);
      page = clamp(page);
    },
    next(): void {
      if (page < pageCount()) {
        page += 1;
      }
    },
    prev(): void {
      if (page > 1) {
        page -= 1;
      }
    },
    first(): void {
      page = 1;
    },
    last(): void {
      page = pageCount();
    },
  };
}

/** 排序方向。 */
export type SortDirection = 'asc' | 'desc';

/** 表格状态。 */
export interface TableState<Row = unknown> {
  /** 当前数据行。 */
  readonly rows: readonly Row[];
  /** 当前排序列键。 */
  readonly sortKey?: string;
  /** 当前排序方向。 */
  readonly sortDirection: SortDirection;
  /** 已选中的行键。 */
  readonly selectedKeys: readonly string[];
  /** 换数据。 */
  setRows(rows: readonly Row[]): void;
  /** 点同一列 → 换方向；点别的列 → 按该列升序。 */
  toggleSort(key: string): void;
  /** 选中一行。 */
  select(key: string): void;
  /** 取消选中。 */
  deselect(key: string): void;
  /** 清空选中。 */
  clearSelection(): void;
  /** 某行是否被选中。 */
  isSelected(key: string): boolean;
  /** 行键（默认用行索引）。 */
  keyOf(row: Row, index: number): string;
}

/** 表格状态的初始值。 */
export interface TableStateOptions<Row = unknown> {
  /** 初始数据行。 */
  readonly rows?: readonly Row[];
  /** 行键计算方式。 */
  readonly rowKey?: (row: Row, index: number) => string;
}

/** 创建表格状态容器。 */
export function createTableState<Row = unknown>(
  options: TableStateOptions<Row> = {},
): TableState<Row> {
  let rows: readonly Row[] = options.rows ?? [];
  let sortKey: string | undefined;
  let sortDirection: SortDirection = 'asc';
  let selected: string[] = [];

  const keyOf = (row: Row, index: number): string =>
    options.rowKey ? options.rowKey(row, index) : String(index);

  return {
    get rows(): readonly Row[] {
      return rows;
    },
    get sortKey(): string | undefined {
      return sortKey;
    },
    get sortDirection(): SortDirection {
      return sortDirection;
    },
    get selectedKeys(): readonly string[] {
      return selected;
    },
    setRows(next: readonly Row[]): void {
      rows = next;
    },
    toggleSort(key: string): void {
      if (sortKey === key) {
        sortDirection = sortDirection === 'asc' ? 'desc' : 'asc';
        return;
      }
      sortKey = key;
      sortDirection = 'asc';
    },
    select(key: string): void {
      if (!selected.includes(key)) {
        selected = [...selected, key];
      }
    },
    deselect(key: string): void {
      selected = selected.filter((item) => item !== key);
    },
    clearSelection(): void {
      selected = [];
    },
    isSelected(key: string): boolean {
      return selected.includes(key);
    },
    keyOf,
  };
}

/** WebSocket 事件（只声明本包用到的部分）。 */
export interface WebSocketEvents {
  onopen: ((event: unknown) => void) | null;
  onmessage: ((event: { data: unknown }) => void) | null;
  onerror: ((event: unknown) => void) | null;
  onclose: ((event: unknown) => void) | null;
}

/** WebSocket 构造器（可注入，单测用假件）。 */
export type WebSocketFactory = (url: string) => WebSocketEvents;

/** 流连接的状态。 */
export type StreamState = 'idle' | 'connecting' | 'open' | 'closed';

/** 流连接的选项。 */
export interface StreamOptions {
  /** 连接地址。 */
  readonly url: string;
  /** WebSocket 构造器（默认用全局 `WebSocket`）。 */
  readonly factory?: WebSocketFactory;
  /** 是否自动重连（默认 true）。 */
  readonly reconnect?: boolean;
  /** 首次重连的等待时长（毫秒，默认 1000）。 */
  readonly initialDelayMs?: number;
  /** 重连等待的上限（毫秒，默认 30000）。 */
  readonly maxDelayMs?: number;
  /** 退避倍数（默认 2）。 */
  readonly factor?: number;
  /** 连接成功时回调。 */
  readonly onOpen?: () => void;
  /** 收到消息时回调。 */
  readonly onMessage?: (data: unknown) => void;
  /** 出错时回调。 */
  readonly onError?: (event: unknown) => void;
  /** 连接关闭时回调。 */
  readonly onClose?: () => void;
}

/** 流连接。 */
export interface StreamConnection {
  /** 当前状态。 */
  readonly state: StreamState;
  /** 已经发起过的连接次数。 */
  readonly attempts: number;
  /** 建立连接。 */
  connect(): void;
  /** 关闭连接（**关闭后不再重连**）。 */
  close(): void;
}

/** 创建 SSE / WebSocket 连接（带重连退避）。 */
export function createStream(options: StreamOptions): StreamConnection {
  const reconnect = options.reconnect ?? true;
  const initialDelay = options.initialDelayMs ?? 1000;
  const maxDelay = options.maxDelayMs ?? 30_000;
  const factor = options.factor ?? 2;
  const factory: WebSocketFactory =
    options.factory ?? ((url: string) => new WebSocket(url) as unknown as WebSocketEvents);

  let state: StreamState = 'idle';
  let attempts = 0;
  let socket: WebSocketEvents | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const clearTimer = (): void => {
    if (timer !== undefined) {
      clearTimeout(timer);
      timer = undefined;
    }
  };

  const scheduleReconnect = (): void => {
    if (!reconnect || state === 'closed') {
      return;
    }
    const delay = Math.min(initialDelay * factor ** (attempts - 1), maxDelay);
    timer = setTimeout(() => {
      timer = undefined;
      open();
    }, delay);
  };

  const open = (): void => {
    if (state === 'closed') {
      return;
    }
    state = 'connecting';
    attempts += 1;
    const current = factory(options.url);
    socket = current;
    current.onopen = () => {
      state = 'open';
      options.onOpen?.();
    };
    current.onmessage = (event) => {
      options.onMessage?.(event.data);
    };
    current.onerror = (event) => {
      options.onError?.(event);
    };
    current.onclose = () => {
      if (state === 'closed') {
        return;
      }
      state = 'idle';
      options.onClose?.();
      scheduleReconnect();
    };
  };

  return {
    get state(): StreamState {
      return state;
    },
    get attempts(): number {
      return attempts;
    },
    connect(): void {
      if (state === 'closed') {
        return;
      }
      clearTimer();
      open();
    },
    close(): void {
      // 关闭后**不再重连** —— 组件卸载后重连会泄漏。
      state = 'closed';
      clearTimer();
      socket?.onclose?.({});
      socket = undefined;
    },
  };
}

/** 剪贴板写入结果。 */
export interface ClipboardResult {
  /** 是否写入成功。 */
  readonly ok: boolean;
  /** 失败原因（成功时为 `undefined`）。 */
  readonly error?: string;
}

/** 剪贴板（可注入，单测用假件）。 */
export interface ClipboardLike {
  writeText(text: string): Promise<void>;
}

/**
 * 复制文本到剪贴板。
 *
 * 不可用（没有剪贴板 API / 被浏览器拒绝）时**返回失败**，不静默吞掉 ——
 * 否则用户会以为复制成功了。
 */
export async function copyToClipboard(
  text: string,
  clipboard?: ClipboardLike,
): Promise<ClipboardResult> {
  const fallback: ClipboardLike | undefined =
    typeof navigator === 'undefined'
      ? undefined
      : (navigator.clipboard as ClipboardLike | undefined);
  const target: ClipboardLike | undefined = clipboard ?? fallback;
  if (target === undefined) {
    return { ok: false, error: '当前环境不支持剪贴板' };
  }
  try {
    await target.writeText(text);
    return { ok: true };
  } catch {
    return { ok: false, error: '剪贴板写入失败' };
  }
}
