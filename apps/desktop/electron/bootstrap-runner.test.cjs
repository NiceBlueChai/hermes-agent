const assert = require('node:assert/strict')
const test = require('node:test')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')

const {
  runBootstrap,
  probeNativeBootstrapCapabilities,
  recordInstallMetadata,
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

function fakeInstallerScript() {
  const manifestJson =
    '{"stages":[{"name":"metadata","title":"Metadata",' +
    '"category":"install","needs_user_input":false}],"protocol_version":1}'
  const stageJson = '{"ok":true,"stage":"metadata"}'
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
    `  printf '%s\\n' '${stageJson}'`,
    '  exit 0',
    'fi',
    'exit 1',
    ''
  ].join('\n')
}
