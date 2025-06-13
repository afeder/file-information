use clap::Parser;

/// Command line interface definition using clap.
#[derive(Parser, Debug)]
#[command(author, version, about)]
pub struct Options {
    /// Interpret the input argument as a URI
    #[arg(short, long)]
    pub uri: bool,

    /// Enable debug output
    #[arg(short, long)]
    pub debug: bool,

    /// File path or URI to open
    pub item: String,
}
