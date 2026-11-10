import type { IconName } from '../ui/Icon.svelte';
import type { TranslationKey } from '../i18n';
import { messages } from '../i18n/messages';

type LabelKey = TranslationKey;
export interface AdminDestination { hash: string; label: string; labelKey: LabelKey; icon: IconName }
/** A sidebar entry. Entries with tabs group related sections behind one destination. */
export interface AdminEntry extends AdminDestination { tabs?: AdminDestination[]; extensionId?: 'media-requests' | 'media-reports' }

/** Scheduled tasks first; metadata automation stays one click away. */
const taskTabs: AdminDestination[] = [
  { hash: '#tasks-section', label: 'Tâches planifiées', labelKey: 'admin_nav_tasks', icon: 'clock' },
  { hash: '#automation-section', label: 'Automatisations', labelKey: 'admin_automation_tab', icon: 'sparkles' }
];
/** Log files and their level first; the live stream is the second tab. */
const logTabs: AdminDestination[] = [
  { hash: '#logs-section', label: 'Fichiers journaux', labelKey: 'admin_logs_files_tab', icon: 'file' },
  { hash: '#logs-live-section', label: 'Temps réel', labelKey: 'admin_logs_live_tab', icon: 'clock' }
];
const serverTabs: AdminDestination[] = [
  { hash: '#general-section', label: 'Général', labelKey: 'admin_general', icon: 'settings' },
  { hash: '#playback-section', label: 'Lecture et transcodage', labelKey: 'admin_playback', icon: 'film' },
  { hash: '#network-section', label: 'Réseau', labelKey: 'admin_network', icon: 'globe' }
];

export const adminNavigation: { label: string; labelKey: LabelKey; links: AdminEntry[] }[] = [
  { label: 'Serveur', labelKey: 'admin_group_server', links: [
    { hash: '#overview', label: 'Vue d’ensemble', labelKey: 'admin_nav_overview', icon: 'server' },
    { hash: '#statistics-section', label: 'Statistiques', labelKey: 'admin_statistics', icon: 'clock' }
  ] },
  { label: 'Médiathèque', labelKey: 'admin_group_media', links: [
    { hash: '#libraries-section', label: 'Bibliothèques', labelKey: 'admin_nav_libraries', icon: 'film' },
    { hash: '#nodes-section', label: 'Media Nodes', labelKey: 'admin_nav_nodes', icon: 'server' },
    { hash: '#avatars-section', label: 'Avatars', labelKey: 'admin_avatars_tab', icon: 'image' }
  ] },
  { label: 'Accès', labelKey: 'admin_group_access', links: [
    { hash: '#users-section', label: 'Utilisateurs', labelKey: 'admin_nav_users', icon: 'users' },
    { hash: '#groups-section', label: 'Groupes et permissions', labelKey: 'admin_groups', icon: 'users' }
  ] },
  { label: 'Extensions', labelKey: 'admin_group_extensions', links: [
    { hash: '#extensions-section', label: 'Gérer les extensions', labelKey: 'admin_manage_extensions', icon: 'sparkles' },
    { hash: '#requests-section', label: 'Demandes', labelKey: 'settings_requests', icon: 'download', extensionId: 'media-requests', tabs: [
      { hash: '#requests-section', label: 'Demandes', labelKey: 'settings_requests', icon: 'download' },
      { hash: '#acquisition-section', label: 'Acquisition et imports', labelKey: 'admin_acquisition', icon: 'server' }
    ] },
    { hash: '#reports-section', label: 'Signalements', labelKey: 'settings_reports', icon: 'alert', extensionId: 'media-reports' }
  ] },
  { label: 'Système', labelKey: 'admin_group_system', links: [
    { hash: '#api-keys-section', label: 'Clés API', labelKey: 'admin_api_keys', icon: 'shield' },
    { hash: '#tasks-section', label: 'Tâches planifiées', labelKey: 'admin_nav_tasks', icon: 'clock', tabs: taskTabs },
    { hash: '#logs-section', label: 'Journaux', labelKey: 'admin_nav_logs', icon: 'file', tabs: logTabs },
    { hash: '#backups-section', label: 'Sauvegardes', labelKey: 'admin_backups', icon: 'server' },
    { hash: '#general-section', label: 'Paramètres du serveur', labelKey: 'admin_server_settings', icon: 'settings', tabs: serverTabs }
  ] }
];
export const adminEntries = adminNavigation.flatMap(group => group.links);
export const adminDestinations: AdminDestination[] = adminEntries.flatMap(entry => entry.tabs ?? [entry]);
export const adminSectionHashes = new Set(adminDestinations.map(link => link.hash));
export function adminEntryFor(hash: string): AdminEntry | undefined {
  return adminEntries.find(entry => (entry.tabs ?? [entry]).some(link => link.hash === hash));
}
export function parseAdminHash(hash: string): { section: string; task: string | null } {
  const [requested, query = ''] = hash.split('?', 2);
  // Former sections: trickplay and the image collection now live in each library's settings;
  // the operations tab was folded into the scheduled tasks.
  const candidate = requested === '#trickplay-section' || requested === '#artwork-section' ? '#libraries-section'
    : requested === '#operations-section' ? '#tasks-section' : requested;
  const section = adminSectionHashes.has(candidate) ? candidate : '#overview';
  return { section, task: section === '#tasks-section' ? new URLSearchParams(query).get('task') || null : null };
}
export function adminDestinationLabel(link: AdminDestination, translate: (key: LabelKey) => string): string {
  return translate(link.labelKey);
}

