# Handoff Report — Milestone 2 Explorer 2: Node SvelteKit Frontend Build Stage

**Agent**: Milestone 2 Explorer 2  
**Recipient**: Orchestrator (`e84c8bab-f4f1-4a93-8220-b8cbda41be06`)  
**Scope**: Dockerfile Stage 1 (`web-builder`), base image selection, layer caching, static output verification, CSP externalization script.  
**Type**: Hard Handoff (Investigation Complete)

---

## 1. Observation

1. **Package Configuration (`apps/web/package.json:1-28`)**:
   - `apps/web/package.json` contains:
     - `"scripts": { "dev": "vite --host 127.0.0.1", "check": "svelte-kit sync && svelte-check --tsconfig ./tsconfig.json", "build": "vite build && node scripts/externalize-bootstrap.mjs", "lint": "eslint src", "test": "vitest run" }`
     - `"devDependencies": { "@opentelemetry/api": "^1.9.1", "@sveltejs/adapter-static": "4.0.0", "@sveltejs/kit": "3.1.0", "@sveltejs/vite-plugin-svelte": "7.3.1", "eslint": "^10.12.0", "eslint-plugin-svelte": "^3.23.1", "playwright": "^1.64.1", "svelte": "5.57.2", "svelte-check": "^4.7.6", "typescript": "^6.0.3", "typescript-eslint": "^8.71.2", "vite": "8.3.4", "vitest": "^5.0.5" }`
   - There are **no production `dependencies`** in `package.json`; all packages are defined in `devDependencies`.
   - There is no root-level `package.json` or `package-lock.json`.

2. **Lockfile Analysis (`apps/web/package-lock.json`)**:
   - Format: `lockfileVersion: 3`.
   - Verified that precompiled native Linux x86_64 platform packages are locked:
     - `@rolldown/binding-linux-x64-gnu` (v1.2.14, lines 493–507)
     - `@rolldown/binding-linux-x64-musl` (v1.2.14, lines 510–524)
     - `lightningcss-linux-x64-gnu` (v1.33.0, lines 1974–1992)
     - `lightningcss-linux-x64-musl` (v1.33.0, lines 1995–2013)

3. **Bootstrap Externalization Script (`apps/web/scripts/externalize-bootstrap.mjs:1-18`)**:
   - Script reads every `.html` in `apps/web/build/` and replaces `<script>([\s\S]*?)<\/script>` with `<script src="./${name}"></script>`, writing the extracted JavaScript to `${entry.name.replace('.html', '')}-bootstrap-${count++}.js`.
   - Line 6 uses `entry.parentPath` on `fs.Dirent`, which requires Node.js v20.1.0+ or v22+.
   - Line 15 throws `new Error("Expected one SvelteKit bootstrap in ${file}, got ${count}")` if any HTML file lacks or has multiple inline scripts.
   - Script is automatically invoked during `npm run build`.

4. **SvelteKit Static Adapter Configuration (`apps/web/vite.config.ts:1-5` & `apps/web/src/routes/+layout.ts:1-3`)**:
   - `apps/web/vite.config.ts`: `adapter: adapter()`, `paths: { base: '/app' }`, `csp: { mode: 'hash', directives: { 'script-src': ['self'] } }`.
   - `apps/web/src/routes/+layout.ts`: `export const prerender = true; export const trailingSlash = 'always';`.

5. **Build Output & Zero Node.js Runtime Verification**:
   - Executing `npm run build` in `apps/web` exited with code 0 in 2.84s.
   - `Get-ChildItem -Recurse apps/web/build -File | Group-Object Extension -NoElement` observed:
     - `.js`: 29 files
     - `.html`: 7 files
     - `.css`: 5 files
     - `.svg`: 2 files
     - `.png`: 1 file
     - `.json`: 1 file
     - Total: 45 files, 2,201,721 bytes (~2.2 MB).
   - Zero `.node` native addons, zero executable binaries, and zero Node server-side script files exist in `apps/web/build`.

6. **Backend Consumption (`crates/core/src/lib.rs:160-189`)**:
   - `configured_web_dist()` defaults to `"apps/web/build"` (or `MEDIA_WEB_DIST` env var).
   - Serves static files using `tower_http::services::ServeDir::new(configured_web_dist()).append_index_html_on_directories(true)` nested under `/app`.
   - Requires zero Node.js binary or process at runtime.

---

## 2. Logic Chain

