mod bus;
mod cartridge;
mod cpu;
mod interrupt;
mod joypad;
mod ppu;
mod timer;

use cartridge::Cartridge;
use std::env;
use std::fs;

fn main() {
    // Leggiamo gli argomenti passati da riga di comando
    let args: Vec<String> = env::args().collect();

    // Verifichiamo che sia stato passato il percorso del file ROM
    if args.len() < 2 {
        eprintln!("Uso: {} <percorso_rom.gb>", args.get(0).map(|s| s.as_str()).unwrap_or("GB_Emulator"));
        eprintln!("Esempio: cargo run -- roms/dmg-acid2.gb");
        return;
    }

    let rom_path = &args[1];

    // Leggiamo tutti i byte del file in memoria
    let rom_bytes = match fs::read(rom_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            eprintln!("Errore nella lettura del file {}: {}", rom_path, err);
            return;
        }
    };

    // Creiamo la cartuccia eseguendo il parsing dell'header
    let cartridge = match Cartridge::new(rom_bytes) {
        Ok(cart) => cart,
        Err(err) => {
            eprintln!("Errore nel parsing della cartuccia: {}", err);
            return;
        }
    };

    println!("=== Dati Cartuccia Game Boy ===");
    println!("File:             {}", rom_path);
    println!("Titolo:           {}", cartridge.header.title);
    println!("Tipo Cartuccia:   {:?}", cartridge.header.cartridge_type);
    println!("Dimensione ROM:   {} KiB", cartridge.header.rom_size / 1024);
    println!("Dimensione RAM:   {} KiB", cartridge.header.ram_size / 1024);
    println!(
        "Checksum Header:  {:#04X} ({})",
        cartridge.header.header_checksum,
        if cartridge.header.is_checksum_valid { "VALIDO" } else { "NON VALIDO!" }
    );
}