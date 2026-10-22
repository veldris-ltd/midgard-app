// Unit tests for how alert labels read (`node --test`, types stripped by Node).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { alertText, alertWords } from '../src/app/alertMarks.ts';

test('labels read as words', () => {
  assert.equal(alertWords('camera-stopped-vehicle'), 'camera stopped vehicle');
  assert.equal(alertWords('power__cut-large'), 'power cut large');
  assert.equal(alertText('camera-stopped-vehicle'), 'CAMERA STOPPED VEHICLE');
});

test('GNSS reads as GPS, as the legend says', () => {
  assert.equal(alertText('gnss-interference-high'), 'GPS INTERFERENCE HIGH');
  assert.equal(alertWords('gnss-interference-medium'), 'GPS interference medium');
  assert.equal(alertText('GNSS_jamming'), 'GPS JAMMING');
  assert.equal(alertText('area-gnss'), 'AREA GPS');
  // Only the whole word.
  assert.equal(alertText('gnssx-test'), 'GNSSX TEST');
});
