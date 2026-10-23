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
use crate::ppu::Ppu;
use crate::timer::Timer;

/// Represents the Game Boy memory bus (MMU), routing memory accesses across all 64 KiB of address space.
pub struct Bus {
    pub cartridge: Cartridge,
    pub timer: Timer,
    pub ppu: Ppu,
    pub hram: [u8; 127], // high ram, super fast ram
    pub wram: [u8; 8192], // work ram

    pub sb: u8, // serial buffer 0xFF01 (load here data for gamelink)
    pub sc: u8, // serial control 0xFF02 (used to send data in sb with gamelink)

    pub ie: u8, // the interrupt mask, to turn off specific interrupts
    pub if_flag: u8, // interrupt flag, one of its bits is set to 1 if there is a pending interrupt
}

impl Bus {
    /// Creates a new [`Bus`] instance connected to the specified cartridge.
    pub fn new(cartridge: Cartridge) -> Self {
        Bus {
            cartridge,
            timer: Timer::new(),
            ppu: Ppu::new(),
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

    /// Reads an 8-bit byte from the 64 KiB address space.
    pub fn read_byte(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cartridge.read(addr), // ROM
            0x8000..=0x9FFF => self.ppu.vram[(addr - 0x8000) as usize], // VRAM
            0xA000..=0xBFFF => self.cartridge.read(addr), // Cartridge RAM
            0xC000..=0xDFFF => self.wram[(addr - 0xC000) as usize], // WRAM
            0xE000..=0xFDFF => self.read_byte(addr - 0x2000), // Echo WRAM
            0xFE00..=0xFE9F => self.ppu.oam[(addr - 0xFE00) as usize], // OAM
            0xFEA0..=0xFEFF => 0xFF, // Prohibited / Unusable memory area
            0xFF01 => self.sb,
            0xFF02 => self.sc,
            0xFF04..=0xFF07 => self.timer.read_byte(addr),
            0xFF0F => self.if_flag,
            0xFF40 => self.ppu.LCDC,
            0xFF41 => self.ppu.STAT,
            0xFF42 => self.ppu.SCY,
            0xFF43 => self.ppu.SCX,
            0xFF44 => self.ppu.LY,
            0xFF45 => self.ppu.LYC,
            0xFF46 => self.ppu.DMA,
            0xFF47 => self.ppu.BGP,
            0xFF48 => self.ppu.OBP0,
            0xFF49 => self.ppu.OBP1,
            0xFF4A => self.ppu.WY,
            0xFF4B => self.ppu.WX,
            0xFF00..=0xFF7F => 0x00, // other I/O registers
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize], // HRAM
            0xFFFF => self.ie,
        }
    }

    /// Writes an 8-bit byte to the 64 KiB address space.
    pub fn write_byte(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000..=0x7FFF => self.cartridge.write(addr, val), // ROM
            0x8000..=0x9FFF => self.ppu.vram[(addr - 0x8000) as usize] = val, // VRAM
            0xA000..=0xBFFF => self.cartridge.write(addr, val), // Cartridge RAM
            0xC000..=0xDFFF => self.wram[(addr - 0xC000) as usize] = val, // WRAM
            0xE000..=0xFDFF => self.write_byte(addr - 0x2000, val), // Echo WRAM
            0xFE00..=0xFE9F => self.ppu.oam[(addr - 0xFE00) as usize] = val, // OAM
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
            0xFF40 => self.ppu.LCDC = val,
            0xFF41 => {
                // STAT bits 0-2 are read-only for the CPU (Mode and LYC=LY match flag).
                // The CPU can only write bits 3-6 (interrupt enable flags).
                self.ppu.STAT = (self.ppu.STAT & 0x07) | (val & 0xF8);
            },
            0xFF42 => self.ppu.SCY = val,
            0xFF43 => self.ppu.SCX = val,
            0xFF44 => {}, // LY is read-only; writes from CPU are ignored
            0xFF45 => self.ppu.LYC = val,
            0xFF46 => {
                self.ppu.DMA = val;
                self.perform_dma(val);
            },
            0xFF47 => self.ppu.BGP = val,
            0xFF48 => self.ppu.OBP0 = val,
            0xFF49 => self.ppu.OBP1 = val,
            0xFF4A => self.ppu.WY = val,
            0xFF4B => self.ppu.WX = val,
            0xFF00..=0xFF7F => {}, // Other I/O registers
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize] = val, // HRAM
            0xFFFF => self.ie = val,
        }
    }

    /// Performs OAM DMA transfer (0xFF46): copies 160 bytes from source address to OAM.
    fn perform_dma(&mut self, val: u8) {
        let base_addr = (val as u16) << 8;
        for i in 0..160 {
            let byte = self.read_byte(base_addr + i);
            self.ppu.oam[i as usize] = byte;
        }
    }

    /// Writes a 16-bit word to the bus in Little-Endian format.
    pub fn write_word(&mut self, addr: u16, val: u16) {
        let low = val as u8;
        let high = (val >> 8) as u8;
        self.write_byte(addr, low);
        self.write_byte(addr.wrapping_add(1), high);
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

    #[test]
    fn test_ppu_vram_and_registers_via_bus() {
        let mut bus = make_test_bus();

        // Test VRAM read/write
        bus.write_byte(0x8000, 0xAA);
        assert_eq!(bus.read_byte(0x8000), 0xAA);
        assert_eq!(bus.ppu.vram[0], 0xAA);

        // Test OAM read/write
        bus.write_byte(0xFE00, 0x55);
        assert_eq!(bus.read_byte(0xFE00), 0x55);
        assert_eq!(bus.ppu.oam[0], 0x55);

        // Test LCDC (0xFF40)
        bus.write_byte(0xFF40, 0x93);
        assert_eq!(bus.read_byte(0xFF40), 0x93);
        assert_eq!(bus.ppu.LCDC, 0x93);

        // Test STAT (0xFF41) - bits 0-2 are read-only
        bus.ppu.STAT = 0x85; // bits 0-2 = 5 (101: LYC flag = 1, Mode = 1)
        bus.write_byte(0xFF41, 0x40); // write only bit 6
        assert_eq!(bus.read_byte(0xFF41) & 0x07, 0x05); // bits 0-2 preserved!
        assert_eq!(bus.read_byte(0xFF41) & 0x40, 0x40); // bit 6 updated!

        // Test LY (0xFF44) is read-only
        bus.ppu.LY = 42;
        bus.write_byte(0xFF44, 0x00);
        assert_eq!(bus.read_byte(0xFF44), 42); // not overwritten

        // Test OAM DMA (0xFF46) from WRAM
        for i in 0..160 {
            bus.write_byte(0xC000 + i, (i & 0xFF) as u8);
        }
        bus.write_byte(0xFF46, 0xC0); // Trigger DMA from 0xC000
        for i in 0..160 {
            assert_eq!(bus.ppu.oam[i as usize], (i & 0xFF) as u8);
        }
    }
}