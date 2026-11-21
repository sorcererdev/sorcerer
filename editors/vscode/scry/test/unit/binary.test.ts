import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { candidates, find } from '../../src/binary';

test('the setting comes first', () => {
  assert.deepEqual(candidates('/opt/scry', { PATH: '/usr/bin' }, '/home/me'),
    ['/opt/scry', '/usr/bin/scry', '/home/me/.cargo/bin/scry']);
});

test('PATH directories come in order, then ~/.cargo/bin', () => {
  assert.deepEqual(candidates('', { PATH: '/a:/b' }, '/home/me'),
    ['/a/scry', '/b/scry', '/home/me/.cargo/bin/scry']);
});

test('an empty setting is skipped', () => {
  assert.ok(!candidates('', { PATH: '/a' }, '/h').includes(''));
  assert.ok(!candidates('   ', { PATH: '/a' }, '/h').includes('   '));
  assert.equal(candidates('', {}, '/h')[0], '/h/.cargo/bin/scry');
});

test('find returns the first that exists, or undefined', () => {
  const c = ['/a/scry', '/b/scry', '/c/scry'];
  assert.equal(find(c, p => p !== '/a/scry'), '/b/scry');
  assert.equal(find(c, () => false), undefined);
  assert.equal(find([], () => true), undefined);
});
