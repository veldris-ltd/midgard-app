/* Midgard launch kit: one switch swaps the page theme, the deck and the video together. */
(function () {
  'use strict';
  var KIT = window.KIT;
  var root = document.documentElement;
  var mode = window.__mgMode === 'light' ? 'light' : 'dark';
  var slide = 1;

  var $ = function (sel) { return document.querySelector(sel); };
  var all = function (sel) { return Array.prototype.slice.call(document.querySelectorAll(sel)); };
  var bind = function (name, value) { all('[data-bind="' + name + '"]').forEach(function (el) { el.textContent = value; }); };
  var pad = function (n) { return (n < 10 ? '0' : '') + n; };
  var mb = function (bytes) { return (bytes / 1048576).toFixed(1) + ' MB'; };
  var clock = function (s) { return Math.floor(s / 60) + ' min ' + pad(s % 60) + ' s'; };
  var media = function (m, file) { return 'media/' + m + '/' + file; };

  var video = $('#demo'), img = $('#slide'), stage = $('#stage'), thumbs = $('#thumbs'), notes = $('#notes');

  // ── slide viewer ─────────────────────────────────────────────────────────
  function buildThumbs() {
    thumbs.textContent = '';
    for (var i = 1; i <= KIT.slides; i++) {
      var b = document.createElement('button');
      b.type = 'button'; b.className = 'thumb'; b.setAttribute('role', 'tab');
      b.dataset.slide = String(i);
      b.setAttribute('aria-label', 'Slide ' + i + ': ' + KIT.titles[i - 1]);
      var t = document.createElement('img');
      t.loading = 'lazy'; t.decoding = 'async'; t.alt = ''; t.width = 320; t.height = 180;
      b.appendChild(t); thumbs.appendChild(b);
    }
  }
  function paintThumbs() {
    all('.thumb').forEach(function (b) {
      var n = Number(b.dataset.slide);
      b.firstChild.src = media(mode, 'thumbs/' + pad(n) + '.webp');
      b.setAttribute('aria-selected', n === slide ? 'true' : 'false');
      b.tabIndex = n === slide ? 0 : -1;
    });
  }
  function showSlide(n, opts) {
    slide = Math.min(KIT.slides, Math.max(1, n));
    img.src = media(mode, 'slides/' + pad(slide) + '.webp');
    img.alt = 'Slide ' + slide + ' of ' + KIT.slides + ': ' + KIT.titles[slide - 1];
    bind('slideNo', pad(slide)); bind('slideCount', String(KIT.slides)); bind('slideTitle', KIT.titles[slide - 1].toUpperCase());
    notes.textContent = KIT.notes[slide - 1] || '';
    $('[data-action="prev"]').disabled = slide === 1;
    $('[data-action="next"]').disabled = slide === KIT.slides;
    paintThumbs();
    var current = thumbs.querySelector('[aria-selected="true"]');
    if (current) {
      // Scrolls the strip only, never the page; instant on first paint and on a mode switch.
      thumbs.scrollTo({ left: current.offsetLeft - thumbs.offsetLeft - (thumbs.clientWidth - current.clientWidth) / 2, behavior: opts && opts.quiet ? 'auto' : 'smooth' });
    }
    [slide - 1, slide + 1].forEach(function (k) { if (k >= 1 && k <= KIT.slides) { new Image().src = media(mode, 'slides/' + pad(k) + '.webp'); } });
    if (!(opts && opts.quiet)) {
      try { history.replaceState(null, '', location.pathname + location.search + '#slide-' + slide); } catch (e) {}
    }
  }

  // ── mode ─────────────────────────────────────────────────────────────────
  function applyMode(next, opts) {
    mode = next === 'light' ? 'light' : 'dark';
    root.classList.toggle('mode-light', mode === 'light');
    try { localStorage.setItem('mg-mode', mode); } catch (e) {}
    var theme = document.querySelector('meta[name="theme-color"]');
    if (theme) theme.content = mode === 'light' ? '#eff0f3' : '#0a0b0d';
    all('.mode-btn').forEach(function (b) { b.setAttribute('aria-pressed', b.dataset.mode === mode ? 'true' : 'false'); });

    var m = KIT.modes[mode], other = mode === 'light' ? 'dark' : 'light';
    bind('modeName', mode.toUpperCase()); bind('otherName', other.toUpperCase());

    // Video: keep the player where it is only on first paint; a switch loads the other recording.
    var wasPlaying = !video.paused && !video.ended;
    video.pause();
    video.poster = media(mode, 'poster.jpg');
    video.src = media(mode, m.video);
    video.setAttribute('aria-label', 'Midgard demo video, ' + mode + ' mode, ' + clock(m.videoSeconds));
    if (wasPlaying && !(opts && opts.first)) { video.play().catch(function () {}); }
    bind('videoMeta', m.video.toUpperCase() + ' · 1920×1080 · ' + clock(m.videoSeconds).toUpperCase() + ' · ' + mb(m.videoBytes));
    all('[data-bind="videoHref"]').forEach(function (a) { a.href = media(mode, m.video); });
    all('[data-bind="deckHref"]').forEach(function (a) { a.href = media(mode, m.deck); });
    bind('deckSize', 'PPTX · ' + mb(m.deckBytes));

    all('[data-file]').forEach(function (a) {
      var parts = a.dataset.file.split('.'), fm = KIT.modes[parts[0]], kind = parts[1];
      a.href = media(parts[0], fm[kind]);
      a.textContent = fm[kind];
      var size = a.parentNode.querySelector('.size') || a.parentNode.appendChild(document.createElement('span'));
      size.className = 'size';
      size.textContent = mb(fm[kind + 'Bytes']) + (kind === 'video' ? ' · ' + clock(fm.videoSeconds) : ' · 18 SLIDES');
      a.parentNode.classList.toggle('current', parts[0] === mode);
    });
    showSlide(slide, { quiet: true });
  }

  // ── wiring ───────────────────────────────────────────────────────────────
  buildThumbs();
  var fromHash = /^#slide-(\d+)$/.exec(location.hash);
  if (fromHash) slide = Number(fromHash[1]);
  applyMode(mode, { first: true });
  if (fromHash) { showSlide(slide, { quiet: true }); }

  document.addEventListener('click', function (e) {
    var t = e.target.closest('[data-mode], [data-action], .thumb');
    if (!t) { if (e.target === img && !document.fullscreenElement) showSlide(slide + 1); return; }
    if (t.dataset.mode) return applyMode(t.dataset.mode);
    if (t.classList.contains('thumb')) return showSlide(Number(t.dataset.slide));
    switch (t.dataset.action) {
      case 'flip': return applyMode(mode === 'light' ? 'dark' : 'light');
      case 'prev': return showSlide(slide - 1);
      case 'next': return showSlide(slide + 1);
      case 'fullscreen':
        if (document.fullscreenElement) return document.exitFullscreen();
        if (stage.requestFullscreen) { stage.requestFullscreen().then(function () { stage.focus(); }).catch(function () {}); }
        return;
    }
  });

  document.addEventListener('keydown', function (e) {
    if (e.altKey || e.ctrlKey || e.metaKey) return;
    var inViewer = document.fullscreenElement === stage || (document.activeElement && document.activeElement.closest && document.activeElement.closest('#viewer'));
    if (!inViewer) return;
    var map = { ArrowRight: 1, ArrowDown: 1, PageDown: 1, ' ': 1, ArrowLeft: -1, ArrowUp: -1, PageUp: -1 };
    if (e.key === 'Home') { e.preventDefault(); return showSlide(1); }
    if (e.key === 'End') { e.preventDefault(); return showSlide(KIT.slides); }
    if (map[e.key] && !(e.key === ' ' && document.activeElement.tagName === 'BUTTON')) { e.preventDefault(); showSlide(slide + map[e.key]); }
  });

  var touchX = null;
  stage.addEventListener('touchstart', function (e) { touchX = e.changedTouches[0].clientX; }, { passive: true });
  stage.addEventListener('touchend', function (e) {
    if (touchX === null) return;
    var dx = e.changedTouches[0].clientX - touchX; touchX = null;
    if (Math.abs(dx) > 48) showSlide(slide + (dx < 0 ? 1 : -1));
  }, { passive: true });
})();
