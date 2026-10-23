// Unit tests for the camera credits (`node --test`, types stripped by Node).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { cameraCredit } from '../src/app/cameraCredit.ts';

test('each recorded camera source is credited', () => {
  assert.equal(cameraCredit('JamCams_00001.07450'), 'POWERED BY TFL OPEN DATA');
  assert.match(cameraCredit('BCCAM-123') ?? '', /^DRIVEBC/);
  assert.match(cameraCredit('NZCAM-45') ?? '', /WAKA KOTAHI/);
  assert.match(cameraCredit('SGCAM-1701') ?? '', /DATA\.GOV\.SG/);
});

test('other cameras get no credit line', () => {
  assert.equal(cameraCredit('my-door-camera'), null);
  assert.equal(cameraCredit(''), null);
});
