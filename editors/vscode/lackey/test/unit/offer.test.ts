import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { remembered } from '../../src/offer';

test('only_ill_install_it_myself_and_a_missing_release_are_remembered', () => {
  assert.equal(remembered('installing it myself'), true);
  assert.equal(remembered('no release'), true);
  assert.equal(remembered('no target'), true);
});

test('a_failed_download_or_mismatch_is_offered_again_next_start', () => {
  assert.equal(remembered('download failed'), false);
  assert.equal(remembered('unreachable'), false);
  assert.equal(remembered('dismissed'), false);
  assert.equal(remembered('downloaded'), false);
});
