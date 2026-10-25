#[allow(non_snake_case)]
pub struct Ppu { // Pixel Processing Unit
    pub framebuffer: [u8; 160 * 144 * 4], // 4 bytes per pixel (RGBA8888, 32-bit) compatible with pixels crate and GBC
    pub vram: [u8; 8192], // 0x8000..=0x9FFF, divided into: 0x8000..=0x97FF for tile data, 0x9800..=0x9FFF for tile maps
    pub oam: [u8; 160], // Object Attribute Memory for sprites (40 sprites * 4 bytes)

    pub LCDC: u8, // LCD Control Register 0xFF40
    pub STAT: u8, // LCD Status Register 0xFF41
    pub SCY: u8, // Scroll Y 0xFF42
    pub SCX: u8, // Scroll X 0xFF43
    pub LY: u8, // LCD Y Coordinate 0xFF44
    pub LYC: u8, // LY Compare 0xFF45
    pub DMA: u8, // OAM DMA Transfer 0xFF46
    pub BGP: u8, // Palette Data 0xFF47
    pub OBP0: u8, // Object Palette 0 Data 0xFF48
    pub OBP1: u8, // Object Palette 1 Data 0xFF49
    pub WY: u8, // Window Y Position 0xFF4A
    pub WX: u8, // Window X Position 0xFF4B

    pub dot_clock: u16,          // Counts cycles (dots) spent on the current scanline (0..456), resets at end of scanline
    pub window_line_counter: u8, // Internal line counter for the window layer (only increments when window is rendered)
    pub frame_ready: bool,       // Flag set to true when an entire frame (144 scanlines) is complete and ready to render
}

impl Ppu {
    pub fn new() -> Self {
        Ppu {
            framebuffer: [0xFF; 160 * 144 * 4], // Initialized with white screen and 100% opaque Alpha
            vram: [0; 8192],
            oam: [0; 160],

            LCDC: 0x91,
            STAT: 0x85,
            SCY: 0x00,
            SCX: 0x00,
            LY: 0x00,
            LYC: 0x00,
            DMA: 0x00,
            BGP: 0xFC,
            OBP0: 0x00,
            OBP1: 0x00,
            WY: 0x00,
            WX: 0x00,

            dot_clock: 0,
            window_line_counter: 0,
            frame_ready: false,
        }
    }

    /// Sets the current PPU Mode in STAT (bits 1-0) and requests an LCD STAT interrupt if enabled.
    fn set_mode(&mut self, mode: u8, if_flag: &mut u8) {
        let prev_mode = self.STAT & 0x03;
        if prev_mode == mode {
            return;
        }

        self.STAT = (self.STAT & !0x03) | (mode & 0x03);

        // Check if a STAT interrupt is requested for entering this mode
        let req_interrupt = match mode {
            0 => (self.STAT & 0x08) != 0, // Mode 0 H-Blank interrupt enable (bit 3)
            1 => (self.STAT & 0x10) != 0, // Mode 1 V-Blank interrupt enable (bit 4)
            2 => (self.STAT & 0x20) != 0, // Mode 2 OAM Search interrupt enable (bit 5)
            _ => false,
        };

        if req_interrupt {
            *if_flag |= 0x02; // Bit 1 of IF = LCD STAT interrupt
        }
    }

    /// Checks if LY equals LYC, updating STAT coincidence flag (bit 2) and requesting interrupt if enabled.
    fn check_lyc(&mut self, if_flag: &mut u8) {
        if self.LY == self.LYC {
            self.STAT |= 0x04; // Set LYC == LY coincidence flag (bit 2)
            if (self.STAT & 0x40) != 0 {
                *if_flag |= 0x02; // Bit 1 of IF = LCD STAT interrupt (bit 6 enable)
            }
        } else {
            self.STAT &= !0x04; // Clear LYC == LY coincidence flag
        }
    }

