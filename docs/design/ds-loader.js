/* Midgard DS fallback loader.
   If the generated _ds_bundle.js is present, its namespace is used.
   Otherwise the component .jsx sources are fetched (sync) and compiled
   with Babel standalone so cards & UI kits render regardless. */
(function () {
  var PATHS = [
    'components/core/Button.jsx',
    'components/core/IconButton.jsx',
    'components/core/Badge.jsx',
    'components/core/Tag.jsx',
    'components/core/StatusPill.jsx',
    'components/forms/Input.jsx',
    'components/forms/Select.jsx',
    'components/forms/Switch.jsx',
    'components/data/Card.jsx',
    'components/data/Table.jsx',
    'components/data/StatBlock.jsx',
    'components/data/SectionLabel.jsx',
    'components/data/Terminal.jsx',
    'components/data/ProvenanceTag.jsx',
    'components/data/MeterBar.jsx',
    'components/nav/Tabs.jsx',
    'components/nav/SideNav.jsx',
    'components/governance/ClassificationBanner.jsx',
    'components/governance/HumanGate.jsx',
    'components/governance/MarkingChip.jsx',
    'components/admin/RiskBadge.jsx',
    'components/admin/GuardedAction.jsx',
    'components/admin/BreakGlass.jsx',
    'components/clients/SyncStatus.jsx',
    'components/clients/TierBadge.jsx',
    'components/clients/KeyCustodyChip.jsx',
    'components/media/Icon.jsx',
    'components/media/ProductMark.jsx'
  ];
  window.__mgNS = function () {
    if (window.MidgardDS && window.MidgardDS.Card && window.MidgardDS.Icon) return window.MidgardDS;
    var keys = Object.keys(window);
    for (var i = 0; i < keys.length; i++) {
      try {
        var v = window[keys[i]];
        if (v && v !== window && typeof v === 'object' &&
            typeof v.Icon === 'function' && typeof v.ProductMark === 'function' &&
            typeof v.ClassificationBanner === 'function' && typeof v.Card === 'function' &&
            typeof v.Table === 'function') {
          window.MidgardDS = v;
          return v;
        }
      } catch (e) {}
    }
    var tag = document.querySelector('script[src*="ds-loader.js"]');
    var base = tag ? tag.src.replace(/ds-loader\.js.*$/, '') : './';
    var ns = {};
    for (var j = 0; j < PATHS.length; j++) {
      try {
        var xhr = new XMLHttpRequest();
        xhr.open('GET', base + PATHS[j], false);
        xhr.send();
        if (xhr.status >= 400) continue;
        var src = xhr.responseText
          .replace(/^import[^\n]*$/mg, '')
          .replace(/^export (function|const)/mg, '$1');
        var code = Babel.transform(src, { presets: ['react'] }).code;
        var names = [];
        var re = /^(?:function|const) ([A-Za-z_$][\w$]*)/mg, m;
        while ((m = re.exec(src))) {
          if (/^[A-Z]/.test(m[1]) && m[1] !== 'PATHS') names.push(m[1]);
        }
        var fn = new Function('React', code + '\nreturn {' + names.join(',') + '};');
        Object.assign(ns, fn(window.React));
      } catch (e) { console.warn('ds-loader failed for', PATHS[j], e); }
    }
    window.MidgardDS = ns;
    return ns;
  };
})();
