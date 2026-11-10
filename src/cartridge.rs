//! Game Boy Cartridge header parsing and Memory Bank Controller (MBC) implementation.
//!
//! Handles ROM and external RAM access across different cartridge types:
//! - `RomOnly`: Simple cartridges up to 32 KiB ROM with no bank switching.
//! - `Mbc1`: Memory Bank Controller 1, supporting up to 2 MiB ROM and 32 KiB RAM.
//!   Implements ROM banking at `0x4000..=0x7FFF`, RAM banking at `0xA000..=0xBFFF`,
//!   RAM write-protection, and address mirroring.

/// Represents the type of cartridge and its memory bank controller (MBC).
#[derive(Debug, PartialEq, Eq)]
pub enum CartridgeType {
    RomOnly,
    Mbc1,
    Mbc2,
    Mbc3,
    Mbc5,
    Unsupported(u8),
}

impl From<u8> for CartridgeType {
    /// Converts a raw byte from cartridge header offset `0x0147` into a [`CartridgeType`].
    fn from(byte: u8) -> Self {
        match byte {
            0x00 => CartridgeType::RomOnly,
            0x01..=0x03 => CartridgeType::Mbc1,
            0x05..=0x06 => CartridgeType::Mbc2,
            0x0F..=0x13 => CartridgeType::Mbc3,
            0x19..=0x1E => CartridgeType::Mbc5,
            other => CartridgeType::Unsupported(other),
        }
    }
}

/// Metadata extracted from the Game Boy cartridge header (`0x0100..=0x014F`).
#[derive(Debug)]
pub struct CartridgeHeader {
    pub title: String,
    pub cartridge_type: CartridgeType,
    pub rom_size: usize,
    pub ram_size: usize,
    pub header_checksum: u8,
    pub is_checksum_valid: bool,
}

impl CartridgeHeader {
    /// Parses the cartridge header from raw ROM bytes.
    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        if data.len() < 0x0150 {
            return Err(format!(
                "File ROM troppo piccolo ({} byte). Un header Game Boy richiede almeno 336 byte (0x0150).",
                data.len()
            ));
        }

        // title (0x0134..=0x0143)
        let raw_title = &data[0x0134..=0x0143];
        let title_bytes: Vec<u8> = raw_title
            .iter()
            .copied()
            .take_while(|&b| b != 0)
            .collect();
        let title = String::from_utf8_lossy(&title_bytes).trim().to_string();

        // cartridge type (0x0147)
        let cartridge_type = CartridgeType::from(data[0x0147]);

        // ROM size (0x0148): 32 KiB * (1 << codice)
        let rom_size_code = data[0x0148];
        let rom_size = match rom_size_code {
            0x00..=0x08 => (32 * 1024) << rom_size_code,
            _ => 32 * 1024,
        };

        // RAM size (0x0149)
        let ram_size_code = data[0x0149];
        let ram_size = match ram_size_code {
            0x00 => 0,
            0x01 => 2 * 1024,
            0x02 => 8 * 1024,
            0x03 => 32 * 1024,
            0x04 => 128 * 1024,
            0x05 => 64 * 1024,
            _ => 0,
        };

        // Checksum (0x0134..=0x014C)
        let mut calculated_checksum: u8 = 0;
        for &byte in &data[0x0134..=0x014C] {
            calculated_checksum = calculated_checksum.wrapping_sub(byte).wrapping_sub(1);
        }
        let header_checksum = data[0x014D];
        let is_checksum_valid = calculated_checksum == header_checksum;

        Ok(Self {
            title,
            cartridge_type,
            rom_size,
            ram_size,
            header_checksum,
            is_checksum_valid,
        })
    }
}

/// Controller-specific state and logic for different cartridge types.
#[derive(Debug)]
pub enum Mbc {
    RomOnly,
    Mbc1 {
        rom_bank: u8,      // ROM bank selected right now
        ram_bank: u8,      // 2-bit register (0x4000-0x5FFF): upper bits 5-6 of ROM bank, or RAM bank in mode 1
        ram_enabled: bool, // if cartridge RAM is enabled
        banking_mode: u8,  // 0 = ROM banking mode, 1 = RAM banking mode
    },
    Mbc2 {
        rom_bank: u8,      // ROM bank selected (1..=15, 4 bit)
        ram_enabled: bool, // if cartridge RAM is enabled
    },
    Mbc3,
    Mbc5,
}

