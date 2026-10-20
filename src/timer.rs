//! Game Boy Hardware Timer implementation.
//!
//! The Game Boy timer subsystem consists of four memory-mapped I/O registers:
//! - `DIV` (`0xFF04`): Divider Register, upper 8 bits of an internal 16-bit counter incremented every T-cycle. Writing any value resets it to 0.
//! - `TIMA` (`0xFF05`): Timer Counter, incremented at a clock frequency configured by `TAC`.
//! - `TMA` (`0xFF06`): Timer Modulo, reloaded into `TIMA` when it overflows.
//! - `TAC` (`0xFF07`): Timer Control, controls enable state and clock frequency (4096, 262144, 65536, or 16384 Hz).
//!
//! When `TIMA` overflows, bit 2 of the Interrupt Flag (`IF`) register (`0xFF0F`) is set to request a Timer Interrupt.

/// Represents the hardware timer of the Game Boy.
pub struct Timer {
    pub div_counter: u16, // internal 16 bit counter, it gets incremented every clock cycle (8 most significant bits make the DIV, Divider Register)
    pub tima: u8,        // 0xFF05: timer Counter
    pub tma: u8,         // 0xFF06: timer Modulo (the value used to inizialize TIMA when it overflows)
    pub tac: u8,         // 0xFF07: configures timer, bit 2 enables (1) or disables (0) timer, and increments timer speed
    prev_edge: bool,     // Stores previous timer signal state (high/low) to detect a falling edge (1 -> 0 transition)
}

impl Timer {
    /// Creates a new [`Timer`] initialized to default post-boot state.
    pub fn new() -> Self {
        Self {
            div_counter: 0,
            tima: 0,
            tma: 0,
            tac: 0,
            prev_edge: false,
        }
    }

    /// Reads a byte from timer memory-mapped registers (`0xFF04..=0xFF07`).
    pub fn read_byte(&self, addr: u16) -> u8 {
        match addr {
            0xFF04 => (self.div_counter >> 8) as u8,
            0xFF05 => self.tima,
            0xFF06 => self.tma,
            0xFF07 => self.tac | 0xF8, // Unused upper 5 bits of TAC always read as 1 on DMG
            _ => 0xFF,
        }
    }

    /// Writes a byte to timer memory-mapped registers (`0xFF04..=0xFF07`).
    pub fn write_byte(&mut self, addr: u16, val: u8) {
        match addr {
            0xFF04 => self.div_counter = 0, // Any write to DIV resets the counter to 0
            0xFF05 => self.tima = val,
            0xFF06 => self.tma = val,
            0xFF07 => self.tac = val & 0x07,
            _ => {},
        }
    }

    /// Advances the timer by the given number of CPU clock cycles (`cycles`),
    /// triggering a Timer Interrupt (bit 2 of `if_flag`) when `TIMA` overflows.
    pub fn step(&mut self, cycles: u32, if_flag: &mut u8) {
        for _ in 0..cycles {
            // Increment the internal 16-bit counter on every single T-cycle
            self.div_counter = self.div_counter.wrapping_add(1);

            // Check if the timer is enabled (Bit 2 of TAC register)
            let timer_enabled = (self.tac & 0x04) != 0;

            // Select which bit of the internal counter to monitor, based on TAC clock select (Bits 1-0):
            //    - 00: 4096 Hz   (every 1024 cycles -> bit 9 transitions from 1 to 0)
            //    - 01: 262144 Hz (every 16 cycles   -> bit 3 transitions from 1 to 0)
            //    - 10: 65536 Hz  (every 64 cycles   -> bit 5 transitions from 1 to 0)
            //    - 11: 16384 Hz  (every 256 cycles  -> bit 7 transitions from 1 to 0)
            let bit_pos = match self.tac & 0x03 {
                0b00 => 9,
                0b01 => 3,
                0b10 => 5,
                0b11 => 7,
                _ => unreachable!(),
            };

            // Test if the selected counter bit is currently high (1)
            let bit_value = (self.div_counter & (1 << bit_pos)) != 0;

            // The timer clock signal is high only if the timer is enabled AND the monitored bit is 1
            let current_edge = timer_enabled && bit_value;

            // Falling Edge Detector: a timer tick occurs specifically when the signal drops from 1 to 0
            // (i.e., signal was high in previous cycle and transitioned to low in current cycle)
            if self.prev_edge && !current_edge {
                // 7. Check if TIMA is at max value (0xFF). If so, an overflow occurs:
                if self.tima == 0xFF {
                    // Reload TIMA with the modulo value from TMA
                    self.tima = self.tma;
                    // Request Timer Interrupt by setting Bit 2 of IF register (0xFF0F)
                    *if_flag |= 0b0000_0100;
                } else {
                    // Otherwise, simply increment TIMA by 1
                    self.tima = self.tima.wrapping_add(1);
                }
            }

            // Remember the current signal state for the next cycle's edge detection
            self.prev_edge = current_edge;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_div_increment_and_reset() {
        let mut timer = Timer::new();
        let mut if_flag = 0u8;

        // DIV should be 0 initially
        assert_eq!(timer.read_byte(0xFF04), 0);

        // Advance 256 cycles: DIV should increment to 1
        timer.step(256, &mut if_flag);
        assert_eq!(timer.read_byte(0xFF04), 1);

        // Writing any value to 0xFF04 resets DIV to 0
        timer.write_byte(0xFF04, 0xAB);
        assert_eq!(timer.read_byte(0xFF04), 0);
        assert_eq!(timer.div_counter, 0);
    }

    #[test]
    fn test_tima_overflow_and_interrupt() {
        let mut timer = Timer::new();
        let mut if_flag = 0u8;

        // Configure TAC: Enabled (bit 2 = 1) and fastest speed (TAC & 3 = 01 -> every 16 cycles)
        timer.write_byte(0xFF07, 0b0000_0101);
        timer.write_byte(0xFF06, 0x50); // TMA modulo = 0x50
        timer.write_byte(0xFF05, 0xFE); // TIMA = 254

        // Advance 16 cycles -> TIMA increments to 255 (0xFF)
        timer.step(16, &mut if_flag);
        assert_eq!(timer.read_byte(0xFF05), 0xFF);
        assert_eq!(if_flag & 0x04, 0); // No interrupt yet

        // Advance another 16 cycles -> TIMA overflows!
        // Should reload TMA (0x50) and request Timer interrupt (bit 2 of if_flag)
        timer.step(16, &mut if_flag);
        assert_eq!(timer.read_byte(0xFF05), 0x50);
        assert_eq!(if_flag & 0x04, 0x04); // Timer interrupt requested!
    }

    #[test]
    fn test_timer_disabled() {
        let mut timer = Timer::new();
        let mut if_flag = 0u8;

        // TAC bit 2 = 0 (disabled), speed = 16 cycles
        timer.write_byte(0xFF07, 0b0000_0001);
        timer.write_byte(0xFF05, 0x00);

        timer.step(1000, &mut if_flag);
        assert_eq!(timer.read_byte(0xFF05), 0x00); // TIMA should not have incremented
        assert_eq!(if_flag, 0);
    }
}