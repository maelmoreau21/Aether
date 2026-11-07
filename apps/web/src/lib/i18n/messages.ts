// Message catalogue. French is the source of truth: `TranslationKey` is derived from it and every
// English domain file is typed against its French counterpart, so a missing or surplus key is a
// compile error (`npm run check`) and a failing test (`npm test`).
//
// Domains keep the files reviewable. They are plain objects imported statically, so Vite ships them
// in the same chunk as before: no extra request and no asynchronous first paint.
import { admin as frAdmin } from './locales/fr/admin';
import { applications as frApplications } from './locales/fr/applications';
import { auth as frAuth } from './locales/fr/auth';
import { catalog as frCatalog } from './locales/fr/catalog';
import { common as frCommon } from './locales/fr/common';
import { music as frMusic } from './locales/fr/music';
import { player as frPlayer } from './locales/fr/player';
import { settings as frSettings } from './locales/fr/settings';
import { stats as frStats } from './locales/fr/stats';
import { admin as enAdmin } from './locales/en/admin';
import { applications as enApplications } from './locales/en/applications';
import { auth as enAuth } from './locales/en/auth';
import { catalog as enCatalog } from './locales/en/catalog';
import { common as enCommon } from './locales/en/common';
import { music as enMusic } from './locales/en/music';
import { player as enPlayer } from './locales/en/player';
import { settings as enSettings } from './locales/en/settings';
import { stats as enStats } from './locales/en/stats';

type Prefixed<P extends string, T> = { [K in keyof T & string as `${P}${K}`]: T[K] };

/** Statistics keys live in their own file and are exposed as `stats_<key>` to avoid collisions. */
function prefixed<P extends string, T extends Readonly<Record<string, string>>>(prefix: P, source: T): Prefixed<P, T> {
  return Object.fromEntries(Object.entries(source).map(([key, value]) => [`${prefix}${key}`, value])) as Prefixed<P, T>;
}

/** Domain tables per language, used by the completeness test and by tooling. */
export const domains = {
  fr: { common: frCommon, admin: frAdmin, settings: frSettings, catalog: frCatalog, player: frPlayer, applications: frApplications, auth: frAuth, music: frMusic, stats: prefixed('stats_', frStats) },
  en: { common: enCommon, admin: enAdmin, settings: enSettings, catalog: enCatalog, player: enPlayer, applications: enApplications, auth: enAuth, music: enMusic, stats: prefixed('stats_', enStats) }
} as const;

const fr = { ...frCommon, ...frAdmin, ...frSettings, ...frCatalog, ...frPlayer, ...frApplications, ...frAuth, ...frMusic, ...prefixed('stats_', frStats) };
export type TranslationKey = keyof typeof fr;
const en: Readonly<Record<TranslationKey, string>> = { ...enCommon, ...enAdmin, ...enSettings, ...enCatalog, ...enPlayer, ...enApplications, ...enAuth, ...enMusic, ...prefixed('stats_', enStats) };

export const messages: Readonly<Record<'fr' | 'en', Readonly<Record<TranslationKey, string>>>> = { fr, en };
export type SupportedLocale = keyof typeof messages;
