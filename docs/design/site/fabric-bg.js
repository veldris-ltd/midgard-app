/* Midgard marketing site: "object fabric" background.
   A fixed, full-viewport canvas of drifting nodes joined by hairlines,
   sitting behind the page (z-index 0; page content stacks above at z-index 1).
   Interactive: nodes link to and drift away from the pointer, clicks emit a
   sealed-pulse ring that pushes the fabric, and scrolling parallaxes nodes by
   depth. Colours are read live from the design tokens (--accent on the body
   theme scope; grey ramp by mode) and re-read on the `mg-mode` event.
   Respects prefers-reduced-motion (single static frame, no interaction). */
(function () {
  'use strict';

  var reduced = false;
  try { reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches; } catch (e) {}

  var canvas, ctx, W = 0, H = 0, dpr = 1;
  var nodes = [], pulses = [];
  var pointer = { x: -9999, y: -9999, active: false };
  var lastScroll = window.scrollY || 0;
  var colors = { accent: [202, 29, 37], grey: [148, 158, 178], light: false };
  var running = false, lastT = 0;

  function hexToRgb(h) {
    h = (h || '').trim().replace('#', '');
    if (h.length === 3) h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2];
    if (!/^[0-9a-fA-F]{6}$/.test(h)) return null;
    return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
  }

  function readColors() {
    var acc = hexToRgb(getComputedStyle(document.body).getPropertyValue('--accent'));
    var light = document.documentElement.classList.contains('mode-light') ||
                document.body.classList.contains('mode-light');
    colors = {
      accent: acc || [202, 29, 37],
      grey: light ? [58, 66, 82] : [148, 158, 178],
      light: light,
    };
  }

  function rgba(c, a) { return 'rgba(' + c[0] + ',' + c[1] + ',' + c[2] + ',' + a + ')'; }

  function resize() {
    W = window.innerWidth; H = window.innerHeight;
    dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = W * dpr; canvas.height = H * dpr;
    canvas.style.width = W + 'px'; canvas.style.height = H + 'px';
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    seed();
    if (reduced) draw(0);
  }

  function seed() {
    var target = Math.min(150, Math.round((W * H) / 13000));
    nodes = [];
    for (var i = 0; i < target; i++) {
      var depth = 0.3 + Math.random() * 0.7; /* far → near */
      nodes.push({
        x: Math.random() * W, y: Math.random() * H,
        vx: (Math.random() - 0.5) * 7, vy: (Math.random() - 0.5) * 7, /* px/s drift */
        depth: depth,
        r: 0.7 + depth * 1.5,
        accent: Math.random() < 0.16,
        square: Math.random() < 0.28, /* a few blueprint-square nodes */
      });
    }
  }

  var MARGIN = 24;
  function wrap(n) {
    if (n.x < -MARGIN) n.x = W + MARGIN; else if (n.x > W + MARGIN) n.x = -MARGIN;
    if (n.y < -MARGIN) n.y = H + MARGIN; else if (n.y > H + MARGIN) n.y = -MARGIN;
  }

  var LINK_D = 96, POINTER_LINK = 170, POINTER_REPEL = 100;

  function step(dt) {
    var i, n;
    for (i = 0; i < nodes.length; i++) {
      n = nodes[i];
      n.x += n.vx * dt * n.depth;
      n.y += n.vy * dt * n.depth;
      /* gentle pointer repel */
      if (pointer.active) {
        var dx = n.x - pointer.x, dy = n.y - pointer.y;
        var d2 = dx * dx + dy * dy;
        if (d2 < POINTER_REPEL * POINTER_REPEL && d2 > 0.01) {
          var d = Math.sqrt(d2), f = (1 - d / POINTER_REPEL) * 26 * dt;
          n.x += (dx / d) * f; n.y += (dy / d) * f;
        }
      }
      /* damp any impulse back to base drift */
      n.vx += (Math.sign(n.vx) || 1) * 0 ; /* keep base drift */
      if (Math.abs(n.vx) > 7) n.vx *= Math.pow(0.35, dt);
      if (Math.abs(n.vy) > 7) n.vy *= Math.pow(0.35, dt);
      wrap(n);
    }
    /* pulses push the fabric outward */
    for (i = pulses.length - 1; i >= 0; i--) {
      var p = pulses[i];
      p.t += dt;
      if (p.t > 1.1) { pulses.splice(i, 1); continue; }
      var R = p.t * 420;
      for (var j = 0; j < nodes.length; j++) {
        n = nodes[j];
        var ddx = n.x - p.x, ddy = n.y - p.y;
        var dd = Math.sqrt(ddx * ddx + ddy * ddy) || 1;
        if (Math.abs(dd - R) < 46) {
          var k = (1 - Math.abs(dd - R) / 46) * 60 * dt;
          n.vx += (ddx / dd) * k; n.vy += (ddy / dd) * k;
        }
      }
    }
  }

  function draw(nowS) {
    ctx.clearRect(0, 0, W, H);
    var baseA = colors.light ? 0.5 : 0.55;
    var i, j, a, b;

    /* links: spatial hash to keep it O(n·k) */
    var cell = LINK_D, grid = {};
    for (i = 0; i < nodes.length; i++) {
      var key = (nodes[i].x / cell | 0) + '_' + (nodes[i].y / cell | 0);
      (grid[key] || (grid[key] = [])).push(nodes[i]);
    }
    ctx.lineWidth = 1;
    for (i = 0; i < nodes.length; i++) {
      a = nodes[i];
      var gx = a.x / cell | 0, gy = a.y / cell | 0;
      for (var ox = 0; ox <= 1; ox++) for (var oy = -1; oy <= 1; oy++) {
        if (ox === 0 && oy < 0) continue;
        var bucket = grid[(gx + ox) + '_' + (gy + oy)];
        if (!bucket) continue;
        for (j = 0; j < bucket.length; j++) {
          b = bucket[j];
          if (b === a || (ox === 0 && oy === 0 && nodes.indexOf(b) <= i)) continue;
          var dx = a.x - b.x, dy = a.y - b.y, d2 = dx * dx + dy * dy;
          if (d2 > LINK_D * LINK_D) continue;
          var t = 1 - Math.sqrt(d2) / LINK_D;
          var acc = a.accent || b.accent;
          ctx.strokeStyle = rgba(acc ? colors.accent : colors.grey, t * (acc ? 0.30 : 0.16) * baseA * ((a.depth + b.depth) / 2 + 0.4));
          ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y); ctx.stroke();
        }
      }
    }

    /* pointer links */
    if (pointer.active && !reduced) {
      for (i = 0; i < nodes.length; i++) {
        a = nodes[i];
        var pdx = a.x - pointer.x, pdy = a.y - pointer.y;
        var pd2 = pdx * pdx + pdy * pdy;
        if (pd2 > POINTER_LINK * POINTER_LINK) continue;
        var pt = 1 - Math.sqrt(pd2) / POINTER_LINK;
        ctx.strokeStyle = rgba(colors.accent, pt * 0.34 * baseA);
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(pointer.x, pointer.y); ctx.stroke();
      }
    }

    /* nodes */
    for (i = 0; i < nodes.length; i++) {
      a = nodes[i];
      var na = (0.25 + a.depth * 0.55) * baseA;
      ctx.fillStyle = rgba(a.accent ? colors.accent : colors.grey, a.accent ? Math.min(1, na + 0.2) : na);
      if (a.square) {
        var s = a.r * 2.1;
        ctx.fillRect(a.x - s / 2, a.y - s / 2, s, s);
      } else {
        ctx.beginPath(); ctx.arc(a.x, a.y, a.r, 0, 6.2832); ctx.fill();
      }
    }

    /* pulse rings */
    for (i = 0; i < pulses.length; i++) {
      var p = pulses[i];
      var fade = Math.max(0, 1 - p.t / 1.1);
      ctx.lineWidth = 1;
      ctx.strokeStyle = rgba(colors.accent, fade * 0.4);
      ctx.beginPath(); ctx.arc(p.x, p.y, p.t * 420, 0, 6.2832); ctx.stroke();
    }
  }

  function loop(now) {
    if (!running) return;
    var dt = Math.min(0.05, (now - lastT) / 1000 || 0.016);
    lastT = now;
    step(dt);
    draw(now / 1000);
    requestAnimationFrame(loop);
  }

  function start() {
    if (running || reduced) return;
    running = true; lastT = performance.now();
    requestAnimationFrame(loop);
  }
  function stop() { running = false; }

  function init() {
    canvas = document.createElement('canvas');
    canvas.setAttribute('aria-hidden', 'true');
    canvas.style.cssText = 'position:fixed;inset:0;z-index:0;pointer-events:none;';
    document.body.appendChild(canvas);
    ctx = canvas.getContext('2d');
    readColors();
    resize();

    window.addEventListener('resize', resize);
    window.addEventListener('mg-mode', function () { readColors(); if (reduced) draw(0); });

    if (reduced) { draw(0); return; }

    window.addEventListener('pointermove', function (e) {
      pointer.x = e.clientX; pointer.y = e.clientY; pointer.active = true;
    }, { passive: true });
    window.addEventListener('pointerleave', function () { pointer.active = false; });
    window.addEventListener('pointerdown', function (e) {
      if (pulses.length < 4) pulses.push({ x: e.clientX, y: e.clientY, t: 0 });
    }, { passive: true });
    window.addEventListener('scroll', function () {
      var y = window.scrollY || 0, delta = y - lastScroll; lastScroll = y;
      for (var i = 0; i < nodes.length; i++) {
        nodes[i].y -= delta * nodes[i].depth * 0.28;
        wrap(nodes[i]);
      }
    }, { passive: true });
    document.addEventListener('visibilitychange', function () {
      if (document.hidden) stop(); else start();
    });
    start();
  }

  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', init);
  else init();
})();
