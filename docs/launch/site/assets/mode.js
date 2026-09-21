/* Colour-mode bootstrap, applied before first paint. Same contract as the consoles:
   localStorage 'mg-mode' holds 'dark' or 'light'; dark is the platform default.
   A ?mode=light or ?mode=dark query wins, so a link can point at one set. */
(function () {
  var mode = 'dark';
  try {
    var q = new URLSearchParams(location.search).get('mode');
    var stored = localStorage.getItem('mg-mode');
    if (q === 'light' || q === 'dark') mode = q;
    else if (stored === 'light' || stored === 'dark') mode = stored;
  } catch (e) {}
  if (mode === 'light') document.documentElement.classList.add('mode-light');
  window.__mgMode = mode;
})();
