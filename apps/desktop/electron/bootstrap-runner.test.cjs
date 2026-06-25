const assert = require('node:assert/strict')
const test = require('node:test')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')

const {
  runBootstrap,
  probeNativeBootstrapCapabilities,
  probeNativeBootstrapManifest,
  recordInstallMetadata,
  runNativeBootstrapStage,
  resolveHermesManagerPath,
  resolveInstallScript,
  installedAgentInstallScript,
  cachedScriptPath
} = require('./bootstrap-runner.cjs')

const SCRIPT_NAME = process.platform === 'win32' ? 'install.ps1' : 'install.sh'

function mkTmpHome() {
  return fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-bootstrap-test-'))
}

test('runBootstrap bails immediately when the signal is already aborted', async () => {
  const controller = new AbortController()
  controller.abort()

  const events = []
  const result = await runBootstrap({
    installStamp: null,
    activeRoot: '/tmp/hermes-runner-test',
    sourceRepoRoot: null,
    hermesHome: '/tmp/hermes-runner-test',
    logRoot: '/tmp/hermes-runner-test',
    onEvent: ev => events.push(ev),
    abortSignal: controller.signal
  })

  // Cancelled before any install script is spawned.
  assert.deepEqual(result, { ok: false, cancelled: true })
  assert.ok(
    events.some(ev => ev.type === 'failed' && /cancelled/i.test(ev.error)),
    'should emit a cancelled failure event'
  )
})

test('installedAgentInstallScript resolves the installer in the agent checkout', () => {
  const home = mkTmpHome()
  try {
    assert.equal(installedAgentInstallScript(home), null, 'absent before the checkout exists')

    const scriptsDir = path.join(home, 'hermes-agent', 'scripts')
    fs.mkdirSync(scriptsDir, { recursive: true })
    const scriptPath = path.join(scriptsDir, SCRIPT_NAME)
    fs.writeFileSync(scriptPath, '#!/bin/sh\necho hi\n')

    assert.equal(installedAgentInstallScript(home), scriptPath)
    assert.equal(installedAgentInstallScript(null), null, 'null home -> null')
  } finally {
    fs.rmSync(home, { recursive: true, force: true })
  }
})

test('resolveInstallScript prefers a cached script without touching the network', async () => {
  const home = mkTmpHome()
  try {
    const commit = 'a'.repeat(40)
    const cached = cachedScriptPath(home, commit)
    fs.mkdirSync(path.dirname(cached), { recursive: true })
    fs.writeFileSync(cached, '#!/bin/sh\necho cached\n')

    const logs = []
    const result = await resolveInstallScript({
      installStamp: { commit },
      sourceRepoRoot: null,
      hermesHome: home,
      emit: ev => logs.push(ev)
    })

    assert.equal(result.source, 'cache')
    assert.equal(result.path, cached)
  } finally {
    fs.rmSync(home, { recursive: true, force: true })
  }
})

test('resolveInstallScript falls back to the installed agent checkout on a 404', async () => {
  const home = mkTmpHome()
  try {
    const commit = 'a'.repeat(40)
    // Seed the installed agent checkout so the fallback has something to resolve.
    const scriptsDir = path.join(home, 'hermes-agent', 'scripts')
    fs.mkdirSync(scriptsDir, { recursive: true })
    const installed = path.join(scriptsDir, SCRIPT_NAME)
    fs.writeFileSync(installed, '#!/bin/sh\necho fallback\n')

    const logs = []
    const result = await resolveInstallScript({
      installStamp: { commit },
      sourceRepoRoot: null,
      hermesHome: home,
      emit: ev => logs.push(ev),
      // Simulate GitHub returning a 404 for the pinned commit.
      _download: async () => {
        throw new Error('Failed to download install.sh: HTTP 404')
      }
    })

    assert.equal(result.source, 'installed-agent')
    // It should have copied the installer into the bootstrap cache.
    assert.equal(result.path, cachedScriptPath(home, commit))
    assert.ok(fs.existsSync(result.path), 'fallback script copied into cache')
    assert.ok(
      logs.some(ev => /falling back to installed agent/.test(ev.line || '')),
      'emits a fallback log line'
    )
  } finally {
    fs.rmSync(home, { recursive: true, force: true })
  }
})

