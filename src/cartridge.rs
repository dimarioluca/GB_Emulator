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

/// Represents a Game Boy cartridge containing ROM data, external RAM, and header metadata.
pub struct Cartridge {
    pub header: CartridgeHeader,
    pub rom: Vec<u8>,
    pub ram: Vec<u8>,

    // MBC1 state:
    pub rom_bank: u8,      // ROM bank selected right now
    pub ram_bank: u8,      // RAM bank or if banking mode is on they act as upper bits for ROM_bank (it's oly 2 bit long)
    pub ram_enabled: bool, // if cartridge RAM is enabled
    pub banking_mode: u8,  // 0 = ROM banking mode, 1 = RAM banking mode
}

impl Cartridge {
    /// Creates a new [`Cartridge`] instance from raw ROM bytes.
    pub fn new(rom: Vec<u8>) -> Result<Self, String> {
        let header = CartridgeHeader::from_bytes(&rom)?;
        let ram = vec![0; header.ram_size];
        Ok(Self {
            header,
            rom, 
            ram,

            rom_bank: 1,
            ram_bank: 0,
            ram_enabled: false,
            banking_mode: 0,
        })
    }

    /// Reads a byte from the cartridge address space (`0x0000..=0x7FFF` for ROM, `0xA000..=0xBFFF` for RAM).
    pub fn read(&self, addr: u16) -> u8 {
        match self.header.cartridge_type {
            CartridgeType::RomOnly => {
                match addr {
                    0x0000..=0x7FFF => self.rom.get(addr as usize).copied().unwrap_or(0xFF), // it's trying to read from the rom
                    0xA000..=0xBFFF if (self.ram.len() > (addr - 0xA000) as usize) => self.ram[(addr - 0xA000) as usize], // it's trying to read from cartridge's ram
                    _ => 0xFF,
                }
            }
            CartridgeType::Mbc1 => {
                match addr {
                    // the first 8KiB contain always the bak 0
                    0x0000..=0x3FFF => {
                        self.rom.get(addr as usize).copied().unwrap_or(0xFF)
                    }
                    // reading the chosen bank
                    0x4000..=0x7FFF => {
                        let bank = self.rom_bank as usize;
                        let relative_addr = (addr - 0x4000) as usize;
                        let rom_addr = (bank * 16384) + relative_addr;
                        
                        self.rom.get(rom_addr % self.rom.len()).copied().unwrap_or(0xFF)
                    }
                    // reading external ram
                    0xA000..=0xBFFF => {
                        if self.ram_enabled && !self.ram.is_empty() {
                            let bank = if self.banking_mode == 1 { self.ram_bank as usize } else { 0 };
                            let ram_addr = (bank * 8192) + ((addr - 0xA000) as usize);
                            
                            self.ram.get(ram_addr % self.ram.len()).copied().unwrap_or(0xFF)
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
        match self.header.cartridge_type {
            CartridgeType::RomOnly => {
                match addr {
                    0xA000..=0xBFFF if (self.ram.len() > (addr - 0xA000) as usize) => self.ram[(addr - 0xA000) as usize] = val, // it's trying to write to cartridge's ram
                    _ => {},
                }
            }
            CartridgeType::Mbc1 => {
                // if the game tries to write at a rom address the MBC1 chip intercept's it
                match addr {
                    0x0000..=0x1FFF => { self.ram_enabled = (val & 0x0F) == 0x0A; }
                    0x2000..=0x3FFF => {
                        let mut bank = val & 0x1F;
                        if bank == 0 {
                            bank = 1;
                        }
                        self.rom_bank = bank;
                    }
                    0x4000..=0x5FFF => { self.ram_bank = val & 0x03; }
                    0x6000..=0x7FFF => { self.banking_mode = val & 0x01; }
                    0xA000..=0xBFFF => { // normal write to the external ram
                        if self.ram_enabled && !self.ram.is_empty() {
                            let bank = if self.banking_mode == 1 { self.ram_bank as usize } else { 0 };
                            let ram_addr = (bank * 8192) + ((addr - 0xA000) as usize);
                            let len = self.ram.len();
                            self.ram[ram_addr % len] = val;
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
        assert_eq!(cart.rom_bank, 2);
        assert_eq!(cart.read(0x4000), 0x22);

        // Switch to Bank 3
        cart.write(0x2000, 3);
        assert_eq!(cart.rom_bank, 3);
        assert_eq!(cart.read(0x4000), 0x33);

        // Hardware Quirk: writing 0 selects Bank 1
        cart.write(0x2000, 0);
        assert_eq!(cart.rom_bank, 1);
        assert_eq!(cart.read(0x4000), 0x11);
    }

    #[test]
    fn test_mbc1_ram_enable_and_banking() {
        // 64 KiB ROM, MBC1 (code 0x03 = MBC1+RAM+BATTERY), 32 KiB RAM (code 0x03)
        let rom = make_test_rom(0x03, 0x01, 0x03);
        let mut cart = Cartridge::new(rom).unwrap();

        // 1. RAM disabled by default: reads return 0xFF and writes are ignored
        assert_eq!(cart.read(0xA000), 0xFF);
        cart.write(0xA000, 0x42);
        assert_eq!(cart.read(0xA000), 0xFF);

        // 2. Enable RAM by writing 0x0A to 0x0000..=0x1FFF
        cart.write(0x0000, 0x0A);
        assert!(cart.ram_enabled);

        // Write and read from Bank 0
        cart.write(0xA000, 0x77);
        assert_eq!(cart.read(0xA000), 0x77);

        // 3. Switch to RAM banking mode (mode 1)
        cart.write(0x6000, 1);
        assert_eq!(cart.banking_mode, 1);

        // Select RAM Bank 1 by writing to 0x4000
        cart.write(0x4000, 1);
        assert_eq!(cart.ram_bank, 1);

        // Write to Bank 1
        cart.write(0xA000, 0x88);
        assert_eq!(cart.read(0xA000), 0x88);

        // Switch back to RAM Bank 0: Bank 0 should still hold 0x77!
        cart.write(0x4000, 0);
        assert_eq!(cart.read(0xA000), 0x77);

        // 4. Disable RAM by writing 0x00 to 0x0000
        cart.write(0x0000, 0x00);
        assert!(!cart.ram_enabled);
        assert_eq!(cart.read(0xA000), 0xFF);
    }
}