1. **Base Image Selection**:
   - Observation 2 demonstrates that prebuilt `linux-x64-gnu` binaries for Rolldown and LightningCSS exist in `package-lock.json`.
   - Observation 3 shows `scripts/externalize-bootstrap.mjs` requires Node.js >= 20.1.0 (`entry.parentPath`). Rolldown additionally declares `engines: { node: "^20.19.0 || >=22.12.0" }`.
   - Stage 1 is an ephemeral build stage; its image layers are discarded during Docker multi-stage assembly and do not increase the final image size.
   - Therefore, `node:22-bookworm-slim` is optimal: it provides glibc ABI parity with standard Linux binaries, guarantees compatibility with `entry.parentPath` and Rolldown, aligns with Debian Bookworm in Stage 3, and avoids musl memory allocator or threading pitfalls.

2. **Layer Caching Optimization**:
   - Observation 1 shows that npm manifests (`package.json`, `package-lock.json`) are self-contained in `apps/web/`.
   - By copying only `apps/web/package.json` and `apps/web/package-lock.json` before `RUN npm ci`, Docker caches the `node_modules` installation layer across all frontend code modifications.
   - Observation 1 also shows all build tools are in `devDependencies`. If `NODE_ENV=production` is set prior to `npm ci`, npm omits devDependencies and the build fails. Thus, `npm ci` must run in default (non-production-omitting) mode.

3. **Build Execution & CSP Compliance**:
   - Observation 1 and 3 show that `npm run build` runs `vite build && node scripts/externalize-bootstrap.mjs`.
   - Running `npm run build` generates all static HTML/CSS/JS and externalizes inline bootstrap scripts into `*-bootstrap-0.js`.
   - Observation 4 shows all routes are prerendered with trailing slashes, matching Axum's `append_index_html_on_directories(true)`.

4. **Zero Node.js Runtime in Final Image**:
   - Observations 5 and 6 confirm that the output in `apps/web/build` consists exclusively of static client assets (HTML/CSS/JS/images) served directly by Axum (`ServeDir`).
   - Copying only `apps/web/build` from `web-builder` into the final runtime container completely eliminates the need for Node.js, npm, or any Node process in the runtime image.

---

## 3. Caveats

1. **`.dockerignore` Dependency**: If `COPY apps/web/ ./` is used in Stage 1, `.dockerignore` must exclude `apps/web/node_modules/`, `apps/web/.svelte-kit/`, and `apps/web/build/` to prevent host pollution from overwriting container files. An explicit copy alternative (`COPY apps/web/src ./src`, etc.) is provided in Section 6 of `analysis.md` as an optional hardening measure.
2. **Multi-Arch Musl vs Glibc**: While musl bindings are present in `package-lock.json`, Alpine was discarded in favor of Debian Bookworm for consistency and robustness; ARM64 builds (`linux/arm64`) on Debian Bookworm are also fully supported by Node 22 and the locked packages.

---

## 4. Conclusion

Stage 1 (`web-builder`) should be implemented using `node:22-bookworm-slim`. It should cache `npm ci` via manifest separation, run `npm run build` without premature `NODE_ENV=production` dependency pruning, and export `apps/web/build` to the final stage.

### Concrete Dockerfile Snippet:

```dockerfile
# ==============================================================================
# Stage 1: SvelteKit Frontend Web Builder
# ==============================================================================
FROM node:22-bookworm-slim AS web-builder

WORKDIR /app/apps/web

# 1. Cache dependency installation layer
COPY apps/web/package.json apps/web/package-lock.json ./
RUN npm ci --no-audit --no-fund

# 2. Copy web application sources and configuration
COPY apps/web/ ./

# 3. Build pre-rendered static frontend and externalize bootstrap scripts for CSP
RUN npm run build
```

---

## 5. Verification Method

1. **Frontend Unit Tests & Checks**:
   ```bash
   cd apps/web
   npm test
   npm run check
   ```
   *Expected result*: Vitest passes (3 test files, 13 tests passed), `svelte-check` reports 0 errors and 0 warnings.
2. **Build Execution & Bootstrap Externalization**:
   ```bash
   cd apps/web
   npm run build
   ```
   *Expected result*: Exit code 0, generates `apps/web/build/index.html` referencing `<script src="./index-bootstrap-0.js"></script>` without inline script tags.
3. **Static Assets Verification**:
   ```powershell
   Get-ChildItem -Recurse apps/web/build -File | Group-Object Extension -NoElement
   ```
   *Expected result*: Contains only static web files (.js, .html, .css, .svg, .png, .json); no binary or node server files.
4. **Invalidation Conditions**:
   - Adding a dependency that requires a Node version > 22 or unsupported native bindings.
   - Removing `node scripts/externalize-bootstrap.mjs` from `"build"`, which would cause CSP violations under Axum's strict `script-src 'self'`.
