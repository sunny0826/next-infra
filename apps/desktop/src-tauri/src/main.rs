fn main() {
    if let Err(message) = next_infra_desktop_adapter::run() {
        eprintln!("next-infra: {message}");
        std::process::exit(1);
    }
}
