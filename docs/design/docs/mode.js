/* Midgard docs, mode bootstrap (light / dark / system).
   Persists choice in localStorage('mg-mode'); resolves 'system' via
   prefers-color-scheme; applies .mode-light to <html>. */
(function () {
  function stored() { return localStorage.getItem('mg-mode') || 'dark'; }
  function resolve(m) {
    return m === 'system'
      ? (window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark')
      : m;
  }
  function apply() {
    document.documentElement.classList.toggle('mode-light', resolve(stored()) === 'light');
  }
  window.__mgMode = {
    get: stored,
    resolved: function () { return resolve(stored()); },
    set: function (m) {
      localStorage.setItem('mg-mode', m);
      apply();
      window.dispatchEvent(new Event('mg-mode'));
    },
  };
  try {
    window.matchMedia('(prefers-color-scheme: light)').addEventListener('change', function () {
      if (stored() === 'system') { apply(); window.dispatchEvent(new Event('mg-mode')); }
    });
  } catch (e) {}
  apply();
})();
