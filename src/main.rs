fn main() {
    if let Err(error) = history_to_md::run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}
