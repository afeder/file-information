# File Information

A GTK4 application that displays metadata for a file from Tracker.

## Prerequisites

You need the development packages for GLib, GTK4, Tracker and Libadwaita. The exact package names vary by distribution.

### Debian/Ubuntu

```bash
sudo apt install libglib2.0-dev libcairo2-dev libpango1.0-dev \
    libgdk-pixbuf2.0-dev libgtk-4-dev libadwaita-1-dev \
    libtracker-sparql-3.0-dev
```

### Fedora

```bash
sudo dnf install glib2-devel cairo-devel pango-devel gdk-pixbuf2-devel \
    gtk4-devel libadwaita-devel tracker-devel
```

## Building

Use `cargo` to build the project:

```bash
cargo build
```

## Running

Run the program with the path to a file or a URI. Prefix `--uri` when passing a Tracker URI directly:

```bash
cargo run -- /path/to/file
cargo run -- --uri tracker://...
```