test('resolveInstallScript rethrows when the 404 fallback is unavailable', async () => {
  const home = mkTmpHome()
  try {
    const commit = 'a'.repeat(40)
    // No installed agent checkout seeded -> nothing to fall back to.
    await assert.rejects(
      resolveInstallScript({
        installStamp: { commit },
        sourceRepoRoot: null,
        hermesHome: home,
        emit: () => {},
        _download: async () => {
          throw new Error('Failed to download install.sh: HTTP 404')
        }
      }),
      /HTTP 404|Failed to download/
    )
  } finally {
    fs.rmSync(home, { recursive: true, force: true })
  }
})

test('resolveHermesManagerPath returns the packaged manager only when present', () => {
  assert.equal(
    resolveHermesManagerPath('C:\\Hermes\\resources', 'win32', file => {
      return file === 'C:\\Hermes\\resources\\hermes-manager\\hermes-manager.exe'
    }),
    'C:\\Hermes\\resources\\hermes-manager\\hermes-manager.exe'
  )
  assert.equal(
    resolveHermesManagerPath('/opt/Hermes/resources', 'linux', file => {
      return file === '/opt/Hermes/resources/hermes-manager/hermes-manager'
    }),
    '/opt/Hermes/resources/hermes-manager/hermes-manager'
  )
  assert.equal(resolveHermesManagerPath('/missing/resources', 'linux', () => false), null)
})

test('recordInstallMetadata runs the native install-metadata bootstrap stage', () => {
  const calls = []
  const ok = recordInstallMetadata({
    hermesHome: 'C:\\Users\\x\\.hermes',
    resourcesPath: 'C:\\Hermes\\resources',
    platform: 'win32',
    exists: file => file.endsWith('hermes-manager.exe'),
    _execFileSync: (command, args, options) => {
      calls.push({ command, args, options })
      return Buffer.from(JSON.stringify({ ok: true, stage: 'install-metadata' }))
    }
  })

  assert.equal(ok, true)
  assert.equal(calls[0].command, 'C:\\Hermes\\resources\\hermes-manager\\hermes-manager.exe')
  assert.deepEqual(calls[0].args, [
    '--hermes-home',
    'C:\\Users\\x\\.hermes',
    '--json',
    'bootstrap-stage',
    'install-metadata'
  ])
  assert.equal(calls[0].options.cwd, 'C:\\Users\\x\\.hermes')
  assert.deepEqual(calls[0].options.stdio, ['ignore', 'pipe', 'ignore'])
})

test('probeNativeBootstrapCapabilities parses manager bootstrap bridge support', () => {
  const probe = probeNativeBootstrapCapabilities({
    hermesHome: '/home/x/.hermes',
    resourcesPath: '/opt/Hermes/resources',
    platform: 'linux',
    exists: file => file.endsWith('/hermes-manager'),
    _execFileSync: (command, args, options) => {
      assert.equal(command, '/opt/Hermes/resources/hermes-manager/hermes-manager')
      assert.deepEqual(args, [
        '--hermes-home',
        '/home/x/.hermes',
        '--json',
        'bootstrap-capabilities'
      ])
      assert.equal(options.cwd, '/home/x/.hermes')
      return Buffer.from(
        JSON.stringify({
          ok: true,
          command: 'bootstrap-capabilities',
          schemaVersion: 1,
          canRunFullBootstrap: false,
          supportedStages: ['install-metadata']
        })
      )
    }
  })

  assert.deepEqual(probe, {
    available: true,
    canRunFullBootstrap: false,
    supportedStages: ['install-metadata']
  })
})

