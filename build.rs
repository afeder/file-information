fn main() {
    if let Err(err) = system_deps::Config::new().probe() {
        eprintln!(
            "error: failed to find required system packages.\n\
             Install: libglib2.0-dev libcairo2-dev libpango1.0-dev \
             libgdk-pixbuf2.0-dev libgtk-4-dev libadwaita-1-dev \
             libtracker-sparql-3.0-dev\n{err}"
        );
        std::process::exit(1);
    }
}
