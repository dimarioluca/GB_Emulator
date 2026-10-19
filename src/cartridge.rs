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
}

impl Cartridge {
    /// Creates a new [`Cartridge`] instance from raw ROM bytes.
    pub fn new(rom: Vec<u8>) -> Result<Self, String> {
        let header = CartridgeHeader::from_bytes(&rom)?;
        let ram = vec![0; header.ram_size];
        Ok(Self { header, rom, ram })
    }

    /// Reads a byte from the cartridge address space (`0x0000..=0x7FFF` for ROM, `0xA000..=0xBFFF` for RAM).
    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.rom[addr as usize], // it's trying to read from the rom
            0xA000..=0xBFFF if (self.ram.len() > (addr - 0xA000) as usize) => self.ram[(addr - 0xA000) as usize], // it's trying to read from cartridge's ram
            _ => 0xFF,
        }
    } 

    /// Writes a byte to the cartridge address space.
    pub fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0xA000..=0xBFFF if (self.ram.len() > (addr - 0xA000) as usize) => self.ram[(addr - 0xA000) as usize] = val, // it's trying to write to cartridge's ram
            _ => {},
        }
    }
}