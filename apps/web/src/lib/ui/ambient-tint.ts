/** Teinte d'ambiance Halo : couleur dominante de l'image mise en avant (calcul local, aucun appel réseau). */

const SAMPLE_WIDTH = 16;
const SAMPLE_HEIGHT = 9;
// Une moyenne d'image de cinéma est souvent très sombre : on remonte la luminosité
// en gardant la teinte, sinon la couche d'ambiance resterait invisible.
const MIN_PEAK = 150;

/** Moyenne pondérée de pixels RGBA ; les pixels saturés et lumineux comptent davantage. */
export function averageTint(data: ArrayLike<number>): string | null {
  let red = 0;
  let green = 0;
  let blue = 0;
  let weight = 0;
  for (let index = 0; index + 3 < data.length; index += 4) {
    if (data[index + 3] < 128) continue;
    const r = data[index];
    const g = data[index + 1];
    const b = data[index + 2];
    const max = Math.max(r, g, b);
    const min = Math.min(r, g, b);
    // Bandes noires et ciels gris pèsent peu ; une couleur franche pèse plus.
    const pixelWeight = 0.1 + (max - min) / 255 + (max > 32 ? 0.15 : 0);
    red += r * pixelWeight;
    green += g * pixelWeight;
    blue += b * pixelWeight;
    weight += pixelWeight;
  }
  if (weight === 0) return null;
  let r = red / weight;
  let g = green / weight;
  let b = blue / weight;
  const peak = Math.max(r, g, b);
  if (peak < 1) return null;
  if (peak < MIN_PEAK) {
    const gain = MIN_PEAK / peak;
    r *= gain;
    g *= gain;
    b *= gain;
  }
  const channel = (value: number) => Math.min(255, Math.max(0, Math.round(value)));
  return `rgb(${channel(r)} ${channel(g)} ${channel(b)})`;
}

/**
 * Lit une image déjà chargée (même origine) dans un petit canevas.
 * Image d'une autre origine (canevas « contaminé ») ou toute autre erreur : pas de teinte.
 */
export function imageTint(image: HTMLImageElement): string | null {
  try {
    if (!image.complete || image.naturalWidth === 0) return null;
    const canvas = document.createElement('canvas');
    canvas.width = SAMPLE_WIDTH;
    canvas.height = SAMPLE_HEIGHT;
    const context = canvas.getContext('2d', { willReadFrequently: true });
    if (!context) return null;
    context.drawImage(image, 0, 0, SAMPLE_WIDTH, SAMPLE_HEIGHT);
    return averageTint(context.getImageData(0, 0, SAMPLE_WIDTH, SAMPLE_HEIGHT).data);
  } catch {
    return null;
  }
}
