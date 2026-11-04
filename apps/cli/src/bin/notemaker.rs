//! `notemaker` CLI binary (`java -jar notemaker.jar` / `com.notemaker.cli.Main`).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(notemaker_cli::main::main(notemaker_cli::main::CommandArgs(args)).0);
}
