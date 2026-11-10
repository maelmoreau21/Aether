/**
 * This browser on the account's remote-control channel (`/api/v1/remote/ws`,
 * session cookie): lists the account's other devices and what they play,
 * reports what plays here, and receives play, pause, seek, stop and open.
 * Same protocol as the Android app; reconnects on its own.
 */
export type Playing = { workId: string; title: string | null; position: number; duration: number | null; paused: boolean };
export type RemoteDevice = { deviceId: string; name: string; kind: string; playing: Playing | null };
export type RemoteAction = 'play' | 'pause' | 'seek' | 'stop' | 'open';
export type RemoteCommand = { fromName: string; action: RemoteAction; position?: number; workId?: string };

import { tvActive } from '../tv/keys';

const DEVICE_KEY = 'aether.remote.device';
const ACTIONS: readonly string[] = ['play', 'pause', 'seek', 'stop', 'open'];

function deviceId(): string {
  try {
    const saved = localStorage.getItem(DEVICE_KEY);
    if (saved && /^[\w-]{8,64}$/.test(saved)) return saved;
    const created = crypto.randomUUID();
    localStorage.setItem(DEVICE_KEY, created);
    return created;
  } catch { return crypto.randomUUID(); }
}

function browserName(): string {
  const agent = navigator.userAgent;
  const browser = /Edg\//.test(agent) ? 'Edge' : /Firefox\//.test(agent) ? 'Firefox' : /Chrome\//.test(agent) ? 'Chrome' : /Safari\//.test(agent) ? 'Safari' : 'Navigateur';
  const system = /Windows/.test(agent) ? 'Windows' : /Mac OS/.test(agent) ? 'Mac' : /Android/.test(agent) ? 'Android' : /Linux/.test(agent) ? 'Linux' : '';
  return system ? `${browser} · ${system}` : browser;
}

/** A TV (Tizen, webOS, or the TV layout forced on) announces itself as one, like the Android TV app. */
export function deviceIdentity(): { name: string; kind: 'tv' | 'web' } {
  if (!tvActive()) return { name: browserName(), kind: 'web' };
  const agent = navigator.userAgent;
  const name = /Tizen/i.test(agent) ? 'Samsung TV · Tizen' : /Web0S|webOS/i.test(agent) ? 'LG TV · webOS' : `Téléviseur · ${browserName()}`;
  return { name, kind: 'tv' };
}

function parseCommand(raw: Record<string, unknown>): RemoteCommand | null {
  if (typeof raw.action !== 'string' || !ACTIONS.includes(raw.action)) return null;
  const position = typeof raw.position === 'number' && Number.isFinite(raw.position) && raw.position >= 0 ? raw.position : undefined;
  const workId = typeof raw.workId === 'string' && raw.workId ? raw.workId : undefined;
  return { fromName: typeof raw.fromName === 'string' ? raw.fromName : '', action: raw.action as RemoteAction, position, workId };
}

class RemoteControl {
  devices = $state<RemoteDevice[]>([]);
  connected = $state(false);
  /** A title to play here, consumed by the mounted catalogue. */
  pendingOpen = $state<{ workId: string; position?: number } | null>(null);
  /** Mounted catalogues able to start playback. */
  catalogs = 0;
  #socket: WebSocket | null = null;
  #state = JSON.stringify({ type: 'state', playing: null });
  #handlers = new Set<(command: RemoteCommand) => void>();
  #retry: ReturnType<typeof setTimeout> | undefined;
  #backoff = 2000;
  #wanted = false;
  #lastReport = 0;

  start() {
    if (this.#wanted || typeof WebSocket === 'undefined') return;
    this.#wanted = true;
    this.#open();
  }

  stop() {
    this.#wanted = false;
    clearTimeout(this.#retry);
    this.#socket?.close(1000);
    this.#socket = null;
    this.devices = [];
    this.connected = false;
  }

  #open() {
    if (!this.#wanted) return;
    const url = new URL('/api/v1/remote/ws', window.location.href);
    url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
    const socket = new WebSocket(url);
    this.#socket = socket;
    socket.onopen = () => {
      this.#backoff = 2000;
      this.connected = true;
      socket.send(JSON.stringify({ type: 'hello', deviceId: deviceId(), ...deviceIdentity() }));
      socket.send(this.#state);
    };
    socket.onmessage = event => {
      let message: Record<string, unknown>;
      try { message = JSON.parse(String(event.data)); } catch { return; }
      if (message.type === 'devices' && Array.isArray(message.devices)) {
        this.devices = (message.devices as RemoteDevice[]).filter(d => d && typeof d.deviceId === 'string');
      } else if (message.type === 'command') {
        const command = parseCommand(message);
        if (command) for (const handler of this.#handlers) handler(command);
      }
    };
    socket.onclose = () => {
      if (this.#socket !== socket) return;
      this.connected = false;
      this.devices = [];
      if (!this.#wanted) return;
      clearTimeout(this.#retry);
      this.#retry = setTimeout(() => this.#open(), this.#backoff);
      this.#backoff = Math.min(this.#backoff * 2, 30000);
    };
  }

  /** What plays here (`null` when nothing). `force` skips the 5 s throttle. */
  report(playing: Playing | null, force = false) {
    const now = Date.now();
    if (!force && now - this.#lastReport < 5000) return;
    this.#lastReport = now;
    this.#state = JSON.stringify({ type: 'state', playing });
    if (this.#socket?.readyState === WebSocket.OPEN) this.#socket.send(this.#state);
  }

  send(to: string, action: RemoteAction, position?: number, workId?: string) {
    const message: Record<string, unknown> = { type: 'command', to, action };
    if (position !== undefined && Number.isFinite(position)) message.position = Math.max(0, position);
    if (workId) message.workId = workId;
    if (this.#socket?.readyState === WebSocket.OPEN) this.#socket.send(JSON.stringify(message));
  }

  /** Returns the unsubscribe function. */
  onCommand(handler: (command: RemoteCommand) => void): () => void {
    this.#handlers.add(handler);
    return () => this.#handlers.delete(handler);
  }
}

export const remote = new RemoteControl();
