/* Colour-mode bootstrap, applied before first paint. Same contract as the
   consoles: localStorage 'mg-mode' holds 'dark', 'light' or 'system'; dark is
   the platform default. A file rather than an inline script, so the page's
   CSP needs no inline allowance. [ADR-PLAY-003] */
(function () {
  try {
    var m = localStorage.getItem('mg-mode') || 'dark';
    var light = m === 'system' ? window.matchMedia('(prefers-color-scheme: light)').matches : m === 'light';
    if (light) document.documentElement.classList.add('mode-light');
  } catch (e) {}
})();
