#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--bind-device") {
        match ferrassh_lib::bind_install_seal() {
            Ok(()) => std::process::exit(0),
            Err(_) => std::process::exit(1),
        }
    }
    if args.iter().any(|a| a == "--unbind-device") {
        let _ = ferrassh_lib::unbind_install_seal();
        std::process::exit(0);
    }
    ferrassh_lib::run();
}
