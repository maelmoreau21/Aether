import type { IconName } from '../ui/Icon.svelte';

type LabelKey = 'admin_nav_overview' | 'admin_nav_libraries' | 'admin_nav_nodes' | 'admin_nav_users' | 'admin_nav_tasks';
export interface AdminDestination { hash: string; label: string; englishLabel?: string; key?: LabelKey; icon: IconName }
/** A sidebar entry. Entries with tabs group related sections behind one destination. */
export interface AdminEntry extends AdminDestination { tabs?: AdminDestination[] }

const artworkTabs: AdminDestination[] = [
  { hash: '#artwork-section', label: 'Images', englishLabel: 'Artwork', icon: 'image' },
  { hash: '#trickplay-section', label: 'Miniatures Trickplay', englishLabel: 'Trickplay thumbnails', icon: 'image' },
  { hash: '#avatars-section', label: 'Avatars', icon: 'image' }
];
const serverTabs: AdminDestination[] = [
  { hash: '#general-section', label: 'Général', englishLabel: 'General', icon: 'settings' },
  { hash: '#network-section', label: 'Réseau', englishLabel: 'Network', icon: 'globe' }
];

export const adminNavigation: { label: string; englishLabel: string; links: AdminEntry[] }[] = [
  { label: 'Serveur', englishLabel: 'Server', links: [
    { hash: '#overview', label: 'Vue d’ensemble', key: 'admin_nav_overview', icon: 'server' },
    { hash: '#statistics-section', label: 'Statistiques', englishLabel: 'Statistics', icon: 'clock' }
  ] },
  { label: 'Médiathèque', englishLabel: 'Media library', links: [
    { hash: '#libraries-section', label: 'Bibliothèques', key: 'admin_nav_libraries', icon: 'film' },
    { hash: '#nodes-section', label: 'Media Nodes', key: 'admin_nav_nodes', icon: 'server' },
    { hash: '#artwork-section', label: 'Métadonnées et images', englishLabel: 'Metadata and artwork', icon: 'image', tabs: artworkTabs }
  ] },
  { label: 'Accès', englishLabel: 'Access', links: [
    { hash: '#users-section', label: 'Utilisateurs', key: 'admin_nav_users', icon: 'users' },
    { hash: '#groups-section', label: 'Groupes et permissions', englishLabel: 'Groups and permissions', icon: 'users' },
    { hash: '#api-keys-section', label: 'Clés API', englishLabel: 'API keys', icon: 'shield' }
  ] },
  { label: 'Système', englishLabel: 'System', links: [
    { hash: '#tasks-section', label: 'Tâches planifiées', key: 'admin_nav_tasks', icon: 'clock' },
    { hash: '#backups-section', label: 'Sauvegardes', englishLabel: 'Backups', icon: 'server' },
    { hash: '#extensions-section', label: 'Extensions', icon: 'sparkles' },
    { hash: '#general-section', label: 'Paramètres du serveur', englishLabel: 'Server settings', icon: 'settings', tabs: serverTabs }
  ] }
];
export const adminEntries = adminNavigation.flatMap(group => group.links);
export const adminDestinations: AdminDestination[] = adminEntries.flatMap(entry => entry.tabs ?? [entry]);
export const adminSectionHashes = new Set(adminDestinations.map(link => link.hash));
export function adminEntryFor(hash: string): AdminEntry | undefined {
  return adminEntries.find(entry => (entry.tabs ?? [entry]).some(link => link.hash === hash));
}
export function parseAdminHash(hash: string): { section: string; task: string | null } {
  const [candidate, query = ''] = hash.split('?', 2);
  const section = adminSectionHashes.has(candidate) ? candidate : '#overview';
  return { section, task: section === '#tasks-section' ? new URLSearchParams(query).get('task') || null : null };
}
export function adminDestinationLabel(link: AdminDestination, translate: (key: LabelKey) => string, english = false): string {
  return link.key ? translate(link.key) : english ? link.englishLabel ?? link.label : link.label;
}

/**
 * Settings and actions that really live in each administration section, drawn
 * from the section components. Used only by the sidebar settings search.
 */
