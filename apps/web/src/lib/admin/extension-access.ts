/**
 * Access a new extension version adds over the installed one. The Core refuses such an
 * update (`extension_update_requires_authorization`) until the administrator approves
 * exactly these permissions and HTTP hosts, so nothing widens silently.
 */
export interface AccessChange { permissions: string[]; httpHosts: string[] }

interface DeclaredAccess { permissions: string[]; httpHosts?: string[] }

export function addedAccess(installed: DeclaredAccess, target: DeclaredAccess): AccessChange {
  const hosts = new Set(installed.httpHosts ?? []);
  const permissions = new Set(installed.permissions);
  return {
    permissions: [...new Set(target.permissions)].filter(permission => !permissions.has(permission)),
    httpHosts: [...new Set(target.httpHosts ?? [])].filter(host => !hosts.has(host))
  };
}

export function needsApproval(change: AccessChange): boolean {
  return change.permissions.length > 0 || change.httpHosts.length > 0;
}
