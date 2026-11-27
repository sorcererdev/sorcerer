import * as assert from 'node:assert/strict';
import { test } from 'node:test';
import { assetFor, assetUrl, releaseBase } from '../../src/download';

test('lackey_assets_are_zip_on_windows_and_tar_gz_elsewhere', () => {
  assert.equal(assetFor('lackey', 'x86_64-pc-windows-msvc'), 'sorcerer-lackey-x86_64-pc-windows-msvc.zip');
  assert.equal(assetFor('lackey', 'aarch64-apple-darwin'), 'sorcerer-lackey-aarch64-apple-darwin.tar.gz');
});

test('lackey_is_downloaded_from_its_own_release_address', () => {
  assert.equal(assetUrl('http://h', 'lackey', '0.1.0', 'x86_64-unknown-linux-gnu'),
    'http://h/v0.1.0/sorcerer-lackey-x86_64-unknown-linux-gnu.tar.gz');
  assert.equal(releaseBase('lackey', { LACKEY_RELEASES_URL: 'http://127.0.0.1:9/r/' }), 'http://127.0.0.1:9/r');
});
