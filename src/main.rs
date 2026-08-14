use std::io::IsTerminal;

use oci_sync::cli;

#[tokio::main]
async fn main() {
    if let Err(err) = cli::run().await {
        // TTY: red "✗ error:" prefix; non-TTY: plain text (scripts/CI).
        if std::io::stderr().is_terminal() {
            eprintln!("\x1b[31m✗ error:\x1b[0m {err:#}");
        } else {
            eprintln!("error: {err:#}");
        }
        std::process::exit(1);
    }
}
