//! Game Boy Joypad implementation.
//!
//! The Game Boy has 8 input buttons multiplexed onto 4 data lines using two selection lines.
//! The register uses active-low logic:
//! - 0 = Selected / Pressed
//! - 1 = Deselected / Released

#[derive(Clone, Copy)]
pub enum JoypadKey {
    Right,
    Left,
    Up,
    Down,
    A,
    B,
    Select,
    Start,
}

pub struct Joypad {
    // selection lines (bit 5 and bit 4 of 0xFF00).
    // in active-low logic: false = 0 (selected), true = 1 (deselected).
    pub select_action: bool,
    pub select_direction: bool,

    // button states: true = pressed, false = released
    pub right: bool,
    pub left: bool,
    pub up: bool,
    pub down: bool,
    pub a: bool,
    pub b: bool,
    pub select: bool,
    pub start: bool,
}

impl Joypad {
    /// Creates a new [`Joypad`] with all buttons released and selection lines deselected.
    pub fn new() -> Self {
        Joypad {
            select_action: true,
            select_direction: true,
            right: false,
            left: false,
            up: false,
            down: false,
            a: false,
            b: false,
            select: false,
            start: false,
        }
    }

    /// Reads register 0xFF00.
    ///
    /// Bit 7-6: Always 1 on DMG (0xC0).
    /// Bit 5: Action select line state (1 = unselected, 0 = selected).
    /// Bit 4: Direction select line state (1 = unselected, 0 = selected).
    /// Bit 3-0: Key state nibble (0 = pressed, 1 = released).
    pub fn read_byte(&self) -> u8 {
        let mut nibble = 0x0F;

        if !self.select_direction {
            if self.right { nibble &= !(1 << 0); }
            if self.left  { nibble &= !(1 << 1); }
            if self.up    { nibble &= !(1 << 2); }
            if self.down  { nibble &= !(1 << 3); }
        }

        if !self.select_action {
            if self.a      { nibble &= !(1 << 0); }
            if self.b      { nibble &= !(1 << 1); }
            if self.select { nibble &= !(1 << 2); }
            if self.start  { nibble &= !(1 << 3); }
        }

        0xC0 | ((self.select_action as u8) << 5) | ((self.select_direction as u8) << 4) | nibble
    }

    /// Writes register 0xFF00. Only bits 4 and 5 are writable by the CPU.
    pub fn write_byte(&mut self, val: u8) {
        self.select_action = (val & (1 << 5)) != 0;
        self.select_direction = (val & (1 << 4)) != 0;
    }

    /// Handles button press. If the corresponding group is currently selected, requests Joypad interrupt (bit 4 of IF).
    pub fn key_down(&mut self, key: JoypadKey, if_flag: &mut u8) {
        let was_pressed = self.is_key_pressed(key);
        self.set_key_state(key, true);

        if !was_pressed {
            let is_direction = matches!(
                key,
                JoypadKey::Right | JoypadKey::Left | JoypadKey::Up | JoypadKey::Down
            );
            let should_interrupt = (is_direction && !self.select_direction)
                || (!is_direction && !self.select_action);

            if should_interrupt {
                *if_flag |= 0x10;
            }
        }
    }

    /// Handles button release.
    pub fn key_up(&mut self, key: JoypadKey) {
        self.set_key_state(key, false);
    }

    /// Returns whether the specified button is currently pressed.
    fn is_key_pressed(&self, key: JoypadKey) -> bool {
        match key {
            JoypadKey::Right => self.right,
            JoypadKey::Left => self.left,
            JoypadKey::Up => self.up,
            JoypadKey::Down => self.down,
            JoypadKey::A => self.a,
            JoypadKey::B => self.b,
            JoypadKey::Select => self.select,
            JoypadKey::Start => self.start,
        }
    }

    /// Sets the pressed/released state for the specified button.
    fn set_key_state(&mut self, key: JoypadKey, pressed: bool) {
        match key {
            JoypadKey::Right => self.right = pressed,
            JoypadKey::Left => self.left = pressed,
            JoypadKey::Up => self.up = pressed,
            JoypadKey::Down => self.down = pressed,
            JoypadKey::A => self.a = pressed,
            JoypadKey::B => self.b = pressed,
            JoypadKey::Select => self.select = pressed,
            JoypadKey::Start => self.start = pressed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_keys_press_and_release() {
        let mut joypad = Joypad::new();
        let mut if_flag = 0u8;

        // Select both groups
        joypad.write_byte(0b0000_0000);

        let keys = [
            JoypadKey::Right,
            JoypadKey::Left,
            JoypadKey::Up,
            JoypadKey::Down,
            JoypadKey::A,
            JoypadKey::B,
            JoypadKey::Select,
            JoypadKey::Start,
        ];

        for key in keys {
            joypad.key_down(key, &mut if_flag);
            assert!(joypad.is_key_pressed(key));
            joypad.key_up(key);
            assert!(!joypad.is_key_pressed(key));
        }
    }

    #[test]
    fn test_initial_state_unselected() {
        let joypad = Joypad::new();
        // With neither group selected, bits 0-3 are 1, bits 4-5 are 1, bits 6-7 are 1 -> 0xFF
        assert_eq!(joypad.read_byte(), 0xFF);
    }

    #[test]
    fn test_direction_buttons() {
        let mut joypad = Joypad::new();
        let mut if_flag = 0u8;

        // Select direction buttons: write bit 4 = 0, bit 5 = 1
        joypad.write_byte(0b0010_0000);
        assert_eq!(joypad.read_byte(), 0xEF); // All directions released

        // Press Left (bit 1)
        joypad.key_down(JoypadKey::Left, &mut if_flag);
        assert_eq!(joypad.read_byte(), 0xED); // Bit 1 is 0
        assert_eq!(if_flag & 0x10, 0x10);    // Joypad interrupt triggered

        // Release Left
        joypad.key_up(JoypadKey::Left);
        assert_eq!(joypad.read_byte(), 0xEF);
    }

    #[test]
    fn test_action_buttons() {
        let mut joypad = Joypad::new();
        let mut if_flag = 0u8;

        // Select action buttons: write bit 5 = 0, bit 4 = 1
        joypad.write_byte(0b0001_0000);
        assert_eq!(joypad.read_byte(), 0xDF); // All actions released

        // Press A (bit 0) and Start (bit 3)
        joypad.key_down(JoypadKey::A, &mut if_flag);
        joypad.key_down(JoypadKey::Start, &mut if_flag);
        assert_eq!(joypad.read_byte(), 0xD6); // Bit 0 and Bit 3 are 0
    }

    #[test]
    fn test_no_interrupt_when_group_not_selected() {
        let mut joypad = Joypad::new();
        let mut if_flag = 0u8;

        // Select Direction buttons only
        joypad.write_byte(0b0010_0000);

        // Press Action button A: should NOT trigger interrupt because Action is not selected
        joypad.key_down(JoypadKey::A, &mut if_flag);
        assert_eq!(if_flag & 0x10, 0);

        // But reading action buttons should now show A pressed once Action is selected
        joypad.write_byte(0b0001_0000);
        assert_eq!(joypad.read_byte() & 0x01, 0);
    }
}
