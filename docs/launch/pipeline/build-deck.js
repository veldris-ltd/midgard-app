// Build the Midgard launch deck (16:9 wide, 18 slides, speaker notes, embedded demo video).
const pptxgen = require('pptxgenjs');
const fs = require('fs'), path = require('path');
const React = require('react');
const RDS = require('react-dom/server');
const Fi = require('react-icons/fi');
const sharp = require('sharp');

const HERE = __dirname;
const LIGHT = process.env.MG_MODE === 'light';
const OUTDIR = path.join(HERE, LIGHT ? 'out-light' : 'out');
const SCREENS = path.join(OUTDIR, 'screens');
const IMG = path.join(HERE, LIGHT ? 'img-light' : 'img');
const VIDEO = process.env.VIDEO || path.join(OUTDIR, LIGHT ? 'midgard-launch-demo-light.mp4' : 'midgard-launch-demo.mp4');
const OUT = process.env.OUT || path.join(OUTDIR, LIGHT ? 'Midgard-Launch-light.pptx' : 'Midgard-Launch.pptx');

// ── palette (ui/design-system/src/tokens/colors.css) ────────────────────────
// Dark is the platform default (:root); light is the .mode-light re-pointing, where accent text steps to -600 on paper.
const P = LIGHT ? {
  BG: 'EFF0F3', CARD: 'FFFFFF', CARD2: 'E4E6EA', BORDER: 'D6D9DE', LINE: 'BFC4CB', FILL2: 'F7F8FA', FRAME: 'FFFFFF', SHADOW: 0.18,
  TXT: '101114', MUTED: '383D44', FAINT: '5C636B', DIM: '8A9099',
  RED: 'CA1D25', RED3: 'A8171E', SECURE: '168A57', WARN: 'B07A12', INFO: '2B6FD6',
  ACC: { midgard: { m: 'CA1D25', t: 'A8171E' }, forge: { m: 'D96E1E', t: 'B25715' }, mimir: { m: '23A8B6', t: '17828E' }, bifrost: { m: '7D5CE6', t: '6244C2' }, heimdall: { m: 'C99A22', t: 'A17A17' } },
} : {
  BG: '0A0B0D', CARD: '16181C', CARD2: '1E2126', BORDER: '2A2D34', LINE: '3A3E46', FILL2: '141519', FRAME: '101114', SHADOW: 0.55,
  TXT: 'E9EBEE', MUTED: 'B6BCC4', FAINT: '9199A3', DIM: '6B7279',
  RED: 'CA1D25', RED3: 'F26B70', SECURE: '21C07A', WARN: 'F0A824', INFO: '3D8BFF',
  ACC: { midgard: { m: 'CA1D25', t: 'F26B70' }, forge: { m: 'D96E1E', t: 'F39B57' }, mimir: { m: '23A8B6', t: '63D3DD' }, bifrost: { m: '7D5CE6', t: 'AB93F5' }, heimdall: { m: 'C99A22', t: 'ECC55C' } },
};
const { BG, CARD, CARD2, BORDER, LINE, FILL2, FRAME, SHADOW, TXT, MUTED, FAINT, DIM, RED, RED3, SECURE, WARN, INFO, ACC } = P;
const TF = 'Arial', BF = 'Calibri', MF = 'Consolas';
const W = 13.333, H = 7.5, M = 0.6;

// ── helpers ─────────────────────────────────────────────────────────────────
const iconCache = new Map();
async function icon(name, color) {
  const key = name + color;
  if (iconCache.has(key)) return iconCache.get(key);
  const svg = RDS.renderToStaticMarkup(React.createElement(Fi[name], { color: '#' + color, size: 256 }));
  const png = await sharp(Buffer.from(svg)).png().toBuffer();
  const data = 'image/png;base64,' + png.toString('base64');
  iconCache.set(key, data);
  return data;
}
function shotPath(name) {
  if (fs.existsSync(SCREENS)) {
    const f = fs.readdirSync(SCREENS).find((x) => x.endsWith(`-${name}.png`));
    if (f) return { path: path.join(SCREENS, f), aspect: 16 / 9 };
  }
  const j = path.join(IMG, `${name}.jpg`);
  if (fs.existsSync(j)) return { path: j, aspect: 16 / 10 };
  if (process.env.ALLOW_MISSING) { console.log('  placeholder for', name); return { path: path.join(IMG, 'midgard-fabric.jpg'), aspect: 16 / 10 }; }
  throw new Error('no screenshot ' + name);
}

const pres = new pptxgen();
pres.layout = 'LAYOUT_WIDE';
pres.author = 'Veldris Ltd';
pres.title = 'Midgard launch';
let n = 0;

