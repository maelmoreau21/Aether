<script lang="ts">
  import { onMount } from 'svelte';
  import { resolve } from '$app/paths';
  import { NativePlayer } from '../lib/player/native';
  import ImmersivePlayer from '../lib/player/ImmersivePlayer.svelte';
  import type { Media } from '../lib/player/types';
  import { DEFAULT_NAME } from '../lib/config/brand';
  import Icon from '../lib/ui/Icon.svelte';
  import Badge from '../lib/ui/Badge.svelte';
  import MediaCard from '../lib/ui/MediaCard.svelte';
  let name = $state(DEFAULT_NAME);
  let media = $state<Media[]>([]);
  let selected = $state('');
  let error = $state('');
  let busy = $state(true);
  let immersive: ImmersivePlayer;
  function selectMedia(id: string) { if (!busy) selected = id; }
  function openPlayer() {
    const current = media.find(item => item.id === selected);
    if (current) immersive.open(current);
    else error = 'Aucune source disponible. Attendez la connexion au Core ou vérifiez les fixtures.';
  }
  onMount(() => {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 10000);
    async function initialize() {
      try {
        const responses = await Promise.all(['/api/v1/config', '/api/v1/demo/media'].map(url => fetch(url, { signal: controller.signal })));
        if (responses.some(response => !response.ok)) throw new Error('Le Core ne répond pas correctement.');
        const [config, catalogue] = await Promise.all(responses.map(response => response.json()));
        name = config.name; media = catalogue.media; selected = media[0]?.id ?? '';
        if (!media.length) error = catalogue.notice;
      } catch (e) { error = controller.signal.aborted ? 'Connexion au Core expirée.' : `Connexion impossible : ${String(e)}`; }
      finally { clearTimeout(timeout); busy = false; }
    }
    void initialize();
    return () => { controller.abort(); clearTimeout(timeout); };
  });
</script>
<svelte:head><title>{name} · Votre cinéma, autrement.</title><meta name="description" content="Un premier regard sur votre cinéma personnel. Démonstration locale de lecture." /></svelte:head>

