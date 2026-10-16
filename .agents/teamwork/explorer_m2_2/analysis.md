# Milestone 2: Node SvelteKit Frontend Build Stage Analysis

**Author**: Milestone 2 Explorer 2  
**Date**: 2026-10-16  
**Status**: Completed  
**Target Component**: Multi-Stage Dockerfile — Stage 1 (`web-builder`)

---

## 1. Executive Summary

This report provides the architectural investigation, empirical verification, and concrete specification for Stage 1 (`web-builder`) of the multi-stage `Dockerfile` for the Aether project.

Stage 1 is responsible for compiling the SvelteKit frontend into pre-rendered, completely static web assets and externalizing inline bootstrap scripts for strict Content Security Policy (CSP) compliance.

### Key Conclusions:
1. **Base Image**: **`node:22-bookworm-slim`** is the optimal base image. It provides native glibc compatibility with precompiled Linux x86_64 binaries (`@rolldown/binding-linux-x64-gnu` and `lightningcss-linux-x64-gnu`), avoids Alpine musl allocator/stack quirks, aligns with the Debian Bookworm runtime image (Stage 3), and imposes zero penalty on the final container image size.
2. **Layer Caching**: Segregate `apps/web/package.json` and `apps/web/package-lock.json` copying before `npm ci` so that dependencies are cached across frontend source code edits.
3. **Build Execution**: `npm run build` runs `vite build && node scripts/externalize-bootstrap.mjs`, producing a fully pre-rendered static site under `apps/web/build`.
4. **Zero Node.js Runtime**: Empirical inspection of `apps/web/build` confirms that the build output consists of 45 static files (HTML, JS, CSS, SVG, PNG, JSON; total 2.2 MB) requiring **zero Node.js runtime, zero V8 engine, and zero server-side Node processes** at container runtime. All assets are served directly by Axum's asynchronous `ServeDir` service.
5. **Critical Pitfall**: In `apps/web/package.json`, all build dependencies are defined under `devDependencies`. Setting `NODE_ENV=production` before `npm ci` would instruct npm to omit devDependencies, breaking the build. The Dockerfile must either omit `NODE_ENV=production` during dependency install or execute `npm ci` cleanly in default mode.

---

## 2. Codebase Inspection & Web Architecture

### 2.1 Package Configuration (`apps/web/package.json`)
```json
{
  "name": "media-web",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite --host 127.0.0.1",
    "check": "svelte-kit sync && svelte-check --tsconfig ./tsconfig.json",
    "build": "vite build && node scripts/externalize-bootstrap.mjs",
    "lint": "eslint src",
    "test": "vitest run"
  },
  "devDependencies": {
    "@opentelemetry/api": "^1.9.1",
    "@sveltejs/adapter-static": "4.0.0",
    "@sveltejs/kit": "3.1.0",
    "@sveltejs/vite-plugin-svelte": "7.3.1",
    "eslint": "^10.12.0",
    "eslint-plugin-svelte": "^3.23.1",
    "playwright": "^1.64.1",
    "svelte": "5.57.2",
    "svelte-check": "^4.7.6",
    "typescript": "^6.0.3",
    "typescript-eslint": "^8.71.2",
    "vite": "8.3.4",
    "vitest": "^5.0.5"
  }
}
```
**Observations**:
- There is no root `package.json` or root `package-lock.json`. The web application is strictly isolated in `apps/web/`.
- All dependencies are located under `devDependencies` because the web application compiles to pure static files; no Node runtime dependencies exist for production execution.
- Key toolchain components:
  - `@sveltejs/kit` v3.1.0 and `svelte` v5.57.2.
  - `@sveltejs/adapter-static` v4.0.0.
  - `vite` v8.3.4 (which incorporates `rolldown` ~1.2.12 and `lightningcss` ^1.33.0).

### 2.2 Lockfile Analysis (`apps/web/package-lock.json`)
- `lockfileVersion: 3` (standard modern npm format).
- Contains locked integrity hashes and registry URLs for all native precompiled platform packages, including:
  - `@rolldown/binding-linux-x64-gnu` (v1.2.14, lines 493–507)
  - `@rolldown/binding-linux-x64-musl` (v1.2.14, lines 510–524)
  - `lightningcss-linux-x64-gnu` (v1.33.0, lines 1974–1992)
  - `lightningcss-linux-x64-musl` (v1.33.0, lines 1995–2013)
- When `npm ci` executes in a Linux container on x86_64, npm automatically resolves and installs the matching `linux-x64-gnu` binaries without recompilation or node-gyp tooling.

