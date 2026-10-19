import { writable, derived } from 'svelte/store';

export type SupportedLocale = 'fr' | 'en';

const translations = {
  fr: {
    // Navigation & Common
    home: 'Accueil',
    films: 'Films',
    series: 'Séries',
    search: 'Recherche',
    admin_dashboard: 'Tableau de bord (Admin)',
    profile_settings: 'Mon profil & Préférences',
    logout: 'Déconnexion',
    login: 'Connexion',
    you: 'Vous',
    save: 'Enregistrer',
    cancel: 'Annuler',
    delete: 'Supprimer',
    edit: 'Modifier',
    close: 'Fermer',
    status: 'Statut',
    active: 'Actif',
    connected: 'Connecté',
    disconnected: 'Hors ligne',
    revoked: 'Révoqué',
    loading: 'Chargement...',
    language: 'Langue de l’interface',
    language_fr: 'Français',
    language_en: 'English',

    // User & Roles
    administrator: 'Administrateur',
    user: 'Utilisateur',
    local_account: 'Compte local',
    sso_account: 'SSO (OpenID Connect)',
    auth_local_sub: 'Compte local (hash Argon2id)',
    sso_badge: 'SSO (OIDC)',

    // Admin Dashboard
    admin_title: 'Tableau de bord administrateur',
    admin_subtitle: 'Supervision globale, gestion des bibliothèques et nœuds distribués',
    admin_category_server: 'SERVEUR',
    admin_category_media: 'MÉDIATHÈQUE',
    admin_category_access: 'ACCÈS & SÉCURITÉ',
    admin_nav_overview: 'Vue d’ensemble',
    admin_nav_libraries: 'Médiathèques & Scans',
    admin_nav_nodes: 'Nœuds de stockage',
    admin_nav_users: 'Utilisateurs & Accès',
    admin_nav_sso: 'SSO (OpenID Connect)',

    // Users section
    users_heading: 'Comptes utilisateurs & Autorisations',
    users_subtitle: 'Gestion des comptes et matrice des permissions de streaming par bibliothèque (RBAC)',
    registered_accounts: 'Comptes enregistrés',
    strict_roles_note: 'Rôles stricts : administrator et user uniquement.',
    col_user_login: 'Identifiant / Pseudo',
    col_user_name: 'Nom complet / Affiché',
    col_auth_type: 'Authentification',
    col_role: 'Rôle',
    col_allowed_libs: 'Bibliothèques autorisées',
    all_libs_granted: '✓ Toutes les bibliothèques (administrateur)',
    no_libs_granted: 'Aucune bibliothèque autorisée',
    users_footer_hint: 'Pour modifier les bibliothèques accessibles par un compte utilisateur ordinaire, éditez directement les bibliothèques concernées dans la section Médiathèques & Scans ci-dessus.',

    // Settings
    settings_title: 'Profil & Paramètres',
    settings_eyebrow: 'VOTRE ESPACE PERSONNEL',
    settings_subtitle: 'Gérez votre identité, vos accès SSO et vos préférences de lecture.',
    settings_user_account: 'Compte utilisateur',
    settings_unique_id: 'Identifiant unique (UUID)',
    settings_auth_type: 'Type d’authentification',
    settings_session_security: 'Sécurité de session',
    settings_session_note: 'Cookie HttpOnly strict, rotation CSRF et expiration glissante.',
    settings_playback_prefs: 'Préférences de lecture & Interface',
    settings_playback_note: 'Ces préférences sont appliquées automatiquement dès le lancement d’un média.',
    settings_pref_audio: 'Langue audio préférée',
    settings_pref_subtitles: 'Comportement des sous-titres',
    settings_sub_forced: 'Sous-titres français forcés uniquement (par défaut)',
    settings_sub_all: 'Toujours afficher les sous-titres complets',
    settings_sub_none: 'Désactiver les sous-titres par défaut',
    settings_saved_alert: 'Préférences enregistrées avec succès',

    // Catalog & Search
    search_placeholder: 'Rechercher un film, une série...',
    play: 'Lecture',
    resume: 'Reprendre',
    start_over: 'Recommencer',
    audio: 'Audio',
    subtitles: 'Sous-titres',
  },
  en: {
    // Navigation & Common
    home: 'Home',
    films: 'Movies',
    series: 'TV Shows',
    search: 'Search',
    admin_dashboard: 'Admin Dashboard',
    profile_settings: 'My Profile & Preferences',
    logout: 'Sign out',
    login: 'Sign in',
    you: 'You',
    save: 'Save',
    cancel: 'Cancel',
    delete: 'Delete',
    edit: 'Edit',
    close: 'Close',
    status: 'Status',
    active: 'Active',
    connected: 'Connected',
    disconnected: 'Offline',
    revoked: 'Revoked',
    loading: 'Loading...',
    language: 'Interface language',
    language_fr: 'Français',
    language_en: 'English',

    // User & Roles
    administrator: 'Administrator',
    user: 'Standard User',
    local_account: 'Local Account',
    sso_account: 'SSO (OpenID Connect)',
    auth_local_sub: 'Local account (Argon2id hash)',
    sso_badge: 'SSO (OIDC)',

    // Admin Dashboard
    admin_title: 'Administrator Dashboard',
    admin_subtitle: 'Global instance overview, media libraries and distributed storage nodes',
    admin_category_server: 'SERVER',
    admin_category_media: 'MEDIA LIBRARY',
    admin_category_access: 'ACCESS & SECURITY',
    admin_nav_overview: 'Overview',
    admin_nav_libraries: 'Libraries & Scans',
    admin_nav_nodes: 'Media Nodes',
    admin_nav_users: 'Users & Permissions',
    admin_nav_sso: 'SSO (OpenID Connect)',

    // Users section
    users_heading: 'User Accounts & Access Matrix',
    users_subtitle: 'Account directory and per-library RBAC streaming permissions',
    registered_accounts: 'Registered Accounts',
    strict_roles_note: 'Strict roles: administrator and user only.',
    col_user_login: 'Username / Handle',
    col_user_name: 'Full / Display Name',
    col_auth_type: 'Authentication',
    col_role: 'Role',
    col_allowed_libs: 'Allowed Libraries',
    all_libs_granted: '✓ All libraries granted (administrator)',
    no_libs_granted: 'No libraries granted',
    users_footer_hint: 'To modify library permissions for standard users, edit the target library directly in the Libraries & Scans section above.',

    // Settings
    settings_title: 'Profile & Settings',
    settings_eyebrow: 'YOUR PERSONAL SPACE',
    settings_subtitle: 'Manage your identity, SSO linkage and playback preferences.',
    settings_user_account: 'User Account',
    settings_unique_id: 'Unique Identifier (UUID)',
    settings_auth_type: 'Authentication Type',
    settings_session_security: 'Session Security',
    settings_session_note: 'Strict HttpOnly cookie, CSRF rotation and sliding expiration.',
    settings_playback_prefs: 'Playback & Interface Preferences',
    settings_playback_note: 'These preferences are automatically applied when launching a media stream.',
    settings_pref_audio: 'Preferred audio language',
    settings_pref_subtitles: 'Subtitle behavior',
    settings_sub_forced: 'Forced subtitles only (default)',
    settings_sub_all: 'Always display full subtitles',
    settings_sub_none: 'Disable subtitles by default',
    settings_saved_alert: 'Preferences saved successfully',

    // Catalog & Search
    search_placeholder: 'Search for movies, series...',
    play: 'Play',
    resume: 'Resume',
    start_over: 'Start over',
    audio: 'Audio',
    subtitles: 'Subtitles',
  }
} as const;

export type TranslationKey = keyof (typeof translations)['fr'];

function getInitialLocale(): SupportedLocale {
  if (typeof window === 'undefined') return 'fr';
  const saved = localStorage.getItem('media_locale');
  if (saved === 'fr' || saved === 'en') return saved;
  const navLang = navigator.language.toLowerCase();
  return navLang.startsWith('en') ? 'en' : 'fr';
}

export const locale = writable<SupportedLocale>(getInitialLocale());

export function setLocale(newLocale: SupportedLocale) {
  locale.set(newLocale);
  if (typeof window !== 'undefined') {
    localStorage.setItem('media_locale', newLocale);
    document.documentElement.lang = newLocale;
  }
}

export const t = derived(locale, ($locale) => {
  return (key: TranslationKey): string => {
    return translations[$locale]?.[key] ?? translations.fr[key] ?? key;
  };
});