export const adminSearchKeywords: Record<string, readonly string[]> = {
  '#statistics-section': ['Statistiques', 'Collecte', 'Conservation', 'Historique de lecture', 'Audio et sous-titres', 'Temps consommé', 'Exporter en CSV', 'Sessions', 'Musique'],
  '#overview': ['Tableau de bord', 'Qui regarde quoi ?', 'Lecteurs ouverts', 'Indexations en cours', 'Actualiser l’activité', 'Informations techniques', 'Version serveur', 'Versions des nœuds', 'Erreurs des derniers scans'],
  '#libraries-section': ['Ajouter une bibliothèque', 'Actualiser toutes les bibliothèques', 'Scanner la bibliothèque', 'Renommer une bibliothèque', 'Modifier les images', 'Rechercher les métadonnées manquantes', 'Planifier l’actualisation', 'Sources et dossiers', 'Bibliothèque active', 'Dossiers ou motifs à exclure', 'Indexation', 'Actualisation rapide', 'Tout réanalyser', 'Images locales', 'Images des fournisseurs', 'Rotation des affiches', 'Utiliser les fichiers NFO locaux', 'Sous-titres intégrés', 'Fournisseurs et enrichissement', 'Détection d’intro', 'Images des chapitres', 'Téléchargement des sous-titres', 'Surveiller les modifications', 'Ajouter automatiquement aux collections'],
  '#nodes-section': ['Ajouter un Media Node', 'Jeton d’appairage', 'Jeton d’invitation', 'Jeton d’amorçage statique', 'Appairage direct entrant', 'Configuration Docker Compose', 'URL interne du Node', 'URL publique de streaming direct', 'Reconnecter', 'Renouveler le secret', 'Révoquer un Media Node', 'Dossiers multimédias'],
  '#artwork-section': ['Images des médiathèques', 'Collecter les images', 'Langues par ordre de préférence', 'Maximum d’images par œuvre', 'Inclure les images sans langue indiquée', 'Affiches disponibles', 'Variantes enregistrées', 'Gérer les images', 'Œuvres à vérifier'],
  '#trickplay-section': ['Trickplay', 'Activer le système de miniatures Trickplay', 'Génération automatique pendant l’analyse', 'Qualité JPEG', 'Largeur des vignettes', 'Intervalle des miniatures', 'Quota cache', 'Aperçus de la barre de lecture'],
  '#avatars-section': ['Galerie d’avatars', 'Importer un pack depuis GitHub', 'Ajouter les avatars', 'Politique des avatars', 'Collections d’avatars', 'Transférer les collections'],
  '#users-section': ['Créer un utilisateur', 'Mot de passe', 'Rôle', 'Administrateur', 'Désactiver un compte', 'Appartenance aux groupes', 'Bibliothèques accessibles', 'Contrôle parental', 'Restrictions parentales', 'Âge maximal autorisé', 'Tags bloqués', 'Horaires d’accès', 'Téléchargement et synchronisation'],
  '#groups-section': ['Groupes d’utilisateurs', 'Groupe par défaut', 'Membres du groupe', 'Permissions sur les bibliothèques', 'Type d’affectation', 'Propriétés du groupe'],
  '#api-keys-section': ['Créer une clé', 'Expiration en jours', 'Révoquer une clé', 'Propriétaire de la clé', 'Jetons d’accès'],
  '#tasks-section': ['Planification', 'Planifications et lancement manuel', 'Déclencheur périodique', 'Intervalle des tâches', 'File d’exécution', 'Opérations en cours', 'Suivi des opérations', 'Automatisation des métadonnées', 'Quotas fournisseurs', 'Nettoyer le journal des extensions'],
  '#backups-section': ['Sauvegarde automatique', 'Activer les sauvegardes automatiques', 'Planification automatique', 'Archives', 'Télécharger une sauvegarde', 'Base de données', 'Consommation mémoire', 'Cache disque Linux'],
  '#extensions-section': ['Extensions installées', 'Catalogue', 'Dépôts', 'Adresse du dépôt', 'Mettre à jour une extension', 'Historique des révisions', 'Accès demandés', 'Paramètres des extensions', 'Plugins'],
  '#general-section': ['Nom du serveur', 'Message d’accueil', 'Texte du bouton de connexion SSO', 'OIDC', 'SSO', 'Identifiants locaux', 'Reprise de lecture', 'Considéré comme vu', 'Durée minimale pour reprendre', 'Langue préférée', 'Pays', 'Métadonnées', 'Empreintes audio', 'Journal des extensions', 'Origines CORS autorisées', 'Partage des playlists'],
  '#network-section': ['Adresse publique', 'MEDIA_PUBLIC_URL', 'Interface Web', 'API', 'Cookies sécurisés', 'Cookies de session sécurisés (HTTPS)', 'HTTPS']
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
      const candidates = [destination.label, ...(destination.hash === entry.hash && entry.label !== destination.label ? [entry.label] : []), ...(adminSearchKeywords[destination.hash] ?? [])];
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
