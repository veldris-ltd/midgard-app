// Print the accessibility affordances whose audit test failed or did not run, from
// `xcrun xcresulttool get test-results tests` JSON. A test that was skipped, expected
// to fail or never ran is not a pass. [RC-49] [AC-C-11]
//   osascript -l JavaScript audit-failures.js <tests.json>
function run(argv) {
  const text = $.NSString.stringWithContentsOfFileEncodingError(argv[0], $.NSUTF8StringEncoding, null).js;
  const all = ['dynamic_type', 'screen_reader_labels', 'contrast_aa', 'focus_order', 'reduced_motion',
               'keyboard_navigation', 'hit_target_size'];
  const seen = {}; const failed = [];
  (function walk(nodes) {
    (nodes || []).forEach(function (n) {
      const m = /^test_affordance_([a-z_]+)\(/.exec(n.name || '');
      if (m && n.nodeType === 'Test Case') {
        seen[m[1]] = true;
        if (n.result !== 'Passed') failed.push(m[1]);
      }
      walk(n.children);
    });
  })(JSON.parse(text).testNodes);
  all.forEach(function (a) { if (!seen[a]) failed.push(a); }); // not measured = not passed
  return all.filter(function (a) { return failed.indexOf(a) >= 0; }).join(' ');
}