function base(notes) {
  const s = pres.addSlide();
  s.background = { color: BG };
  n++;
  if (n > 1) {
    s.addText('MIDGARD · VELDRIS LTD · LAUNCH 2026', { x: M, y: H - 0.42, w: 6, h: 0.25, fontFace: MF, fontSize: 8, color: DIM, charSpacing: 2, margin: 0, isTextBox: true });
    s.addText(String(n).padStart(2, '0'), { x: W - M - 1, y: H - 0.42, w: 1, h: 0.25, fontFace: MF, fontSize: 8, color: DIM, align: 'right', margin: 0, isTextBox: true });
  }
  if (notes) s.addNotes(notes);
  return s;
}
function eyebrow(s, num, text, x, y, color = RED3, w = 8) {
  s.addText(`${String(num).padStart(2, '0')} // ${text}`, { x, y, w, h: 0.28, fontFace: MF, fontSize: 10.5, color, charSpacing: 2.5, margin: 0, isTextBox: true });
}
function title(s, text, x, y, w, size = 32, h = 0.9) {
  s.addText(text, { x, y, w, h, fontFace: TF, fontSize: size, bold: true, color: TXT, margin: 0, valign: 'top', isTextBox: true, fit: 'shrink' });
}
function card(s, x, y, w, h, fill = CARD, line = BORDER) {
  s.addShape(pres.shapes.ROUNDED_RECTANGLE, { x, y, w, h, fill: { color: fill }, line: { color: line, width: 0.75 }, rectRadius: 0.08 });
}
function frame(s, shot, x, y, w, h) {
  // A screenshot in a thin ink frame. Fit inside (w,h) keeping aspect; frame hugs the image.
  let iw = w, ih = w / shot.aspect;
  if (ih > h) { ih = h; iw = h * shot.aspect; }
  const fx = x + (w - iw) / 2, fy = y + (h - ih) / 2;
  s.addShape(pres.shapes.ROUNDED_RECTANGLE, { x: fx - 0.06, y: fy - 0.06, w: iw + 0.12, h: ih + 0.12, fill: { color: FRAME }, line: { color: LINE, width: 0.75 }, rectRadius: 0.06, shadow: { type: 'outer', blur: 18, offset: 6, angle: 90, color: '000000', opacity: SHADOW } });
  s.addImage({ path: shot.path, x: fx, y: fy, w: iw, h: ih });
  return { x: fx, y: fy, w: iw, h: ih };
}
function cropShot(shot, top, height) {
  // Crop a horizontal band (fractions of image height) out of a screenshot, for a wide strip.
  const out = path.join(OUTDIR, 'crop-' + path.basename(shot.path).replace(/\.[a-z]+$/, '') + '.png');
  return sharp(shot.path).metadata().then((m) => sharp(shot.path).extract({ left: 0, top: Math.round(m.height * top), width: m.width, height: Math.round(m.height * height) }).png().toFile(out).then(() => ({ path: out, aspect: m.width / (m.height * height) })));
}
function bullets(s, items, x, y, w, h, size = 14, color = MUTED) {
  s.addText(items.map((t, i) => ({ text: t, options: { bullet: { code: '25AA' }, breakLine: i < items.length - 1, paraSpaceAfter: 8 } })),
    { x, y, w, h, fontFace: BF, fontSize: size, color, valign: 'top', margin: 0, isTextBox: true, fit: 'shrink' });
}
function para(s, text, x, y, w, h, size = 14, color = MUTED, opts = {}) {
  s.addText(text, { x, y, w, h, fontFace: BF, fontSize: size, color, valign: 'top', margin: 0, isTextBox: true, ...opts });
}
function mono(s, text, x, y, w, h, size = 9.5, color = FAINT, opts = {}) {
  s.addText(text, { x, y, w, h, fontFace: MF, fontSize: size, color, charSpacing: 1.5, margin: 0, isTextBox: true, ...opts });
}
function chip(s, text, x, y, w, color = FAINT, dot) {
  s.addShape(pres.shapes.ROUNDED_RECTANGLE, { x, y, w, h: 0.36, fill: { color: BG }, line: { color: BORDER, width: 0.75 }, rectRadius: 0.06 });
  if (dot) s.addShape(pres.shapes.OVAL, { x: x + 0.14, y: y + 0.13, w: 0.1, h: 0.1, fill: { color: dot }, line: { color: dot, width: 0 } });
  s.addText(text, { x: x + (dot ? 0.3 : 0.12), y, w: w - (dot ? 0.36 : 0.2), h: 0.36, fontFace: MF, fontSize: 9, color, charSpacing: 1.5, valign: 'middle', margin: 0, isTextBox: true });
}
async function iconCircle(s, name, color, x, y, d = 0.5) {
  s.addShape(pres.shapes.OVAL, { x, y, w: d, h: d, fill: { color: CARD2 }, line: { color: BORDER, width: 0.5 } });
  s.addImage({ data: await icon(name, color), x: x + d * 0.24, y: y + d * 0.24, w: d * 0.52, h: d * 0.52 });
}
// A product deep-dive: eyebrow, title, bullets one side, framed screenshot the other.
async function productSlide({ num, product, label, heading, points, shot, side = 'right', notes, iconName, sub }) {
  const s = base(notes);
  const a = ACC[product] || { m: RED, t: RED3 };
  const textX = side === 'right' ? M : W - M - 4.9, textW = 4.9;
  const imgX = side === 'right' ? M + 5.3 : M, imgW = W - 2 * M - 5.3;
  eyebrow(s, num, label, textX, 0.55, a.t);
  title(s, heading, textX, 0.9, textW, 28, 1.3);
  if (iconName) await iconCircle(s, iconName, a.t, textX, 2.35, 0.5);
  bullets(s, points, textX, iconName ? 3.0 : 2.4, textW, 3.6, 13.5);
  frame(s, shot, imgX, 1.0, imgW, 5.2);
  if (sub) mono(s, sub, imgX, 6.35, imgW, 0.3, 8.5, DIM);
  return s;
}

