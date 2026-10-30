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
                252..=455 => {
                    // Render the scanline once upon entering Mode 0 (H-Blank)
                    if (self.STAT & 0x03) != 0 {
                        self.render_scanline();
                        self.set_mode(0, if_flag);
                    }
                },
                _ => {}
            }
        } else {
            self.set_mode(1, if_flag); // Mode 1: V-Blank
        }
    }

    /// Converts a shade value to an RGBA color.
    fn shade_to_rgba(shade: u8) -> [u8; 4] {
        match shade {
            0 => [255, 255, 255, 255], // white
            1 => [170, 170, 170, 255], // light grey
            2 => [85,  85,  85,  255], // dark grey
            3 => [0,   0,   0,   255], // black
            _ => unreachable!(),
        }
    }

    fn render_background(&mut self) {
        if (self.LCDC & 0x01) == 0 { // background is disabled, fill scanline with white
            let y: usize = self.LY as usize;
            for x in 0..160 {
                let idx: usize = (y * 160 + x) * 4;
                self.framebuffer[idx..idx + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
            return;
        }
        else {
            let world_y: u8 = self.LY.wrapping_add(self.SCY); // y in the background map
            let tile_row: u8 = world_y / 8; // each tile is 8 pixels high
            let tile_y: u8 = world_y % 8; // y within the tile

            // calculate the base address of the background tile map and current row
            let map_base: usize = if (self.LCDC & 0x08) != 0 { 0x1C00 } else { 0x1800 };
            let row_base = map_base + (tile_row as usize) * 32;

            for x in 0..160 {
                let world_x: u8 = (x as u8).wrapping_add(self.SCX); // x in the background map
                let tile_col: u8 = world_x / 8; // each tile is 8 pixels wide
                let tile_x: u8 = world_x % 8; // x within the tile

                // get the tile index from the background map
                let tile_index: u8 = self.vram[row_base + (tile_col as usize)];

                // determine the address of the tile data in VRAM
                let tile_addr: usize = if (self.LCDC & 0x10) != 0 {
                    (tile_index as usize) * 16 // unsigned addressing: 0x8000 base (0x0000 in VRAM)
                } else {
                    // signed addressing: 0x9000 base (0x1000 in VRAM) with signed offset
                    (0x1000_isize + (tile_index as i8 as isize) * 16) as usize
                };

                // read the two bytes for this row of the tile
                let byte1: u8 = self.vram[tile_addr + (tile_y as usize) * 2];
                let byte2: u8 = self.vram[tile_addr + (tile_y as usize) * 2 + 1];

                // extract the color index for this pixel
                let bit_index: u8 = 7 - tile_x; // bits are stored MSB to LSB
                let color_index: u8 = ((byte2 >> bit_index) & 0x01) << 1 | ((byte1 >> bit_index) & 0x01);

                // apply the background palette to get the shade
                let shade: u8 = (self.BGP >> (color_index * 2)) & 0x03;

                // convert shade to RGBA and write to framebuffer
                let rgba: [u8; 4] = Ppu::shade_to_rgba(shade);
                let idx: usize = (self.LY as usize * 160 + x as usize) * 4;
                self.framebuffer[idx..idx + 4].copy_from_slice(&rgba);
            }
        }
    }

    fn render_window(&mut self) {
        if (self.LCDC & 0x20) == 0 || (self.LCDC & 0x01) == 0 || self.LY < self.WY || self.WX > 166 { 
            // window is disabled, don't render anything
            return;
        }
        else {
            let tile_row: u8 = self.window_line_counter / 8; // each tile is 8 pixels high
            let tile_y: u8 = self.window_line_counter % 8; // y within the tile

            // calculate the base address of the window tile map and current row
            let map_base: usize = if (self.LCDC & 0x40) != 0 { 0x1C00 } else { 0x1800 };
            let row_base = map_base + (tile_row as usize) * 32;

            for x in 0..160 {

                if (x as i32) + 7 < (self.WX as i32) {continue;}

                let window_x: u8 = (x as i32 + 7 - self.WX as i32) as u8; // x in the window map
                let tile_col: u8 = window_x / 8; // each tile is 8 pixels wide
                let tile_x: u8 = window_x % 8; // x within the tile

                // get the tile index from the window map
                let tile_index: u8 = self.vram[row_base + (tile_col as usize)];

                // determine the address of the tile data in VRAM
                let tile_addr: usize = if (self.LCDC & 0x10) != 0 {
                    (tile_index as usize) * 16 // unsigned addressing: 0x8000 base (0x0000 in VRAM)
                } else {
                    // signed addressing: 0x9000 base (0x1000 in VRAM) with signed offset
                    (0x1000_isize + (tile_index as i8 as isize) * 16) as usize
                };

                // read the two bytes for this row of the tile
                let byte1: u8 = self.vram[tile_addr + (tile_y as usize) * 2];
                let byte2: u8 = self.vram[tile_addr + (tile_y as usize) * 2 + 1];

                // extract the color index for this pixel
                let bit_index: u8 = 7 - tile_x; // bits are stored MSB to LSB
                let color_index: u8 = ((byte2 >> bit_index) & 0x01) << 1 | ((byte1 >> bit_index) & 0x01);

                // apply the window palette to get the shade
                let shade: u8 = (self.BGP >> (color_index * 2)) & 0x03;

                // convert shade to RGBA and write to framebuffer
                let rgba: [u8; 4] = Ppu::shade_to_rgba(shade);
                let idx: usize = (self.LY as usize * 160 + x as usize) * 4;
                self.framebuffer[idx..idx + 4].copy_from_slice(&rgba);
            }
            self.window_line_counter += 1;
        }
    }

    fn render_sprites(&mut self) {
        if (self.LCDC & 0x02) == 0 { // sprites are disabled
            return;
        }

        // Render sprites for the current scanline
        let sprite_height: i32 = if (self.LCDC & 0x04) != 0 { 16 } else { 8 }; // bit 2 of LCDC decides sprites height: 0 = 8x8, 1 = 8x16
        let mut sprites_on_line = Vec::new();

        for i in 0..40 {
            let sprite_y = (self.oam[i * 4] as i32) - 16;
            let ly = self.LY as i32;

            // Check if the sprite is on the current scanline
            if ly >= sprite_y && ly < sprite_y + sprite_height {
                sprites_on_line.push(i);

                if sprites_on_line.len() == 10 {
                    break; // hardware limit: max 10 sprites per scanline
                }
            }
        }

        // Sort sprites by X coordinate (ascending) and then by OAM index (ascending)
        sprites_on_line.sort_by(|&a, &b| {
            let xa: u8 = self.oam[a * 4 + 1];
            let xb: u8 = self.oam[b * 4 + 1];
            b.cmp(&a).then(xb.cmp(&xa))
        });

        let sprite_y = (self.oam[i * 4] as i32) - 16;
        let sprite_x = (self.oam[i * 4 + 1] as i32) - 8;
        let tile_index = self.oam[i * 4 + 2];
        let flags = self.oam[i * 4 + 3];

        let mut line: i32 = (self.LY as i32) - sprite_y;

    }

    fn render_scanline(&mut self) {
        self.render_background();
        self.render_window();
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

    #[test]
    fn test_ppu_background_render() {
        let mut ppu = Ppu::new();
        // LCDC: LCD on (bit 7), Tile data at 0x8000 (bit 4), BG tilemap at 0x9800 (bit 3), BG on (bit 0)
        ppu.LCDC = 0x91;
        // BGP: 0b11_10_01_00 (Color 3 -> Black, Color 2 -> Dark grey, Color 1 -> Light grey, Color 0 -> White)
        ppu.BGP = 0xE4;

        // Tile 0 row 0 in VRAM:
        // Byte 1: 0b10000000 (pixel 0 bit 0 = 1)
        // Byte 2: 0b10000000 (pixel 0 bit 1 = 1)
        // Color for pixel 0: (1 << 1) | 1 = 3 -> Black [0, 0, 0, 255]
        // Other pixels: Color 0 -> White [255, 255, 255, 255]
        ppu.vram[0] = 0x80;
        ppu.vram[1] = 0x80;

        // Tilemap at 0x1800 (0x9800): tile (0, 0) = Tile 0
        ppu.vram[0x1800] = 0;

        // Render line 0
        ppu.LY = 0;
        ppu.render_scanline();

        // Pixel 0 of line 0 should be Black
        assert_eq!(&ppu.framebuffer[0..4], &[0, 0, 0, 255]);
        // Pixel 1 of line 0 should be White
        assert_eq!(&ppu.framebuffer[4..8], &[255, 255, 255, 255]);
    }

    #[test]
    fn test_ppu_window_render() {
        let mut ppu = Ppu::new();
        // LCDC: LCD on (bit 7), Window map at 0x9C00 (bit 6), Window on (bit 5), Tile data 0x8000 (bit 4), BG on (bit 0)
        ppu.LCDC = 0xF1;
        ppu.BGP = 0xE4; // Standard palette
        ppu.WY = 10;
        ppu.WX = 7; // Window starts at screen X = 0

        // Tile 1 in VRAM: pixel 0 is Black (Color 3)
        ppu.vram[16] = 0x80;
        ppu.vram[17] = 0x80;

        // Window tilemap at 0x1C00 (0x9C00): tile (0, 0) = Tile 1
        ppu.vram[0x1C00] = 1;

        // Line 5: before WY (10) -> Window not drawn, counter stays 0
        ppu.LY = 5;
        ppu.render_scanline();
        assert_eq!(ppu.window_line_counter, 0);

        // Line 10: at WY -> Window drawn, counter increments to 1
        ppu.LY = 10;
        ppu.render_scanline();
        assert_eq!(ppu.window_line_counter, 1);

        // Pixel 0 of line 10 should be drawn by Window (Black: [0, 0, 0, 255])
        let idx = (10 * 160 + 0) * 4;
        assert_eq!(&ppu.framebuffer[idx..idx + 4], &[0, 0, 0, 255]);
    }
}

