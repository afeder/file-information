apt-get update
apt-get install -y libglib2.0-dev libcairo2-dev libpango1.0-dev libgdk-pixbuf2.0-dev libgtk-4-dev libadwaita-1-dev libtracker-sparql-3.0-dev pkg-config
apt-get install -y xvfb dbus-x11 xdotool imagemagick
apt-get install -y tracker gedit xdg-utils
xdg-mime default org.gnome.gedit.desktop text/plain
echo "ENVIRONMENT SETUP COMPLETE."
