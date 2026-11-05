/** Logique pure de la gestion des comptes (filtre, tri, groupes, messages d'erreur). */

export type UserSortField = 'name' | 'role' | 'status' | 'lastSeen';

export interface ListedUser {
  id: string;
  login: string;
  displayName?: string | null;
  role: string;
  disabledAt?: number | null;
  lastSeenAt?: number | null;
}

export interface UserGroup {
  id: string;
  name: string;
  isDefault?: boolean;
  userIds: string[];
  libraryIds: string[];
}

export interface UserListFilters {
  search: string;
  roleFilter: string;
  statusFilter: string;
  sortField: UserSortField;
  sortAsc: boolean;
}

/** Filtre (recherche, rôle, statut) puis trie les comptes ; ne modifie pas la liste d'origine. */
export function filterAndSortUsers<T extends ListedUser>(users: readonly T[], filters: UserListFilters): T[] {
  const q = filters.search.trim().toLowerCase();
  return users.filter(user => {
    const matchSearch = !q || [user.login, user.displayName ?? '', user.id].some(v => v.toLowerCase().includes(q));
    const matchRole = filters.roleFilter === 'all' || user.role === filters.roleFilter;
    const matchStatus = filters.statusFilter === 'all' ||
      (filters.statusFilter === 'disabled' ? user.disabledAt != null : user.disabledAt == null);
    return matchSearch && matchRole && matchStatus;
  }).sort((a, b) => {
    let cmp = 0;
    if (filters.sortField === 'name') {
      cmp = (a.displayName || a.login).localeCompare(b.displayName || b.login);
    } else if (filters.sortField === 'role') {
      cmp = a.role.localeCompare(b.role);
    } else if (filters.sortField === 'status') {
      cmp = (a.disabledAt != null ? 1 : 0) - (b.disabledAt != null ? 1 : 0);
    } else if (filters.sortField === 'lastSeen') {
      cmp = (a.lastSeenAt ?? 0) - (b.lastSeenAt ?? 0);
    }
    return filters.sortAsc ? cmp : -cmp;
  });
}

/** Libellé de dernière activité (timestamp en secondes). */
export function activityLabel(timestamp: number | null | undefined, locale: string): string {
  if (timestamp == null) return 'Jamais';
  return new Date(timestamp * 1000).toLocaleString(locale === 'en' ? 'en-GB' : 'fr-FR', {
    dateStyle: 'short',
    timeStyle: 'short'
  });
}

const MUTATION_ERRORS: Record<string, string> = {
  login_exists: 'Cet identifiant existe déjà.',
  self_protection: 'Vous ne pouvez pas désactiver ou rétrograder votre propre compte.',
  last_administrator: 'Le dernier administrateur actif doit être conservé.',
  invalid_password: 'Le mot de passe doit contenir de 15 à 128 caractères sans caractère de contrôle.',
  local_credentials_required: 'Ce compte est géré par le fournisseur SSO.',
  invalid_login: 'Identifiant : 3 à 64 caractères minuscules, chiffres, points, tirets ou underscores.',
  invalid_display_name: 'Nom : 128 caractères maximum, sans caractère de contrôle.'
};

/** Message affichable pour un code d'erreur de l'API comptes. */
export function userMutationErrorMessage(code: string | undefined): string {
  return (code !== undefined && MUTATION_ERRORS[code]) || 'Opération refusée ou serveur indisponible.';
}

export interface GroupMembershipUpdate {
  group: UserGroup;
  userIds: string[];
}

/** Groupes (hors groupe par défaut) dont l'appartenance de `userId` doit changer pour atteindre `targetGroupIds`. */
export function groupMembershipUpdates(
  groups: readonly UserGroup[],
  userId: string,
  targetGroupIds: readonly string[]
): GroupMembershipUpdate[] {
  const updates: GroupMembershipUpdate[] = [];
  for (const group of groups) {
    if (group.isDefault) continue;
    const isMember = group.userIds.includes(userId);
    const shouldBeMember = targetGroupIds.includes(group.id);
    if (isMember !== shouldBeMember) {
      updates.push({
        group,
        userIds: shouldBeMember ? [...group.userIds, userId] : group.userIds.filter(id => id !== userId)
      });
    }
  }
  return updates;
}
