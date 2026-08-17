# STARFORGE

[中文](README.md) · [English](README_EN.md)

STARFORGE's mainline migration from a browser-based Three.js game to a native Bevy application is complete. The current main project lives in [`starforge-bevy/`](starforge-bevy/). The legacy web game, Node.js multiplayer server, and browser regression suite are archived together in [`legacy-web/`](legacy-web/).

## Bevy main version

```powershell
cd starforge-bevy
cargo run --release
cargo run -- --smoke
```

See [`starforge-bevy/README.md`](starforge-bevy/README.md) for the feature list, controls, asset licenses, and test commands.

## Legacy web archive

The old source and its documentation are kept under [`legacy-web/`](legacy-web/). That directory is no longer maintained or included in CI; it exists only as a historical archive and migration reference, and is neither a dependency nor a compatibility target for the Bevy version.

## Migration references

- [`starforge-bevy/MIGRATION_REPORT.md`](starforge-bevy/MIGRATION_REPORT.md): migration audit report
- [`STARFORGE_BEVY_PORT_SPEC.md`](STARFORGE_BEVY_PORT_SPEC.md): overall port specification
- [`SPEC_data.md`](SPEC_data.md), [`SPEC_player.md`](SPEC_player.md), and [`SPEC_world.md`](SPEC_world.md): core system specifications
- [`TEXTURES_SPEC.md`](TEXTURES_SPEC.md): procedural texture specification

## Repository layout

```text
starforge/
├── starforge-bevy/   # Current Bevy/Rust main project
├── legacy-web/       # Legacy Three.js/Node.js version and browser tests
├── .github/          # Continuous integration for the Bevy mainline
└── *_SPEC.md         # Port specifications and audit references
```

The code is released under the [MIT License](LICENSE). Third-party asset licenses are documented in [`starforge-bevy/CREDITS.md`](starforge-bevy/CREDITS.md).