/**
 * Settings and actions that really live in each administration section, drawn
 * from the section components. Used only by the sidebar settings search.
 */
export const adminSearchKeywords: Record<string, readonly string[]> = {
  '#statistics-section': ['Statistiques', 'Collecte', 'Conservation', 'Historique de lecture', 'Audio et sous-titres', 'Temps consommé', 'Exporter en CSV', 'Sessions', 'Musique'],
  '#overview': ['Tableau de bord', 'Qui regarde quoi ?', 'Lecteurs ouverts', 'Indexations en cours', 'Actualiser l’activité', 'Informations techniques', 'Version serveur', 'Versions des nœuds', 'Erreurs des derniers scans'],
  '#libraries-section': ['Langue des métadonnées', 'Langue préférée', 'Pays des classifications', 'Ajouter une bibliothèque', 'Actualiser toutes les bibliothèques', 'Scanner la bibliothèque', 'Renommer une bibliothèque', 'Modifier les images', 'Compléter les descriptions manquantes', 'Tout réanalyser', 'Animés', 'Sources et dossiers', 'Bibliothèque active', 'Dossiers ou motifs à exclure', 'Indexation', 'Actualisation rapide', 'Tout réanalyser', 'Images locales', 'Images des fournisseurs', 'Mode multi-images', 'Logos', 'Utiliser les fichiers NFO locaux', 'Sous-titres intégrés', 'Fournisseurs et enrichissement', 'Détection d’intro', 'Images des chapitres', 'Téléchargement des sous-titres', 'Surveiller les modifications', 'Ajouter automatiquement aux collections', 'Trickplay', 'Miniatures de la barre de lecture', 'Génération automatique pendant l’analyse', 'Qualité JPEG', 'Largeur des vignettes', 'Intervalle des miniatures'],
  '#nodes-section': ['Ajouter un Media Node', 'Jeton d’appairage', 'Jeton d’invitation', 'Jeton d’amorçage statique', 'Appairage direct entrant', 'Configuration Docker Compose', 'URL interne du Node', 'URL publique de streaming direct', 'Reconnecter', 'Renouveler le secret', 'Révoquer un Media Node', 'Dossiers multimédias'],
  '#avatars-section': ['Galerie d’avatars', 'Ajouter les avatars', 'Politique des avatars', 'Collections d’avatars', 'Transférer les collections'],
  '#users-section': ['Créer un utilisateur', 'Mot de passe', 'Rôle', 'Administrateur', 'Désactiver un compte', 'Appartenance aux groupes', 'Bibliothèques accessibles', 'Contrôle parental', 'Restrictions parentales', 'Âge maximal autorisé', 'Tags bloqués', 'Horaires d’accès', 'Téléchargement et synchronisation'],
  '#groups-section': ['Groupes d’utilisateurs', 'Groupe par défaut', 'Membres du groupe', 'Permissions sur les bibliothèques', 'Type d’affectation', 'Propriétés du groupe'],
  '#api-keys-section': ['Créer une clé', 'Expiration en jours', 'Révoquer une clé', 'Propriétaire de la clé', 'Jetons d’accès'],
  '#tasks-section': ['Planification', 'Tâches planifiées', 'Ajouter un déclencheur', 'Tous les jours', 'Hebdomadaire', 'Intervalle', 'Au démarrage du serveur', 'Limite de temps', 'Nettoyer le cache', 'Nettoyer le journal des extensions', 'Sauvegarde planifiée'],
  '#logs-section': ['Journaux', 'Journaux du serveur', 'Fichiers journaux', 'Logs', 'Niveau de journalisation', 'Débogage', 'Conservation des journaux', 'Télécharger les journaux', 'Diagnostic'],
  '#logs-live-section': ['Journaux en temps réel', 'Erreurs du serveur', 'Avertissements', 'Copier les lignes'],
  '#automation-section': ['Automatisations', 'Règles d’automatisation', 'Historique des automatisations', 'Limites des fournisseurs', 'Sources des métadonnées', 'Relancer les éléments manquants'],
  '#backups-section': ['Sauvegarde automatique', 'Activer les sauvegardes automatiques', 'Planification automatique', 'Archives', 'Télécharger une sauvegarde', 'Base de données', 'Consommation mémoire', 'Cache disque Linux'],
  '#extensions-section': ['Extensions installées', 'Catalogue', 'Dépôts', 'Adresse du dépôt', 'Mettre à jour une extension', 'Versions des extensions', 'Accès demandés', 'Paramètres des extensions', 'Plugins'],
  '#requests-section': ['Demandes', 'Valider une demande', 'Acquisition', 'Épingler les demandes'],
  '#acquisition-section': ['Serveurs d’acquisition', 'Radarr', 'Sonarr', 'Lidarr', 'Imports', 'Règles de validation'],
  '#reports-section': ['Signalements', 'Problèmes de lecture', 'Résoudre un signalement', 'Épingler les signalements'],
  '#playback-section': ['Transcodage', 'Transcodage vidéo', 'Transcodage audio', 'Tone mapping', 'HDR', 'Sous-titres incrustés', 'Débit maximal', 'Télécommande', 'Envoyer vers', 'Téléchargements', 'Regarder ensemble'],
  '#general-section': ['Nom du serveur', 'Message d’accueil', 'Texte du bouton de connexion SSO', 'OIDC', 'SSO', 'Identifiants locaux', 'Reprise de lecture', 'Considéré comme vu', 'Durée minimale pour reprendre', 'Empreintes audio', 'Journal des extensions', 'Origines CORS autorisées', 'Partage des playlists'],
  '#network-section': ['Adresse publique', 'MEDIA_PUBLIC_URL', 'Interface Web', 'API', 'Cookies sécurisés', 'Cookies de session sécurisés (HTTPS)', 'HTTPS']
};