test('probeNativeBootstrapManifest parses manager bridge stages', () => {
  const probe = probeNativeBootstrapManifest({
    hermesHome: '/home/x/.hermes',
    resourcesPath: '/opt/Hermes/resources',
    platform: 'linux',
    exists: file => file.endsWith('/hermes-manager'),
    _execFileSync: (command, args, options) => {
      assert.equal(command, '/opt/Hermes/resources/hermes-manager/hermes-manager')
      assert.deepEqual(args, [
        '--hermes-home',
        '/home/x/.hermes',
        '--json',
        'bootstrap-manifest'
      ])
      assert.equal(options.cwd, '/home/x/.hermes')
      return Buffer.from(
        JSON.stringify({
          ok: true,
          command: 'bootstrap-manifest',
          protocol_version: 1,
          stages: [
            {
              name: 'install-metadata',
              title: 'Record install metadata',
              category: 'finalize',
              needs_user_input: false
            }
          ]
        })
      )
    }
  })

  assert.equal(probe.available, true)
  assert.equal(probe.protocolVersion, 1)
  assert.deepEqual(probe.stages.map(stage => stage.name), ['install-metadata'])
})

test('runNativeBootstrapStage passes install pins to manager stage command', async () => {
  const calls = []
  const ev = await runNativeBootstrapStage({
    stage: { name: 'bootstrap-marker' },
    hermesHome: 'C:\\Users\\x\\.hermes',
    activeRoot: 'C:\\Users\\x\\.hermes\\hermes-agent',
    resourcesPath: 'C:\\Hermes\\resources',
    platform: 'win32',
    installStamp: { commit: 'abcdef1234567890', branch: 'main' },
    exists: file => file.endsWith('hermes-manager.exe'),
    _execFileSync: (command, args, options) => {
      calls.push({ command, args, options })
      return Buffer.from(JSON.stringify({ ok: true, stage: 'bootstrap-marker', skipped: false }))
    }
  })

  assert.equal(ev.state, 'succeeded')
  assert.deepEqual(calls[0].args, [
    '--hermes-home',
    'C:\\Users\\x\\.hermes',
    '--json',
    'bootstrap-stage',
    'bootstrap-marker',
    '--install-root',
    'C:\\Users\\x\\.hermes\\hermes-agent',
    '--wheelhouse-dir',
    'C:\\Hermes\\resources\\wheelhouse',
    '--bootstrap-tools-dir',
    'C:\\Hermes\\resources\\bootstrap-tools',
    '--commit',
    'abcdef1234567890',
    '--branch',
    'main'
  ])
})

test('runBootstrap records install metadata through the native manager hook after success', async () => {
  const home = mkTmpHome()
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-bootstrap-repo-'))
  try {
    const scripts = path.join(repo, 'scripts')
    fs.mkdirSync(scripts, { recursive: true })
    const script = path.join(scripts, SCRIPT_NAME)
    fs.writeFileSync(script, fakeInstallerScript(), { mode: 0o755 })

    const calls = []
    const events = []
    const result = await runBootstrap({
      installStamp: { commit: 'a'.repeat(40), branch: 'main' },
      activeRoot: path.join(home, 'hermes-agent'),
      sourceRepoRoot: repo,
      hermesHome: home,
      logRoot: path.join(home, 'logs'),
      onEvent: ev => events.push(ev),
      writeMarker: payload => ({ ...payload, schemaVersion: 1 }),
      _recordInstallMetadata: args => calls.push(args)
    })

    assert.equal(result.ok, true)
    assert.equal(calls.length, 1)
    assert.equal(calls[0].hermesHome, home)
  assert.ok(events.some(ev => ev.type === 'complete'), 'bootstrap should complete')
  } finally {
    fs.rmSync(home, { recursive: true, force: true })
    fs.rmSync(repo, { recursive: true, force: true })
  }
})

