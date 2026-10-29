/* Finding a tour target inside a console frame. The frame is same-origin, so
   the shell can read its document. A target is visible text: an element whose
   own text equals it (case-insensitive, whitespace collapsed) wins; otherwise
   the smallest visible element whose text contains it. Hidden elements never
   match. [ADR-PLAY-002] */

const norm = (s: string) => s.replace(/\s+/g, ' ').trim().toLowerCase();

const SKIP = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE', 'OPTION']);

function visible(el: Element): boolean {
  const r = el.getBoundingClientRect();
  if (r.width === 0 || r.height === 0) return false;
  const cs = el.ownerDocument.defaultView?.getComputedStyle(el);
  return !cs || (cs.visibility !== 'hidden' && cs.display !== 'none');
}

export function findTarget(doc: Document, text: string): HTMLElement | null {
  const want = norm(text);
  if (!want || !doc.body) return null;
  let exact: HTMLElement | null = null;
  let best: HTMLElement | null = null;
  let bestLen = Infinity;
  const walker = doc.createTreeWalker(doc.body, NodeFilter.SHOW_ELEMENT);
  for (let n = walker.nextNode(); n; n = walker.nextNode()) {
    const el = n as HTMLElement;
    if (SKIP.has(el.tagName)) continue;
    const t = norm(el.textContent ?? '');
    if (!t.includes(want) || !visible(el)) continue;
    if (t === want && !exact) exact = el;
    if (t.length < bestLen) {
      best = el;
      bestLen = t.length;
    }
  }
  // Prefer the control or element that is exactly the text, but if a smaller
  // element inside it also matches (a label inside a button), keep the outer one:
  // the ring should surround what the visitor clicks.
  return exact ?? best;
}

/* A ring drawn in the shell, positioned over an element inside the frame. It
   follows scrolling and resizing until stopped. */
export function trackRing(frame: HTMLIFrameElement, el: HTMLElement, ring: HTMLElement): () => void {
  let raf = 0;
  const place = () => {
    const fr = frame.getBoundingClientRect();
    const r = el.getBoundingClientRect();
    const pad = 4;
    const top = fr.top + r.top - pad;
    const left = fr.left + r.left - pad;
    const width = r.width + pad * 2;
    const height = r.height + pad * 2;
    // Hide the ring while the element is scrolled out of the frame.
    const inside = r.bottom > 0 && r.top < fr.height && r.right > 0 && r.left < fr.width;
    ring.hidden = !inside || !el.isConnected;
    ring.style.transform = `translate(${left}px, ${top}px)`;
    ring.style.width = `${width}px`;
    ring.style.height = `${height}px`;
    raf = requestAnimationFrame(place);
  };
  el.scrollIntoView({ block: 'center', inline: 'nearest' });
  place();
  return () => {
    cancelAnimationFrame(raf);
    ring.hidden = true;
  };
}
