import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  copyToClipboard,
  createPagination,
  createStream,
  createTableState,
  createToggle,
  type WebSocketEvents,
  type WebSocketFactory,
} from '../src/index';

/** 假 WebSocket：测试手动触发事件。 */
class FakeSocket implements WebSocketEvents {
  onopen: ((event: unknown) => void) | null = null;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  onclose: ((event: unknown) => void) | null = null;

  constructor(readonly url: string) {}

  emitOpen(): void {
    this.onopen?.({});
  }

  emitMessage(data: unknown): void {
    this.onmessage?.({ data });
  }

  emitClose(): void {
    this.onclose?.({});
  }
}

describe('@base/hooks 的开关状态', () => {
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

describe('@base/hooks 的分页状态', () => {
  it('初始值与页数算得对', () => {
    const pagination = createPagination({ page: 2, pageSize: 10, total: 25 });
    expect(pagination.page).toBe(2);
    expect(pagination.pageCount).toBe(3);
    expect(pagination.from).toBe(11);
    expect(pagination.to).toBe(20);
    expect(pagination.canPrev).toBe(true);
    expect(pagination.canNext).toBe(true);
  });

  it('翻页在边界处不动', () => {
    const pagination = createPagination({ page: 1, pageSize: 10, total: 25 });
    pagination.prev();
    expect(pagination.page).toBe(1);
    pagination.last();
    expect(pagination.page).toBe(3);
    expect(pagination.canNext).toBe(false);
    pagination.next();
    expect(pagination.page).toBe(3);
    pagination.first();
    expect(pagination.page).toBe(1);
  });

  it('越界页码被夹住', () => {
    const pagination = createPagination({ page: 99, pageSize: 10, total: 25 });
    expect(pagination.page).toBe(3);
    pagination.setPage(0);
    expect(pagination.page).toBe(1);
  });

  it('改总数后页码被夹住', () => {
    const pagination = createPagination({ page: 3, pageSize: 10, total: 25 });
    pagination.setTotal(5);
    expect(pagination.page).toBe(1);
    expect(pagination.pageCount).toBe(1);
  });
});

describe('@base/hooks 的表格状态', () => {
  it('排序：同列换方向，换列重置为升序', () => {
    const table = createTableState<{ id: string }>({ rows: [{ id: 'a' }] });
    table.toggleSort('id');
    expect(table.sortKey).toBe('id');
    expect(table.sortDirection).toBe('asc');
    table.toggleSort('id');
    expect(table.sortDirection).toBe('desc');
    table.toggleSort('name');
    expect(table.sortKey).toBe('name');
    expect(table.sortDirection).toBe('asc');
  });

  it('选中与取消选中', () => {
    const table = createTableState();
    table.select('a');
    expect(table.isSelected('a')).toBe(true);
    table.select('a');
    expect(table.selectedKeys).toEqual(['a']);
    table.deselect('a');
    expect(table.isSelected('a')).toBe(false);
    table.select('b');
    table.clearSelection();
    expect(table.selectedKeys).toEqual([]);
  });

  it('行键默认用行索引，也可自定义', () => {
    const table = createTableState<{ id: string }>({ rowKey: (row) => row.id });
    expect(table.keyOf({ id: 'r-1' }, 0)).toBe('r-1');
    expect(createTableState().keyOf({}, 2)).toBe('2');
  });
});

describe('@base/hooks 的流连接', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('连接成功进入 open 状态', () => {
    const sockets: FakeSocket[] = [];
    const factory: WebSocketFactory = (url) => {
      const socket = new FakeSocket(url);
      sockets.push(socket);
      return socket;
    };
    const stream = createStream({ url: 'ws://example.test/stream', factory });

    stream.connect();
    expect(stream.state).toBe('connecting');
    expect(stream.attempts).toBe(1);
    sockets[0]?.emitOpen();
    expect(stream.state).toBe('open');
  });

  it('断开后按退避重连', () => {
    vi.useFakeTimers();
    const sockets: FakeSocket[] = [];
    const factory: WebSocketFactory = (url) => {
      const socket = new FakeSocket(url);
      sockets.push(socket);
      return socket;
    };
    const stream = createStream({
      url: 'ws://example.test/stream',
      factory,
      initialDelayMs: 100,
      factor: 2,
    });

    stream.connect();
    sockets[0]?.emitOpen();
    sockets[0]?.emitClose();

    expect(stream.state).toBe('idle');
    vi.advanceTimersByTime(99);
    expect(stream.attempts).toBe(1);
    vi.advanceTimersByTime(1);
    expect(stream.attempts).toBe(2);

    // 第二次退避更长（100 → 200）。
    sockets[1]?.emitClose();
    vi.advanceTimersByTime(199);
    expect(stream.attempts).toBe(2);
    vi.advanceTimersByTime(1);
    expect(stream.attempts).toBe(3);
  });

  it('close 之后不再重连', () => {
    vi.useFakeTimers();
    const sockets: FakeSocket[] = [];
    const factory: WebSocketFactory = (url) => {
      const socket = new FakeSocket(url);
      sockets.push(socket);
      return socket;
    };
    const stream = createStream({
      url: 'ws://example.test/stream',
      factory,
      initialDelayMs: 100,
    });

    stream.connect();
    sockets[0]?.emitOpen();
    stream.close();
    expect(stream.state).toBe('closed');

    vi.advanceTimersByTime(10_000);
    expect(stream.attempts).toBe(1);
    expect(sockets).toHaveLength(1);
  });

  it('消息透传给回调', () => {
    const messages: unknown[] = [];
    const sockets: FakeSocket[] = [];
    const factory: WebSocketFactory = (url) => {
      const socket = new FakeSocket(url);
      sockets.push(socket);
      return socket;
    };
    const stream = createStream({
      url: 'ws://example.test/stream',
      factory,
      onMessage: (data) => messages.push(data),
    });

    stream.connect();
    sockets[0]?.emitMessage({ hello: 'world' });

    expect(messages).toEqual([{ hello: 'world' }]);
  });
});

describe('@base/hooks 的剪贴板', () => {
  it('写入成功返回 ok', async () => {
    const written: string[] = [];
    const result = await copyToClipboard('内容', {
      async writeText(text: string) {
        written.push(text);
      },
    });
    expect(result).toEqual({ ok: true });
    expect(written).toEqual(['内容']);
  });

  it('写入失败返回失败（不静默吞掉）', async () => {
    const result = await copyToClipboard('内容', {
      async writeText() {
        throw new Error('denied');
      },
    });
    expect(result.ok).toBe(false);
    expect(result.error).toBe('剪贴板写入失败');
  });

  it('没有剪贴板时返回失败', async () => {
    const result = await copyToClipboard('内容');
    expect(result.ok).toBe(false);
    expect(result.error).toBe('当前环境不支持剪贴板');
  });
});