test('runBootstrap dispatches manifest-matched native stages through the manager bridge', async () => {
  const home = mkTmpHome()
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-bootstrap-repo-'))
  try {
    const scripts = path.join(repo, 'scripts')
    fs.mkdirSync(scripts, { recursive: true })
    const script = path.join(scripts, SCRIPT_NAME)
    fs.writeFileSync(
      script,
      fakeInstallerScript({ stageName: 'install-metadata', stageOk: false }),
      { mode: 0o755 }
    )

    const nativeCalls = []
    const events = []
    const result = await runBootstrap({
      installStamp: { commit: 'a'.repeat(40), branch: 'main' },
      activeRoot: path.join(home, 'hermes-agent'),
      sourceRepoRoot: repo,
      hermesHome: home,
      logRoot: path.join(home, 'logs'),
      resourcesPath: '/opt/Hermes/resources',
      platform: 'linux',
      onEvent: ev => events.push(ev),
      writeMarker: payload => ({ ...payload, schemaVersion: 1 }),
      _recordInstallMetadata: () => true,
      _probeNativeBootstrapCapabilities: () => ({
        available: true,
        canRunFullBootstrap: false,
        supportedStages: ['install-metadata']
      }),
      _probeNativeBootstrapManifest: () => ({
        available: true,
        protocolVersion: 1,
        stages: [{ name: 'install-metadata', title: 'Record install metadata' }]
      }),
      _runNativeBootstrapStage: async ({ stage }) => {
        nativeCalls.push(stage.name)
        return {
          type: 'stage',
          name: stage.name,
          state: 'succeeded',
          durationMs: 1,
          json: { ok: true, stage: stage.name }
        }
      }
    })

    assert.equal(result.ok, true)
    assert.deepEqual(nativeCalls, ['install-metadata'])
    assert.ok(
      events.some(ev => ev.type === 'stage' && ev.name === 'install-metadata' && ev.state === 'succeeded'),
      'native stage success should be emitted'
    )
  } finally {
    fs.rmSync(home, { recursive: true, force: true })
    fs.rmSync(repo, { recursive: true, force: true })
  }
})

test('runBootstrap uses native manifest without installer script when full native bootstrap is gated on', async () => {
  const home = mkTmpHome()
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-bootstrap-empty-repo-'))
  try {
    const stages = [{ name: 'install-metadata', title: 'Record install metadata' }]
    const nativeCalls = []
    const events = []
    const result = await runBootstrap({
      installStamp: null,
      activeRoot: path.join(home, 'hermes-agent'),
      sourceRepoRoot: repo,
      hermesHome: home,
      logRoot: path.join(home, 'logs'),
      resourcesPath: '/opt/Hermes/resources',
      platform: 'linux',
      onEvent: ev => events.push(ev),
      writeMarker: payload => ({ ...payload, schemaVersion: 1 }),
      _recordInstallMetadata: () => true,
      _probeNativeBootstrapCapabilities: () => ({
        available: true,
        canRunFullBootstrap: true,
        supportedStages: stages.map(stage => stage.name)
      }),
      _probeNativeBootstrapManifest: () => ({
        available: true,
        protocolVersion: 1,
        stages
      }),
      _runNativeBootstrapStage: async ({ stage }) => {
        nativeCalls.push(stage.name)
        return {
          type: 'stage',
          name: stage.name,
          state: 'succeeded',
          durationMs: 1,
          runner: 'native',
          json: { ok: true, stage: stage.name }
        }
      }
    })

    assert.equal(result.ok, true)
    assert.deepEqual(nativeCalls, ['install-metadata'])
    assert.ok(
      events.some(ev => ev.type === 'manifest' && ev.protocolVersion === 1),
      'native manifest should be emitted through the normal manifest event'
    )
    assert.ok(
      events.some(ev => ev.type === 'log' && /using full native bootstrap manifest/.test(ev.line || '')),
      'full native bootstrap should be logged'
    )
  } finally {
    fs.rmSync(home, { recursive: true, force: true })
    fs.rmSync(repo, { recursive: true, force: true })
  }
})

