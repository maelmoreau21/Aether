import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { remote, type RemoteCommand } from './remote.svelte';

class FakeSocket {
  static OPEN = 1;
  static sockets: FakeSocket[] = [];
  readyState = 1;
  url: string;
  onopen?: () => void;
  onmessage?: (event: { data: string }) => void;
  onclose?: () => void;
  send = vi.fn();
  close = vi.fn();
  constructor(url: URL) { this.url = String(url); FakeSocket.sockets.push(this); }
  receive(message: unknown) { this.onmessage?.({ data: JSON.stringify(message) }); }
  sent() { return this.send.mock.calls.map(([text]) => JSON.parse(text as string)); }
}

describe('remote control client', () => {
  beforeEach(() => {
    FakeSocket.sockets = [];
    vi.stubGlobal('WebSocket', FakeSocket);
    vi.stubGlobal('window', { location: { href: 'http://aether.test/app/' } });
    vi.useFakeTimers();
  });
  afterEach(() => { remote.stop(); vi.useRealTimers(); vi.unstubAllGlobals(); });

  it('says hello as a web device, lists devices and relays valid commands only', () => {
    remote.start();
    const socket = FakeSocket.sockets[0];
    expect(socket.url).toMatch(/^ws:\/\/.+\/api\/v1\/remote\/ws$/);
    socket.onopen?.();
    const [hello, state] = socket.sent();
    expect(hello).toMatchObject({ type: 'hello', kind: 'web' });
    expect(hello.deviceId).toMatch(/^[\w-]{8,64}$/);
    expect(state).toEqual({ type: 'state', playing: null });

    socket.receive({ type: 'devices', devices: [{ deviceId: 'tv-1', name: 'Salon', kind: 'tv', playing: null }] });
    expect(remote.devices.map(d => d.name)).toEqual(['Salon']);

    const received: RemoteCommand[] = [];
    const unsubscribe = remote.onCommand(command => received.push(command));
    socket.receive({ type: 'command', fromName: 'Téléphone', action: 'seek', position: 42 });
    socket.receive({ type: 'command', fromName: 'x', action: 'format-disk' });
    socket.receive({ type: 'command', fromName: 'x', action: 'seek', position: -5 });
    unsubscribe();
    expect(received).toEqual([
      { fromName: 'Téléphone', action: 'seek', position: 42, workId: undefined },
      { fromName: 'x', action: 'seek', position: undefined, workId: undefined }
    ]);
  });

  it('throttles playback reports and sends commands to a device', () => {
    remote.start();
    const socket = FakeSocket.sockets[0];
    socket.onopen?.();
    socket.send.mockClear();
    const playing = { workId: 'w', title: 'Film', position: 10, duration: 100, paused: false };
    remote.report(playing);
    remote.report({ ...playing, position: 11 });
    expect(socket.send).toHaveBeenCalledTimes(1);
    remote.report(null, true);
    remote.send('tv-1', 'open', -3, 'w');
    expect(socket.sent().slice(1)).toEqual([
      { type: 'state', playing: null },
      { type: 'command', to: 'tv-1', action: 'open', position: 0, workId: 'w' }
    ]);
  });

  it('reconnects after the connection drops', () => {
    remote.start();
    FakeSocket.sockets[0].onclose?.();
    expect(remote.connected).toBe(false);
    vi.advanceTimersByTime(2000);
    expect(FakeSocket.sockets).toHaveLength(2);
  });
});
