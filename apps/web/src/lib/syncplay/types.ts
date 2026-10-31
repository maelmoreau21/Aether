export type ControlMode = 'host_only' | 'shared' | 'request';
export type PartyState = 'waiting' | 'playing' | 'paused' | 'ended';

export interface PartySummary {
  id: string;
  name: string;
  hostUserId: string;
  hostName: string;
  mediaId: string;
  mediaTitle: string;
  controlMode: ControlMode;
  state: PartyState;
  currentPosition: number;
  isLocked: boolean;
  waitForBuffering: boolean;
  membersCount: number;
  createdAt: number;
  isHost: boolean;
  isMember: boolean;
}

export interface MemberPayload {
  userId: string;
  displayName: string;
  role: 'host' | 'participant';
  isReady: boolean;
  isBuffering: boolean;
  isOnline: boolean;
  joinedAt: number;
  hasControl: boolean;
}

export interface ChatMessagePayload {
  id: string;
  userId: string;
  displayName: string;
  content: string;
  kind: 'chat' | 'system' | 'reaction';
  createdAt: number;
}

export interface QueueItemPayload {
  id: string;
  mediaId: string;
  mediaTitle: string;
  addedByUserId: string;
  addedByName: string;
  position: number;
  createdAt: number;
}

export interface PollOptionPayload {
  text: string;
  votes: number;
  voterIds: string[];
}

export interface PollPayload {
  id: string;
  question: string;
  options: PollOptionPayload[];
  createdBy: string;
  isClosed: boolean;
  createdAt: number;
  totalVotes: number;
}

export interface PartyStatePayload {
  id: string;
  name: string;
  hostUserId: string;
  hostName: string;
  mediaId: string;
  mediaTitle: string;
  controlMode: ControlMode;
  state: PartyState;
  position: number;
  positionUpdatedServerMs: number;
  isLocked: boolean;
  waitForBuffering: boolean;
  createdAt: number;
}

export interface UserInvitePayload {
  token: string;
  partyId: string;
  partyName: string;
  hostName: string;
  mediaId: string;
  mediaTitle: string;
  createdAt: number;
  expiresAt: number;
}

export type ClientMessage =
  | { type: 'ping'; clientEpochMs: number }
  | { type: 'setReady'; ready: boolean }
  | { type: 'setBuffering'; buffering: boolean; position: number }
  | { type: 'play'; position: number; countdown?: number }
  | { type: 'pause'; position: number }
  | { type: 'seek'; position: number }
  | { type: 'reportPosition'; position: number }
  | { type: 'chatMessage'; content: string }
  | { type: 'reaction'; emoji: string }
  | { type: 'requestControl' }
  | { type: 'grantControl'; userId: string }
  | { type: 'revokeControl'; userId: string }
  | { type: 'changeMedia'; mediaId: string }
  | { type: 'addToQueue'; mediaId: string }
  | { type: 'removeFromQueue'; queueId: string }
  | { type: 'createPoll'; question: string; options: string[] }
  | { type: 'votePoll'; pollId: string; optionIndex: number }
  | { type: 'closePoll'; pollId: string }
  | { type: 'kickMember'; userId: string }
  | { type: 'transferHost'; newHostId: string }
  | {
      type: 'setSettings';
      name?: string;
      controlMode?: ControlMode;
      isLocked?: boolean;
      waitForBuffering?: boolean;
    }
  | { type: 'leaveParty' };

export type ServerMessage =
  | {
      type: 'pong';
      clientEpochMs: number;
      serverEpochMs: number;
    }
  | {
      type: 'syncState';
      party: PartyStatePayload;
      members: MemberPayload[];
      messages: ChatMessagePayload[];
      queue: QueueItemPayload[];
      poll: PollPayload | null;
      serverEpochMs: number;
      seq: number;
    }
  | {
      type: 'playbackUpdate';
      state: PartyState;
      position: number;
      serverEpochMs: number;
      causedByUserId: string;
      causedByName: string;
      seq: number;
    }
  | {
      type: 'countdown';
      seconds: number;
      targetServerEpochMs: number;
      position: number;
      causedByUserId: string;
      causedByName: string;
      seq: number;
    }
  | {
      type: 'memberJoined';
      member: MemberPayload;
    }
  | {
      type: 'memberLeft';
      userId: string;
      displayName: string;
      reason?: string;
    }
  | {
      type: 'memberStatus';
      userId: string;
      isReady: boolean;
      isBuffering: boolean;
      position: number;
    }
  | {
      type: 'hostTransferred';
      newHostId: string;
      newHostName: string;
    }
  | {
      type: 'controlGranted';
      userId: string;
    }
  | {
      type: 'controlRevoked';
      userId: string;
    }
  | {
      type: 'controlRequested';
      userId: string;
      displayName: string;
    }
  | {
      type: 'kicked';
      userId: string;
    }
  | {
      type: 'newMessage';
      message: ChatMessagePayload;
    }
  | {
      type: 'newReaction';
      userId: string;
      displayName: string;
      emoji: string;
    }
  | {
      type: 'queueUpdated';
      queue: QueueItemPayload[];
    }
  | {
      type: 'pollUpdated';
      poll: PollPayload | null;
    }
  | {
      type: 'mediaChanged';
      mediaId: string;
      mediaTitle: string;
      causedByName: string;
    }
  | {
      type: 'partyEnded';
      reason: string;
    }
  | {
      type: 'error';
      code: string;
      message: string;
    };
