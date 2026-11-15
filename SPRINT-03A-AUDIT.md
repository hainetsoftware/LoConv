# Sprint 03A dependency audit — 2026-11-15

No JavaScript dependency versions, lockfile entries or package scripts were
changed. No automatic fixes or breaking upgrades were applied.

`npm audit --json` failed with `ENOTFOUND registry.npmjs.org` when contacting the
npm security advisory endpoint. Therefore a fresh npm finding count is not
available. Dependency installation succeeded offline using a copy of the local
npm package cache in a test-owned temporary directory. `npm audit --offline
--json` returned zero findings, but that cache-only result does not establish
that the current dependency tree is free of vulnerabilities.

The following review compares `apps/desktop/package-lock.json` with upstream
maintainer advisories. It is a targeted review, not a replacement for a fresh
full transitive audit.

| Locked dependency | Findings and applicability |
| --- | --- |
| `vitest` 3.2.7 (direct dev dependency), `@vitest/mocker` 3.2.7 (transitive via Vitest) | Both are in the affected range for [GHSA-82fw-gwwq-j7x9](https://github.com/vitest-dev/vitest/security/advisories/GHSA-82fw-gwwq-j7x9): redirect mocks can disclose files outside the project. Fix starts at 4.1.11; the related Windows bypass requires 4.1.12. The unauthenticated route requires standalone mocker/interceptor plugins. This repo uses `vitest run` with jsdom and does not configure those plugins or Browser Mode. |
| `@vitest/mocker` 3.2.7 | Also in the affected range for [GHSA-qxg6-89mg-rqv8](https://github.com/vitest-dev/vitest/security/advisories/GHSA-qxg6-89mg-rqv8): unauthenticated manual mock registration can inject browser JavaScript. Fix starts at 4.1.12. Same standalone plugin exposure condition; affected code is installed even though that server configuration is not enabled here. |
| `vite` 6.4.6 (direct dev dependency, also used by Vitest) | Already the patched 6.x release for [GHSA-p4f5-76q4-ffq7](https://github.com/vitejs/vite/security/advisories/GHSA-p4f5-76q4-ffq7), a dev-server code execution issue affecting versions through 6.4.5. No update needed for this specific advisory. |
| `vitest` 3.2.7 | Above the 3.2.5 fix for [GHSA-5xrq-8626-4rwp](https://github.com/vitest-dev/vitest/security/advisories/GHSA-5xrq-8626-4rwp), involving exposed Vitest UI/API. `@vitest/browser` and `@vitest/ui` are absent from the lockfile; browser-mode findings do not describe this jsdom test configuration. |
| `esbuild` 0.25.12 (transitive via Vite) | [GHSA-gv7w-rqvm-qjhr](https://github.com/evanw/esbuild/security/advisories/GHSA-gv7w-rqvm-qjhr) concerns esbuild's **Deno module**, which downloads binaries without integrity verification; this repo uses the npm/Node distribution. Report as a conditional/upstream finding, not an exploited LoConv runtime path. [GHSA-g7r4-m6w7-qqqr](https://github.com/evanw/esbuild/security/advisories/GHSA-g7r4-m6w7-qqqr) lists 0.27.3 for Windows dev-server traversal, not this locked version. |

Other reviewed inventory includes `@vitejs/plugin-react` 4.7.0, `rollup` 4.68.3,
and `jsdom` 26.1.0. No comprehensive vulnerability-free conclusion is made for
these or the remaining transitive packages while the live audit is unavailable.

The confirmed affected packages above belong to the development/test toolchain;
Node, Vitest and Vite servers are not shipped as converter runtime services.
This does not remove development risk. Keep dev/test servers local and use
synthetic fixtures only. A reviewed Vitest upgrade to a supported patched line
(including its matching mocker dependency) and a fresh npm audit are follow-up
work; forcing transitive version overrides across major versions has not been
attempted. None of these findings justifies adding runtime network access.