### 2.3 Post-Build Script (`apps/web/scripts/externalize-bootstrap.mjs`)
```javascript
// Keep Axum's strict script-src 'self' without inline execution or unsafe-inline.
import { readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = fileURLToPath(new URL('../build/', import.meta.url));
for (const entry of readdirSync(root, { recursive: true, withFileTypes: true })) {
  if (!entry.isFile() || !entry.name.endsWith('.html')) continue;
  const file = join(entry.parentPath, entry.name);
  let count = 0;
  const html = readFileSync(file, 'utf8').replace(/<script>([\s\S]*?)<\/script>/g, (_, body) => {
    const name = `${entry.name.replace('.html', '')}-bootstrap-${count++}.js`;
    writeFileSync(join(dirname(file), name), body);
    return `<script src="./${name}"></script>`;
  });
  if (count !== 1) throw new Error(`Expected one SvelteKit bootstrap in ${file}, got ${count}`);
  writeFileSync(file, html);
}
```
**Function & Rationale**:
- SvelteKit generates inline `<script>` tags by default for client hydration bootstrap (`__sveltekit_... = { base: ... }; import(...)`).
- Aether's backend Axum server enforces strict CSP headers (`script-src 'self'`) forbidding inline scripts and `unsafe-inline`.
- This script parses every generated `.html` file in `build/`, extracts the inline script content into a standalone `<name>-bootstrap-0.js` file, and modifies the HTML to reference `<script src="./<name>-bootstrap-0.js"></script>`.
- **Runtime requirement**: It uses `entry.parentPath`, introduced in Node.js v20.1.0+. Node 22 fully supports this API.
- Because `npm run build` is configured as `"vite build && node scripts/externalize-bootstrap.mjs"`, executing `npm run build` during Docker build automatically executes this script.

### 2.4 SvelteKit Routing & Prerendering Configuration
- In `apps/web/vite.config.ts`:
  ```typescript
  sveltekit({
    adapter: adapter(),
    paths: { base: '/app' },
    csp: { mode: 'hash', directives: { 'script-src': ['self'] } }
  })
  ```
- In `apps/web/src/routes/+layout.ts`:
  ```typescript
  export const prerender = true;
  export const trailingSlash = 'always';
  ```
- **Consequence**: Every route (`/`, `/login`, `/films`, `/series`, `/search`, `/settings`, `/admin/libraries`) is fully pre-rendered at build time as an `index.html` file inside its respective subdirectory under `apps/web/build`.

---

## 3. Base Image Evaluation: `node:22-bookworm-slim` vs `node:22-alpine`

| Criterion | `node:22-bookworm-slim` | `node:22-alpine` | Evaluation / Verdict |
| :--- | :--- | :--- | :--- |
| **C Library / ABI** | **glibc 2.36** (Debian 12) | **musl libc** (Alpine 3.20/3.21) | **Winner: Bookworm-slim**. glibc is the standard reference ABI for `@rolldown/binding-linux-x64-gnu` and `lightningcss-linux-x64-gnu`. |
| **Compatibility & Stability** | 100% flawless compatibility. No DNS, thread stack, or native addon anomalies. | Potential edge cases with musl allocator and stack sizing during deep AST parsing. | **Winner: Bookworm-slim**. Guarantees zero native binding resolution failures. |
| **Stage Isolation & Image Size** | Download: ~75 MB, Disk: ~220 MB. | Download: ~55 MB, Disk: ~180 MB. | **Tie (Irrelevant)**. Stage 1 is an ephemeral build stage; its filesystem is discarded. Final image only receives `apps/web/build` (~2.2 MB). |
| **Ecosystem Uniformity** | Matches Stage 3 runtime (`debian:bookworm-slim`). | Divergent base OS from Stage 2 and Stage 3. | **Winner: Bookworm-slim**. Keeps the Debian Bookworm distribution family consistent across builders and runtime. |
| **Node Version** | Node v22.x LTS (npm v10+) | Node v22.x LTS (npm v10+) | Both satisfy Rolldown's requirement (`node: ^20.19.0 \|\| >=22.12.0`) and `entry.parentPath`. |

**Recommendation**: Use **`node:22-bookworm-slim`** (or `node:22-bookworm-slim AS web-builder`).

---

## 4. Layer Caching Architecture

### 4.1 Dependency Layer vs Source Layer
A critical requirement for developer experience and CI efficiency is Docker layer caching:
1. Manifests (`package.json`, `package-lock.json`) change infrequently (only when adding/updating dependencies).
2. Source code (`src/`, `static/`) changes frequently during feature development.

By structuring the build steps as:
```dockerfile
WORKDIR /app/apps/web

# Step 1: Copy ONLY manifests
COPY apps/web/package.json apps/web/package-lock.json ./

# Step 2: Install dependencies strictly against lockfile
RUN npm ci --no-audit --no-fund

# Step 3: Copy source files and configs
COPY apps/web/ ./

# Step 4: Execute build
RUN npm run build
```

When developers edit Svelte components or CSS, Steps 1 and 2 are 100% cache hits (`CACHED`), avoiding the 10–20 second `npm ci` overhead on subsequent container builds.

### 4.2 `.dockerignore` Safeguard
If `apps/web/node_modules` exists on the host machine (e.g. from local development on Windows or macOS), `COPY apps/web/ ./` could inadvertently overwrite the freshly installed Linux `node_modules` if `.dockerignore` is omitted or misconfigured.
To guarantee isolation:
- `.dockerignore` **MUST** include:
  ```dockerignore
  apps/web/node_modules
  apps/web/.svelte-kit
  apps/web/build
  ```
