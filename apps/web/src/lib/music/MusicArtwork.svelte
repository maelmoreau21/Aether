<script lang="ts">
  let { src, label, circular = false }: { src?: string | null; label: string; circular?: boolean } = $props();
  let failed = $state(false);
  $effect(() => { void src; failed = false; });
</script>

<span class="artwork" class:circular aria-hidden="true">
  {#if src && !failed}<img {src} alt="" width="400" height="400" loading="lazy" decoding="async" onerror={() => failed = true} />
  {:else}<span>{label.trim().slice(0, 1).toLocaleUpperCase() || '♪'}</span>{/if}
</span>

<style>
  .artwork { display:grid; place-items:center; width:100%; height:100%; aspect-ratio:1; overflow:hidden; border-radius:var(--r-sm); background:var(--pn-2); color:var(--mu); }
  .circular { border-radius:50%; } img { width:100%; height:100%; object-fit:cover; } .artwork > span { font-size:clamp(20px,5vw,56px); font-weight:800; letter-spacing:-0.03em; line-height:1; }
</style>
