# Python Wheelhouse

Place release-time Python wheels here before building the Tauri bundle.

The Rust bootstrapper checks this bundled resource directory before falling back
to `uv.lock` and online PyPI resolution. Keep the directory empty for normal
development builds.