test('runBootstrap dispatches native path stage when the bridge advertises it', async () => {
  const home = mkTmpHome()
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-bootstrap-repo-'))
  try {
    const scripts = path.join(repo, 'scripts')
    fs.mkdirSync(scripts, { recursive: true })
    const script = path.join(scripts, SCRIPT_NAME)
    fs.writeFileSync(script, fakeInstallerScript({ stageName: 'path', stageOk: false }), { mode: 0o755 })

    const nativeCalls = []
    const result = await runBootstrap({
      installStamp: { commit: 'a'.repeat(40), branch: 'main' },
      activeRoot: path.join(home, 'hermes-agent'),
      sourceRepoRoot: repo,
      hermesHome: home,
      logRoot: path.join(home, 'logs'),
      resourcesPath: 'C:\\Hermes\\resources',
      platform: 'win32',
      onEvent: () => {},
      writeMarker: payload => ({ ...payload, schemaVersion: 1 }),
      _recordInstallMetadata: () => true,
      _probeNativeBootstrapCapabilities: () => ({
        available: true,
        canRunFullBootstrap: false,
        supportedStages: ['path']
      }),
      _probeNativeBootstrapManifest: () => ({
        available: true,
        protocolVersion: 1,
        stages: [{ name: 'path', title: 'Add Hermes to PATH' }]
      }),
      _runNativeBootstrapStage: async ({ stage, installStamp }) => {
        nativeCalls.push({ name: stage.name, installStamp })
        return {
          type: 'stage',
          name: stage.name,
          state: 'succeeded',
          durationMs: 1,
          json: { ok: true, stage: stage.name }
        }
      }
    })

    assert.equal(result.ok, true)
    assert.deepEqual(nativeCalls.map(call => call.name), ['path'])
    assert.equal(nativeCalls[0].installStamp.commit, 'a'.repeat(40))
  } finally {
    fs.rmSync(home, { recursive: true, force: true })
    fs.rmSync(repo, { recursive: true, force: true })
  }
})

test('runBootstrap dispatches non-interactive stages through the native bridge', async () => {
  const home = mkTmpHome()
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-bootstrap-repo-'))
  try {
    const scripts = path.join(repo, 'scripts')
    fs.mkdirSync(scripts, { recursive: true })
    const script = path.join(scripts, SCRIPT_NAME)
    fs.writeFileSync(
      script,
      fakeInstallerScript({
        manifestStages: [
          { name: 'configure', title: 'Configure API keys and models', needs_user_input: true },
          { name: 'gateway', title: 'Starting messaging gateway', needs_user_input: true }
        ],
        stageOk: false
      }),
      { mode: 0o755 }
    )

    const nativeCalls = []
    const result = await runBootstrap({
      installStamp: { commit: 'a'.repeat(40), branch: 'main' },
      activeRoot: path.join(home, 'hermes-agent'),
      sourceRepoRoot: repo,
      hermesHome: home,
      logRoot: path.join(home, 'logs'),
      resourcesPath: 'C:\\Hermes\\resources',
      platform: 'win32',
      onEvent: () => {},
      writeMarker: payload => ({ ...payload, schemaVersion: 1 }),
      _recordInstallMetadata: () => true,
      _probeNativeBootstrapCapabilities: () => ({
        available: true,
        canRunFullBootstrap: false,
        supportedStages: ['configure', 'gateway']
      }),
      _probeNativeBootstrapManifest: () => ({
        available: true,
        protocolVersion: 1,
        stages: [
          { name: 'configure', title: 'Configure API keys and models', needs_user_input: true },
          { name: 'gateway', title: 'Starting messaging gateway', needs_user_input: true }
        ]
      }),
      _runNativeBootstrapStage: async ({ stage }) => {
        nativeCalls.push(stage.name)
        return {
          type: 'stage',
          name: stage.name,
          state: 'skipped',
          durationMs: 1,
          runner: 'native',
          json: { ok: true, stage: stage.name, skipped: true }
        }
      }
    })

    assert.equal(result.ok, true)
    assert.deepEqual(nativeCalls, ['configure', 'gateway'])
  } finally {
    fs.rmSync(home, { recursive: true, force: true })
    fs.rmSync(repo, { recursive: true, force: true })
  }
})

