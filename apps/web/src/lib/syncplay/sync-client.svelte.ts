import type {
  ClientMessage,
  ControlMode,
  MemberPayload,
  ChatMessagePayload,
  PartyStatePayload,
  PollPayload,
  QueueItemPayload,
  ServerMessage
} from './types';
import {
  calculateClockOffset,
  calculateTargetPosition,
  evaluateDriftCorrection,
  type DriftCorrection,
  type DriftOptions
} from './sync-math';

export class SyncPartyClient {
  public party = $state<PartyStatePayload | null>(null);
  public members = $state<MemberPayload[]>([]);
  public messages = $state<ChatMessagePayload[]>([]);
  public queue = $state<QueueItemPayload[]>([]);
  public poll = $state<PollPayload | null>(null);
  public connected = $state(false);
  public clockOffsetMs = $state(0);
  public rttMs = $state(0);
  public countdownSeconds = $state<number | null>(null);
  public lastNotification = $state<{ text: string; id: number } | null>(null);
  public recentReactions = $state<{ id: string; emoji: string; name: string }[]>([]);
  public errorMessage = $state<string | null>(null);

  private ws: WebSocket | null = null;
  private partyId: string;
  private currentUserId: string;
  private shouldReconnect = true;
  private reconnectAttempts = 0;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private pingInterval: ReturnType<typeof setInterval> | null = null;
  private lastPingSent = 0;
  private latestSeq = 0;
  private notificationCounter = 0;

  // External hooks for player coordination
  public onPlaybackUpdate?: (state: string, position: number, causedByName: string) => void;
  public onCountdown?: (seconds: number, targetPosition: number) => void;
  public onMediaChanged?: (mediaId: string, mediaTitle: string) => void;
  public onKicked?: () => void;
  public onPartyEnded?: (reason: string) => void;

  constructor(partyId: string, currentUserId: string) {
    this.partyId = partyId;
    this.currentUserId = currentUserId;
    this.connect();
  }

  private notify(text: string) {
    this.notificationCounter += 1;
    this.lastNotification = { text, id: this.notificationCounter };
  }

  public connect() {
    if (typeof window === 'undefined') return;
    this.shouldReconnect = true;

    try {
      const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
      const url = `${protocol}//${window.location.host}/api/v1/parties/${encodeURIComponent(this.partyId)}/ws`;
      this.ws = new WebSocket(url);

      this.ws.onopen = () => {
        this.connected = true;
        this.reconnectAttempts = 0;
        this.errorMessage = null;
        this.startPingLoop();
      };

      this.ws.onmessage = (event) => {
        try {
          const msg = JSON.parse(event.data) as ServerMessage;
          this.handleServerMessage(msg);
        } catch {}
      };

      this.ws.onclose = () => {
        this.connected = false;
        this.stopPingLoop();
        if (this.shouldReconnect) {
          const delay = Math.min(1000 * 2 ** this.reconnectAttempts, 10000);
          this.reconnectAttempts += 1;
          this.reconnectTimer = setTimeout(() => this.connect(), delay);
        }
      };

      this.ws.onerror = () => {
        this.connected = false;
      };
    } catch (e) {
      this.errorMessage = e instanceof Error ? e.message : 'Erreur WebSocket';
    }
  }

  private startPingLoop() {
    this.stopPingLoop();
    this.sendPing();
    this.pingInterval = setInterval(() => this.sendPing(), 15000);
  }

  private stopPingLoop() {
    if (this.pingInterval) {
      clearInterval(this.pingInterval);
      this.pingInterval = null;
    }
  }

  private sendPing() {
    this.lastPingSent = Date.now();
    this.send({ type: 'ping', clientEpochMs: this.lastPingSent });
  }