<a href="#player" onclick={event => { event.preventDefault(); openPlayer(); }} class="skip-link">Aller au lecteur</a>
<div class="page-shell">
  <header class="site-header">
    <a href={resolve('/')} class="brand" aria-label="{name}, accueil"><img src="/app/images/brand-mark.svg" width="25" height="30" alt="" /><span>{name.toLowerCase()}</span></a>
    <nav aria-label="Navigation principale"><a href="#discover" class="nav-active">Découvrir</a><a href="#player" onclick={event => { event.preventDefault(); openPlayer(); }}>La salle</a></nav>
    <div class="header-end"><span class="local-label"><span class="status-dot"></span>Espace local</span><span class="edition-mark">APERÇU P1</span></div>
  </header>

  <main>
    <section class="hero" aria-labelledby="hero-title">
      <div class="hero-art" aria-hidden="true"></div>
      <div class="hero-shade" aria-hidden="true"></div>
      <div class="hero-content">
        <p class="eyebrow"><span class="eyebrow-line"></span>VOTRE CINÉMA, AUTREMENT</p>
        <h1 id="hero-title">Laissez le monde<br />sur <em>pause.</em></h1>
        <p class="hero-copy">Les lumières s’éteignent. L’essentiel reste.<br />Un espace à vous, pour le plaisir de regarder.</p>
        <div class="hero-actions"><a href="#player" onclick={event => { event.preventDefault(); openPlayer(); }} class="hero-cta"><Icon name="play" size={17} />Ouvrir le lecteur</a><span class="hero-note">La première séance commence ici.</span></div>
        <div class="hero-specs"><Badge>AV1 · 10 BITS</Badge><Badge>OPUS</Badge><span>Deux langues. Votre choix.</span></div>
      </div>
      <div class="hero-caption"><span class="art-index">01 — HORIZONS</span><span>Visuel d’ambiance original<br />La démonstration utilise une mire synthétique.</span></div>
    </section>

    <section id="discover" class="editions page-section" aria-labelledby="editions-title">
      <div class="section-heading"><div><p class="eyebrow">UNE PREMIÈRE SÉANCE</p><h2 id="editions-title">Choisissez votre édition.</h2></div><p class="section-aside">Même image. Même liberté de choix.<br /><span>Deux sources de démonstration locales.</span></p></div>
      <div class="edition-grid">
        {#each media as item (item.id)}<MediaCard container={item.id.endsWith('mkv') ? 'MKV' : 'MP4'} active={selected === item.id} disabled={busy} onclick={() => selectMedia(item.id)} />{/each}
        {#if busy && !media.length}<div class="catalogue-message" role="status">Votre espace se prépare…</div>{:else if !media.length}<div class="catalogue-message">Aucune source disponible pour le moment.</div>{/if}
        <div class="edition-detail"><span class="detail-orbit" aria-hidden="true"><Icon name="film" size={24} /></span><h3>Le contenu au premier plan.</h3><p>Une image préservée, des langues au choix et des sous-titres quand vous le souhaitez.</p><a href="#player" onclick={event => { event.preventDefault(); openPlayer(); }}>Installez-vous <Icon name="arrow" size={15} /></a></div>
      </div>
    </section>

    {#if error}<div class="home-error" role="alert">{error}</div>{/if}
    <div class="closing-note"><span class="closing-rule"></span><p>Le plaisir de regarder.<br /><span>La liberté de garder le contrôle.</span></p><span class="closing-rule"></span></div>
  </main>
  <footer><a class="footer-brand" href={resolve('/')}>{name.toLowerCase()}<span>LE CINÉMA, CHEZ VOUS.</span></a><p>Première édition · Prototype P1<br /><span>Fixtures locales uniquement. Vos bibliothèques personnelles restent privées.</span></p><a href="#discover" class="back-top">Retour à la sélection <Icon name="arrow" size={14} /></a></footer>
</div>


<ImmersivePlayer bind:this={immersive} catalogue={media} createPlayer={(video, onError) => new NativePlayer(video, onError)} />

<style>
  .page-shell { overflow: clip; }
  .skip-link { position: fixed; top: -80px; left: 20px; z-index: 20; padding: 12px 18px; background: var(--color-text); color: var(--color-canvas); border-radius: 6px; }.skip-link:focus { top: 12px; }
  .site-header { position: relative; z-index: 2; display: flex; align-items: center; gap: 64px; height: 88px; padding: 0 var(--page-gutter); background: #09090e; }
  .brand { display: inline-flex; align-items: center; gap: 9px; }.brand span { font-size: 30px; font-weight: 650; letter-spacing: -.065em; }.brand img { width: 26px; height: 30px; }
  nav { display: flex; gap: 30px; height: 100%; align-items: center; }nav a { display: flex; align-items: center; height: 100%; font-size: 12px; color: var(--color-muted); position: relative; }nav a:hover,nav .nav-active { color: var(--color-text); }.nav-active::after { position: absolute; content: ''; height: 2px; width: 18px; bottom: 22px; left: 0; background: var(--color-accent); }
  .header-end { margin-left: auto; display: flex; align-items: center; gap: 24px; }.local-label { display: flex; align-items: center; gap: 8px; color: #bbb7c9; font-size: 11px; }.status-dot { display: inline-block; width: 5px; height: 5px; flex: 0 0 5px; border-radius: 50%; background: var(--color-accent); }.edition-mark { font-size: 8px; letter-spacing: .18em; color: var(--color-subtle); border-left: 1px solid #ffffff18; padding-left: 24px; }
  .hero { position: relative; min-height: 570px; display: flex; align-items: center; }.hero-art,.hero-shade { position: absolute; inset: 0; }.hero-art { background: url('/images/cinematic-horizon.png') center 58% / cover; }.hero-shade { background: linear-gradient(90deg,#09090ef2 0%,#09090e85 36%,#09090e00 68%),linear-gradient(0deg,var(--color-canvas),transparent 30%); }.hero-content { position: relative; z-index: 1; padding: 20px var(--page-gutter) 76px; width: 100%; }.hero .eyebrow { display: flex; align-items: center; gap: 12px; font-size: 9px; }.eyebrow-line { height: 1px; width: 24px; background: var(--color-accent); }
  h1 { font-family: var(--font-display); font-size: clamp(54px,5.5vw,84px); font-weight: 550; line-height: 1.05; letter-spacing: -.065em; margin: 26px 0 22px; }h1 em { font-family: Georgia,serif; font-weight: 400; color: #cfc1fa; letter-spacing: -.07em; }.hero-copy { color: #b4afbf; font-size: 14px; line-height: 1.8; }.hero-actions { display: flex; align-items: center; gap: 22px; margin-top: 30px; }.hero-cta { display: inline-flex; align-items: center; justify-content: center; gap: 10px; background: var(--color-text); color: #15131b; padding: 14px 22px; min-height: 46px; border-radius: var(--radius-sm); font-size: 12px; font-weight: 650; }.hero-cta:hover { background: white; transform: translateY(-2px); }.hero-note { font-size: 10px; color: #a9a1b4; }.hero-specs { display: flex; gap: 7px; align-items: center; margin-top: 26px; }.hero-specs>span { font-size: 10px; color: #a49aad; margin-left: 7px; }.hero-caption { position: absolute; right: var(--page-gutter); bottom: 54px; text-align: right; display: grid; gap: 10px; }.art-index { font-size: 9px; letter-spacing: .2em; color: #e4d9ec; }.hero-caption>span:last-child { font-size: 9px; line-height: 1.6; color: #aba0b5; }
  .page-section { padding: 0 var(--page-gutter); max-width: 1600px; margin: 0 auto; }.editions { padding-top: 30px; }.section-heading { display: flex; justify-content: space-between; align-items: flex-end; gap: 24px; margin-bottom: 26px; }.section-heading .eyebrow { font-size: 9px; }h2 { font-family: var(--font-display); font-size: clamp(24px,2.4vw,34px); font-weight: 500; letter-spacing: -.035em; margin: 10px 0 0; }.section-aside { text-align: right; font-size: 11px; line-height: 1.8; color: #cbc6d4; margin: 0 0 2px; }.section-aside span { color: var(--color-subtle); }.edition-grid { display: grid; grid-template-columns: 1fr 1fr .85fr; gap: 24px; }.edition-detail { padding: 15px 14px 15px 24px; }.detail-orbit { color: #b8a7df; }.edition-detail h3 { font-size: 16px; font-weight: 500; letter-spacing: -.02em; margin: 15px 0 10px; }.edition-detail p { font-size: 11px; color: var(--color-muted); line-height: 1.8; max-width: 250px; margin: 0; }.edition-detail a { display: inline-flex; align-items: center; gap: 20px; margin-top: 20px; font-size: 10px; color: #c6b5f0; }.edition-detail a:hover { color: white; }.catalogue-message { padding: 40px; background: var(--color-surface); color: var(--color-muted); border-radius: 12px; grid-column: span 2; }
  .closing-note { display: flex; align-items: center; justify-content: center; gap: 30px; text-align: center; margin: 74px var(--page-gutter) 64px; }.closing-note p { font-size: 14px; line-height: 1.8; font-weight: 400; margin: 0; color: #dbd5e5; }.closing-note p span { color: var(--color-subtle); font-size: 12px; }.closing-rule { width: 90px; height: 1px; background: linear-gradient(90deg,transparent,#ffffff20,transparent); }footer { display: flex; align-items: center; justify-content: space-between; gap: 24px; padding: 30px var(--page-gutter); border-top: 1px solid var(--color-line); }.footer-brand { font-size: 25px; letter-spacing: -.065em; color: #b6b0c2; }.footer-brand span { display: block; margin-top: 5px; font-size: 6px; letter-spacing: .2em; color: var(--color-subtle); }footer p { font-size: 9px; line-height: 1.8; color: #b1a9c0; }footer p span { color: var(--color-subtle); }.back-top { display: flex; gap: 15px; align-items: center; font-size: 9px; color: #a59bb8; }.back-top:hover { color: white; }

  .home-error { margin: 30px var(--page-gutter); padding: 18px; background: #301c23; color: var(--color-danger); border-radius: 10px; font-size: 13px; }
  @media(min-width:1600px) { .hero-content { max-width: 1600px; margin: auto; }.hero { min-height: 640px; } }
  @media(max-width:1050px) { .hero { min-height: 550px; }.hero-note { display: none; }.hero-caption { bottom: 30px; }.edition-grid { grid-template-columns: 1fr 1fr; }.edition-detail { display: none; } }
  @media(max-width:800px) { .site-header { gap: 34px; height: 76px; }.edition-mark { display: none; }nav { gap: 20px; }.hero { min-height: 540px; }.hero-art { background-position: 62% center; }.hero-shade { background: linear-gradient(90deg,#09090edb, #09090e25),linear-gradient(0deg,var(--color-canvas),transparent 50%); }.hero-content { padding-bottom: 70px; }.hero-caption { bottom: 10px; }.hero-caption .art-index { display: none; }.section-aside { font-size: 10px; }footer .back-top { display: none; } }
  @media(max-width:550px) { .site-header { height: 72px; gap: 0; }.brand span { font-size: 27px; }.brand img { width: 22px; height: 26px; }nav { margin-left: auto; gap: 22px; }nav a { font-size: 11px; }.header-end { display: none; }.nav-active::after { bottom: 17px; }.hero { min-height: 570px; align-items: flex-start; }.hero-art { background-position: 64% center; }.hero-shade { background: linear-gradient(90deg,#09090eaf,#09090e10),linear-gradient(0deg,var(--color-canvas),#09090e10 65%); }.hero-content { padding-top: 72px; padding-bottom: 40px; }.hero .eyebrow { font-size: 7px; letter-spacing: .18em; }h1 { font-size: clamp(47px,12vw,66px); margin-top: 24px; margin-bottom: 22px; line-height: 1.08; }.hero-copy { font-size: 12px; color: #d2cbd8; line-height: 1.9; }.hero-actions { margin-top: 28px; }.hero-cta { padding: 13px 19px; font-size: 11px; }.hero-specs { margin-top: 24px; }.hero-specs>span { font-size: 8px; }.hero-caption { right: 20px; bottom: 20px; }.hero-caption>span:last-child { font-size: 8px; }.editions { padding-top: 18px; }.section-heading { align-items: flex-start; margin-bottom: 22px; }.section-heading .eyebrow { font-size: 8px; }h2 { font-size: 24px; max-width: 260px; line-height: 1.15; }.section-aside { display: none; }.edition-grid { grid-template-columns: 1fr; gap: 10px; }.closing-note { margin: 52px 20px; gap: 15px; }.closing-rule { width: 36px; }.closing-note p { font-size: 12px; }.closing-note p span { font-size: 10px; }footer { align-items: flex-start; padding-top: 26px; padding-bottom: 26px; }footer p { font-size: 8px; max-width: 200px; margin-top: 0; text-align: right; }.footer-brand { font-size: 23px; } }
</style>