/**
 * The same keywords in English, in the same order. The sidebar search looks in both languages so a
 * setting is found whichever language the administrator types, and the interface language only
 * decides which label is displayed.
 */
export const adminSearchKeywordsEn: Record<string, readonly string[]> = {
  '#statistics-section': ['Statistics', 'Collection', 'Retention', 'Playback history', 'Audio and subtitles', 'Time spent', 'Export as CSV', 'Sessions', 'Music'],
  '#overview': ['Dashboard', 'Who is watching what?', 'Open players', 'Indexing in progress', 'Refresh the activity', 'Technical information', 'Server version', 'Node versions', 'Errors from the latest scans'],
  '#libraries-section': ['Metadata language', 'Preferred language', 'Rating countries', 'Add a library', 'Refresh all libraries', 'Scan the library', 'Rename a library', 'Edit images', 'Fill missing descriptions', 'Rescan everything', 'Anime', 'Sources and folders', 'Active library', 'Folders or patterns to exclude', 'Indexing', 'Quick refresh', 'Re-analyse everything', 'Local images', 'Provider images', 'Multi-image mode', 'Logos', 'Use local NFO files', 'Embedded subtitles', 'Providers and enrichment', 'Intro detection', 'Chapter images', 'Subtitle download', 'Watch for changes', 'Add to collections automatically', 'Trickplay', 'Playback bar thumbnails', 'Automatic generation during analysis', 'JPEG quality', 'Thumbnail width', 'Thumbnail interval'],
  '#nodes-section': ['Add a Media Node', 'Pairing token', 'Invitation token', 'Static bootstrap token', 'Direct inbound pairing', 'Docker Compose configuration', 'Node internal URL', 'Public direct-streaming URL', 'Reconnect', 'Renew the secret', 'Revoke a Media Node', 'Media folders'],
  '#avatars-section': ['Avatar gallery', 'Add avatars', 'Avatar policy', 'Avatar collections', 'Transfer collections'],
  '#users-section': ['Create a user', 'Password', 'Role', 'Administrator', 'Disable an account', 'Group membership', 'Accessible libraries', 'Parental controls', 'Parental restrictions', 'Maximum allowed age', 'Blocked tags', 'Access hours', 'Download and sync'],
  '#groups-section': ['User groups', 'Default group', 'Group members', 'Library permissions', 'Assignment type', 'Group properties'],
  '#api-keys-section': ['Create a key', 'Expiry in days', 'Revoke a key', 'Key owner', 'Access tokens'],
  '#tasks-section': ['Scheduling', 'Scheduled tasks', 'Add a trigger', 'Every day', 'Weekly', 'Interval', 'At server startup', 'Time limit', 'Clear the cache', 'Clear the extension log', 'Scheduled backup'],
  '#logs-section': ['Logs', 'Server logs', 'Log files', 'Journal', 'Log level', 'Debug', 'Log retention', 'Download the logs', 'Diagnostics'],
  '#logs-live-section': ['Live logs', 'Server errors', 'Warnings', 'Copy the lines'],
  '#automation-section': ['Automations', 'Automation rules', 'Automation history', 'Provider limits', 'Metadata sources', 'Re-run missing items'],
  '#backups-section': ['Automatic backup', 'Enable automatic backups', 'Automatic scheduling', 'Archives', 'Download a backup', 'Database', 'Memory usage', 'Linux disk cache'],
  '#extensions-section': ['Installed extensions', 'Catalogue', 'Repositories', 'Repository address', 'Update an extension', 'Extension versions', 'Requested access', 'Extension settings', 'Plugins'],
  '#requests-section': ['Requests', 'Approve a request', 'Acquisition', 'Pin requests'],
  '#acquisition-section': ['Acquisition servers', 'Radarr', 'Sonarr', 'Lidarr', 'Imports', 'Approval rules'],
  '#reports-section': ['Reports', 'Playback problems', 'Resolve a report', 'Pin reports'],
  '#playback-section': ['Transcoding', 'Video transcoding', 'Audio transcoding', 'Tone mapping', 'HDR', 'Burnt-in subtitles', 'Maximum bit rate', 'Remote control', 'Send to', 'Downloads', 'Watch together'],
  '#general-section': ['Server name', 'Welcome message', 'SSO sign-in button text', 'OIDC', 'SSO', 'Local credentials', 'Playback resume', 'Counted as watched', 'Minimum time to resume', 'Audio fingerprints', 'Extension log', 'Allowed CORS origins', 'Playlist sharing'],
  '#network-section': ['Public address', 'MEDIA_PUBLIC_URL', 'Web interface', 'API', 'Secure cookies', 'Secure session cookies (HTTPS)', 'HTTPS']
};

