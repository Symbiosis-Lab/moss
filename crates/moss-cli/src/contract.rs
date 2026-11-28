//! `moss-contract`: see `moss_build::plugins::manager::contract`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(moss_build::plugins::manager::contract::run(&args));
}