(async () => {
  // ── 1 · title ────────────────────────────────────────────────────────────
  {
    const s = base(
      'Welcome. Midgard is a Veldris Ltd product: a sovereign, open-source, post-quantum data and decision platform, built against a complete specification: five services run today and the rest is tested as libraries. ' +
      'Over the next twenty minutes I will show you what it is, watch it run end to end, and be precise about what is proven today and what still stands between this release candidate and general availability. (≈1 min)');
    eyebrow(s, 0, 'VELDRIS LTD · PRODUCT LAUNCH · SEPTEMBER 2026', M, 1.55, FAINT, 6);
    s.addText([{ text: 'MIDGARD', options: { color: TXT } }, { text: '.', options: { color: RED } }],
      { x: M - 0.04, y: 1.9, w: 6.4, h: 1.35, fontFace: TF, fontSize: 72, bold: true, margin: 0, isTextBox: true });
    para(s, 'The sovereign enclosure.', M, 3.3, 6, 0.5, 22, TXT, { bold: true });
    para(s, 'A post-quantum, air-gap-capable platform for your data, your models and your operations. One core, four products, one trust boundary.', M, 3.85, 5.9, 1.1, 15, MUTED);
    chip(s, 'PQ-SEALED · ML-KEM-1024', M, 5.15, 2.75, MUTED, SECURE);
    chip(s, 'NO EGRESS', M + 2.9, 5.15, 1.3, MUTED);
    chip(s, 'AIR-GAP READY', M + 4.35, 5.15, 1.6, MUTED);
    frame(s, shotPath('midgard-fabric'), 7.0, 1.15, 5.75, 5.2);
    mono(s, 'MIDGARD CONSOLE · RECORDED AGAINST THE REAL ENCLOSURE EDGE', 7.0, 6.45, 5.75, 0.3, 8, DIM, { align: 'center' });
  }

  // ── 2 · agenda ───────────────────────────────────────────────────────────
  {
    const s = base('Five parts. The problem we are solving, the platform itself, a recorded live demo of the software running, the security and verification evidence, and an honest statement of where the release stands. Questions are welcome at the end. (≈0.5 min)');
    eyebrow(s, 1, 'AGENDA', M, 0.55);
    title(s, 'Twenty minutes, five parts', M, 0.9, 10);
    const items = [
      ['01', 'The problem', 'Why sovereign data platforms are overdue', '2 min'],
      ['02', 'The platform', 'One enclosure, four products, one trust boundary', '6 min'],
      ['03', 'Live demo', 'The real edge, the consoles, a human-gated action', '5 min'],
      ['04', 'Proof', 'Post-quantum by default, and the verification record', '4 min'],
      ['05', 'Where we are', 'Release status, blockers and the next 90 days', '3 min'],
    ];
    const cw = (W - 2 * M - 4 * 0.25) / 5;
    items.forEach(([k, h, d, t], i) => {
      const x = M + i * (cw + 0.25), y = 2.3;
      card(s, x, y, cw, 3.4);
      s.addText(k, { x: x + 0.25, y: y + 0.25, w: cw - 0.5, h: 0.8, fontFace: TF, fontSize: 40, bold: true, color: i === 2 ? RED3 : TXT, margin: 0, isTextBox: true });
      para(s, h, x + 0.25, y + 1.2, cw - 0.5, 0.5, 17, TXT, { bold: true });
      para(s, d, x + 0.25, y + 1.7, cw - 0.5, 1.1, 12.5, MUTED);
      mono(s, t.toUpperCase(), x + 0.25, y + 2.85, cw - 0.5, 0.3, 9, FAINT);
    });
  }

  // ── 3 · the problem ──────────────────────────────────────────────────────
  {
    const s = base(
      'Three pressures. First, the tooling organisations use for analytics, AI and software delivery assumes an always-on cloud; for regulated, defence and critical-infrastructure work that is disqualifying. ' +
      'Second, harvest-now-decrypt-later: traffic and archives captured today become readable once quantum computers mature, and NIST has already finalised the replacement algorithms, so migration is now a programme, not a research topic. ' +
      'Third, the incumbent platform is closed, hosted and opaque. You cannot audit what it does with your data and you cannot leave easily. Midgard was designed to answer all three at once. (≈2 min)');
    eyebrow(s, 2, 'THE PROBLEM', M, 0.55);
    title(s, 'Three pressures on every serious data platform', M, 0.9, 11);
    const cols = [
      ['FiCloudOff', 'Platforms that phone home', 'Analytics, AI and delivery tooling assume an always-on cloud and a vendor in the loop. For regulated, defence and critical-infrastructure work, that is disqualifying.', 'SOVEREIGNTY'],
      ['FiKey', 'Harvest now, decrypt later', 'Traffic and archives captured today will be readable once large quantum computers exist. NIST finalised FIPS 203, 204 and 205 in 2024. Migration is a multi-year programme, and it has to start with the platform.', 'POST-QUANTUM'],
      ['FiEyeOff', 'Lock-in without visibility', 'The incumbent is closed, hosted and expensive. You cannot audit what it does with your data, reproduce its builds, or take your ontology with you when you leave.', 'OPEN CORE'],
    ];
    const cw = (W - 2 * M - 2 * 0.3) / 3;
    for (let i = 0; i < 3; i++) {
      const [ic, h, d, tag] = cols[i]; const x = M + i * (cw + 0.3), y = 2.15;
      card(s, x, y, cw, 3.9);
      await iconCircle(s, ic, RED3, x + 0.3, y + 0.3, 0.6);
      para(s, h, x + 0.3, y + 1.1, cw - 0.6, 0.5, 18, TXT, { bold: true });
      para(s, d, x + 0.3, y + 1.65, cw - 0.6, 1.9, 13, MUTED);
      mono(s, `MIDGARD ANSWERS WITH · ${tag}`, x + 0.3, y + 3.45, cw - 0.6, 0.3, 8.5, RED3);
    }
    para(s, 'Midgard was built to answer all three at once: no outbound dependency, post-quantum cryptography applied by default, and an Apache-2.0 core with reproducible builds.', M, 6.3, W - 2 * M, 0.6, 13, FAINT, { italic: true });
  }

  // ── 4 · the suite ────────────────────────────────────────────────────────
  {
    const s = base(
      'Midgard is the core platform: the enclosure. Identity, the object and ontology fabric, storage, compute, governance and post-quantum cryptography by default. It is the foundation every product builds on. ' +
      'On top sit four independently useful products, each with its own domain: Forge for data operations, Mimir for local agentic AI, Bifrost for continuous delivery, and Heimdall for operational fusion. ' +
      'Two cross-cutting layers complete it: a self-governing admin control panel with no super-admin, and native mobile, desktop and web clients. (≈1.5 min)');
    eyebrow(s, 3, 'THE PLATFORM', M, 0.55);
    title(s, 'One core platform, four composable products', M, 0.9, 11);
    const prods = [
      ['forge', 'FORGE', 'Data operations', 'Ingest, transform, version, project to the fabric'],
      ['mimir', 'MIMIR', 'Local agentic AI', 'In-enclosure inference, grounding, agents, evals'],
      ['bifrost', 'BIFROST', 'Continuous delivery', 'PQ-signed artefacts, rings, fleet, offline bundles'],
      ['heimdall', 'HEIMDALL', 'Operational fusion', 'Feeds, one common picture, decision workflows'],
    ];
    const cw = (W - 2 * M - 3 * 0.25) / 4;
    prods.forEach(([k, name, dom, d], i) => {
      const x = M + i * (cw + 0.25), y = 2.0, a = ACC[k];
      card(s, x, y, cw, 2.35);
      s.addShape(pres.shapes.OVAL, { x: x + 0.25, y: y + 0.3, w: 0.16, h: 0.16, fill: { color: a.m }, line: { color: a.m, width: 0 } });
      s.addText(name, { x: x + 0.5, y: y + 0.18, w: cw - 0.7, h: 0.4, fontFace: TF, fontSize: 17, bold: true, color: a.t, margin: 0, isTextBox: true });
      para(s, dom, x + 0.25, y + 0.68, cw - 0.5, 0.35, 13.5, TXT, { bold: true });
      para(s, d, x + 0.25, y + 1.05, cw - 0.5, 0.8, 12, MUTED);
    });
    // base platform
    card(s, M, 4.6, W - 2 * M, 1.15, FILL2, ACC.midgard.m);
    s.addShape(pres.shapes.OVAL, { x: M + 0.3, y: 4.9, w: 0.16, h: 0.16, fill: { color: RED }, line: { color: RED, width: 0 } });
    s.addText('MIDGARD', { x: M + 0.55, y: 4.75, w: 2.2, h: 0.45, fontFace: TF, fontSize: 19, bold: true, color: RED3, margin: 0, isTextBox: true });
    para(s, 'The enclosure: identity, object/ontology fabric, storage, compute, governance, PQ crypto substrate', M + 0.55, 5.2, 8.5, 0.4, 12.5, MUTED);
    // cross-cutting
    const hw = (W - 2 * M - 0.25) / 2;
    card(s, M, 6.0, hw, 0.85);
    para(s, 'Admin Control Panel', M + 0.3, 6.08, 2.6, 0.35, 13, TXT, { bold: true });
    para(s, 'Role-separated RBAC and no super-admin. Dual control is live for enrolment; break-glass and the rest are tested libraries', M + 0.3, 6.4, hw - 0.6, 0.4, 11, MUTED);
    card(s, M + hw + 0.25, 6.0, hw, 0.85);
    para(s, 'Client platforms', M + hw + 0.55, 6.08, 2.6, 0.35, 13, TXT, { bold: true });
    para(s, 'One shared Rust core, tested as a library: offline-first sync, tiered inference. Six shells are specified, not built', M + hw + 0.55, 6.4, hw - 0.6, 0.4, 11, MUTED);
  }

  // ── 5 · design principles ───────────────────────────────────────────────
  {
    const s = base(
      'Six principles, and each one is enforced by construction rather than by policy document. Sovereign: no core function requires an outbound call, and the build and the test suite run under docker network none. ' +
      'Post-quantum by default: hybrid ML-KEM in transit, ML-KEM-wrapped keys at rest, ML-DSA signatures everywhere. The object fabric is the centre, not a bolt-on. Every product calls Mimir for AI, never a hosted model. ' +
      'Every claim has a source and every action has lineage. And the core is open: Apache-2.0, reproducible builds, a CycloneDX SBOM with every release. (≈1 min)');
    eyebrow(s, 4, 'DESIGN PRINCIPLES', M, 0.55);
    title(s, 'Enforced by construction, not by policy document', M, 0.9, 11);
    const P = [
      ['FiShield', 'Sovereign by construction', 'No core function makes an outbound call. The full acceptance gate runs under --network none and passes.'],
      ['FiLock', 'Post-quantum by default', 'Hybrid X25519 + ML-KEM-768 in transit; AES-256-GCM under ML-KEM-1024-wrapped keys at rest; ML-DSA and SLH-DSA signatures.'],
      ['FiBox', 'Object fabric at the centre', 'Objects, properties, links, actions and dynamic security: a decision-centric semantic layer, the answer to Ontology.'],
      ['FiCpu', 'Local AI native', 'Mimir serves every model in-enclosure. Products call Mimir, never a hosted LLM. Signed model manifests, verified before serving.'],
      ['FiGitCommit', 'Provenance everywhere', 'Every claim has a source, every action has lineage. Append-only signed audit and human gates are first-class.'],
      ['FiUnlock', 'Open-source core', 'Apache-2.0. Reproducible builds, a CycloneDX SBOM per release, every binary signed and attested.'],
    ];
    const cw = (W - 2 * M - 2 * 0.3) / 3, ch = 2.05;
    for (let i = 0; i < 6; i++) {
      const [ic, h, d] = P[i]; const x = M + (i % 3) * (cw + 0.3), y = 2.05 + Math.floor(i / 3) * (ch + 0.3);
      card(s, x, y, cw, ch);
      await iconCircle(s, ic, RED3, x + 0.25, y + 0.25, 0.5);
      para(s, h, x + 0.9, y + 0.3, cw - 1.1, 0.4, 15, TXT, { bold: true });
      para(s, d, x + 0.25, y + 0.9, cw - 0.5, 1.1, 12, MUTED);
    }
  }

  // ── 6 · architecture ────────────────────────────────────────────────────
  {
    const s = base(
      'The shape of a deployment. Operators reach the enclosure through one edge, the HTTP gateway, which terminates TLS and issues ML-DSA-signed session tokens. Behind it the core composes identity, the object fabric, key management and the governance ledger in one process, so edge admission and every fabric call land in a single PQ-signed ledger. ' +
      'The four product services talk to the core and to each other only over PQ-mTLS, each with a certified mesh identity. The only things that cross the boundary are deliberate: Forge reaches external sources through an egress broker with a pinned CA per route, and Bifrost carries releases into air-gapped sites as signed bundles that verify themselves offline. (≈1.5 min)');
    eyebrow(s, 5, 'ARCHITECTURE', M, 0.55);
    title(s, 'One boundary. Everything inside it.', M, 0.9, 10);
    const box = (x, y, w, h, head, sub, col = TXT, fill = CARD, line = BORDER) => {
      s.addShape(pres.shapes.ROUNDED_RECTANGLE, { x, y, w, h, fill: { color: fill }, line: { color: line, width: 0.75 }, rectRadius: 0.06 });
      s.addText(head, { x: x + 0.12, y: y + 0.08, w: w - 0.24, h: 0.32, fontFace: TF, fontSize: 11.5, bold: true, color: col, margin: 0, isTextBox: true });
      if (sub) s.addText(sub, { x: x + 0.12, y: y + 0.4, w: w - 0.24, h: h - 0.46, fontFace: BF, fontSize: 9.5, color: MUTED, margin: 0, valign: 'top', isTextBox: true });
    };
    const arrow = (x1, y1, x2, y2, col = LINE) => s.addShape(pres.shapes.LINE, { x: Math.min(x1, x2), y: Math.min(y1, y2), w: Math.abs(x2 - x1) || 0.01, h: Math.abs(y2 - y1) || 0.01, line: { color: col, width: 1.25, endArrowType: 'triangle' }, flipH: x2 < x1, flipV: y2 < y1 });
    const gapLabel = (text, x, y) => mono(s, text, x, y, 0.75, 0.22, 6.5, FAINT, { align: 'center' });
    // boundary
    const bx = 3.55, by = 1.95, bw = 9.15, bh = 4.85;
    s.addShape(pres.shapes.ROUNDED_RECTANGLE, { x: bx, y: by, w: bw, h: bh, fill: { color: FILL2 }, line: { color: RED, width: 1, dashType: 'dash' }, rectRadius: 0.1 });
    mono(s, 'THE ENCLOSURE · AIR-GAP BOUNDARY · NO OUTBOUND CALL REQUIRED', bx + 0.2, by + 0.1, bw - 0.4, 0.25, 8.5, RED3);
    // left column, outside the boundary
    box(M, 2.45, 2.55, 1.35, 'Operators & clients', 'Console · API · client core\nShells specified, not built\nTLS 1.3 / PQ-mTLS to the edge');
    arrow(M + 2.55, 3.12, bx + 0.35, 3.12, RED3);
    box(M, 4.45, 2.55, 1.0, 'External sources', 'PostgreSQL · S3 · Kafka\nvia the egress broker: TLS 1.3, pinned CA', FAINT, FILL2);
    arrow(bx + 0.35, 4.95, M + 2.55, 4.95, ACC.forge.m);
    gapLabel('EGRESS', M + 2.55, 4.68);
    box(M, 5.75, 2.55, 1.0, 'Air-gapped sites', 'Signed .mgbundle on media; every artefact is verified offline before install', ACC.bifrost.t, FILL2);
    arrow(bx + 0.35, 6.25, M + 2.55, 6.25, ACC.bifrost.m);
    gapLabel('BUNDLE', M + 2.55, 5.98);
    // edge + core
    box(bx + 0.35, 2.45, 2.2, 1.35, 'Edge · http-gateway', 'Policy decision point\nML-DSA-signed session tokens\nUniform 401, per-object filtering', RED3);
    arrow(bx + 2.55, 3.12, bx + 2.95, 3.12);
    box(bx + 2.95, 2.45, 2.75, 1.35, 'Enclosure core', 'IAM · Object fabric · KMS\nGovernance: one PQ-signed ledger\nSealed, append-only journals', RED3);
    // products, fed from the core over the mesh
    const px = bx + 6.05, pw = 2.85, ph = 0.72;
    const prods = [['forge', 'Forge worker', 'connectors · datasets · projection'], ['mimir', 'Mimir server', 'signed models · guardrails · agents'], ['bifrost', 'Bifrost fleet', 'artefacts · rings · bundles'], ['heimdall', 'Heimdall feed', 'signed observations · fused picture']];
    prods.forEach(([k, h, d], i) => {
      const y = 2.45 + i * (ph + 0.14);
      box(px, y, pw, ph, h, d, ACC[k].t);
      arrow(bx + 5.7, 3.12, px, y + ph / 2, ACC[k].m);
    });
    mono(s, 'PQ-mTLS SERVICE MESH · HYBRID X25519 + ML-KEM-768', bx + 0.35, 3.95, 5.35, 0.22, 7.5, FAINT);
    box(bx + 0.35, 4.3, 5.35, 0.95, 'Admin Control Panel', 'Dual control on enrolment today · no super-admin · break-glass and signed configuration are tested libraries, not yet served', TXT, FILL2);
    box(bx + 0.35, 5.45, 5.35, 1.1, 'Deployment', 'Laptop (dev mode) · docker compose (5 services) · Kubernetes via Helm (hardened pods, default-deny network) · signed air-gap bundle for disconnected sites', TXT, FILL2);
    box(px, 5.95, pw, 0.6, 'Every hop audited', 'one ledger, ML-DSA-65, re-verified on start', SECURE, FILL2);
  }

  // ── 7 · object fabric ───────────────────────────────────────────────────
  await productSlide({
    num: 6, product: 'midgard', label: 'THE ENCLOSURE · OBJECT FABRIC', heading: 'Your organisation, described once', iconName: 'FiBox',
    points: [
      'Objects, properties and links: aircraft, facilities, crew, sensor feeds as real things, not rows in a table.',
      'Markings (clearance band plus compartment) and purposes are enforced per object at query time. What you cannot read never appears.',
      'Actions are typed and policy-authorised, and gated by default: refused without a human-gate token. Tying that token to a named person is still to do.',
      'Every response carries the audit id of the signed ledger entry that authorised it.',
      'Schema comes from an ontology file; data is imported offline and validated against it, all or nothing.',
    ],
    shot: shotPath('midgard-humangate-authorised'), side: 'right', sub: 'MIDGARD CONSOLE · HUMAN-GATED ACTION AUTHORISED AGAINST THE REAL EDGE',
    notes: 'This is the Midgard console against the real edge. The left panel is the object-type registry; the inspector on the right shows an object type, its links and the actions a person may take. ' +
      'Ground aircraft is human-gated: the first attempt is refused with a human-gate-required error, an operator authorises it, and the authorisation comes back with the audit id that recorded it. The same flow works from the API with curl. (≈1 min)',
  });

  // ── 8 · governance ──────────────────────────────────────────────────────
  {
    const s = base(
      'Governance is the spine, not a report. Policy is deny by default: role, marking and purpose must all match a written rule, mandatory access control runs first, and anything not granted is denied. ' +
      'You can dry-run a decision for a subject, clearance and purpose before it goes live, and the decision itself is ML-DSA-signed. Every read, write and action at the edge lands in one append-only ledger, and the service re-verifies that ledger on every start: a tampered ledger, keystore or policy file stops it from starting. (≈1 min)');
    eyebrow(s, 7, 'THE ENCLOSURE · GOVERNANCE', M, 0.55);
    title(s, 'Access is proven, not assumed', M, 0.9, 4.9, 28, 1.0);
    await iconCircle(s, 'FiShield', RED3, M, 2.0, 0.5);
    bullets(s, [
      'Deny by default. Role, marking and purpose must all match a written rule; mandatory access control runs first.',
      'Dry-run any decision before it goes live. The decision is ML-DSA-signed and audited like everything else.',
      'One append-only ledger for reads, writes and actions at the edge, re-verified on every start.',
      'Fail closed. A tampered ledger, keystore or policy file stops the service from starting.',
    ], M, 2.65, 4.9, 3.8, 13.5);
    const iw = 6.0, ix = W - M - iw;
    const f1 = frame(s, shotPath('midgard-policy-dryrun'), ix, 0.95, iw, 3.4);
    const strip = await cropShot(shotPath('midgard-audit'), 0.0, 0.58);
    const f2 = frame(s, strip, ix, f1.y + f1.h + 0.2, iw, 2.0);
    mono(s, 'POLICY DRY-RUN AND AUDIT LINEAGE · EVERY ENTRY RE-VERIFIED · ML-DSA-65', ix, f2.y + f2.h + 0.12, iw, 0.25, 8, DIM);
  }

  // ── 9–12 · products ─────────────────────────────────────────────────────
  await productSlide({
    num: 8, product: 'forge', label: 'FORGE · DATA OPERATIONS', heading: 'Getting data in, without losing track of it', iconName: 'FiDatabase', side: 'left',
    points: [
      'Connectors for PostgreSQL, S3 and Kafka, syncing incrementally from checkpoints inside an air-gapped sandbox.',
      'Sources outside the enclosure are reached only through the egress broker: TLS 1.3, one pinned CA per route.',
      'Datasets are content-addressed and versioned with ingest lineage; an unchanged source is the same version.',
      'Expectation checks that block promotion are a tested library; the running worker does not apply them yet.',
      'A pure-Rust partitioned compute engine: join, aggregate, sort and window, with spill to disk.',
    ],
    shot: shotPath('forge-pipeline'), sub: 'FORGE CONSOLE · PIPELINE BUILDER · BUILT-IN SAMPLE DATA',
    notes: 'Forge is the data-operations product. Draw a pipeline or write it in code; either way every build is repeatable and versioned like source control. The worker service already runs: it reads a source, commits rows to a sealed dataset store, projects them onto the ontology and syncs them into the fabric through the gateway\'s PQ-mTLS door. Connectors have been tested against real PostgreSQL 16 and MinIO. (≈1 min)',
  });
  await productSlide({
    num: 9, product: 'mimir', label: 'MIMIR · LOCAL AGENTIC AI', heading: 'AI that never phones home', iconName: 'FiCpu', side: 'right',
    points: [
      'All inference runs in-enclosure over PQ-mTLS. Every product calls Mimir; nothing calls a hosted model.',
      'A model loads only from a signed manifest whose weights stream to the signed digest. One changed byte and the service refuses to start.',
      'Granite 3.3 2B Instruct (Apache-2.0) served on llama.cpp today: first token in 325 ms on a 12-core CPU. vLLM on GPU is wired, not yet run.',
      'Grounded, cited answers and input guardrails are tested libraries. The running service answers a prompt without retrieval.',
      'AI proposes, humans dispose: in the agent loop, a tested library, no consequential action runs without a recorded human decision.',
    ],
    shot: shotPath('mimir-studio'), sub: 'MIMIR CONSOLE · AGENT STUDIO · BUILT-IN SAMPLE DATA',
    notes: 'Mimir is the local AI product. The inference plane is a running service: a certified caller opens a PQ-mTLS channel, policy decides whether it may infer, and every call, permitted or denied, is written to a sealed ledger the service re-verifies on restart. We have run a real model end to end: Granite 3.3 2B on CPU, staged from a signed manifest. (≈1 min)',
  });
  await productSlide({
    num: 10, product: 'heimdall', label: 'HEIMDALL · OPERATIONAL FUSION', heading: 'Many feeds, one picture everyone shares', iconName: 'FiEye', side: 'left',
    points: [
      'Sources submit observations signed with their certified mesh key; an observation is fused only if policy, feed identity, signature and audit all pass.',
      'Observations sharing an entity key fuse into one tracked entity with contributing feeds, first and last seen, and latest attributes.',
      'The picture is sealed at rest after each change and recovered on restart; analysts read it as JSON or in the console.',
      'Ranked, cited options and human-authorised workflows are tested libraries. The running service does not generate options yet.',
    ],
    shot: shotPath('heimdall-picture'), sub: 'HEIMDALL CONSOLE · COMMON OPERATING PICTURE · SYNTHETIC REFERENCE LAYOUT',
    notes: 'Heimdall is the operational-fusion product. The feed gateway runs as a service today: a source with a certified identity signs its observations, four checks gate each one, refusals are audited too, and the fused picture survives a restart. The console view here is the product\'s own synthetic reference layout. (≈0.75 min)',
  });
  await productSlide({
    num: 11, product: 'bifrost', label: 'BIFROST · CONTINUOUS DELIVERY', heading: 'Nothing deploys unless it proves itself', iconName: 'FiPackage', side: 'right',
    points: [
      'Every binary is rebuilt bit-for-bit, gets a CycloneDX SBOM, an offline CVE and licence scan, an ML-DSA signature and an attestation, in that order, each gate fail-closed.',
      'Services ship as verify-before-run images: the binary is re-verified inside the container before it starts.',
      'Ring-by-ring rollout with health gates and automatic rollback is a tested library. Services run one replica today.',
      'The fleet manager records every installation\'s version, liveness and drift from its channel over PQ-mTLS.',
      'Air-gapped sites receive one signed bundle on media. It verifies itself offline and streams a model\'s weights without holding them in memory.',
    ],
    shot: shotPath('bifrost-release'), sub: 'BIFROST CONSOLE · RELEASE CONSOLE · BUILT-IN SAMPLE DATA',
    notes: 'Bifrost is the continuous-delivery product, and it is also how Midgard itself ships. The release pipeline has been rehearsed end to end: all eighteen binaries gated, six images built and started through their own verifying entrypoints, a fifty-four-artefact bundle packed and installed on a cluster, and an image with one changed byte refusing to start. (≈0.75 min)',
  });

  // ── 13 · admin & clients ────────────────────────────────────────────────
  {
    const s = base(
      'Two cross-cutting layers. The admin control panel is designed to govern itself: role-separated RBAC with no super-admin, and dual control, which is live today for enrolling a principal. The guarded-action orchestrator, break-glass and its alerts are tested libraries, and the console runs on sample data. ' +
      'And the client platforms: one shared core, tested as a library, with offline-first, conflict-free sync. The native, desktop and web shells are specified and not built yet. (≈0.75 min)');
    eyebrow(s, 12, 'CONTROL & REACH', M, 0.55);
    title(s, 'No single person can do the dangerous things. And it works where the work happens.', M, 0.9, 12, 26, 1.0);
    const hw = (W - 2 * M - 0.4) / 2;
    frame(s, shotPath('admin-approvals'), M, 2.05, hw, 3.5);
    frame(s, shotPath('clients-mobile'), M + hw + 0.4, 2.05, hw, 3.5);
    para(s, 'Admin Control Panel', M, 5.7, hw, 0.35, 15, TXT, { bold: true });
    para(s, 'Dual control with a second, different approver: live for enrolment, a tested library for other high-risk changes. Break-glass is a tested library.', M, 6.05, hw, 0.8, 12, MUTED);
    para(s, 'Client platforms', M + hw + 0.4, 5.7, hw, 0.35, 15, TXT, { bold: true });
    para(s, 'One shared core, tested as a library. Native, desktop, web and extension shells are specified in SAD_06; none is built, so no key is held in device hardware.', M + hw + 0.4, 6.05, hw, 0.8, 12, MUTED);
  }

  // ── 14 · live demo ──────────────────────────────────────────────────────
  {
    const s = base(
      'Play the video. It is about four minutes and it was recorded against the software as it is: the Midgard console is talking to the real Rust edge running in a container, and the other consoles run on their built-in sample data. ' +
      'Watch for three things: the token that comes back from login is post-quantum signed; the grounding action is refused until a person authorises it, and the authorisation is audited; and every ledger entry shows as re-verified. If the embedded video will not play, open ' + path.basename(VIDEO) + ' beside the deck. (≈5 min)');
    eyebrow(s, 13, 'LIVE DEMO', M, 0.55);
    title(s, 'The enclosure edge, end to end', M, 0.9, 8);
    const vw = 8.0, vh = vw * 9 / 16, vx = M, vy = 1.9;
    s.addShape(pres.shapes.ROUNDED_RECTANGLE, { x: vx - 0.06, y: vy - 0.06, w: vw + 0.12, h: vh + 0.12, fill: { color: FRAME }, line: { color: LINE, width: 0.75 }, rectRadius: 0.06 });
    if (fs.existsSync(VIDEO)) {
      const cover = fs.readFileSync(shotPath('edge-terminal').path).toString('base64');
      s.addMedia({ type: 'video', path: VIDEO, x: vx, y: vy, w: vw, h: vh, cover: 'image/png;base64,' + cover });
    } else {
      s.addImage({ path: shotPath('edge-terminal').path, x: vx, y: vy, w: vw, h: vh });
    }
    mono(s, `${path.basename(VIDEO).toUpperCase()} · 1920×1080 · ${process.env.DEMO_LEN || '4 MIN 09 S'} · CLICK TO PLAY`, vx, vy + vh + 0.15, vw, 0.3, 8, DIM);
    const lx = M + vw + 0.5, lw = W - M - lx;
    para(s, 'What you will see', lx, 1.9, lw, 0.4, 15, TXT, { bold: true });
    const steps = [
      ['01', 'The edge starts in a container. Login returns an ML-DSA-signed token; a query returns only the objects the operator is cleared for, with the audit id that authorised it.'],
      ['02', 'Object fabric. "Ground aircraft" is refused until a person authorises it, then the authorisation itself is audited.'],
      ['03', 'Policy dry-run and audit lineage. Every entry from the session is already there, each one ML-DSA-65 verified.'],
      ['04', 'Forge, Mimir, Heimdall, Bifrost, Admin and the client shells, on built-in sample data.'],
    ];
    steps.forEach(([k, t], i) => {
      const y = 2.45 + i * 1.05;
      s.addText(k, { x: lx, y, w: 0.5, h: 0.4, fontFace: TF, fontSize: 18, bold: true, color: RED3, margin: 0, isTextBox: true });
      para(s, t, lx + 0.55, y + 0.03, lw - 0.55, 1.0, 11.5, MUTED);
    });
  }

  // ── 15 · post-quantum ───────────────────────────────────────────────────
  {
    const s = base(
      'The cryptography, layer by layer. In transit, every hop is a hybrid handshake: classical X25519 alongside ML-KEM-768, so an attacker has to break both. At rest, data keys are AES-256-GCM, wrapped under ML-KEM-1024 custody roots. ' +
      'Service identities, session tokens and audit entries are signed with ML-DSA-65; release signing uses ML-DSA-87; SLH-DSA is available for long-lived artefacts. The FIPS backend passes the official known-answer tests in the repository, and the two backends are mutually exclusive at compile time so a build cannot quietly mix them. What is not yet done is FIPS 140-3 module certification itself, which is external. (≈1.5 min)');
    eyebrow(s, 14, 'POST-QUANTUM BY DEFAULT', M, 0.55);
    title(s, 'Every layer, already migrated', M, 0.9, 8);
    const hdr = { fontFace: MF, fontSize: 9, color: FAINT, bold: true, fill: { color: FILL2 }, margin: 0.08, charSpacing: 1.5 };
    const cell = (t, o = {}) => ({ text: t, options: { fontFace: BF, fontSize: 11.5, color: MUTED, margin: 0.08, valign: 'middle', ...o } });
    const rows = [
      [{ text: 'LAYER', options: hdr }, { text: 'MECHANISM', options: hdr }, { text: 'STANDARD', options: hdr }],
      [cell('In transit', { color: TXT, bold: true }), cell('PQ-mTLS between every service and at the edge: hybrid X25519 + ML-KEM-768, session resumption, certified workload identities'), cell('NIST FIPS 203', { fontFace: MF, fontSize: 10 })],
      [cell('At rest', { color: TXT, bold: true }), cell('AES-256-GCM data keys wrapped under ML-KEM-1024 custody roots; every stateful store is a sealed, append-only journal'), cell('FIPS 203 · SP 800-38D', { fontFace: MF, fontSize: 10 })],
      [cell('Identity & tokens', { color: TXT, bold: true }), cell('ML-DSA-65 for issuer, audit and mesh keys; ML-DSA-signed session tokens and admin passkeys'), cell('NIST FIPS 204', { fontFace: MF, fontSize: 10 })],
      [cell('Release signing', { color: TXT, bold: true }), cell('ML-DSA-87 release key; ML-DSA-65 Bifrost admission key; SLH-DSA available for long-lived artefacts'), cell('FIPS 204 · FIPS 205', { fontFace: MF, fontSize: 10 })],
      [cell('Agility', { color: TXT, bold: true }), cell('Pluggable backends; the FIPS backend passes ACVP known-answer tests in-repo; backends are mutually exclusive at compile time'), cell('ACVP KATs', { fontFace: MF, fontSize: 10 })],
    ];
    s.addTable(rows, { x: M, y: 2.0, w: 8.3, colW: [1.6, 5.2, 1.5], border: { type: 'solid', color: BORDER, pt: 0.5 }, fill: { color: CARD }, rowH: [0.35, 0.72, 0.72, 0.72, 0.72, 0.72] });
    const cx = M + 8.3 + 0.4, cw = W - M - cx;
    card(s, cx, 2.0, cw, 3.95, FILL2, RED);
    await iconCircle(s, 'FiLock', RED3, cx + 0.3, 2.3, 0.55);
    para(s, 'Fail closed, everywhere', cx + 0.3, 3.0, cw - 0.6, 0.4, 16, TXT, { bold: true });
    para(s, 'Signature verification, policy enforcement, journal replay and certificate revocation all refuse rather than degrade. A tampered ledger stops the service. A changed weight byte stops the model. A changed image byte refuses to start.', cx + 0.3, 3.45, cw - 0.6, 1.7, 12, MUTED);
    mono(s, 'THREAT MODEL · docs/security/THREAT_MODEL.md', cx + 0.3, 5.45, cw - 0.6, 0.3, 8, FAINT);
    para(s, 'Not yet done: FIPS 140-3 module certification of the crypto module is external work and gates GA. Key custody today is software; HSM and TPM trust roots are designed and documented, not wired.', M, 6.25, W - 2 * M, 0.6, 11.5, FAINT, { italic: true });
  }

  // ── 16 · proof ──────────────────────────────────────────────────────────
  {
    const s = base(
      'These are measurements, each with its run. On a fresh clone, in the pinned toolchain container with networking disabled, the workspace builds and eight hundred and seventy tests pass on both crypto backends. The seventy-two acceptance criteria each map to a test, and that gate has not yet been run against a public commit. ' +
      'Every one of the eighteen binaries the workspace builds passes the per-artefact gate, and a missing one fails the build. The release pipeline produced a fifty-four-artefact signed bundle that installed on a three-node cluster, and the deployment tier passed its restart and cross-node handshake checks there. The GPU and HSM checks skipped because we have no such hardware yet, and the record says so. (≈1.5 min)');
    eyebrow(s, 15, 'PROOF', M, 0.55);
    title(s, 'Measured, not promised', M, 0.9, 8);
    para(s, 'Each figure names its run. The acceptance gate has not been run against a commit in the public history; that run comes before the tag.', M, 1.55, 11, 0.4, 12.5, FAINT);
    const stats = [
      ['72', 'ACCEPTANCE CRITERIA, EACH MAPPED TO A TEST', 'One script runs them all under docker --network none. No run is recorded against a public commit yet.'],
      ['870', 'TESTS PASSED · 0 FAILED', '288 test binaries on a fresh clone, 2026-10-13, both crypto backends, in a container with no network.'],
      ['0', 'NETWORK CALLS', 'Vendored dependencies; builds, tests and the gate need no network at all.'],
      ['18 / 18', 'BINARIES GATED', 'Reproduced, SBOM, scanned, signed, attested. A new binary without a gate entry fails CI.'],
      ['54', 'ARTEFACTS IN ONE SIGNED BUNDLE', 'Packed by the rehearsed release pipeline and installed offline on a 3-node kind cluster.'],
      ['325 ms', 'FIRST TOKEN · GRANITE 3.3 2B · CPU', 'A certified PQ-mTLS inference call answered by a real model, staged from a signed manifest.'],
    ];
    const cw = (W - 2 * M - 2 * 0.3) / 3, ch = 2.1;
    stats.forEach(([v, l, d], i) => {
      const x = M + (i % 3) * (cw + 0.3), y = 2.15 + Math.floor(i / 3) * (ch + 0.3);
      card(s, x, y, cw, ch);
      s.addText(v, { x: x + 0.3, y: y + 0.2, w: cw - 0.6, h: 0.85, fontFace: TF, fontSize: 40, bold: true, color: i === 0 ? RED3 : TXT, margin: 0, isTextBox: true, fit: 'shrink' });
      mono(s, l, x + 0.3, y + 1.05, cw - 0.6, 0.3, 8.5, FAINT);
      para(s, d, x + 0.3, y + 1.38, cw - 0.6, 0.7, 11, MUTED);
    });
  }

  // ── 17 · status & roadmap ───────────────────────────────────────────────
  {
    const s = base(
      'Where we are, honestly. Five services run and are tested, most other subsystems are tested libraries, and version 0.1.0-rc.1 is prepared: the edge on the real fabric, five services on one PQ-mTLS runtime, compose and Helm deployment, connectors, real model serving, and a release pipeline rehearsed end to end on the fifteenth of September. ' +
      'What blocks general availability is external or hardware-bound: HSM-backed key custody, GPU serving, FIPS 140-3 certification, and the key ceremony and signing runner that let us tag the release. The authoritative go/no-go record is GA_READINESS.md and it currently reads no-go, naming every blocker. The next ninety days are about closing those and putting live data through Forge into the fabric. (≈2 min)');
    eyebrow(s, 16, 'WHERE WE ARE', M, 0.55);
    title(s, 'Five services run and are tested. Release candidate prepared. GA gated on named blockers.', M, 0.9, 12, 24, 0.9);
    const cols = [
      ['DONE · v0.1.0-rc.1 PREPARED', SECURE, 'FiCheckCircle', [
        'The edge serves the real object fabric with one PQ-signed ledger',
        'Five services on one PQ-mTLS runtime: gateway, Mimir, Bifrost fleet, Heimdall feed, Forge worker',
        'Docker compose and a Helm chart; run on a 3-node kind cluster',
        'PostgreSQL, S3 and Kafka connectors; egress broker',
        'Real model serving from signed manifests (Granite on CPU)',
        'Release pipeline rehearsed end to end, 2026-09-15',
      ]],
      ['BEFORE GA · NAMED BLOCKERS', WARN, 'FiAlertTriangle', [
        'Key custody is software: keystores are files, no HSM wired in',
        'GPU serving (vLLM) is wired but has not run; 800 ms target unproven',
        'FIPS 140-3 module certification (external)',
        'Key ceremony and a provisioned signing runner, then tag',
        'Consoles other than Midgard\'s still run on sample data',
        'Single replica per service: recovery, not redundancy',
      ]],
      ['NEXT 90 DAYS', INFO, 'FiArrowRight', [
        'Tag v0.1.0-rc.1 and publish the draft pre-release',
        'Run the deployment tier on real HSM, GPU and multi-node hardware',
        'Live sources through Forge into the fabric; PostgreSQL TLS',
        'Wire Forge, Mimir, Heimdall and Bifrost consoles to their services',
        'Design-partner pilots on the compose and Helm paths',
        'Disclosure decision on the design documents',
      ]],
    ];
    const cw = (W - 2 * M - 2 * 0.3) / 3;
    for (let i = 0; i < 3; i++) {
      const [h, col, ic, items] = cols[i]; const x = M + i * (cw + 0.3), y = 2.0;
      card(s, x, y, cw, 4.75);
      await iconCircle(s, ic, col, x + 0.25, y + 0.25, 0.45);
      mono(s, h, x + 0.85, y + 0.33, cw - 1.0, 0.3, 9, col);
      bullets(s, items, x + 0.25, y + 0.95, cw - 0.5, 3.6, 11.5);
    }
    mono(s, 'GO / NO-GO RECORD · docs/compliance/GA_READINESS.md · CURRENTLY NO-GO, EVERY BLOCKER NAMED', M, 6.85, W - 2 * M, 0.25, 8, DIM);
  }

  // ── 18 · close ──────────────────────────────────────────────────────────
  {
    const s = base(
      'To close. Midgard runs on a laptop with one command, on docker compose with five services, on Kubernetes through a hardened Helm chart, and in air-gapped sites from a signed bundle. ' +
      'We are looking for three things: design partners willing to run a pilot on the compose or Helm path, security teams to review the open core, and feedback on the roadmap you just saw. The repository, the specification and the verification record are all public. Thank you. Questions. (≈1 min)');
    eyebrow(s, 17, 'NEXT STEPS', M, 0.55);
    title(s, 'Run it on a laptop today. Run it in an air-gapped site tomorrow.', M, 0.9, 12, 26, 0.9);
    const paths = [
      ['FiMonitor', 'Laptop', 'Development mode, one command. Real edge, real policy, real signed ledger, illustrative data.'],
      ['FiLayers', 'Docker compose', 'Five services, non-root, read-only filesystems, PQ-mTLS between them. just compose-up.'],
      ['FiServer', 'Kubernetes', 'Helm chart: hardened StatefulSets, default-deny network policies. Runs on a 3-node kind cluster.'],
      ['FiPackage', 'Air-gapped sites', 'One signed .mgbundle on media. Every artefact is verified offline, then installed.'],
    ];
    const cw = (W - 2 * M - 3 * 0.25) / 4;
    for (let i = 0; i < 4; i++) {
      const [ic, h, d] = paths[i]; const x = M + i * (cw + 0.25), y = 2.0;
      card(s, x, y, cw, 2.2);
      await iconCircle(s, ic, RED3, x + 0.25, y + 0.25, 0.5);
      para(s, h, x + 0.25, y + 0.9, cw - 0.5, 0.4, 15, TXT, { bold: true });
      para(s, d, x + 0.25, y + 1.3, cw - 0.5, 0.85, 11.5, MUTED);
    }
    card(s, M, 4.5, 7.4, 2.3, FILL2, RED);
    para(s, 'What we are asking for', M + 0.3, 4.7, 6.8, 0.4, 16, TXT, { bold: true });
    bullets(s, [
      'Design partners for a pilot on the compose or Helm path, with your own ontology and data.',
      'Security teams to review the open core: threat model, crypto backends, supply-chain gates.',
      'Feedback on the 90-day roadmap and the GA blockers you just saw.',
    ], M + 0.3, 5.2, 6.8, 1.5, 12.5);
    const cx = M + 7.4 + 0.3, cw2 = W - M - cx;
    card(s, cx, 4.5, cw2, 2.3);
    para(s, 'Veldris Ltd', cx + 0.3, 4.7, cw2 - 0.6, 0.4, 16, TXT, { bold: true });
    mono(s, 'github.com/veldris-ltd/midgard-app', cx + 0.3, 5.2, cw2 - 0.6, 0.3, 10, RED3);
    mono(s, 'hello@veldris.io  ·  sales@veldris.io', cx + 0.3, 5.55, cw2 - 0.6, 0.3, 10, MUTED);
    mono(s, 'security@veldris.io  ·  /.well-known/security.txt', cx + 0.3, 5.9, cw2 - 0.6, 0.3, 10, MUTED);
    mono(s, 'APACHE-2.0 CORE · CYCLONEDX SBOM PER RELEASE · REPRODUCIBLE BUILDS', cx + 0.3, 6.35, cw2 - 0.6, 0.3, 8, FAINT);
  }

  await pres.writeFile({ fileName: OUT });
  console.log('wrote', OUT, n, 'slides');
})().catch((e) => { console.error(e); process.exit(1); });
