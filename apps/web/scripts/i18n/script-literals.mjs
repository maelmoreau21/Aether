/* eslint-disable @typescript-eslint/ban-ts-comment */
// @ts-nocheck -- build tooling parsed with the Svelte and TypeScript compilers, not application code.
// Finds user-visible sentences written as string literals in TypeScript and in Svelte `<script>`
// blocks. Shared by the migration tooling and by the hardcoded-text guard test.
import ts from 'typescript';
import { parse } from 'svelte/compiler';

const letters = text => (text.match(/\p{L}/gu) ?? []).length;

/** A sentence a person reads: several words, or accented/uppercase prose with punctuation. */
export function isSentence(text) {
  const bare = text.replace(/\{[A-Za-z0-9_]+\}/g, '').trim();
  if (letters(bare) < 3) return false;
  if (/^[a-z0-9_.:/#?&=%\-[\]()'"`*+,|\\]+$/i.test(bare) && !/\s/.test(bare)) return false; // identifiers, paths, mime types
  if (/^(https?:|\/|\.{1,2}\/|#|[a-z-]+:\/\/)/.test(bare)) return false;
  if (/^[a-z][a-z0-9]*([ ][a-z0-9-]+)*$/.test(bare) && !/[À-ÿ]/.test(bare) && bare.split(' ').length <= 3) return false; // css classes, short lowercase tokens
  if (/[<>]|=>|\bfunction\b|\bconst\b|^\s*[{[]/.test(bare)) return false;
  const words = bare.split(/\s+/).length;
  if (words >= 2 && /^[A-ZÀ-Ý«“"']/.test(bare)) return true;
  if (words >= 3) return /[À-ÿ]/.test(bare) || /^[A-Z]/.test(bare);
  return /[À-ÿ]/.test(bare) && /\s/.test(bare);
}

const SKIPPED_PARENTS = new Set([
  ts.SyntaxKind.ImportDeclaration, ts.SyntaxKind.ExportDeclaration, ts.SyntaxKind.LiteralType, ts.SyntaxKind.CaseClause,
  ts.SyntaxKind.ElementAccessExpression, ts.SyntaxKind.ImportType, ts.SyntaxKind.ExternalModuleReference
]);

const isFunctionLike = node => ts.isArrowFunction(node) || ts.isFunctionDeclaration(node) || ts.isFunctionExpression(node)
  || ts.isMethodDeclaration(node) || ts.isGetAccessor(node) || ts.isSetAccessor(node) || ts.isConstructorDeclaration(node);

function simpleChain(node, sf) {
  const source = node.getText(sf);
  return source.length <= 80 && !/[`'"{}]/.test(source) && !/=>|\bfunction\b|\?\s|\|\||&&/.test(source);
}

function paramName(source, used) {
  const chain = /^[A-Za-z_$][\w$]*(\??\.[A-Za-z_$][\w$]*)*$/.test(source);
  const tail = chain ? source.replace(/\?\./g, '.').split('.').pop() : '';
  const base = tail && tail !== 'length' ? tail : tail === 'length' ? 'count' : 'value';
  let name = base;
  for (let n = 2; used.has(name); n += 1) name = `${base}${n}`;
  used.add(name);
  return name;
}

/**
 * @param {string} code TypeScript source
 * @param {number} offset position of `code` inside the original file (Svelte script blocks)
 */
export function findScriptLiterals(code, offset = 0, fileName = 'x.ts') {
  const sf = ts.createSourceFile(fileName, code, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const found = [];
  const skipped = [];

  function context(node) {
    let inFunction = false;
    let parent = node.parent;
    let statement = null;
    while (parent) {
      if (!statement && (ts.isReturnStatement(parent) || ts.isThrowStatement(parent) || ts.isVariableDeclaration(parent) || ts.isPropertyAssignment(parent) || ts.isCallExpression(parent) || ts.isNewExpression(parent) || ts.isBinaryExpression(parent))) statement = parent;
      if (isFunctionLike(parent)) inFunction = true;
      parent = parent.parent;
    }
    return { inFunction, statement };
  }

  function visit(node) {
    if (ts.isStringLiteral(node) || ts.isNoSubstitutionTemplateLiteral(node) || ts.isTemplateExpression(node)) {
      const parent = node.parent;
      if (!parent || SKIPPED_PARENTS.has(parent.kind)) return;
      if (ts.isPropertyAssignment(parent) && parent.name === node) return;
      if (ts.isBinaryExpression(parent) && [ts.SyntaxKind.EqualsEqualsEqualsToken, ts.SyntaxKind.ExclamationEqualsEqualsToken, ts.SyntaxKind.EqualsEqualsToken, ts.SyntaxKind.ExclamationEqualsToken].includes(parent.operatorToken.kind)) return;
      if (ts.isCallExpression(parent) && /^(console|JSON|Object|Symbol|document|window|localStorage|sessionStorage)\b/.test(parent.expression.getText(sf)) ) return;
      // Probes and lookups, not text shown to people.
      if (ts.isCallExpression(parent) && ts.isPropertyAccessExpression(parent.expression)
        && /^(includes|startsWith|endsWith|indexOf|lastIndexOf|test|match|matchAll|replace|replaceAll|split|has|get|set|delete|add|localeCompare|querySelector|querySelectorAll|getAttribute|setAttribute|closest|matches|addEventListener|removeEventListener|dispatchEvent|getItem|setItem|removeItem|append|toLocaleString|toLocaleDateString|toLocaleTimeString|DateTimeFormat|NumberFormat)$/.test(parent.expression.name.text)) return;
      // Already translated: `$t('key')` / `tr('key')` / `t('key')`.
      if (ts.isCallExpression(parent) && /^\$?(t|tr|st)$/.test(parent.expression.getText(sf))) return;

      let text;
      const params = {};
      if (ts.isTemplateExpression(node)) {
        const used = new Set();
        text = node.head.text;
        let ok = true;
        for (const span of node.templateSpans) {
          if (!simpleChain(span.expression, sf)) { ok = false; break; }
          const source = span.expression.getText(sf);
          const name = paramName(source, used);
          params[name] = source;
          text += `{${name}}${span.literal.text}`;
        }
        if (!ok) {
          const probe = node.getText(sf).replace(/\$\{[^}]*\}/g, ' ');
          if (isSentence(probe.slice(1, -1))) skipped.push({ start: node.getStart(sf) + offset, line: sf.getLineAndCharacterOfPosition(node.getStart(sf)).line + 1, reason: 'complex template', text: node.getText(sf).slice(0, 120) });
          return;
        }
      } else text = node.text;
      if (!isSentence(text)) return;
      const { inFunction } = context(node);
      const start = node.getStart(sf);
      found.push({
        start: start + offset, end: node.getEnd() + offset, text, params, inFunction,
        line: sf.getLineAndCharacterOfPosition(start).line + 1, parentKind: ts.SyntaxKind[parent.kind],
        property: ts.isPropertyAssignment(parent) ? parent.name.getText(sf) : undefined
      });
      return;
    }
    ts.forEachChild(node, visit);
  }
  visit(sf);
  return { found, skipped };
}

/** Literals in a `.svelte` file's script blocks (instance and module). */
export function findSvelteScriptLiterals(source) {
  const ast = parse(source, { modern: true });
  const out = { found: [], skipped: [] };
  for (const script of [ast.instance, ast.module]) {
    if (!script) continue;
    const start = script.content.start;
    const code = source.slice(start, script.content.end);
    const { found, skipped } = findScriptLiterals(code, start, 'x.ts');
    const lineOffset = source.slice(0, start).split('\n').length - 1;
    for (const item of found) { item.line += lineOffset; item.inModuleScript = script === ast.module; }
    for (const item of skipped) item.line += lineOffset;
    out.found.push(...found); out.skipped.push(...skipped);
  }
  return out;
}

const SVELTE_NODE_TYPES = new Set([
  'Root', 'Fragment', 'Text', 'Comment', 'ExpressionTag', 'HtmlTag', 'ConstTag', 'DebugTag', 'RenderTag', 'AttachTag',
  'RegularElement', 'Component', 'SvelteComponent', 'SvelteElement', 'SvelteSelf', 'SvelteFragment', 'SvelteHead',
  'SvelteWindow', 'SvelteDocument', 'SvelteBody', 'SvelteBoundary', 'TitleElement', 'SlotElement', 'SnippetBlock',
  'IfBlock', 'EachBlock', 'AwaitBlock', 'KeyBlock', 'Attribute', 'SpreadAttribute', 'BindDirective', 'ClassDirective',
  'StyleDirective', 'OnDirective', 'UseDirective', 'AnimateDirective', 'TransitionDirective', 'LetDirective', 'Script',
  'SvelteOptions'
]);

/**
 * Sentences inside template expressions: `{cond ? 'Oui' : 'Non'}`, `placeholder={x ? 'A…' : 'B…'}`.
 * Markup is re-evaluated when `$t` changes, so these can always use the store.
 */
export function findSvelteTemplateExpressionLiterals(source) {
  const ast = parse(source, { modern: true });
  const found = [];
  const skipped = [];
  const seen = new Set();
  function visitJs(node) {
    if (seen.has(node.start)) return;
    seen.add(node.start);
    const code = `(${source.slice(node.start, node.end)})`;
    const result = findScriptLiterals(code, node.start - 1, 'x.ts');
    const lineOffset = source.slice(0, node.start).split('\n').length - 1;
    for (const item of result.found) { found.push({ ...item, inFunction: true, line: item.line + lineOffset, inTemplate: true }); }
    for (const item of result.skipped) skipped.push({ ...item, line: item.line + lineOffset });
  }
  function walk(node) {
    if (!node || typeof node !== 'object') return;
    if (Array.isArray(node)) { node.forEach(walk); return; }
    if (typeof node.type === 'string' && !SVELTE_NODE_TYPES.has(node.type)) { visitJs(node); return; }
    for (const [key, value] of Object.entries(node)) {
      if (key === 'metadata' || key === 'type') continue;
      if (value && typeof value === 'object') walk(value);
    }
  }
  walk(ast.fragment);
  return { found, skipped };
}
