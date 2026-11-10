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

/// Represents the Real-Time Clock (RTC) chip found in MBC3 cartridges.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Rtc {
    // Active registers (0x08..=0x0C)
    pub seconds: u8,
    pub minutes: u8,
    pub hours: u8,
    pub days_low: u8,
    pub days_high: u8, // bit 0: day bit 8, bit 6: halt, bit 7: day carry

    // Latched registers (frozen snapshot read at 0xA000..=0xBFFF)
    pub latched_seconds: u8,
    pub latched_minutes: u8,
    pub latched_hours: u8,
    pub latched_days_low: u8,
    pub latched_days_high: u8,

    // Base time tracking for real-time advancement
    pub last_time: Option<std::time::Instant>,
}

impl Default for Rtc {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl Rtc {
    /// Creates a new [`Rtc`] instance with initial zeroed registers and real-time tracking enabled.
    pub fn new() -> Self {
        Self {
            seconds: 0,
            minutes: 0,
            hours: 0,
            days_low: 0,
            days_high: 0,
            latched_seconds: 0,
            latched_minutes: 0,
            latched_hours: 0,
            latched_days_low: 0,
            latched_days_high: 0,
            last_time: Some(std::time::Instant::now()),
        }
    }

    /// Latches the current RTC registers into the read buffers.
    pub fn latch(&mut self) {
        self.update_time();
        self.latched_seconds = self.seconds;
        self.latched_minutes = self.minutes;
        self.latched_hours = self.hours;
        self.latched_days_low = self.days_low;
        self.latched_days_high = self.days_high;
    }

    /// Reads a latched RTC register (0x08..=0x0C).
    pub fn read_reg(&self, reg: u8) -> u8 {
        match reg {
            0x08 => self.latched_seconds,
            0x09 => self.latched_minutes,
            0x0A => self.latched_hours,
            0x0B => self.latched_days_low,
            0x0C => self.latched_days_high,
            _ => 0xFF,
        }
    }

    /// Writes to an active RTC register (0x08..=0x0C).
    pub fn write_reg(&mut self, reg: u8, val: u8) {
        match reg {
            0x08 => self.seconds = val & 0x3F,   // 0..=59
            0x09 => self.minutes = val & 0x3F,   // 0..=59
            0x0A => self.hours = val & 0x1F,     // 0..=23
            0x0B => self.days_low = val,
            0x0C => self.days_high = val & 0xC1, // bit 0: day bit 8, bit 6: halt, bit 7: carry
            _ => {}
        }
    }

    /// Updates internal time registers based on elapsed real-world time.
    pub fn update_time(&mut self) {
        if let Some(last) = self.last_time {
            let now = std::time::Instant::now();
            let elapsed = now.duration_since(last).as_secs();
            if elapsed > 0 {
                self.last_time = Some(now);
                if (self.days_high & 0x40) == 0 {
                    self.add_seconds(elapsed);
                }
            }
        } else {
            self.last_time = Some(std::time::Instant::now());
        }
    }

    /// Adds seconds to the active RTC registers, propagating carries to minutes, hours, and days.
    pub fn add_seconds(&mut self, secs: u64) {
        if secs == 0 || (self.days_high & 0x40) != 0 {
            return;
        }
        let total_secs = self.seconds as u64 + secs;
        self.seconds = (total_secs % 60) as u8;

        let total_mins = self.minutes as u64 + (total_secs / 60);
        self.minutes = (total_mins % 60) as u8;

        let total_hours = self.hours as u64 + (total_mins / 60);
        self.hours = (total_hours % 24) as u8;

        let total_days = self.days() as u64 + (total_hours / 24);
        let day_count = (total_days % 512) as u16;
        self.days_low = (day_count & 0xFF) as u8;

        let day_bit_8 = ((day_count >> 8) & 1) as u8;
        let carry = if total_days >= 512 { 0x80 } else { self.days_high & 0x80 };
        let halt = self.days_high & 0x40;
        self.days_high = carry | halt | day_bit_8;
    }

