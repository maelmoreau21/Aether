<script lang="ts">
  import type { CoreHealth, AdminNode, Library, Scan } from './types';
  let {coreHealth,nodes,libraries,scans}:{coreHealth:CoreHealth|null;nodes:AdminNode[];libraries:Library[];scans:Record<string,Scan>}=$props();
  const connected=$derived(nodes.filter(node=>node.status==='connected').length);
  const offline=$derived(nodes.filter(node=>node.status==='disconnected').length);
  const running=$derived(Object.values(scans).filter(scan=>scan.status==='queued'||scan.status==='running').length);
  const errors=$derived(Object.values(scans).reduce((total,scan)=>total+(scan.errorsCount??scan.errors?.length??0),0));
  const versions=$derived([...new Set(nodes.map(node=>node.version||'Non communiquée'))]);
</script>
<section id="overview" aria-labelledby="overview-heading">
  <h2 id="overview-heading">Vue d’ensemble</h2>
  <div class="overview-grid">
    <article><span>Serveur</span><strong>{coreHealth?.status==='ok'?'Actif':'État indisponible'}</strong></article>
    <article><span>Bibliothèques</span><strong>{libraries.length}</strong></article>
    <article><span>Nœuds connectés</span><strong>{connected} <small>/ {nodes.length}</small></strong>{#if offline}<p class="warning">{offline} hors ligne</p>{/if}</article>
  </div>
  {#if running}<p role="status">{running} indexation{running>1?'s':''} en cours.</p>{/if}
  {#if errors}<p role="alert">{errors} erreur{errors>1?'s':''} d’indexation. Consultez les bibliothèques concernées.</p>{/if}
  <details><summary>Informations techniques</summary><dl><div><dt>Version serveur</dt><dd>{coreHealth?.version??'Non communiquée'}</dd></div><div><dt>Mode</dt><dd>{coreHealth?.mode??'Non communiqué'}</dd></div><div><dt>Versions des nœuds</dt><dd>{versions.join(', ')||'Aucun nœud'}</dd></div></dl></details>
</section>
<style>
  h2{font-size:22px;letter-spacing:-.02em;margin:0 0 24px;color:var(--color-text);}.overview-grid{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:16px;}.overview-grid article{display:flex;flex-direction:column;gap:12px;padding:20px;background:var(--color-surface);border:1px solid var(--color-line);border-radius:var(--radius-md);}.overview-grid span{font-size:13px;color:var(--color-muted);}.overview-grid strong{font-size:26px;color:var(--color-text);}.overview-grid small{font-size:16px;color:var(--color-muted);font-weight:400;}.warning,[role=alert]{color:var(--color-danger);}.warning{margin:0;font-size:13px;}details{margin-top:24px;border:1px solid var(--color-line);border-radius:var(--radius-sm);padding:16px;}summary{cursor:pointer;font-size:13px;color:var(--color-text);}dl{display:grid;gap:12px;font-size:13px;}dl div{display:flex;justify-content:space-between;gap:16px;flex-wrap:wrap;}dt{color:var(--color-muted);}dd{margin:0;color:var(--color-text);overflow-wrap:anywhere;}@media(max-width:600px){.overview-grid{grid-template-columns:1fr;}.overview-grid article{padding:16px;}}
</style>