/// Represents a Game Boy cartridge containing ROM data, external RAM, and header metadata.
pub struct Cartridge {
    pub header: CartridgeHeader,
    pub rom: Vec<u8>,
    pub ram: Vec<u8>,
    pub mbc: Mbc,
}

impl Cartridge {
    /// Creates a new [`Cartridge`] instance from raw ROM bytes.
    pub fn new(rom: Vec<u8>) -> Result<Self, String> {
        let header = CartridgeHeader::from_bytes(&rom)?;

        // MBC2 has 512 nibbles (4-bit) of built-in RAM even if header ram_size is 0
        let ram_size = match header.cartridge_type {
            CartridgeType::Mbc2 => 512,
            _ => header.ram_size,
        };
        let ram = vec![0; ram_size];

        let mbc = match header.cartridge_type {
            CartridgeType::RomOnly => Mbc::RomOnly,
            CartridgeType::Mbc1 => Mbc::Mbc1 {
                rom_bank: 1,
                ram_bank: 0,
                ram_enabled: false,
                banking_mode: 0,
            },
            CartridgeType::Mbc2 => Mbc::Mbc2 {
                rom_bank: 1,
                ram_enabled: false,
            },
            CartridgeType::Mbc3 => Mbc::Mbc3,
            CartridgeType::Mbc5 => Mbc::Mbc5,
            CartridgeType::Unsupported(_) => Mbc::RomOnly,
        };

        Ok(Self {
            header,
            rom, 
            ram,
            mbc,
        })
    }

    // Helper to inspect mbc state
    #[allow(dead_code)]
    pub fn rom_bank(&self) -> u8 {
        match &self.mbc {
            Mbc::Mbc1 { rom_bank, .. } => *rom_bank,
            Mbc::Mbc2 { rom_bank, .. } => *rom_bank,
            _ => 0,
        }
    }

    #[allow(dead_code)]
    pub fn ram_bank(&self) -> u8 {
        match &self.mbc {
            Mbc::Mbc1 { ram_bank, .. } => *ram_bank,
            _ => 0,
        }
    }

    #[allow(dead_code)]
    pub fn ram_enabled(&self) -> bool {
        match &self.mbc {
            Mbc::Mbc1 { ram_enabled, .. } => *ram_enabled,
            Mbc::Mbc2 { ram_enabled, .. } => *ram_enabled,
            _ => false,
        }
    }

    #[allow(dead_code)]
    pub fn banking_mode(&self) -> u8 {
        match &self.mbc {
            Mbc::Mbc1 { banking_mode, .. } => *banking_mode,
            _ => 0,
        }
    }

    /// Reads a byte from the cartridge address space (`0x0000..=0x7FFF` for ROM, `0xA000..=0xBFFF` for RAM).
    pub fn read(&self, addr: u16) -> u8 {
        match &self.mbc {
            Mbc::RomOnly => {
                match addr {
                    0x0000..=0x7FFF => self.rom.get(addr as usize).copied().unwrap_or(0xFF), // it's trying to read from the rom
                    0xA000..=0xBFFF if (self.ram.len() > (addr - 0xA000) as usize) => self.ram[(addr - 0xA000) as usize], // it's trying to read from cartridge's ram
                    _ => 0xFF,
                }
            }
            Mbc::Mbc1 { rom_bank, ram_bank, ram_enabled, banking_mode } => {
                match addr {
                    // fixed 16 KiB ROM bank: bank 0 in Mode 0, or bank (ram_bank << 5) in Mode 1
                    0x0000..=0x3FFF => {
                        let num_rom_banks = (self.rom.len() / 0x4000).max(1);
                        let raw_bank = if *banking_mode == 1 {
                            (*ram_bank << 5) as usize
                        } else {
                            0
                        };
                        let bank = raw_bank % num_rom_banks;
                        let rom_addr = bank * 0x4000 + (addr as usize);
                        self.rom.get(rom_addr).copied().unwrap_or(0xFF)
                    }
                    // reading the chosen bank
                    0x4000..=0x7FFF => {
                        let num_rom_banks = (self.rom.len() / 0x4000).max(1);
                        let raw_bank = ((*ram_bank << 5) | *rom_bank) as usize;
                        let bank = raw_bank % num_rom_banks;
                        let relative_addr = (addr - 0x4000) as usize;
                        let rom_addr = (bank * 16384) + relative_addr;
                        
                        self.rom.get(rom_addr).copied().unwrap_or(0xFF)
                    }
                    // reading external ram
                    0xA000..=0xBFFF => {
                        if *ram_enabled && !self.ram.is_empty() {
                            let bank = if *banking_mode == 1 { *ram_bank as usize } else { 0 };
                            let ram_addr = (bank * 8192) + ((addr - 0xA000) as usize);
                            
                            self.ram.get(ram_addr % self.ram.len()).copied().unwrap_or(0xFF)
                        } else {
                            0xFF
                        }
                    }
                    _ => 0xFF,
                }
            }
            Mbc::Mbc2 { rom_bank, ram_enabled } => {
                match addr {
                    // the first bank is fixed
                    0x0000..=0x3FFF => {
                        self.rom.get(addr as usize).copied().unwrap_or(0xFF)
                    }
                    // reading the switchable bank
                    0x4000..=0x7FFF => {
                        let num_rom_banks = (self.rom.len() / 0x4000).max(1);
                        let bank = (*rom_bank as usize) % num_rom_banks;
                        let rom_addr = bank * 0x4000 + ((addr - 0x4000) as usize);
                        self.rom.get(rom_addr).copied().unwrap_or(0xFF)
                    }
                    // reading from mcb2 internal ram
                    0xA000..=0xBFFF => {
                        if *ram_enabled && !self.ram.is_empty() {
                            let ram_addr = ((addr - 0xA000) & 0x01FF) as usize;
                            0xF0 | (self.ram[ram_addr] & 0x0F)
                        } else {
                            0xFF
                        }
                    }
                    _ => 0xFF,
                }
            }
            _ => 0xFF,
        }
    } 

