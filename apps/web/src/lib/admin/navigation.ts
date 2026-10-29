import type { IconName } from '../ui/Icon.svelte';

type LabelKey = 'admin_nav_overview' | 'admin_nav_libraries' | 'admin_nav_nodes' | 'admin_nav_users' | 'admin_nav_tasks';
export interface AdminDestination { hash: string; label: string; englishLabel?: string; key?: LabelKey; icon: IconName }
export const adminNavigation: { label: string; englishLabel: string; links: AdminDestination[] }[] = [
  { label: 'Serveur', englishLabel: 'Server', links: [
    { hash: '#overview', label: 'Vue d’ensemble', key: 'admin_nav_overview', icon: 'server' },
    { hash: '#general-section', label: 'Général', englishLabel: 'General', icon: 'settings' },
    { hash: '#network-section', label: 'Réseau', englishLabel: 'Network', icon: 'globe' }
  ] },
  { label: 'Médiathèque', englishLabel: 'Media library', links: [
    { hash: '#libraries-section', label: 'Bibliothèques', key: 'admin_nav_libraries', icon: 'film' },
    { hash: '#artwork-section', label: 'Images', englishLabel: 'Artwork', icon: 'image' },
    { hash: '#nodes-section', label: 'Nœuds de stockage', key: 'admin_nav_nodes', icon: 'server' },
    { hash: '#avatars-section', label: 'Avatars', icon: 'image' }
  ] },
  { label: 'Accès', englishLabel: 'Access', links: [
    { hash: '#users-section', label: 'Utilisateurs', key: 'admin_nav_users', icon: 'users' },
    { hash: '#groups-section', label: 'Groupes d’utilisateurs', englishLabel: 'User groups', icon: 'users' },
    { hash: '#api-keys-section', label: 'Clés API', englishLabel: 'API keys', icon: 'shield' }
  ] },
  { label: 'Exploitation', englishLabel: 'Operations', links: [
    { hash: '#tasks-section', label: 'Planificateur de tâches', key: 'admin_nav_tasks', icon: 'clock' },
    { hash: '#backups-section', label: 'Sauvegardes', englishLabel: 'Backups', icon: 'server' },
    { hash: '#extensions-section', label: 'Extensions', icon: 'sparkles' }
  ] }
];
export const adminDestinations = adminNavigation.flatMap(group => group.links);
export const adminSectionHashes = new Set(adminDestinations.map(link => link.hash));
export function adminDestinationLabel(link: AdminDestination, translate: (key: LabelKey) => string, english = false): string {
  return link.key ? translate(link.key) : english ? link.englishLabel ?? link.label : link.label;
}
