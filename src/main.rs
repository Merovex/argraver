//! Thin binary entry point — all logic lives in the library.

fn main() -> anyhow::Result<()> {
    argraver::cli::run()
}
