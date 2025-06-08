# File Information

This application displays metadata about files using GNOME Tracker.

## Building

```bash
cargo build --release
```

## Packaging

A Debian package can be created with [`cargo-deb`](https://github.com/mmstick/cargo-deb).
After installing `cargo-deb`, run:

```bash
cargo deb --no-build
```

The resulting `.deb` installs a desktop entry so the application appears
in GNOME's **Open With** dialog for folders and any file type. The
desktop database is refreshed on install and removal.
