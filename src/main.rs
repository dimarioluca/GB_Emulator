mod bus;
mod cartridge;
mod cpu;
mod interrupt;
mod joypad;
mod ppu;
mod timer;

use bus::Bus;
use cartridge::Cartridge;
use cpu::Cpu;
use std::env;
use std::fs;

fn main() {
    // Read command-line arguments
    let args: Vec<String> = env::args().collect();

    // Verify that a ROM file path was provided
    if args.len() < 2 {
        eprintln!("Usage: {} <rom_path.gb>", args.get(0).map(|s| s.as_str()).unwrap_or("GB_Emulator"));
        eprintln!("Example: cargo run -- roms/dmg-acid2.gb");
        return;
    }

    let rom_path = &args[1];

    // Read all bytes from the ROM file into memory
    let rom_bytes = match fs::read(rom_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("Error reading file {}: {}", rom_path, err);
            return;
        }
    };

    // Create the cartridge by parsing its header
    let cartridge = match Cartridge::new(rom_bytes) {
        Ok(cart) => cart,
        Err(err) => {
            eprintln!("Error parsing cartridge: {}", err);
            return;
        }
    };

    println!("=== Game Boy Cartridge Info ===");
    println!("File:             {}", rom_path);
    println!("Title:            {}", cartridge.header.title);
    println!("Cartridge Type:   {:?}", cartridge.header.cartridge_type);
    println!("ROM Size:         {} KiB", cartridge.header.rom_size / 1024);
    println!("RAM Size:         {} KiB", cartridge.header.ram_size / 1024);
    println!(
        "Header Checksum:  {:#04X} ({})",
        cartridge.header.header_checksum,
        if cartridge.header.is_checksum_valid { "VALID" } else { "INVALID!" }
    );

    println!("\n=== Starting Emulator Execution ===\n");

    let mut bus = Bus::new(cartridge);
    let mut cpu = Cpu::new();

    loop {
        let cycles = cpu.step(&mut bus);
        bus.timer.step(cycles, &mut bus.if_flag);
        bus.ppu.step(cycles, &mut bus.if_flag);
    }
}
