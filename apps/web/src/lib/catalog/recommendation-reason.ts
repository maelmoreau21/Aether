export type RecommendationBecause = { kind: 'liked' | 'watched'; title?: string | null } | null | undefined;
type ReasonKey = 'recommended_because_liked' | 'recommended_because_watched' | 'recommended_to_discover';
type Translate = (key: ReasonKey, values?: Record<string, string>) => string;

/** Localized "Recommended because…" line; older servers only send the French `recommendationReason`. */
export function recommendationReasonText(item: { recommendationBecause?: RecommendationBecause; recommendationReason?: string }, t: Translate): string {
  const because = item.recommendationBecause;
  if (because?.title && (because.kind === 'liked' || because.kind === 'watched')) {
    return t(because.kind === 'liked' ? 'recommended_because_liked' : 'recommended_because_watched', { title: because.title });
  }
  if (item.recommendationBecause === null) return t('recommended_to_discover');
  return item.recommendationReason ?? t('recommended_to_discover');
}
