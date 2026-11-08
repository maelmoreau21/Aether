// Adapte le cadre d'une illustration à son ratio réel au lieu de remplir les
// bords avec une copie floutée (letterbox / pillarbox) : dès que l'image est
// décodée, son ratio natif est publié sur le cadre (`--artwork-ratio`) avec une
// forme (`data-artwork-shape`). Les cadres dont l'`aspect-ratio` suit cette
// variable épousent alors l'image, qui remplit bord à bord en `cover`.

export type ArtworkShape = 'portrait' | 'square' | 'landscape' | 'wide';

export function cropFraction(imageRatio: number, boxRatio: number): number {
  if (!(imageRatio > 0) || !(boxRatio > 0)) return 0;
  return 1 - Math.min(imageRatio, boxRatio) / Math.max(imageRatio, boxRatio);
}

/** Paliers : affiche 2:3, album 1:1, fond/vignette 16:9 (et 4:3), cinéma 2.39:1. */
export function artworkShape(ratio: number): ArtworkShape | null {
  if (!(ratio > 0) || !Number.isFinite(ratio)) return null;
  if (ratio < 0.85) return 'portrait';
  if (ratio <= 1.18) return 'square';
  if (ratio <= 2) return 'landscape';
  return 'wide';
}

/** Ratio CSS borné pour éviter qu'une image aberrante (bandeau 10:1) casse la mise en page. */
export function artworkRatio(width: number, height: number): string | null {
  if (!(width > 0) || !(height > 0)) return null;
  const ratio = Math.min(Math.max(width / height, 0.5), 2.4);
  return `${Math.round(ratio * 10000) / 10000}`;
}

type Options = { frame?: HTMLElement | null; adapt?: boolean } | undefined;

export function fitArtwork(node: HTMLImageElement, options: Options = undefined) {
  let adapt = options?.adapt ?? true;
  let frame = options?.frame ?? node.parentElement;
  node.dataset.artworkFitted = '';

  const clear = () => {
    if (!frame) return;
    frame.style.removeProperty('--artwork-ratio');
    delete frame.dataset.artworkShape;
    delete frame.dataset.artworkAdapt;
  };

  const evaluate = () => {
    const { naturalWidth, naturalHeight } = node;
    const ratio = artworkRatio(naturalWidth, naturalHeight);
    const shape = artworkShape(naturalWidth / naturalHeight);
    if (!frame || !ratio || !shape) return;
    frame.style.setProperty('--artwork-ratio', ratio);
    frame.dataset.artworkShape = shape;
    if (adapt) frame.dataset.artworkAdapt = '';
    else delete frame.dataset.artworkAdapt;
  };

  const onLoad = () => evaluate();
  node.addEventListener('load', onLoad);
  if (node.complete) evaluate();

  return {
    update(next: Options) {
      clear();
      adapt = next?.adapt ?? true;
      frame = next?.frame ?? node.parentElement;
      evaluate();
    },
    destroy() {
      node.removeEventListener('load', onLoad);
      clear();
      delete node.dataset.artworkFitted;
    }
  };
}
