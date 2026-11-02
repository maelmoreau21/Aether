import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { SyncPartyClient } from './sync-client.svelte';
import { applyPartyPlayback } from './player-sync';
import type { PartyStatePayload, ServerMessage } from './types';
import type { Player } from '../player/types';

class FakeSocket {
  static OPEN = 1;
  static sockets: FakeSocket[] = [];
  readyState = 1;
  onopen?: () => void;
  onmessage?: (event: { data: string }) => void;
  onclose?: (() => void) | null;
  onerror?: () => void;
  send = vi.fn();
  close = vi.fn();
  constructor() { FakeSocket.sockets.push(this); }
  receive(msg: ServerMessage) { this.onmessage?.({ data: JSON.stringify(msg) }); }
}

const party: PartyStatePayload = {
  id: 'p', name: 'Room', hostUserId: 'host', hostName: 'Host', mediaId: 'm',
  mediaTitle: 'Movie', controlMode: 'host_only', state: 'paused', position: 20,
  positionUpdatedServerMs: 100_000, isLocked: false, waitForBuffering: true, createdAt: 100
};

describe('SyncPlay browser protocol and playback', () => {
  const clients: SyncPartyClient[] = [];
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(100_000);
    FakeSocket.sockets = [];
    vi.stubGlobal('window', { location: { protocol: 'http:', host: 'localhost' } });
    vi.stubGlobal('WebSocket', FakeSocket);
  });
  afterEach(() => {
    for (const client of clients) client.destroy();
    clients.length = 0;
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });
  function connect() {
    const client = new SyncPartyClient('p', 'follower');
    clients.push(client);
    const socket = FakeSocket.sockets.at(-1)!;
    socket.onopen?.();
    const update = vi.fn();
    client.onPlaybackUpdate = update;
    return { client, socket, update };
  }
  function snapshot(socket: FakeSocket, overrides: Partial<PartyStatePayload> = {}) {
    socket.receive({ type: 'syncState', party: { ...party, ...overrides }, members: [],
      messages: [], queue: [], poll: null, serverEpochMs: 100_000, seq: 1 });
  }

  it('applies the snapshot on join and reconnect without requiring control', () => {
    const { client, socket, update } = connect();
    snapshot(socket, { state: 'playing', position: 10, positionUpdatedServerMs: 90_000 });
    expect(client.canControl).toBe(false);
    expect(update).toHaveBeenLastCalledWith('playing', 20, '');
    socket.onclose?.();
    vi.advanceTimersByTime(1000);
    const reconnected = FakeSocket.sockets.at(-1)!;
    reconnected.onopen?.();
    snapshot(reconnected, { position: 42 });
    expect(update).toHaveBeenLastCalledWith('paused', 42, '');
  });

  it('starts at the server countdown deadline and cancels a superseded start', () => {
    const { socket, update } = connect();
    snapshot(socket);
    socket.receive({ type: 'countdown', seconds: 3, targetServerEpochMs: 103_000,
      position: 25, causedByUserId: 'host', causedByName: 'Host', seq: 2 });
    expect(update).toHaveBeenLastCalledWith('paused', 25, 'Host');
    vi.advanceTimersByTime(2999);
    expect(update).toHaveBeenLastCalledWith('paused', 25, 'Host');
    vi.advanceTimersByTime(1);
    expect(update).toHaveBeenLastCalledWith('playing', 25, 'Host');
    socket.receive({ type: 'countdown', seconds: 3, targetServerEpochMs: 106_000,
      position: 30, causedByUserId: 'host', causedByName: 'Host', seq: 3 });
    socket.receive({ type: 'playbackUpdate', state: 'paused', position: 31,
      serverEpochMs: 103_000, causedByUserId: 'host', causedByName: 'Host', seq: 4 });
    vi.advanceTimersByTime(4000);
    expect(update).toHaveBeenLastCalledWith('paused', 31, 'Host');
  });

  it('uses clock offset and sends browser camelCase commands', () => {
    const { client, socket } = connect();
    vi.advanceTimersByTime(100);
    socket.receive({ type: 'pong', clientEpochMs: 100_000, serverEpochMs: 105_050 });
    expect(client.clockOffsetMs).toBe(5000);
    expect(client.rttMs).toBe(100);
    client.grantControl('u');
    expect(socket.send).toHaveBeenLastCalledWith('{"type":"grantControl","userId":"u"}');
  });

  it('applies an authoritative seek and pause directly to the player', async () => {
    const state = { position: 0, duration: 100, paused: true, volume: 1, speed: 1 };
    const player = {
      state: () => state,
      seek: vi.fn((position: number) => { state.position = position; }),
      pause: vi.fn(() => { state.paused = true; }),
      play: vi.fn(async () => { state.paused = false; }),
      setSpeed: vi.fn()
    } as unknown as Player;
    await applyPartyPlayback(player, 'playing', 50);
    expect(player.seek).toHaveBeenCalledWith(50);
    expect(player.play).toHaveBeenCalledOnce();
    await applyPartyPlayback(player, 'paused', 55);
    expect(player.pause).toHaveBeenCalledOnce();
    expect(state.position).toBe(55);
    expect(state.paused).toBe(true);
  });
});