    /// Writes a byte to the cartridge address space.
    pub fn write(&mut self, addr: u16, val: u8) {
        match &mut self.mbc {
            Mbc::RomOnly => {
                match addr {
                    0xA000..=0xBFFF if (self.ram.len() > (addr - 0xA000) as usize) => self.ram[(addr - 0xA000) as usize] = val, // it's trying to write to cartridge's ram
                    _ => {},
                }
            }
            Mbc::Mbc1 { rom_bank, ram_bank, ram_enabled, banking_mode } => {
                // if the game tries to write at a rom address the MBC1 chip intercept's it
                match addr {
                    0x0000..=0x1FFF => { *ram_enabled = (val & 0x0F) == 0x0A; }
                    0x2000..=0x3FFF => {
                        let mut bank = val & 0x1F;
                        if bank == 0 {
                            bank = 1;
                        }
                        *rom_bank = bank;
                    }
                    0x4000..=0x5FFF => { *ram_bank = val & 0x03; }
                    0x6000..=0x7FFF => { *banking_mode = val & 0x01; }
                    0xA000..=0xBFFF => { // normal write to the external ram
                        if *ram_enabled && !self.ram.is_empty() {
                            let bank = if *banking_mode == 1 { *ram_bank as usize } else { 0 };
                            let ram_addr = (bank * 8192) + ((addr - 0xA000) as usize);
                            let len = self.ram.len();
                            self.ram[ram_addr % len] = val;
                        }
                    }
                    _ => {}
                }
            }
            Mbc::Mbc2 { rom_bank, ram_enabled } => {
                match addr {
                    0x0000..=0x3FFF => {
                        if (addr & 0x0100) == 0 { // if the eighth bit is 0 it's trying to enable/disable ram
                            *ram_enabled = (val & 0x0F) == 0x0A;
                        } else { // if the eighth bit is 1 it's trying to switch rom bank
                            let bank = val & 0x0F;
                            *rom_bank = if bank == 0 { 1 } else { bank }; // bank 0 can't be mapped
                        }
                    }
                    0xA000..=0xBFFF => {
                        if *ram_enabled && !self.ram.is_empty() {
                            let ram_addr = ((addr - 0xA000) & 0x01FF) as usize;
                            self.ram[ram_addr] = val & 0x0F;
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_rom(cart_type: u8, rom_size_code: u8, ram_size_code: u8) -> Vec<u8> {
        let rom_size = (32 * 1024) << rom_size_code;
        let mut rom = vec![0u8; rom_size];

        // Fill Title
        let title = b"TEST_ROM";
        rom[0x0134..0x0134 + title.len()].copy_from_slice(title);

        rom[0x0147] = cart_type;
        rom[0x0148] = rom_size_code;
        rom[0x0149] = ram_size_code;

        // Compute header checksum
        let mut chk = 0u8;
        for i in 0x0134..=0x014C {
            chk = chk.wrapping_sub(rom[i]).wrapping_sub(1);
        }
        rom[0x014D] = chk;

        rom
    }

    #[test]
    fn test_mbc1_rom_bank_switching() {
        // 64 KiB ROM (code 1), MBC1 (code 1), No RAM (code 0)
        let mut rom = make_test_rom(0x01, 0x01, 0x00);

        // Fill Bank 0 (0x0000..0x3FFF) with 0x00
        // Fill Bank 1 (0x4000..0x7FFF) with 0x11
        // Fill Bank 2 (0x8000..0xBFFF) with 0x22
        // Fill Bank 3 (0xC000..0xFFFF) with 0x33
        rom[0x4000..0x8000].fill(0x11);
        rom[0x8000..0xC000].fill(0x22);
        rom[0xC000..0x10000].fill(0x33);

        let mut cart = Cartridge::new(rom).unwrap();
        assert_eq!(cart.header.cartridge_type, CartridgeType::Mbc1);

        // Default state: Bank 1 is mapped at 0x4000..=0x7FFF
        assert_eq!(cart.read(0x4000), 0x11);

        // Switch to Bank 2 by writing to 0x2000
        cart.write(0x2000, 2);
        assert_eq!(cart.rom_bank(), 2);
        assert_eq!(cart.read(0x4000), 0x22);

        // Switch to Bank 3
        cart.write(0x2000, 3);
        assert_eq!(cart.rom_bank(), 3);
        assert_eq!(cart.read(0x4000), 0x33);

        // Hardware Quirk: writing 0 selects Bank 1
        cart.write(0x2000, 0);
        assert_eq!(cart.rom_bank(), 1);
        assert_eq!(cart.read(0x4000), 0x11);
    }

    #[test]
    fn test_mbc1_ram_enable_and_banking() {
        // 64 KiB ROM, MBC1 (code 0x03 = MBC1+RAM+BATTERY), 32 KiB RAM (code 0x03)
        let rom = make_test_rom(0x03, 0x01, 0x03);
        let mut cart = Cartridge::new(rom).unwrap();

        // RAM disabled by default: reads return 0xFF and writes are ignored
        assert_eq!(cart.read(0xA000), 0xFF);
        cart.write(0xA000, 0x42);
        assert_eq!(cart.read(0xA000), 0xFF);

        // Enable RAM by writing 0x0A to 0x0000..=0x1FFF
        cart.write(0x0000, 0x0A);
        assert!(cart.ram_enabled());

        // Write and read from Bank 0
        cart.write(0xA000, 0x77);
        assert_eq!(cart.read(0xA000), 0x77);

        // Switch to RAM banking mode (mode 1)
        cart.write(0x6000, 1);
        assert_eq!(cart.banking_mode(), 1);

        // Select RAM Bank 1 by writing to 0x4000
        cart.write(0x4000, 1);
        assert_eq!(cart.ram_bank(), 1);

        // Write to Bank 1
        cart.write(0xA000, 0x88);
        assert_eq!(cart.read(0xA000), 0x88);

        // Switch back to RAM Bank 0: Bank 0 should still hold 0x77!
        cart.write(0x4000, 0);
        assert_eq!(cart.read(0xA000), 0x77);

        // Disable RAM by writing 0x00 to 0x0000
        cart.write(0x0000, 0x00);
        assert!(!cart.ram_enabled());
        assert_eq!(cart.read(0xA000), 0xFF);
    }

    #[test]
    fn test_mbc1_large_rom_and_mode1() {
        // 1 MiB ROM (code 0x05 = 64 banks), MBC1+RAM (code 0x03), 32 KiB RAM (code 0x03)
        let mut rom = make_test_rom(0x03, 0x05, 0x03);

        // Fill notable banks to identify them (preserving the header):
        rom[0x0000..0x0100].fill(0xAA);
        rom[0x0150..0x4000].fill(0xAA);
        rom[0x4000..0x8000].fill(0x11);
        rom[32 * 0x4000..33 * 0x4000].fill(0x22);
        rom[33 * 0x4000..34 * 0x4000].fill(0x33);

        let mut cart = Cartridge::new(rom).unwrap();

        // In Mode 0 (default):
        // 0x0000..=0x3FFF always reads bank 0
        assert_eq!(cart.read(0x0000), 0xAA);

        // Select bank 33 by setting ram_bank = 1 (bit 5) and rom_bank = 1
        cart.write(0x4000, 1); // ram_bank = 1 (bit 5 = 1 => 32)
        cart.write(0x2000, 1); // rom_bank = 1 => 32 | 1 = bank 33
        assert_eq!(cart.read(0x4000), 0x33); // Bank 33 accessible!
        // In Mode 0, the 0x0000..=0x3FFF area continues to point to bank 0
        assert_eq!(cart.read(0x0000), 0xAA);

        // Switch to Mode 1 (Advanced Banking Mode)
        cart.write(0x6000, 1);
        // In Mode 1, 0x0000..=0x3FFF maps (ram_bank << 5) = bank 32 (0x20)!
        assert_eq!(cart.read(0x0000), 0x22);

        // Test ROM bank wrapping / masking:
        // If we request bank 65 (bit 6 = 1, bit 0 = 1 => 64 | 1 = 65),
        // on a 64-bank ROM (0..63), 65 % 64 = bank 1 (0x11)!
        cart.write(0x4000, 2); // bit 6 = 1 => 64
        cart.write(0x2000, 1); // bank 65
        assert_eq!(cart.read(0x4000), 0x11); // Correctly wrapped to bank 1
    }

    #[test]
    fn test_mbc2_rom_banking() {
        // 256 KiB ROM (code 0x03 = 16 banks), MBC2 (code 0x05), RAM code 0
        let mut rom = make_test_rom(0x05, 0x03, 0x00);

        // Fill banks to verify them
        rom[0x0000..0x0100].fill(0xAA);
        rom[0x0150..0x4000].fill(0xAA);
        rom[0x4000..0x8000].fill(0x11);
        rom[0x8000..0xC000].fill(0x22);
        rom[0xC000..0x10000].fill(0x33);

        let mut cart = Cartridge::new(rom).unwrap();
        assert_eq!(cart.header.cartridge_type, CartridgeType::Mbc2);

        // At startup, bank 1 is mapped to 0x4000..=0x7FFF
        assert_eq!(cart.read(0x4000), 0x11);
        assert_eq!(cart.rom_bank(), 1);

        // Write with bit 8 = 1 (e.g. 0x2100): selects ROM bank 2
        cart.write(0x2100, 2);
        assert_eq!(cart.rom_bank(), 2);
        assert_eq!(cart.read(0x4000), 0x22);

        // Write with bit 8 = 0 (e.g. 0x2000): must NOT change ROM bank!
        // (Acts instead on the RAM enable register)
        cart.write(0x2000, 3);
        assert_eq!(cart.rom_bank(), 2);
        assert_eq!(cart.read(0x4000), 0x22);

        // Hardware quirk: writing 0 to an address with bit 8 = 1 remaps to bank 1
        cart.write(0x2100, 0);
        assert_eq!(cart.rom_bank(), 1);
        assert_eq!(cart.read(0x4000), 0x11);
    }

    #[test]
    fn test_mbc2_ram_enable_and_nibbles() {
        // 256 KiB ROM, MBC2+BATTERY (code 0x06), RAM code 0 in header
        let rom = make_test_rom(0x06, 0x03, 0x00);
        let mut cart = Cartridge::new(rom).unwrap();

        // Even though header has ram_size_code = 0, 512 bytes must be allocated for MBC2
        assert_eq!(cart.ram.len(), 512);

        // RAM disabled at startup: reads return 0xFF and writes are ignored
        assert!(!cart.ram_enabled());
        assert_eq!(cart.read(0xA000), 0xFF);
        cart.write(0xA000, 0x05);
        assert_eq!(cart.read(0xA000), 0xFF);

        // Enable RAM by writing 0x0A with bit 8 = 0 (e.g. 0x0000 or 0x0200)
        cart.write(0x0000, 0x0A);
        assert!(cart.ram_enabled());

        // 4-bit write and read:
        // Write 0x35 (lower nibble 5, upper nibble 3).
        // Only lower 4 bits must be saved! On read, upper 4 bits are 1 (0xF0 | val)
        cart.write(0xA000, 0x35);
        assert_eq!(cart.read(0xA000), 0xF5);

        // Mirroring: the 512 bytes are mirrored across 0xA000..=0xBFFF
        // Address 0xA200 (0xA000 + 512) points to the same cell as 0xA000!
        assert_eq!(cart.read(0xA200), 0xF5);

        // Write to address 511 (0xA1FF)
        cart.write(0xA1FF, 0x0C);
        assert_eq!(cart.read(0xA1FF), 0xFC);
        // Mirrored to 0xA3FF (0xA1FF + 512)
        assert_eq!(cart.read(0xA3FF), 0xFC);

        // Disable RAM by writing a value != 0x0A with bit 8 = 0
        cart.write(0x0200, 0x00);
        assert!(!cart.ram_enabled());
        assert_eq!(cart.read(0xA000), 0xFF);
    }
}
