"""Regression tests for install.sh browser setup.

Browser automation is optional. The installer should not leave Hermes
half-installed just because Playwright's managed Chromium download hangs on an
unsupported distribution.
"""

from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parent.parent
INSTALL_SH = REPO_ROOT / "scripts" / "install.sh"


def test_install_script_skips_playwright_download_when_system_browser_exists() -> None:
    text = INSTALL_SH.read_text(encoding="utf-8")

    assert "find_system_browser()" in text
    assert "brave-browser brave-browser-stable brave" in text
    assert "microsoft-edge microsoft-edge-stable msedge" in text
    assert "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser" in text
    assert "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge" in text
    assert "Skipping Playwright browser download; Hermes will use the system browser." in text


def test_windows_install_script_detects_brave_before_browser_download() -> None:
    text = (REPO_ROOT / "scripts" / "install.ps1").read_text(encoding="utf-8")

    assert "Find-SystemBrowser" in text
    assert "BraveSoftware\\Brave-Browser\\Application\\brave.exe" in text


def test_install_script_persists_system_browser_for_agent_browser() -> None:
    text = INSTALL_SH.read_text(encoding="utf-8")

    assert "configure_browser_env_from_system_browser()" in text
    assert "AGENT_BROWSER_EXECUTABLE_PATH=$browser_path" in text


def test_ensure_browser_uses_hermes_managed_caches() -> None:
    text = INSTALL_SH.read_text(encoding="utf-8")
    ps_text = (REPO_ROOT / "scripts" / "install.ps1").read_text(encoding="utf-8")

    assert 'npm_config_cache="$HERMES_HOME/npm-cache"' in text
    assert 'PLAYWRIGHT_BROWSERS_PATH="$HERMES_HOME/playwright-browsers"' in text
    assert '$env:npm_config_cache = Join-Path $HermesHome "npm-cache"' in ps_text
    assert '$env:PLAYWRIGHT_BROWSERS_PATH = Join-Path $HermesHome "playwright-browsers"' in ps_text


def test_full_install_scripts_use_hermes_managed_node_caches() -> None:
    text = INSTALL_SH.read_text(encoding="utf-8")
    ps_text = (REPO_ROOT / "scripts" / "install.ps1").read_text(encoding="utf-8")

    assert 'export npm_config_cache="$HERMES_HOME/npm-cache"' in text
    assert 'export PLAYWRIGHT_BROWSERS_PATH="$HERMES_HOME/playwright-browsers"' in text
    assert 'export electron_config_cache="$HERMES_HOME/electron-cache"' in text
    assert 'export ELECTRON_CACHE="$HERMES_HOME/electron-cache"' in text
    assert 'export ELECTRON_BUILDER_CACHE="$HERMES_HOME/electron-cache"' in text
    assert '$env:npm_config_cache = Join-Path $HermesHome "npm-cache"' in ps_text
    assert '$env:PLAYWRIGHT_BROWSERS_PATH = Join-Path $HermesHome "playwright-browsers"' in ps_text
    assert '$env:electron_config_cache = Join-Path $HermesHome "electron-cache"' in ps_text
    assert '$env:ELECTRON_CACHE = Join-Path $HermesHome "electron-cache"' in ps_text
    assert '$env:ELECTRON_BUILDER_CACHE = Join-Path $HermesHome "electron-cache"' in ps_text


def test_full_install_scripts_prefer_managed_npm_cache() -> None:
    text = INSTALL_SH.read_text(encoding="utf-8")
    ps_text = (REPO_ROOT / "scripts" / "install.ps1").read_text(encoding="utf-8")

    assert "--prefer-offline --no-audit --fund=false" in text
    assert "--prefer-offline --no-audit --fund=false" in ps_text


def test_playwright_installs_are_timeout_guarded() -> None:
    text = INSTALL_SH.read_text(encoding="utf-8")

    assert "run_browser_install_with_timeout()" in text
    assert "run_browser_install_with_timeout 600 npx playwright install chromium" in text
    # --with-deps is still invoked on apt-based systems, but only when sudo
    # is available non-interactively (root or passwordless sudo). Non-sudo
    # service users fall back to the browser-only install — see
    # install_node_deps() in install.sh.
    assert "run_browser_install_with_timeout" in text
    assert "600 npx playwright install --with-deps chromium" in text


def test_install_script_supports_skip_browser_flag() -> None:
    """--skip-browser (and --no-playwright alias) skips the Playwright install."""
    text = INSTALL_SH.read_text(encoding="utf-8")

    assert "--skip-browser|--no-playwright)" in text
    assert "SKIP_BROWSER=true" in text
    assert 'if [ "$SKIP_BROWSER" = true ]; then' in text
    assert "--skip-browser Skip Playwright/Chromium install" in text


def test_install_script_skips_with_deps_when_no_sudo() -> None:
    """Non-sudo users on apt distros must not block on an interactive sudo prompt."""
    text = INSTALL_SH.read_text(encoding="utf-8")

    # The apt branch must gate --with-deps behind a sudo capability check
    # (root or non-interactive sudo), otherwise the installer hangs for
    # service-user installs (systemd accounts, operator users, etc.).
    assert 'if [ "$(id -u)" -eq 0 ] || (command -v sudo >/dev/null 2>&1 && sudo -n true 2>/dev/null); then' in text
    assert "sudo npx playwright install-deps chromium" in text
