/** Close a details dropdown on an outside pointer press or Escape. */
export function dismissibleDetails(node: HTMLDetailsElement) {
  const outside = (event: PointerEvent) => {
    if (node.open && !event.composedPath().includes(node)) node.open = false;
  };
  const escape = (event: KeyboardEvent) => {
    if (event.key === 'Escape' && node.open) {
      node.open = false;
      node.querySelector('summary')?.focus();
    }
  };
  document.addEventListener('pointerdown', outside, true);
  document.addEventListener('keydown', escape, true);
  return {
    destroy() {
      document.removeEventListener('pointerdown', outside, true);
      document.removeEventListener('keydown', escape, true);
    }
  };
}
