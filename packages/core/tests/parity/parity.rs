//! `cargo run -p notemaker_core --example parity -- FIXTURES COMMAND [ARGS...]`
//! Prints the same canonical sections as `java Parity FIXTURES COMMAND [ARGS...]`.

#[path = "driver.rs"]
mod driver;

use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("usage: parity FIXTURES (all | read DIR DOC... | write DIR | mutate DIR DOC)");
        std::process::exit(2);
    }
    let out = driver::run(std::path::Path::new(&args[0]), &args[1], &args[2..]);
    std::io::stdout().write_all(out.as_bytes()).unwrap();
}
