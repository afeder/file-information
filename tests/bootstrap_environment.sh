# This script is designed to bootstrap the environment for testing. It assumes a
# minimal Debian/Ubuntu base environment.

#!/bin/sh

# Abort if it looks like we're running on a regular desktop system unless the
# user explicitly overrides.  We check common environment variables that are
# present in desktop sessions.
force=0
for arg in "$@"; do
    case "$arg" in
        -f|--force)
            force=1
            ;;
    esac
done

if [ "$force" = 0 ] && { [ -n "$XDG_CURRENT_DESKTOP" ] || [ -n "$DESKTOP_SESSION" ] || [ -n "$DISPLAY" ]; }; then
    echo "Refusing to run in a desktop environment. Use -f to force." >&2
    exit 1
fi

apt-get update

# Install main app dependencies.
apt-get install -y libglib2.0-dev libcairo2-dev libpango1.0-dev libgdk-pixbuf2.0-dev libgtk-4-dev libadwaita-1-dev libtracker-sparql-3.0-dev pkg-config

# Install testing dependencies.
apt-get install -y xvfb dbus-x11 xdotool imagemagick
apt-get install -y tracker gedit xdg-utils
xdg-mime default org.gnome.gedit.desktop text/plain

# Clean out any existing build artifacts in case the space was not pristine
# when handed to us.
cargo clean

echo "ENVIRONMENT SETUP COMPLETE."