test('runBootstrap keeps current Windows non-interactive stage budget native-covered', async () => {
  const home = mkTmpHome()
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-bootstrap-repo-'))
  try {
    const stages = [
      { name: 'uv', title: 'Installing uv package manager', category: 'prereqs', needs_user_input: false },
      { name: 'python', title: 'Verifying Python 3.11', category: 'prereqs', needs_user_input: false },
      { name: 'git', title: 'Installing Git', category: 'prereqs', needs_user_input: false },
      { name: 'node', title: 'Detecting Node.js', category: 'prereqs', needs_user_input: false },
      { name: 'system-packages', title: 'Installing ripgrep and ffmpeg', category: 'prereqs', needs_user_input: false },
      { name: 'repository', title: 'Cloning Hermes repository', category: 'install', needs_user_input: false },
      { name: 'venv', title: 'Creating Python virtual environment', category: 'install', needs_user_input: false },
      { name: 'dependencies', title: 'Installing Python dependencies', category: 'install', needs_user_input: false },
      { name: 'node-deps', title: 'Installing Node.js dependencies', category: 'install', needs_user_input: false },
      { name: 'desktop', title: 'Building desktop app', category: 'install', needs_user_input: false },
      { name: 'path', title: 'Adding Hermes to PATH', category: 'finalize', needs_user_input: false },
      {
        name: 'config-templates',
        title: 'Writing configuration templates',
        category: 'finalize',
        needs_user_input: false
      },
      {
        name: 'platform-sdks',
        title: 'Installing messaging platform SDKs',
        category: 'finalize',
        needs_user_input: false
      },
      { name: 'bootstrap-marker', title: 'Marking install complete', category: 'finalize', needs_user_input: false }
    ]
    const scripts = path.join(repo, 'scripts')
    fs.mkdirSync(scripts, { recursive: true })
    const script = path.join(scripts, SCRIPT_NAME)
    fs.writeFileSync(
      script,
      fakeInstallerScript({
        stageName: 'script-budget-regression',
        manifestStages: stages,
        stageOk: false
      }),
      { mode: 0o755 }
    )

    const nativeCalls = []
    const events = []
    const result = await runBootstrap({
      installStamp: { commit: 'a'.repeat(40), branch: 'main' },
      activeRoot: path.join(home, 'hermes-agent'),
      sourceRepoRoot: repo,
      hermesHome: home,
      logRoot: path.join(home, 'logs'),
      resourcesPath: 'C:\\Hermes\\resources',
      platform: 'win32',
      onEvent: ev => events.push(ev),
      writeMarker: payload => ({ ...payload, schemaVersion: 1 }),
      _recordInstallMetadata: () => true,
      _probeNativeBootstrapCapabilities: () => ({
        available: true,
        canRunFullBootstrap: false,
        supportedStages: stages.map(stage => stage.name)
      }),
      _probeNativeBootstrapManifest: () => ({
        available: true,
        protocolVersion: 1,
        stages
      }),
      _runNativeBootstrapStage: async ({ stage }) => {
        nativeCalls.push(stage.name)
        return {
          type: 'stage',
          name: stage.name,
          state: 'succeeded',
          durationMs: 1,
          runner: 'native',
          json: { ok: true, stage: stage.name }
        }
      }
    })

    assert.equal(result.ok, true)
    assert.deepEqual(nativeCalls, stages.map(stage => stage.name))
    assert.ok(!events.some(ev => /script fallback should not run/.test(ev.error || ev.line || '')))
  } finally {
    fs.rmSync(home, { recursive: true, force: true })
    fs.rmSync(repo, { recursive: true, force: true })
  }
})