  private send(msg: ClientMessage) {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      this.ws.send(JSON.stringify(msg));
    }
  }

  private handleServerMessage(msg: ServerMessage) {
    switch (msg.type) {
      case 'pong': {
        const clientRecv = Date.now();
        const { offsetMs, roundTripMs } = calculateClockOffset(
          msg.clientEpochMs,
          msg.serverEpochMs,
          clientRecv
        );
        this.clockOffsetMs = offsetMs;
        this.rttMs = roundTripMs;
        break;
      }

      case 'syncState': {
        this.party = msg.party;
        this.members = msg.members;
        this.messages = msg.messages;
        this.queue = msg.queue;
        this.poll = msg.poll;
        this.latestSeq = msg.seq;
        break;
      }

      case 'playbackUpdate': {
        if (msg.seq < this.latestSeq) return; // Drop out-of-order events
        this.latestSeq = msg.seq;

        if (this.party) {
          this.party.state = msg.state;
          this.party.position = msg.position;
          this.party.positionUpdatedServerMs = msg.serverEpochMs;
        }

        const actionFr = msg.state === 'playing' ? 'a repris la lecture' : 'a mis en pause';
        this.notify(`${msg.causedByName} ${actionFr}`);
        this.onPlaybackUpdate?.(msg.state, msg.position, msg.causedByName);
        break;
      }

      case 'countdown': {
        if (msg.seq < this.latestSeq) return;
        this.latestSeq = msg.seq;

        this.countdownSeconds = msg.seconds;
        this.notify(`Lancement de la lecture dans ${msg.seconds}s...`);
        this.onCountdown?.(msg.seconds, msg.position);

        const interval = setInterval(() => {
          if (this.countdownSeconds !== null && this.countdownSeconds > 1) {
            this.countdownSeconds -= 1;
          } else {
            this.countdownSeconds = null;
            clearInterval(interval);
          }
        }, 1000);
        break;
      }

      case 'memberJoined': {
        const existingIdx = this.members.findIndex((m) => m.userId === msg.member.userId);
        if (existingIdx >= 0) {
          this.members[existingIdx] = msg.member;
        } else {
          this.members.push(msg.member);
        }
        this.notify(`${msg.member.displayName} a rejoint la séance`);
        break;
      }

      case 'memberLeft': {
        const member = this.members.find((m) => m.userId === msg.userId);
        if (member) {
          member.isOnline = false;
        }
        this.notify(`${msg.displayName} a quitté la séance`);
        break;
      }

      case 'memberStatus': {
        const member = this.members.find((m) => m.userId === msg.userId);
        if (member) {
          member.isReady = msg.isReady;
          member.isBuffering = msg.isBuffering;
        }
        break;
      }

      case 'hostTransferred': {
        if (this.party) {
          this.party.hostUserId = msg.newHostId;
          this.party.hostName = msg.newHostName;
        }
        for (const m of this.members) {
          m.role = m.userId === msg.newHostId ? 'host' : 'participant';
        }
        this.notify(`${msg.newHostName} est désormais l'organisateur`);
        break;
      }

      case 'controlGranted': {
        const m = this.members.find((mem) => mem.userId === msg.userId);
        if (m) m.hasControl = true;
        if (msg.userId === this.currentUserId) {
          this.notify("Vous avez désormais le contrôle de la lecture");
        }
        break;
      }

      case 'controlRevoked': {
        const m = this.members.find((mem) => mem.userId === msg.userId);
        if (m) m.hasControl = false;
        if (msg.userId === this.currentUserId) {
          this.notify("Votre contrôle de lecture a été retiré");
        }
        break;
      }

      case 'controlRequested': {
        if (this.isHost) {
          this.notify(`${msg.displayName} demande le contrôle de la lecture`);
        }
        break;
      }

      case 'kicked': {
        if (msg.userId === this.currentUserId) {
          this.shouldReconnect = false;
          this.destroy();
          this.onKicked?.();
        } else {
          this.members = this.members.filter((m) => m.userId !== msg.userId);
        }
        break;
      }

      case 'newMessage': {
        this.messages.push(msg.message);
        if (this.messages.length > 100) this.messages.shift();
        break;
      }

      case 'newReaction': {
        const id = `${Date.now()}-${Math.random()}`;
        this.recentReactions.push({ id, emoji: msg.emoji, name: msg.displayName });
        setTimeout(() => {
          this.recentReactions = this.recentReactions.filter((r) => r.id !== id);
        }, 3500);
        break;
      }

      case 'queueUpdated': {
        this.queue = msg.queue;
        break;
      }

      case 'pollUpdated': {
        this.poll = msg.poll;
        break;
      }

      case 'mediaChanged': {
        if (this.party) {
          this.party.mediaId = msg.mediaId;
          this.party.mediaTitle = msg.mediaTitle;
          this.party.state = 'waiting';
          this.party.position = 0;
        }
        this.notify(`Nouveau média lancé par ${msg.causedByName} : ${msg.mediaTitle}`);
        this.onMediaChanged?.(msg.mediaId, msg.mediaTitle);
        break;
      }

      case 'partyEnded': {
        this.shouldReconnect = false;
        if (this.party) this.party.state = 'ended';
        this.notify(msg.reason);
        this.onPartyEnded?.(msg.reason);
        this.destroy();
        break;
      }

      case 'error': {
        this.errorMessage = msg.message;
        break;
      }
    }
  }

  // --- Public Action Methods ---

  public setReady(ready: boolean) {
    this.send({ type: 'setReady', ready });
  }

  public setBuffering(buffering: boolean, position: number) {
    this.send({ type: 'setBuffering', buffering, position });
  }

  public play(position: number, countdown?: number) {
    this.send({ type: 'play', position, countdown });
  }

  public pause(position: number) {
    this.send({ type: 'pause', position });
  }

  public seek(position: number) {
    this.send({ type: 'seek', position });
  }

  public reportPosition(position: number) {
    this.send({ type: 'reportPosition', position });
  }

  public sendChat(content: string) {
    this.send({ type: 'chatMessage', content });
  }

  public sendReaction(emoji: string) {
    this.send({ type: 'reaction', emoji });
  }

  public requestControl() {
    this.send({ type: 'requestControl' });
  }

  public grantControl(userId: string) {
    this.send({ type: 'grantControl', userId });
  }

  public revokeControl(userId: string) {
    this.send({ type: 'revokeControl', userId });
  }

  public changeMedia(mediaId: string) {
    this.send({ type: 'changeMedia', mediaId });
  }

  public addToQueue(mediaId: string) {
    this.send({ type: 'addToQueue', mediaId });
  }

  public removeFromQueue(queueId: string) {
    this.send({ type: 'removeFromQueue', queueId });
  }

  public createPoll(question: string, options: string[]) {
    this.send({ type: 'createPoll', question, options });
  }

  public votePoll(pollId: string, optionIndex: number) {
    this.send({ type: 'votePoll', pollId, optionIndex });
  }

  public closePoll(pollId: string) {
    this.send({ type: 'closePoll', pollId });
  }

  public kickMember(userId: string) {
    this.send({ type: 'kickMember', userId });
  }

  public transferHost(newHostId: string) {
    this.send({ type: 'transferHost', newHostId });
  }

  public updateSettings(settings: {
    name?: string;
    controlMode?: ControlMode;
    isLocked?: boolean;
    waitForBuffering?: boolean;
  }) {
    this.send({ type: 'setSettings', ...settings });
  }

  public leave() {
    this.send({ type: 'leaveParty' });
    this.shouldReconnect = false;
    this.destroy();
  }

  // --- Calculations & Helper Getters ---

  public get isHost(): boolean {
    return this.party?.hostUserId === this.currentUserId;
  }

  public get canControl(): boolean {
    if (this.isHost) return true;
    if (this.party?.controlMode === 'shared') return true;
    if (this.party?.controlMode === 'request') {
      const self = this.members.find((m) => m.userId === this.currentUserId);
      return self?.hasControl ?? false;
    }
    return false;
  }

  public getCurrentServerEpochMs(): number {
    return Date.now() + this.clockOffsetMs;
  }

  public getComputedPartyPosition(): number {
    if (!this.party) return 0;
    return calculateTargetPosition(
      this.party.state,
      this.party.position,
      this.party.positionUpdatedServerMs,
      this.getCurrentServerEpochMs()
    );
  }

  public evaluateDrift(
    currentVideoPosition: number,
    options?: DriftOptions
  ): DriftCorrection {
    const target = this.getComputedPartyPosition();
    return evaluateDriftCorrection(currentVideoPosition, target, options);
  }

  public destroy() {
    this.shouldReconnect = false;
    this.stopPingLoop();
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.ws) {
      this.ws.onclose = null;
      this.ws.close();
      this.ws = null;
    }
    this.connected = false;
  }
}
