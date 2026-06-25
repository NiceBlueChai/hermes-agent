/**
 * Resolves the ffmpeg binary used for WhatsApp voice-message conversion.
 */

import path from 'path';
import { existsSync } from 'fs';

/**
 * Return the preferred ffmpeg executable path for the current Hermes runtime.
 *
 * The managed binary under HERMES_HOME lets packaged installs use a bundled
 * ffmpeg without requiring users to install it separately. Falling back to the
 * plain command preserves PATH-based installs and existing developer setups.
 */
export function resolveFfmpegBinary(env = process.env, platform = process.platform) {
  const hermesHome = env?.HERMES_HOME;
  if (hermesHome) {
    const managedName = platform === 'win32' ? 'ffmpeg.exe' : 'ffmpeg';
    const managedPath = path.join(hermesHome, 'bin', managedName);
    if (existsSync(managedPath)) {
      return managedPath;
    }
  }

  return 'ffmpeg';
}