test('runBootstrap keeps script execution for stages absent from the native bridge manifest', async () => {
  const home = mkTmpHome()
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-bootstrap-repo-'))
  try {
    const scripts = path.join(repo, 'scripts')
    fs.mkdirSync(scripts, { recursive: true })
    const script = path.join(scripts, SCRIPT_NAME)
    fs.writeFileSync(
      script,
      fakeDynamicInstallerScript({
        manifestStages: [
          { name: 'venv', title: 'Creating virtual environment', needs_user_input: false },
          { name: 'dependencies', title: 'Installing Python dependencies', needs_user_input: false }
        ]
      }),
      { mode: 0o755 }
    )

    const events = []
    const nativeCalls = []
    const result = await runBootstrap({
      installStamp: { commit: 'a'.repeat(40), branch: 'main' },
      activeRoot: path.join(home, 'hermes-agent'),
      sourceRepoRoot: repo,
      hermesHome: home,
      logRoot: path.join(home, 'logs'),
      resourcesPath: 'C:\\Hermes\\resources',
      platform: 'win32',
      onEvent: ev => events.push(ev),
      writeMarker: payload => ({ ...payload, schemaVersion: 1 }),
      _recordInstallMetadata: () => true,
      _probeNativeBootstrapCapabilities: () => ({
        available: true,
        canRunFullBootstrap: false,
        supportedStages: ['python']
      }),
      _probeNativeBootstrapManifest: () => ({
        available: true,
        protocolVersion: 1,
        stages: [{ name: 'python', title: 'Verify Python 3.11', needs_user_input: false }]
      }),
      _runNativeBootstrapStage: async ({ stage }) => {
        nativeCalls.push(stage.name)
        return {
          type: 'stage',
          name: stage.name,
          state: 'succeeded',
          durationMs: 1,
          runner: 'native',
          json: { ok: true, stage: stage.name }
        }
      }
    })

    assert.equal(result.ok, true)
    assert.deepEqual(nativeCalls, [])
    assert.ok(events.some(ev => ev.type === 'stage' && ev.name === 'venv' && ev.state === 'succeeded'))
    assert.ok(events.some(ev => ev.type === 'stage' && ev.name === 'dependencies' && ev.state === 'succeeded'))
    assert.ok(!events.some(ev => ev.runner === 'native' && ['venv', 'dependencies'].includes(ev.name)))
  } finally {
    fs.rmSync(home, { recursive: true, force: true })
    fs.rmSync(repo, { recursive: true, force: true })
  }
})

test('runBootstrap falls back to script when native stage asks for fallback', async () => {
  const home = mkTmpHome()
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), 'hermes-bootstrap-repo-'))
  try {
    const scripts = path.join(repo, 'scripts')
    fs.mkdirSync(scripts, { recursive: true })
    const script = path.join(scripts, SCRIPT_NAME)
    fs.writeFileSync(script, fakeInstallerScript({ stageName: 'bootstrap-marker' }), { mode: 0o755 })

    const events = []
    const result = await runBootstrap({
      installStamp: { commit: 'a'.repeat(40), branch: 'main' },
      activeRoot: path.join(home, 'hermes-agent'),
      sourceRepoRoot: repo,
      hermesHome: home,
      logRoot: path.join(home, 'logs'),
      resourcesPath: '/opt/Hermes/resources',
      platform: 'linux',
      onEvent: ev => events.push(ev),
      writeMarker: payload => ({ ...payload, schemaVersion: 1 }),
      _recordInstallMetadata: () => true,
      _probeNativeBootstrapCapabilities: () => ({
        available: true,
        canRunFullBootstrap: false,
        supportedStages: ['bootstrap-marker']
      }),
      _probeNativeBootstrapManifest: () => ({
        available: true,
        protocolVersion: 1,
        stages: [{ name: 'bootstrap-marker', title: 'Mark install complete' }]
      }),
      _runNativeBootstrapStage: async ({ stage }) => ({
        type: 'stage',
        name: stage.name,
        state: 'failed',
        durationMs: 1,
        runner: 'native',
        fallbackToScript: true,
        error: 'native stage unavailable at runtime'
      })
    })

    assert.equal(result.ok, true)
    assert.ok(
      events.some(ev => ev.type === 'log' && /falling back to script stage bootstrap-marker/.test(ev.line || '')),
      'fallback should be logged'
    )
    assert.ok(
      events.some(ev => ev.type === 'stage' && ev.name === 'bootstrap-marker' && ev.state === 'succeeded'),
      'script stage should succeed after native fallback'
    )
  } finally {
    fs.rmSync(home, { recursive: true, force: true })
    fs.rmSync(repo, { recursive: true, force: true })
  }
})

