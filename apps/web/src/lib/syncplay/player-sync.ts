import type { Player } from '../player/types';

/** Apply an authoritative server command without emitting a user command. */
export async function applyPartyPlayback(
  player: Player, state: string, position: number
): Promise<void> {
  player.setSpeed(1);
  if (state !== 'playing') player.pause();
  if (Number.isFinite(position) && Math.abs(player.state().position - position) > 0.25) {
    player.seek(Math.max(0, position));
  }
  if (state === 'playing' && player.state().paused) await player.play();
}
