// Évite de rogner une illustration quand son format ne correspond pas à son cadre
// (affiche 2:3 utilisée en secours dans un cadre 16:9, vignette carrée, etc.).
// Si `object-fit: cover` masquerait plus que `threshold` de l'image, l'image passe
// en `contain` et une copie floutée remplit le cadre derrière elle.

export function cropFraction(imageRatio: number, boxRatio: number): number {
  if (!(imageRatio > 0) || !(boxRatio > 0)) return 0;
  return 1 - Math.min(imageRatio, boxRatio) / Math.max(imageRatio, boxRatio);
}

type Options = { threshold?: number } | undefined;

export function fitArtwork(node: HTMLImageElement, options: Options = undefined) {
  let threshold = options?.threshold ?? 0.3;
  let fill: HTMLImageElement | null = null;

  // La copie reprend les classes de l'image (styles scopés, état actif d'un fondu).
  const syncClass = () => { if (fill) fill.className = `${node.className} artwork-fill`; };
  const classes = typeof MutationObserver === 'undefined' ? null : new MutationObserver(syncClass);
  classes?.observe(node, { attributes: true, attributeFilter: ['class'] });

  let promoted = false;
  const removeFill = () => {
    fill?.remove();
    fill = null;
    delete node.dataset.artworkFit;
    if (promoted) { node.style.position = ''; promoted = false; }
  };

  const evaluate = () => {
    const { naturalWidth, naturalHeight, clientWidth, clientHeight } = node;
    if (!naturalWidth || !naturalHeight || !clientWidth || !clientHeight) return;
    const fraction = cropFraction(naturalWidth / naturalHeight, clientWidth / clientHeight);
    if (fraction <= threshold) { removeFill(); return; }
    node.dataset.artworkFit = 'contain';
    if (!fill) {
      fill = document.createElement('img');
      fill.alt = '';
      fill.setAttribute('aria-hidden', 'true');
      fill.decoding = 'async';
      node.before(fill);
    }
    syncClass();
    // Image dans le flux : la copie se place dessous, en absolu dans le même cadre.
    if (!promoted && getComputedStyle(node).position === 'static') {
      node.style.position = 'relative';
      promoted = true;
    }
    if (promoted) Object.assign(fill.style, { position: 'absolute', inset: '0', width: '100%', height: '100%' });
    if (fill.src !== node.currentSrc) fill.src = node.currentSrc || node.src;
  };

  const onLoad = () => evaluate();
  node.addEventListener('load', onLoad);
  if (node.complete) evaluate();
  const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver(() => evaluate());
  observer?.observe(node);

  return {
    update(next: Options) { threshold = next?.threshold ?? 0.3; evaluate(); },
    destroy() {
      node.removeEventListener('load', onLoad);
      observer?.disconnect();
      classes?.disconnect();
      removeFill();
    }
  };
}