- Alternatively, the Dockerfile can explicitly copy only the required source directories:
  ```dockerfile
  COPY apps/web/vite.config.ts apps/web/tsconfig.json ./
  COPY apps/web/scripts ./scripts
  COPY apps/web/src ./src
  COPY apps/web/static ./static
  ```
  Explicit copying provides absolute immunity against host pollution even if `.dockerignore` is missing or modified. Both patterns are documented in Section 6.

---

## 5. Verification of Static Output & Zero Node.js Runtime Requirement

### 5.1 Verification Commands and Metrics
Local verification was executed on the current workspace:
```powershell
npm run build
Get-ChildItem -Recurse apps/web/build -File | Group-Object Extension -NoElement
Get-ChildItem -Recurse apps/web/build | Measure-Object -Property Length -Sum
```

**Results**:
- Exit code: `0`
- Total files: `45`
- Total uncompressed size: `2,201,721 bytes` (~2.2 MB)
- Extension breakdown:
  - `.js`: 29 files (client-side bundled chunks and externalized bootstrap scripts)
  - `.html`: 7 files (`index.html`, `login/index.html`, `films/index.html`, `series/index.html`, `search/index.html`, `settings/index.html`, `admin/libraries/index.html`)
  - `.css`: 5 files (scoped CSS and global theme tokens)
  - `.svg`: 2 files (`brand-mark.svg`, `chevron.svg`)
  - `.png`: 1 file (`cinematic-horizon.png`)
  - `.json`: 1 file (`_app/version.json`)

### 5.2 Verification of Zero Node.js Runtime
- **No Node binary in final container**: Stage 3 (`runtime`) does not install `nodejs` or `npm`.
- **No server script**: There is no `server.js` or `index.mjs` backend process. SvelteKit SSR server code is used only at build time by `@sveltejs/adapter-static` for static prerendering and is discarded.
- **Backend Serving**: The Axum Rust backend serves `apps/web/build` directly via `tower_http::services::ServeDir::new(configured_web_dist()).append_index_html_on_directories(true)`.
- **Runtime memory footprint**: Zero Node.js processes exist in memory. The entire application runs as a single Rust process (`media-poc`).

---

## 6. Concrete Recommendation & Dockerfile Snippet

### Recommended Stage 1 Snippet (Standard Idiomatic):

```dockerfile
# ==============================================================================
# Stage 1: SvelteKit Frontend Web Builder
# ==============================================================================
FROM node:22-bookworm-slim AS web-builder

WORKDIR /app/apps/web

# 1. Cache dependency installation layer
# Copy manifests first so npm ci is cached unless dependencies change
COPY apps/web/package.json apps/web/package-lock.json ./
RUN npm ci --no-audit --no-fund

# 2. Copy web application sources and configuration
COPY apps/web/ ./

# 3. Build pre-rendered static frontend and externalize bootstrap scripts for CSP
# Executes: vite build && node scripts/externalize-bootstrap.mjs
RUN npm run build
```

### Alternative Stage 1 Snippet (Strictly Isolated / Hardened against `.dockerignore` omissions):

```dockerfile
# ==============================================================================
# Stage 1: SvelteKit Frontend Web Builder (Hardened Explicit Copy)
# ==============================================================================
FROM node:22-bookworm-slim AS web-builder

WORKDIR /app/apps/web

# 1. Cache dependency installation layer
COPY apps/web/package.json apps/web/package-lock.json ./
RUN npm ci --no-audit --no-fund

# 2. Copy explicitly only required source files and build configurations
# This prevents host node_modules/.svelte-kit contamination even if .dockerignore fails
COPY apps/web/vite.config.ts apps/web/tsconfig.json ./
COPY apps/web/scripts ./scripts
COPY apps/web/src ./src
COPY apps/web/static ./static

# 3. Build pre-rendered static frontend and externalize bootstrap scripts for CSP
RUN npm run build
```

### Stage 3 Runtime Integration Snippet:

In Stage 3 (`runtime`), the compiled static directory is copied into the container filesystem:

```dockerfile
# Copy static web assets into container WORKDIR (/app)
COPY --from=web-builder /app/apps/web/build /app/apps/web/build
```

Because `crates/core/src/lib.rs` configures:
- Default `MEDIA_WEB_DIST`: `"apps/web/build"`
- Relative resolution against container `WORKDIR /app` resolves directly to `/app/apps/web/build`.
- Setting `ENV MEDIA_WEB_DIST=/app/apps/web/build` in Stage 3 can also be specified as an explicit absolute path.

---

## 7. Quality & Test Status

Prior to finalizing this analysis, all frontend tests and type checks were executed locally:
1. `npm test` (`vitest run`): 3 test files, 13 tests passed (0 failures).
2. `npm run check` (`svelte-kit sync && svelte-check --tsconfig ./tsconfig.json`): 0 errors, 0 warnings.
3. `npm run build`: built in 2.84s, 45 output files cleanly generated.

All checks passed without warnings or regressions.
