/**
 * Regression tests for resolving the ffmpeg binary used by WhatsApp voice conversion.
 */

import test from 'node:test';
import assert from 'node:assert/strict';
import os from 'node:os';
import path from 'node:path';
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';

import { resolveFfmpegBinary } from './managed-ffmpeg.js';

test('resolveFfmpegBinary prefers the Hermes-managed binary when present', () => {
  const hermesHome = mkdtempSync(path.join(os.tmpdir(), 'hermes-wa-ffmpeg-'));

  try {
    const managedName = process.platform === 'win32' ? 'ffmpeg.exe' : 'ffmpeg';
    const managedPath = path.join(hermesHome, 'bin', managedName);
    mkdirSync(path.dirname(managedPath), { recursive: true });
    writeFileSync(managedPath, '');
    chmodSync(managedPath, 0o755);

    assert.equal(resolveFfmpegBinary({ HERMES_HOME: hermesHome }, process.platform), managedPath);
  } finally {
    rmSync(hermesHome, { recursive: true, force: true });
  }
});

test('resolveFfmpegBinary falls back to PATH lookup when Hermes has no managed binary', () => {
  const hermesHome = mkdtempSync(path.join(os.tmpdir(), 'hermes-wa-ffmpeg-'));

  try {
    assert.equal(resolveFfmpegBinary({ HERMES_HOME: hermesHome }, process.platform), 'ffmpeg');
  } finally {
    rmSync(hermesHome, { recursive: true, force: true });
  }
});
