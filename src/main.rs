mod bus;
mod cartridge;
mod cpu;
mod joypad;
mod ppu;
mod timer;

mod emulator;
mod launcher;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() >= 3 && args[1] == "--emulator" {
        if let Err(e) = emulator::run(&args[2]) {
            eprintln!("Emulator error: {e}");
            std::process::exit(1);
        }
        return;
    }

    if let Err(e) = launcher::run() {
        eprintln!("Launcher error: {e}");
        std::process::exit(1);
    }
}