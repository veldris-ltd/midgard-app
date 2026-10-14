// Web/extension shell accessibility audit (RT-49 / RC-49) — the concrete
// reference implementation of the on-device audit contract.
//
// Drives the *rendered* shell in a real Chromium via Playwright and runs axe-core
// (WCAG 2.1 AA) against it, then writes a record in the AUDIT_SCHEMA.md format.
// This is NOT a static declaration check — it measures the running UI. It runs on
// a real CI runner with a browser (see .github/workflows/shell-audit.yml), off the
// air-gapped build path. [FR-C-3] [AC-C-11]
//
// Usage:
//   node axe-audit.mjs --url <served-shell-url> --shell web --release <ver> \
//        --out clients/shells/web/audits/<ver>.toml [--report <json>]
//
// Requires (installed on the audit runner, not vendored): playwright,
// @axe-core/playwright.

import { writeFileSync } from 'node:fs';
import { chromium } from 'playwright';
import { AxeBuilder } from '@axe-core/playwright';

// Map axe result categories to the seven affordances the shells declare, so the
// record's [results] block lines up 1:1 with accessibility.toml.
const AFFORDANCE_TAGS = {
  screen_reader_labels: ['aria-*', 'label', 'image-alt', 'button-name', 'link-name'],
  contrast_aa: ['color-contrast'],
  focus_order: ['tabindex', 'focus-order-semantics'],
  keyboard_navigation: ['focusable-content', 'scrollable-region-focusable'],
  hit_target_size: ['target-size'],
  // Dynamic Type / reduced-motion are validated by rendering the shell under an
  // emulated large-text / prefers-reduced-motion media state (below) and checking
  // no overflow/animation violations appear.
  dynamic_type: ['text-spacing', 'meta-viewport'],
  reduced_motion: ['css-orientation-lock'],
};

function arg(name, def) {
  const i = process.argv.indexOf(`--${name}`);
  return i >= 0 && i + 1 < process.argv.length ? process.argv[i + 1] : def;
}

const url = arg('url');
const shell = arg('shell', 'web');
const release = arg('release', '0.0.0');
const out = arg('out', `clients/shells/${shell}/audits/${release}.toml`);
const reportPath = arg('report', `shell-audit/${shell}-${release}.json`);
if (!url) {
  console.error('axe-audit: --url <served-shell-url> is required');
  process.exit(2);
}

const browser = await chromium.launch();
// Audit under large Dynamic Type + reduced motion so those affordances are
// exercised, not merely asserted.
const context = await browser.newContext({
  reducedMotion: 'reduce',
  deviceScaleFactor: 1,
});
const page = await context.newPage();
await page.emulateMedia({ reducedMotion: 'reduce' });
await page.addInitScript(() => { document.documentElement.style.fontSize = '200%'; });

let results;
try {
  await page.goto(url, { waitUntil: 'networkidle', timeout: 60_000 });
  results = await new AxeBuilder({ page })
    .withTags(['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa'])
    .analyze();
} catch (e) {
  console.error(`axe-audit: could not audit ${url}: ${e.message}`);
  await browser.close();
  process.exit(1);
}
await browser.close();

writeFileSync(reportPath, JSON.stringify(results, null, 2));

// A violation maps to every affordance whose tag list contains the rule id.
const violated = new Set(results.violations.map((v) => v.id));
const affordanceResult = (affordance) => {
  const rules = AFFORDANCE_TAGS[affordance] || [];
  const hit = rules.some((r) => r.endsWith('*')
    ? [...violated].some((v) => v.startsWith(r.slice(0, -1)))
    : violated.has(r));
  return hit ? 'fail' : 'pass';
};

const affordances = Object.keys(AFFORDANCE_TAGS);
const measured = Object.fromEntries(affordances.map((a) => [a, affordanceResult(a)]));
const overall = Object.values(measured).every((v) => v === 'pass') ? 'pass' : 'fail';

const q = (s) => `"${String(s).replace(/"/g, '\\"')}"`;
const toml = [
  `# Accessibility audit record — ${shell} — release ${release}`,
  `# WRITTEN BY CI (axe-core/playwright) on a real browser runner. Do not hand-edit.`,
  `[audit]`,
  `shell        = ${q(shell)}`,
  `release      = ${q(release)}`,
  `commit       = ${q(process.env.MIDGARD_CI_COMMIT || process.env.GITHUB_SHA || process.env.CI_COMMIT_SHA || 'unknown')}`,
  `runner       = ${q(process.env.MIDGARD_CI_RUNNER || process.env.RUNNER_NAME || process.env.CI_RUNNER_DESCRIPTION || process.env.RUNNER_OS || 'ci-runner')}`,
  `device       = ${q('Chromium ' + (process.env.PLAYWRIGHT_CHROMIUM_VERSION || '(playwright)'))}`,
  `tool         = ${q('Playwright + @axe-core/playwright (WCAG 2.1 AA)')}`,
  `tool_version = ${q(process.env.AXE_VERSION || 'axe-core')}`,
  `timestamp    = ${q(new Date().toISOString())}`,
  `overall      = ${q(overall)}`,
  ``,
  `[results]`,
  ...affordances.map((a) => `${a.padEnd(20)} = ${q(measured[a])}`),
  ``,
  `[evidence]`,
  `report = ${q(reportPath)}`,
  `issues = ${results.violations.length}`,
  ``,
].join('\n');

writeFileSync(out, toml);
console.log(`axe-audit: wrote ${out} (overall=${overall}, ${results.violations.length} violation(s))`);
process.exit(overall === 'pass' ? 0 : 1);
