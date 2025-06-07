# This script is designed to bootstrap the environment for testing. It assumes a
# minimal Debian/Ubuntu base environment.

apt-get update

# Install main app dependencies.
apt-get install -y libglib2.0-dev libcairo2-dev libpango1.0-dev libgdk-pixbuf2.0-dev libgtk-4-dev libadwaita-1-dev libtracker-sparql-3.0-dev pkg-config

# Install testing dependencies.
apt-get install -y xvfb dbus-x11 xdotool imagemagick
apt-get install -y tracker faketime gedit xdg-utils
xdg-mime default org.gnome.gedit.desktop text/plain

# Clean out any existing build artifacts in case the space was not pristine
# when handed to us.
cargo clean

echo "ENVIRONMENT SETUP COMPLETE."