export interface AdminSearchResult {
  /** Destination hash, navigated to exactly like a sidebar link. */
  hash: string;
  /** Setting, keyword or label that matched the query. */
  match: string;
  /** Sidebar entry that owns the destination (its label is the section label). */
  entry: AdminEntry;
  /** Destination itself: the entry or one of its tabs. */
  destination: AdminDestination;
}

const searchLimit = 30;
/** Lowercase, without diacritics and with straight apostrophes, for accent-insensitive matching. */
export function normalizeAdminSearch(value: string): string {
  return value.normalize('NFD').replace(/[̀-ͯ]/g, '').replace(/[’‘`]/g, "'").replace(/\s+/g, ' ').trim().toLowerCase();
}

/** Accent- and case-insensitive substring search over section labels and their settings. */
export function searchAdmin(query: string): AdminSearchResult[] {
  const needle = normalizeAdminSearch(query);
  if (!needle) return [];
  const found: (AdminSearchResult & { rank: number; order: number })[] = [];
  let order = 0;
  for (const entry of adminEntries) {
    for (const destination of entry.tabs ?? [entry]) {
      // French and English labels and keywords are searched together: the interface language only
      // decides how a result is displayed, not whether it can be found.
      const candidates = [
        destination.label, messages.en[destination.labelKey],
        ...(destination.hash === entry.hash && entry.label !== destination.label ? [entry.label, messages.en[entry.labelKey]] : []),
        ...(adminSearchKeywords[destination.hash] ?? []), ...(adminSearchKeywordsEn[destination.hash] ?? [])
      ];
      const seen = new Set<string>();
      for (const candidate of candidates) {
        const normalized = normalizeAdminSearch(candidate);
        if (seen.has(normalized)) continue;
        seen.add(normalized);
        const index = normalized.indexOf(needle);
        if (index < 0) continue;
        const rank = index === 0 ? 0 : /[\s'(-]/.test(normalized[index - 1]) ? 1 : 2;
        found.push({ hash: destination.hash, match: candidate, entry, destination, rank, order: order++ });
      }
    }
  }
  return found
    .sort((a, b) => a.rank - b.rank || a.order - b.order)
    .slice(0, searchLimit)
    .map(({ hash, match, entry, destination }) => ({ hash, match, entry, destination }));
}
