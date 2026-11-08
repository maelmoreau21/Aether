import { describe, expect, it } from 'vitest';
import { activityLabel, filterAndSortUsers, groupMembershipUpdates, userMutationErrorMessage, type ListedUser } from './user-list';

const users: ListedUser[] = [
  { id: 'u1', login: 'zoe', displayName: 'Zoé', role: 'user', disabledAt: null, lastSeenAt: 50 },
  { id: 'u2', login: 'adam', displayName: null, role: 'administrator', disabledAt: null, lastSeenAt: 100 },
  { id: 'u3', login: 'bob', displayName: 'Bob', role: 'user', disabledAt: 10, lastSeenAt: null }
];
const base = { search: '', roleFilter: 'all', statusFilter: 'all', sortField: 'name' as const, sortAsc: true };

describe('filterAndSortUsers', () => {
  it('sorts by display name or login, ascending and descending', () => {
    expect(filterAndSortUsers(users, base).map(u => u.id)).toEqual(['u2', 'u3', 'u1']);
    expect(filterAndSortUsers(users, { ...base, sortAsc: false }).map(u => u.id)).toEqual(['u1', 'u3', 'u2']);
  });

  it('filters by search, role and status without mutating the input', () => {
    expect(filterAndSortUsers(users, { ...base, search: ' BOB ' }).map(u => u.id)).toEqual(['u3']);
    expect(filterAndSortUsers(users, { ...base, search: 'u2' }).map(u => u.id)).toEqual(['u2']);
    expect(filterAndSortUsers(users, { ...base, roleFilter: 'administrator' }).map(u => u.id)).toEqual(['u2']);
    expect(filterAndSortUsers(users, { ...base, statusFilter: 'disabled' }).map(u => u.id)).toEqual(['u3']);
    expect(filterAndSortUsers(users, { ...base, statusFilter: 'active' })).toHaveLength(2);
    expect(users.map(u => u.id)).toEqual(['u1', 'u2', 'u3']);
  });

  it('sorts by last seen and status', () => {
    expect(filterAndSortUsers(users, { ...base, sortField: 'lastSeen' }).map(u => u.id)).toEqual(['u3', 'u1', 'u2']);
    expect(filterAndSortUsers(users, { ...base, sortField: 'status' }).at(-1)?.id).toBe('u3');
  });
});

describe('user helpers', () => {
  it('labels activity and API errors', () => {
    expect(activityLabel(null, 'fr')).toBe('Jamais');
    expect(activityLabel(1_700_000_000, 'en')).toMatch(/2023/);
    expect(userMutationErrorMessage('login_exists')).toBe('Cet identifiant existe déjà.');
    expect(userMutationErrorMessage('unknown')).toBe('Opération refusée ou serveur indisponible.');
    expect(userMutationErrorMessage(undefined)).toBe('Opération refusée ou serveur indisponible.');
  });

  it('lists only the groups whose membership changes, skipping the default group', () => {
    const groups = [
      { id: 'g1', name: 'A', userIds: ['u1'], libraryIds: [] },
      { id: 'g2', name: 'B', userIds: [], libraryIds: [] },
      { id: 'g3', name: 'C', userIds: ['u1'], libraryIds: [] },
      { id: 'role', name: 'Role', systemRole: 'user' as const, userIds: ['u1'], libraryIds: [] },
      { id: 'def', name: 'Default', isDefault: true, userIds: [], libraryIds: [] }
    ];
    const updates = groupMembershipUpdates(groups, 'u1', ['g2', 'g3', 'def']);
    expect(updates.map(u => [u.group.id, u.userIds])).toEqual([['g1', []], ['g2', ['u1']]]);
  });
});
