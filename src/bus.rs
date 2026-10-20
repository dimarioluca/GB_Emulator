//! Game Boy Memory Bus (MMU) implementation.
//!
//! The Sharp SM83 CPU uses a 16-bit address bus to access a unified 64 KiB memory map:
//! - `0x0000..=0x3FFF`: 16 KiB ROM Bank 00 (Cartridge fixed bank)
//! - `0x4000..=0x7FFF`: 16 KiB ROM Bank 01..NN (Cartridge switchable bank via MBC)
//! - `0x8000..=0x9FFF`: 8 KiB Video RAM (VRAM)
//! - `0xA000..=0xBFFF`: 8 KiB External Cartridge RAM (optional, banked)
//! - `0xC000..=0xCFFF`: 4 KiB Work RAM Bank 0 (WRAM)
//! - `0xD000..=0xDFFF`: 4 KiB Work RAM Bank 1 (WRAM)
//! - `0xE000..=0xFDFF`: Echo of 8 KiB Internal WRAM (mirror of `0xC000..=0xDDFF`)
//! - `0xFE00..=0xFE9F`: Sprite Attribute Table (OAM)
//! - `0xFEA0..=0xFEFF`: Prohibited / Unusable memory
//! - `0xFF00..=0xFF7F`: Memory Mapped I/O Registers (Joypad, Serial, Timer, Audio, PPU)
//! - `0xFF80..=0xFFFE`: High RAM (HRAM, 127 bytes)
//! - `0xFFFF`: Interrupt Enable Register (IE)

use crate::Cartridge;
use crate::timer::Timer;

/// Represents the Game Boy memory bus (MMU), routing memory accesses across all 64 KiB of address space.
pub struct Bus {
    pub cartridge: Cartridge,
    pub timer: Timer,
    pub hram: [u8; 127], // high ram, super fast ram
    pub wram: [u8; 8192], // work ram

    pub sb: u8, // serial buffer 0xFF01 (load here data for gamelink)
    pub sc: u8, // serial control 0xFF02 (used to send data in sbwith gamelink)

    pub ie: u8, // the interrupt mask, to turn off specific interrupts
    pub if_flag: u8, // interrupt flag, one of it's bits in set on 1 if there is a pending interrupt
}

impl Bus {
    /// Creates a new [`Bus`] instance connected to the specified cartridge.
    pub fn new(cartridge: Cartridge) -> Self {
        Bus {
            cartridge,
            timer: Timer::new(),
            hram: [0; 127],
            wram: [0; 8192],
            sb: 0,
            sc: 0,
            ie: 0,
            if_flag: 0,
        }
    }

    /// Reads a 16-bit word from the bus in Little-Endian format (`low` byte at `addr`, `high` byte at `addr + 1`).
    pub fn read_word(&self, addr: u16) -> u16 {
        let low = self.read_byte(addr) as u16;
        let high = self.read_byte(addr.wrapping_add(1)) as u16;
        (high << 8) | low
    }

    /// Writes a 16-bit word to the bus in Little-Endian format.
    pub fn write_word(&mut self, addr: u16, val: u16) {
        let low = val as u8;
        let high = (val >> 8) as u8;
        self.write_byte(addr, low);
        self.write_byte(addr.wrapping_add(1), high);
    }

    /// Reads an 8-bit byte from the 64 KiB address space.
    pub fn read_byte(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cartridge.read(addr), // ROM
            0x8000..=0x9FFF => 0x00,
            0xA000..=0xBFFF => self.cartridge.read(addr), // RAM
            0xC000..=0xDFFF => self.wram[(addr - 0xC000) as usize], // WRAM
            0xE000..=0xFDFF => self.read_byte(addr - 0x2000), // Echo WRAM
            0xFE00..=0xFE9F => 0x00, // OAM (Object Attribute Memory) for sprites
            0xFEA0..=0xFEFF => 0xFF, // forbidden
            0xFF01 => self.sb,
            0xFF02 => self.sc,
            0xFF04..=0xFF07 => self.timer.read_byte(addr),
            0xFF0F => self.if_flag,
            0xFF00..=0xFF7F => 0x00, // other I/O
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize], // HRAM
            0xFFFF => self.ie,
        }
    }

    /// Writes an 8-bit byte to the 64 KiB address space.
    pub fn write_byte(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000..=0x7FFF => self.cartridge.write(addr, val), // ROM
            0x8000..=0x9FFF => {},
            0xA000..=0xBFFF => self.cartridge.write(addr, val), // RAM
            0xC000..=0xDFFF => self.wram[(addr - 0xC000) as usize] = val, // WRAM
            0xE000..=0xFDFF => self.write_byte(addr - 0x2000, val), // Echo WRAM
            0xFE00..=0xFE9F => {}, // OAM (Object Attribute Memory) for sprites
            0xFEA0..=0xFEFF => {}, // forbidden
            0xFF01 => self.sb = val,
            0xFF02 => {
                self.sc = val;

                // 0x81 is also used for debug so it gets printed
                if val == 0x81 {
                    print!("{}", self.sb as char);
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                }
            },
            0xFF04..=0xFF07 => self.timer.write_byte(addr, val),
            0xFF0F => self.if_flag = val,
            0xFF00..=0xFF7F => {}, // other I/O
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize] = val, // HRAM
            0xFFFF => self.ie = val,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_bus() -> Bus {
        let mut rom = vec![0; 0x8000];
        rom[0x0104..0x0134].copy_from_slice(&[
            0xCE, 0xED, 0x66, 0x66, 0xCC, 0x0D, 0x00, 0x0B, 0x03, 0x73, 0x00, 0x83, 0x00, 0x0C, 0x00, 0x0D,
            0x00, 0x08, 0x11, 0x1F, 0x88, 0x89, 0x00, 0x0E, 0xDC, 0xCC, 0x6E, 0xE6, 0xDD, 0xDD, 0xD9, 0x99,
            0xBB, 0xBB, 0x67, 0x63, 0x6E, 0x0E, 0xEC, 0xCC, 0xDD, 0xDC, 0x99, 0x9F, 0xBB, 0xB9, 0x33, 0x3E,
        ]);
        let mut chk = 0u8;
        for i in 0x0134..=0x014C {
            chk = chk.wrapping_sub(rom[i]).wrapping_sub(1);
        }
        rom[0x014D] = chk;
        let cart = Cartridge::new(rom).unwrap();
        Bus::new(cart)
    }

    #[test]
    fn test_timer_io_access_via_bus() {
        let mut bus = make_test_bus();

        // Test DIV: advance counter directly and read through bus
        bus.timer.div_counter = 0x1234;
        assert_eq!(bus.read_byte(0xFF04), 0x12);

        // Writing to DIV resets it
        bus.write_byte(0xFF04, 0xFF);
        assert_eq!(bus.read_byte(0xFF04), 0x00);
        assert_eq!(bus.timer.div_counter, 0);

        // Test TIMA (0xFF05)
        bus.write_byte(0xFF05, 0x42);
        assert_eq!(bus.read_byte(0xFF05), 0x42);

        // Test TMA (0xFF06)
        bus.write_byte(0xFF06, 0x80);
        assert_eq!(bus.read_byte(0xFF06), 0x80);

        // Test TAC (0xFF07): upper 5 bits read as 1
        bus.write_byte(0xFF07, 0x05);
        assert_eq!(bus.read_byte(0xFF07), 0x05 | 0xF8);
    }
}