    /// Returns the full 9-bit day count (0..=511).
    pub fn days(&self) -> u16 {
        ((self.days_high as u16 & 1) << 8) | (self.days_low as u16)
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
    Mbc3 {
        rom_bank: u8,         // 7-bit ROM bank selected (1..=127)
        ram_bank_or_rtc: u8, // RAM bank (0x00..=0x03) or RTC register (0x08..=0x0C)
        ram_enabled: bool,    // if cartridge RAM and RTC registers are enabled
        rtc: Rtc,
        rtc_latch: u8,        // previous value written to 0x6000..=0x7FFF
    },
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
            CartridgeType::Mbc3 => Mbc::Mbc3 {
                rom_bank: 1,
                ram_bank_or_rtc: 0,
                ram_enabled: false,
                rtc: Rtc::new(),
                rtc_latch: 0xFF,
            },
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

    // Helper methods to inspect MBC state (useful for tests and debugging)
    #[allow(dead_code)]
    pub fn rom_bank(&self) -> u8 {
        match &self.mbc {
            Mbc::Mbc1 { rom_bank, .. } => *rom_bank,
            Mbc::Mbc2 { rom_bank, .. } => *rom_bank,
            Mbc::Mbc3 { rom_bank, .. } => *rom_bank,
            _ => 0,
        }
    }

    #[allow(dead_code)]
    pub fn ram_bank(&self) -> u8 {
        match &self.mbc {
            Mbc::Mbc1 { ram_bank, .. } => *ram_bank,
            Mbc::Mbc3 { ram_bank_or_rtc, .. } => *ram_bank_or_rtc,
            _ => 0,
        }
    }

    #[allow(dead_code)]
    pub fn ram_enabled(&self) -> bool {
        match &self.mbc {
            Mbc::Mbc1 { ram_enabled, .. } => *ram_enabled,
            Mbc::Mbc2 { ram_enabled, .. } => *ram_enabled,
            Mbc::Mbc3 { ram_enabled, .. } => *ram_enabled,
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
            Mbc::Mbc3 { rom_bank, ram_bank_or_rtc, ram_enabled, rtc, .. } => {
                match addr {
                    // the first bank is fixed
                    0x0000..=0x3FFF => self.rom.get(addr as usize).copied().unwrap_or(0xFF),
                    
                    // selected bank
                    0x4000..=0x7FFF => {
                        let num_rom_banks = (self.rom.len() / 0x4000).max(1);
                        let bank = (*rom_bank as usize) % num_rom_banks;
                        let rom_addr = bank * 0x4000 + ((addr - 0x4000) as usize);
                        self.rom.get(rom_addr).copied().unwrap_or(0xFF)
                    }

                    // RAM o RTC
                    0xA000..=0xBFFF => {
                        if *ram_enabled {
                            if *ram_bank_or_rtc <= 0x03 && !self.ram.is_empty() { // reding from RAM (0..=3)
                                let ram_addr = (*ram_bank_or_rtc as usize * 8192) + ((addr - 0xA000) as usize);
                                self.ram.get(ram_addr % self.ram.len()).copied().unwrap_or(0xFF)
                            } else if *ram_bank_or_rtc >= 0x08 && *ram_bank_or_rtc <= 0x0C { // reading from RTC
                                rtc.read_reg(*ram_bank_or_rtc)
                            } else {
                                0xFF
                            }
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
            Mbc::Mbc3 { rom_bank, ram_bank_or_rtc, ram_enabled, rtc, rtc_latch } => {
                match addr {
                    // enable RAM and RTC
                    0x0000..=0x1FFF => {
                        *ram_enabled = (val & 0x0F) == 0x0A;
                    }
                    // select ROM bank
                    0x2000..=0x3FFF => {
                        let bank = val & 0x7F;
                        *rom_bank = if bank == 0 { 1 } else { bank };
                    }
                    // RAM Bank / RTC Register Select
                    0x4000..=0x5FFF => {
                        *ram_bank_or_rtc = val;
                    }
                    // latch RTC
                    0x6000..=0x7FFF => {
                        if *rtc_latch == 0x00 && val == 0x01 {
                            rtc.latch();
                        }
                        *rtc_latch = val;
                    }
                    // writing on ram or RTC, depending on what's enabled
                    0xA000..=0xBFFF => {
                        if *ram_enabled {
                            if *ram_bank_or_rtc <= 0x03 && !self.ram.is_empty() {
                                let ram_addr = (*ram_bank_or_rtc as usize * 8192) + ((addr - 0xA000) as usize);
                                let len = self.ram.len();
                                self.ram[ram_addr % len] = val;
                            } else if *ram_bank_or_rtc >= 0x08 && *ram_bank_or_rtc <= 0x0C {
                                // Scrittura nel registro RTC attivo!
                                rtc.write_reg(*ram_bank_or_rtc, val);
                            }
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

    #[test]
    fn test_mbc3_rom_banking() {
        // 2 MiB ROM (code 0x06 = 128 banks), MBC3 (code 0x11), RAM code 0
        let mut rom = make_test_rom(0x11, 0x06, 0x00);

        // Fill Bank 0, 1, 2, 65
        rom[0x0000..0x0100].fill(0xAA);
        rom[0x0150..0x4000].fill(0xAA);
        rom[0x4000..0x8000].fill(0x11);
        rom[2 * 0x4000..3 * 0x4000].fill(0x22);
        rom[65 * 0x4000..66 * 0x4000].fill(0x66);

        let mut cart = Cartridge::new(rom).unwrap();
        assert_eq!(cart.header.cartridge_type, CartridgeType::Mbc3);

        // Default state: Bank 1 is mapped at 0x4000..=0x7FFF
        assert_eq!(cart.read(0x4000), 0x11);
        assert_eq!(cart.rom_bank(), 1);

        // Switch to Bank 2 (7-bit register at 0x2000)
        cart.write(0x2000, 2);
        assert_eq!(cart.rom_bank(), 2);
        assert_eq!(cart.read(0x4000), 0x22);

        // Switch to Bank 65 (direct 7-bit, unlike MBC1!)
        cart.write(0x2000, 65);
        assert_eq!(cart.rom_bank(), 65);
        assert_eq!(cart.read(0x4000), 0x66);

        // Quirk: writing 0 selects Bank 1
        cart.write(0x2000, 0);
        assert_eq!(cart.rom_bank(), 1);
        assert_eq!(cart.read(0x4000), 0x11);
    }

    #[test]
    fn test_mbc3_ram_banking() {
        // 64 KiB ROM, 32 KiB RAM (4 banks of 8 KiB), MBC3+RAM+BATTERY (code 0x13)
        let rom = make_test_rom(0x13, 0x01, 0x03);
        let mut cart = Cartridge::new(rom).unwrap();

        // RAM disabled by default: reads return 0xFF and writes are ignored
        assert_eq!(cart.read(0xA000), 0xFF);
        cart.write(0xA000, 0x42);
        assert_eq!(cart.read(0xA000), 0xFF);

        // Enable RAM by writing 0x0A to 0x0000..=0x1FFF
        cart.write(0x0000, 0x0A);
        assert!(cart.ram_enabled());

        // Write and read from Bank 0
        cart.write(0xA000, 0x55);
        assert_eq!(cart.read(0xA000), 0x55);

        // Switch to RAM Bank 1 by writing to 0x4000..=0x5FFF
        cart.write(0x4000, 1);
        assert_eq!(cart.ram_bank(), 1);
        cart.write(0xA000, 0x77);
        assert_eq!(cart.read(0xA000), 0x77);

        // Switch back to RAM Bank 0: Bank 0 should still hold 0x55!
        cart.write(0x4000, 0);
        assert_eq!(cart.ram_bank(), 0);
        assert_eq!(cart.read(0xA000), 0x55);

        // Disable RAM
        cart.write(0x0000, 0x00);
        assert!(!cart.ram_enabled());
        assert_eq!(cart.read(0xA000), 0xFF);
    }

    #[test]
    fn test_mbc3_rtc_registers_and_latch() {
        // 64 KiB ROM, MBC3+TIMER+RAM+BATTERY (code 0x10), 32 KiB RAM (code 0x03)
        let rom = make_test_rom(0x10, 0x01, 0x03);
        let mut cart = Cartridge::new(rom).unwrap();

        // Enable RAM and Timer access by writing 0x0A to 0x0000
        cart.write(0x0000, 0x0A);
        assert!(cart.ram_enabled());

        // Write active RTC registers via 0xA000:
        // Seconds (0x08): 45
        cart.write(0x4000, 0x08);
        cart.write(0xA000, 45);

        // Minutes (0x09): 30
        cart.write(0x4000, 0x09);
        cart.write(0xA000, 30);

        // Hours (0x0A): 14 (2 PM)
        cart.write(0x4000, 0x0A);
        cart.write(0xA000, 14);

        // Days Low (0x0B): 100
        cart.write(0x4000, 0x0B);
        cart.write(0xA000, 100);

        // Days High / Flags (0x0C): 0x41 (bit 0 = day bit 8, bit 6 = halt)
        cart.write(0x4000, 0x0C);
        cart.write(0xA000, 0x41);

        // Before latching, reads return 0 (initial latched state)
        assert_eq!(cart.read(0xA000), 0);

        // Latch clock data: write 0x00 then 0x01 to 0x6000
        cart.write(0x6000, 0x00);
        cart.write(0x6000, 0x01);

        // Verify that latched values can now be read:
        cart.write(0x4000, 0x08);
        assert_eq!(cart.read(0xA000), 45);

        cart.write(0x4000, 0x09);
        assert_eq!(cart.read(0xA000), 30);

        cart.write(0x4000, 0x0A);
        assert_eq!(cart.read(0xA000), 14);

        cart.write(0x4000, 0x0B);
        assert_eq!(cart.read(0xA000), 100);

        cart.write(0x4000, 0x0C);
        assert_eq!(cart.read(0xA000), 0x41);
    }
}
