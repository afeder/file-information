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

## Testing

Run the test suite with:

```bash
tests/run_tests.sh
```

This executes unit tests and graphical tests using Xvfb, so the process may take a while.

## Contributing

Contributions are welcome. Please ensure `tests/run_tests.sh` passes and keep commit messages concise.
