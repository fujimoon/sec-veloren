//! psy – Psypher CLI toolset
//!
//! Usage: psy <command>
//!
//! Commands:
//!   dungeon   Navigate the filesystem as a 2-D ASCII dungeon

mod dungeon;

fn main() {
    let sub = std::env::args().nth(1);
    match sub.as_deref() {
        Some("dungeon") => {
            if let Err(e) = dungeon::run() {
                eprintln!("psy dungeon: {e}");
                std::process::exit(1);
            }
        },
        _ => {
            eprintln!("Usage: psy <command>");
            eprintln!("");
            eprintln!("Commands:");
            eprintln!("  dungeon   Navigate the filesystem as a 2-D ASCII dungeon");
        },
    }
}