function fakeDynamicInstallerScript(options = {}) {
  const manifestStages = options.manifestStages || [
    { name: 'metadata', title: 'Metadata', category: 'install', needs_user_input: false }
  ]
  const manifestJson = JSON.stringify({ stages: manifestStages, protocol_version: 1 })
  if (process.platform === 'win32') {
    return [
      'param(',
      '  [switch]$Manifest,',
      '  [string]$Stage,',
      '  [switch]$NonInteractive,',
      '  [switch]$Json,',
      '  [string]$Commit,',
      '  [string]$Branch',
      ')',
      'if ($Manifest) {',
      `  Write-Output '${manifestJson}'`,
      '  exit 0',
      '}',
      'if ($Stage) {',
      '  Write-Output "{`"ok`":true,`"stage`":`"$Stage`"}"',
      '  exit 0',
      '}',
      'exit 1',
      ''
    ].join('\r\n')
  }
  return [
    '#!/usr/bin/env sh',
    'if [ "$1" = "--manifest" ]; then',
    `  printf '%s\\n' '${manifestJson}'`,
    '  exit 0',
    'fi',
    'if [ "$1" = "--stage" ]; then',
    '  printf \'{"ok":true,"stage":"%s"}\\n\' "$2"',
    '  exit 0',
    'fi',
    'exit 1',
    ''
  ].join('\n')
}

function fakeInstallerScript(options = {}) {
  const stageName = options.stageName || 'metadata'
  const stageOk = options.stageOk !== false
  const manifestStages = options.manifestStages || [
    { name: stageName, title: 'Metadata', category: 'install', needs_user_input: false }
  ]
  const manifestJson = JSON.stringify({ stages: manifestStages, protocol_version: 1 })
  const stageJson = stageOk
    ? `{"ok":true,"stage":"${stageName}"}`
    : `{"ok":false,"stage":"${stageName}","reason":"script fallback should not run"}`
  if (process.platform === 'win32') {
    return [
      'param(',
      '  [switch]$Manifest,',
      '  [string]$Stage,',
      '  [switch]$NonInteractive,',
      '  [switch]$Json,',
      '  [string]$Commit,',
      '  [string]$Branch',
      ')',
      'if ($Manifest) {',
      `  Write-Output '${manifestJson}'`,
      '  exit 0',
      '}',
      'if ($Stage) {',
      `  Write-Output '${stageJson}'`,
      stageOk ? '  exit 0' : '  exit 1',
      '}',
      'exit 1',
      ''
    ].join('\r\n')
  }
  return [
    '#!/usr/bin/env sh',
    'if [ "$1" = "--manifest" ]; then',
    `  printf '%s\\n' '${manifestJson}'`,
    '  exit 0',
    'fi',
    'if [ "$1" = "--stage" ]; then',
    `  printf '%s\\n' '${stageJson}'`,
    stageOk ? '  exit 0' : '  exit 1',
    'fi',
    'exit 1',
    ''
  ].join('\n')
}