    /// Advances the PPU state by the given number of CPU clock cycles (dots).
    pub fn step(&mut self, cycles: u32, if_flag: &mut u8) {
        // If LCD is disabled, keep PPU reset and in Mode 0
        if (self.LCDC & 0x80) == 0 {
            self.LY = 0;
            self.dot_clock = 0;
            self.STAT &= !0x03; // Mode 0
            return;
        }

        // Advance dot clock
        self.dot_clock += cycles as u16;

        // Check if scanline has finished (456 dots)
        if self.dot_clock >= 456 {
            self.dot_clock -= 456;
            self.LY += 1;

            if self.LY == 144 {
                // Entering V-Blank mode
                self.set_mode(1, if_flag);
                *if_flag |= 0x01; // Bit 0 of IF = V-Blank interrupt
                self.frame_ready = true;
            } else if self.LY > 153 {
                // End of V-Blank: frame completed, reset to scanline 0
                self.LY = 0;
                self.window_line_counter = 0;
                self.set_mode(2, if_flag);
            }

            // Check LY == LYC coincidence on the new scanline
            self.check_lyc(if_flag);
        }

        // Update mode based on current scanline (LY) and dot_clock
        if self.LY < 144 {
            match self.dot_clock {
                0..=79 => self.set_mode(2, if_flag),    // Mode 2: OAM Search
                80..=251 => self.set_mode(3, if_flag),  // Mode 3: Pixel Transfer
                252..=455 => self.set_mode(0, if_flag), // Mode 0: H-Blank
                _ => {}
            }
        } else {
            self.set_mode(1, if_flag); // Mode 1: V-Blank
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ppu_scanline_modes() {
        let mut ppu = Ppu::new();
        let mut if_flag = 0u8;

        // Start at dot_clock = 0 on scanline 0 -> Mode 2 (OAM Search)
        ppu.step(4, &mut if_flag);
        assert_eq!(ppu.STAT & 0x03, 2);
        assert_eq!(ppu.LY, 0);

        // Advance to dot_clock = 80 -> Mode 3 (Pixel Transfer)
        ppu.step(76, &mut if_flag);
        assert_eq!(ppu.STAT & 0x03, 3);

        // Advance to dot_clock = 252 -> Mode 0 (H-Blank)
        ppu.step(172, &mut if_flag);
        assert_eq!(ppu.STAT & 0x03, 0);

        // Complete scanline (456 cycles total) -> advances to LY = 1, Mode 2
        ppu.step(204, &mut if_flag);
        assert_eq!(ppu.LY, 1);
        assert_eq!(ppu.STAT & 0x03, 2);
    }

    #[test]
    fn test_ppu_vblank_and_frame_ready() {
        let mut ppu = Ppu::new();
        let mut if_flag = 0u8;

        // Step 144 full scanlines (144 * 456 cycles)
        for _ in 0..144 {
            ppu.step(456, &mut if_flag);
        }

        assert_eq!(ppu.LY, 144);
        assert_eq!(ppu.STAT & 0x03, 1); // Mode 1 V-Blank
        assert_eq!(if_flag & 0x01, 1);  // V-Blank interrupt requested
        assert!(ppu.frame_ready);

        // Step remaining 10 V-Blank lines to complete full frame (154 lines total)
        for _ in 0..10 {
            ppu.step(456, &mut if_flag);
        }

        assert_eq!(ppu.LY, 0); // Wrapped back to line 0
    }

    #[test]
    fn test_ppu_lyc_coincidence() {
        let mut ppu = Ppu::new();
        let mut if_flag = 0u8;

        ppu.LYC = 2;
        ppu.STAT |= 0x40; // Enable LYC interrupt in STAT register

        // Advance to scanline 1
        ppu.step(456, &mut if_flag);
        assert_eq!(ppu.LY, 1);
        assert_eq!(ppu.STAT & 0x04, 0); // No coincidence yet

        // Advance to scanline 2 -> coincidence!
        ppu.step(456, &mut if_flag);
        assert_eq!(ppu.LY, 2);
        assert_eq!(ppu.STAT & 0x04, 0x04); // LYC == LY set
        assert_eq!(if_flag & 0x02, 0x02);  // STAT interrupt fired!
    }

    #[test]
    fn test_ppu_disabled() {
        let mut ppu = Ppu::new();
        let mut if_flag = 0u8;

        ppu.LCDC &= !0x80; // Disable LCD
        ppu.step(1000, &mut if_flag);

        assert_eq!(ppu.LY, 0);
        assert_eq!(ppu.dot_clock, 0);
        assert_eq!(ppu.STAT & 0x03, 0);
    }
}

