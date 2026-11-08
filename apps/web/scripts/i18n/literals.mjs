/* eslint-disable @typescript-eslint/ban-ts-comment */
// @ts-nocheck -- build tooling parsed with the Svelte and TypeScript compilers, not application code.
// Finds user-visible text written directly in Svelte templates (text nodes and accessibility /
// label attributes). Shared by the migration tooling and by the hardcoded-text guard test.
import { parse } from 'svelte/compiler';

/** Attributes whose static value is shown to people or read by assistive technology. */
export const TEXT_ATTRIBUTES = new Set([
  'aria-label', 'aria-description', 'aria-placeholder', 'aria-roledescription', 'aria-valuetext',
  'title', 'placeholder', 'alt', 'label', 'heading', 'subtitle', 'eyebrow', 'caption', 'hint', 'legend',
  'emptyText', 'empty', 'message', 'summary', 'description', 'text', 'tooltip', 'ariaLabel', 'buttonLabel'
]);
const SKIPPED_ELEMENTS = new Set(['script', 'style', 'pre', 'code', 'kbd', 'samp']);

const letters = text => (text.match(/\p{L}/gu) ?? []).length;

/** True when `text` (placeholders removed) reads as prose rather than a symbol, number or code. */
export function isProse(text) {
  const bare = text.replace(/\{[A-Za-z0-9_]+\}/g, '').trim();
  const count = letters(bare);
  if (count < 2) return false;
  if (/^[A-Z0-9_.\-/:+ ]+$/.test(bare) && count <= 5) return false;
  if (/^[a-z]{1,3}$/.test(bare)) return false;
  if (/^(https?:|\/|#|\.|[a-z-]+:)/.test(bare) && !/\s/.test(bare)) return false;
  return count >= 3 || /\s/.test(bare);
}

const isSimpleExpression = source => source.length <= 80 && !/[`'"{}]/.test(source) && !/=>|\bfunction\b|\?\s|\|\||&&/.test(source);

function paramName(source, used) {
  const chain = /^[A-Za-z_$][\w$]*(\??\.[A-Za-z_$][\w$]*)*$/.test(source);
  const tail = chain ? source.replace(/\?\./g, '.').split('.').pop() : '';
  let base = tail && tail !== 'length' ? tail : tail === 'length' ? 'count' : 'value';
  let name = base;
  for (let n = 2; used.has(name); n += 1) name = `${base}${n}`;
  used.add(name);
  return name;
}

/**
 * @typedef {{ start: number, end: number, text: string, params: Record<string,string>, kind: 'text'|'attr', attr?: string, line: number }} Literal
 * @returns {{ literals: Literal[], skipped: { line: number, reason: string, source: string }[], ast: any }}
 */
export function findTemplateLiterals(source) {
  const ast = parse(source, { modern: true });
  const literals = [];
  const skipped = [];
  const lineOf = offset => source.slice(0, offset).split('\n').length;

  function collectRun(nodes, from) {
    let to = from;
    while (to < nodes.length && (nodes[to].type === 'Text' || nodes[to].type === 'ExpressionTag')) to += 1;
    return to;
  }

  function handleRun(nodes, from, to) {
    const run = nodes.slice(from, to);
    if (!run.some(node => node.type === 'Text' && isProse(node.data.replace(/\s+/g, ' ')))) {
      // A run of expressions plus punctuation (`{a} / {b}`) is not prose.
      const prose = run.some(node => node.type === 'Text' && letters(node.data) >= 2);
      if (prose) skipped.push({ line: lineOf(run[0].start), reason: 'short text', source: source.slice(run[0].start, run.at(-1).end).trim() });
      return;
    }
    const used = new Set();
    const params = {};
    let text = '';
    for (const node of run) {
      if (node.type === 'Text') text += node.data;
      else {
        const expression = source.slice(node.expression.start, node.expression.end);
        if (!isSimpleExpression(expression)) {
          skipped.push({ line: lineOf(node.start), reason: 'complex expression', source: source.slice(run[0].start, run.at(-1).end).trim().slice(0, 120) });
          return;
        }
        const name = paramName(expression, used);
        params[name] = expression;
        text += `{${name}}`;
      }
    }
    const first = run[0];
    const last = run.at(-1);
    const lead = first.type === 'Text' ? (first.raw.match(/^\s*/)?.[0].length ?? 0) : 0;
    const trail = last.type === 'Text' ? (last.raw.match(/\s*$/)?.[0].length ?? 0) : 0;
    const start = first.start + lead;
    const end = last.end - trail;
    if (end <= start) return;
    literals.push({ start, end, text: text.replace(/\s+/g, ' ').trim(), params, kind: 'text', line: lineOf(start) });
  }

  function handleAttribute(attribute, elementName) {
    if (attribute.type !== 'Attribute' || !TEXT_ATTRIBUTES.has(attribute.name)) return;
    // `title`, `label`... on components are props: only treat them as text when clearly prose.
    const value = attribute.value;
    if (value === true) return;
    const parts = Array.isArray(value) ? value : [value];
    if (parts.length === 1 && parts[0].type === 'ExpressionTag') {
      const expression = parts[0].expression;
      const raw = source.slice(expression.start, expression.end);
      if (expression.type === 'Literal' && typeof expression.value === 'string' && isProse(expression.value)) {
        literals.push({ start: attribute.start, end: attribute.end, text: expression.value, params: {}, kind: 'attr', attr: attribute.name, expressionForm: true, line: lineOf(attribute.start) });
      } else if (expression.type === 'TemplateLiteral') {
        const used = new Set();
        const params = {};
        let text = '';
        let ok = true;
        expression.quasis.forEach((quasi, index) => {
          text += quasi.value.cooked;
          const piece = expression.expressions[index];
          if (!piece) return;
          const code = source.slice(piece.start, piece.end);
          if (!isSimpleExpression(code)) { ok = false; return; }
          const name = paramName(code, used);
          params[name] = code;
          text += `{${name}}`;
        });
        if (ok && isProse(text)) literals.push({ start: attribute.start, end: attribute.end, text, params, kind: 'attr', attr: attribute.name, expressionForm: true, line: lineOf(attribute.start) });
        else if (!ok) skipped.push({ line: lineOf(attribute.start), reason: 'complex attribute template', source: raw.slice(0, 120) });
      }
      return;
    }
    const used = new Set();
    const params = {};
    let text = '';
    for (const part of parts) {
      if (part.type === 'Text') text += part.data;
      else {
        const code = source.slice(part.expression.start, part.expression.end);
        if (!isSimpleExpression(code)) { skipped.push({ line: lineOf(attribute.start), reason: 'complex attribute expression', source: source.slice(attribute.start, attribute.end).slice(0, 120) }); return; }
        const name = paramName(code, used);
        params[name] = code;
        text += `{${name}}`;
      }
    }
    if (!isProse(text)) return;
    if (/^[A-Z]/.test(elementName) && !/\s/.test(text.trim()) && !/[À-ÿ]/.test(text)) return; // component prop such as label="Films"
    literals.push({ start: attribute.start, end: attribute.end, text: text.replace(/\s+/g, ' ').trim(), params, kind: 'attr', attr: attribute.name, line: lineOf(attribute.start) });
  }

  function walk(node, parentName) {
    if (!node || typeof node !== 'object') return;
    if (Array.isArray(node)) { node.forEach(child => walk(child, parentName)); return; }
    const name = node.name ?? parentName;
    if (node.type === 'RegularElement' || node.type === 'Component' || node.type === 'SvelteComponent' || node.type === 'SvelteElement') {
      for (const attribute of node.attributes ?? []) handleAttribute(attribute, node.name ?? '');
    }
    if (Array.isArray(node.nodes)) {
      if (!SKIPPED_ELEMENTS.has(parentName)) {
        let index = 0;
        while (index < node.nodes.length) {
          const kind = node.nodes[index].type;
          if (kind === 'Text' || kind === 'ExpressionTag') {
            const to = collectRun(node.nodes, index);
            handleRun(node.nodes, index, to);
            index = to;
          } else index += 1;
        }
      }
    }
    for (const [key, value] of Object.entries(node)) {
      if (key === 'type' || key === 'start' || key === 'end' || key === 'expression' || key === 'metadata') continue;
      if (value && typeof value === 'object') walk(value, key === 'fragment' || key === 'nodes' ? name : name);
    }
  }
  walk(ast.fragment, '');
  return { literals, skipped, ast };
}
