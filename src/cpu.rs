/// Represents the Sharp SM83 (LR35902) CPU of the Game Boy.
pub struct Cpu {
    pub a: u8, // Accumulator: primary register for arithmetic and logic results
    pub f: u8, // flags: Z (zero), N (subtraction), H (half-carry), C (carry)

    pub b: u8, // General purpose (often loop counter)
    pub c: u8, // General purpose (often I/O port offset)

    pub d: u8, // General purpose
    pub e: u8, // General purpose

    pub h: u8, // High byte of memory pointer
    pub l: u8, // Low byte of memory pointer

    pub sp: u16, // Stack Pointer
    pub pc: u16, // Program Counter

    pub ime: bool,    // Interrupt Master Enable (completely disables interrupts)
    pub halted: bool, // CPU halted state (for standby)
}

impl Cpu {
    /// Creates a new [`Cpu`] initialized to the standard DMG post-boot state.
    pub fn new() -> Self {
        Self {
            a: 0x01,
            f: 0xB0,
            b: 0x00,
            c: 0x13,
            d: 0x00,
            e: 0xD8,
            h: 0x01,
            l: 0x4D,
            sp: 0xFFFE,
            pc: 0x0100,
            ime: false,
            halted: false,
        }
    }

    /// Gets the combined 16-bit AF register value.
    pub fn af(&self) -> u16 {
        ((self.a as u16) << 8) | (self.f as u16)
    }

    /// Sets the combined 16-bit AF register value.
    /// Note: The lower 4 bits of F are hardwired to zero in Game Boy hardware.
    pub fn set_af(&mut self, val: u16) {
        self.a = (val >> 8) as u8;
        self.f = (val as u8) & 0xF0;
    }

    /// Gets the combined 16-bit BC register value.
    pub fn bc(&self) -> u16 {
        ((self.b as u16) << 8) | (self.c as u16)
    }

    /// Sets the combined 16-bit BC register value.
    pub fn set_bc(&mut self, val: u16) {
        self.b = (val >> 8) as u8;
        self.c = val as u8;
    }

    /// Gets the combined 16-bit DE register value.
    pub fn de(&self) -> u16 {
        ((self.d as u16) << 8) | (self.e as u16)
    }

    /// Sets the combined 16-bit DE register value.
    pub fn set_de(&mut self, val: u16) {
        self.d = (val >> 8) as u8;
        self.e = val as u8;
    }

    /// Gets the combined 16-bit HL register value.
    pub fn hl(&self) -> u16 {
        ((self.h as u16) << 8) | (self.l as u16)
    }

    /// Sets the combined 16-bit HL register value.
    pub fn set_hl(&mut self, val: u16) {
        self.h = (val >> 8) as u8;
        self.l = val as u8;
    }

    /// Returns true if the Zero (Z) flag is set.
    pub fn flag_z(&self) -> bool {
        (self.f & 0x80) != 0
    }

    /// Sets or clears the Zero (Z) flag (Bit 7 of F).
    pub fn set_flag_z(&mut self, val: bool) {
        if val {
            self.f |= 0x80;
        } else {
            self.f &= !0x80;
        }
    }

    /// Returns true if the Subtraction (N) flag is set.
    pub fn flag_n(&self) -> bool {
        (self.f & 0x40) != 0
    }

    /// Sets or clears the Subtraction (N) flag (Bit 6 of F).
    pub fn set_flag_n(&mut self, val: bool) {
        if val {
            self.f |= 0x40;
        } else {
            self.f &= !0x40;
        }
    }

    /// Returns true if the Half-Carry (H) flag is set.
    pub fn flag_h(&self) -> bool {
        (self.f & 0x20) != 0
    }

    /// Sets or clears the Half-Carry (H) flag (Bit 5 of F).
    pub fn set_flag_h(&mut self, val: bool) {
        if val {
            self.f |= 0x20;
        } else {
            self.f &= !0x20;
        }
    }

    /// Returns true if the Carry (C) flag is set.
    pub fn flag_c(&self) -> bool {
        (self.f & 0x10) != 0
    }

    /// Sets or clears the Carry (C) flag (Bit 4 of F).
    pub fn set_flag_c(&mut self, val: bool) {
        if val {
            self.f |= 0x10;
        } else {
            self.f &= !0x10;
        }
    }

    /// Adds `val` to register A and updates the Z, N, H, and C flags.
    pub fn alu_add(&mut self, val: u8) {
        let res = self.a.wrapping_add(val);

        self.set_flag_z(res == 0); // set z to true if result is 0
        self.set_flag_n(false); // is not a subtaction

        // check overflow
        let carry = (self.a as u16 + val as u16) > 0xFF;
        self.set_flag_c(carry);

        // check half carry
        let half_carry = ((self.a & 0x0F) + (val & 0x0F)) > 0x0F;
        self.set_flag_h(half_carry);

        self.a = res;
    }

    /// Adds `val` plus the Carry flag to register A and updates the Z, N, H, and C flags.
    pub fn alu_adc(&mut self, val: u8) {
        let c = if self.flag_c() { 1 } else { 0 };
        let res = self.a.wrapping_add(val).wrapping_add(c);

        self.set_flag_z(res == 0);
        self.set_flag_n(false);

        self.set_flag_c((self.a as u16 + val as u16 + c as u16) > 0xFF);
        self.set_flag_h(((self.a & 0x0F) + (val & 0x0F) + c) > 0x0F);

        self.a = res;
    }

    /// Subtracts `val` from register A and updates the Z, N, H, and C flags.
    pub fn alu_sub(&mut self, val: u8) {
        let res = self.a.wrapping_sub(val);

        self.set_flag_z(res == 0);
        self.set_flag_n(true); // is a subtaction

        // check overflow
        let carry = (self.a as u16) < (val as u16);
        self.set_flag_c(carry);

        // check half carry
        let half_carry = (self.a & 0x0F) < ((val & 0x0F));
        self.set_flag_h(half_carry);

        self.a = res;
    }

    /// Subtracts `val` and the Carry flag from register A and updates the Z, N, H, and C flags.
    pub fn alu_sbc(&mut self, val: u8) {
        let c = if self.flag_c() { 1 } else { 0 };

        let res = self.a.wrapping_sub(val).wrapping_sub(c);

        self.set_flag_z(res == 0);
        self.set_flag_n(true);

        let carry = (self.a as u16) < (val as u16 + c as u16);
        self.set_flag_c(carry);

        let half_carry = (self.a & 0x0F) < ((val & 0x0F) + c);
        self.set_flag_h(half_carry);

        self.a = res;
    }

    /// Compares register A with `val` by calculating `A - val` and updating flags without modifying A.
    pub fn alu_cp(&mut self, val: u8) { // it's the same as sub, but without saving the result
        let res = self.a.wrapping_sub(val);

        self.set_flag_z(res == 0);
        self.set_flag_n(true);

        let carry = (self.a as u16) < (val as u16);
        self.set_flag_c(carry);

        let half_carry = (self.a & 0x0F) < ((val & 0x0F));
        self.set_flag_h(half_carry);
    }

    /// Performs bitwise AND between register A and `val`. Resets N and C, sets H to 1.
    pub fn alu_and(&mut self, val: u8) {
        self.a &= val;
        self.set_flag_z(self.a == 0);
        self.set_flag_n(false);
        self.set_flag_c(false);
        self.set_flag_h(true);
    }

    /// Performs bitwise OR between register A and `val`. Resets N, H, and C.
    pub fn alu_or(&mut self, val: u8) {
        self.a |= val;
        self.set_flag_z(self.a == 0);
        self.set_flag_n(false);
        self.set_flag_c(false);
        self.set_flag_h(false);
    }

    /// Performs bitwise XOR between register A and `val`. Resets N, H, and C.
    pub fn alu_xor(&mut self, val: u8) {
        self.a ^= val;
        self.set_flag_z(self.a == 0);
        self.set_flag_n(false);
        self.set_flag_c(false);
        self.set_flag_h(false);
    }

    /// Increments `val` by 1 and updates the Z, N, and H flags. Carry flag is preserved.
    pub fn alu_inc(&mut self, val: u8) -> u8 {
        let res = val.wrapping_add(1);
        self.set_flag_z(res == 0);
        self.set_flag_n(false);
        self.set_flag_h((val & 0x0F) == 0x0F);
        res
    }

    /// Decrements `val` by 1 and updates the Z, N, and H flags. Carry flag is preserved.
    pub fn alu_dec(&mut self, val: u8) -> u8 {
        let res = val.wrapping_sub(1);
        self.set_flag_z(res == 0);
        self.set_flag_n(true);
        self.set_flag_h((val & 0x0F) == 0x00);
        res
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_post_boot_state() {
        let cpu = Cpu::new();
        assert_eq!(cpu.a, 0x01);
        assert_eq!(cpu.f, 0xB0);
        assert_eq!(cpu.af(), 0x01B0);
        assert_eq!(cpu.bc(), 0x0013);
        assert_eq!(cpu.de(), 0x00D8);
        assert_eq!(cpu.hl(), 0x014D);
        assert_eq!(cpu.sp, 0xFFFE);
        assert_eq!(cpu.pc, 0x0100);

        assert!(cpu.flag_z());
        assert!(!cpu.flag_n());
        assert!(cpu.flag_h());
        assert!(cpu.flag_c());
    }

    #[test]
    fn test_flags_individual_set() {
        let mut cpu = Cpu::new();
        cpu.f = 0x00;

        cpu.set_flag_z(true);
        assert!(cpu.flag_z());
        assert_eq!(cpu.f, 0x80);

        cpu.set_flag_n(true);
        assert!(cpu.flag_n());
        assert_eq!(cpu.f, 0xC0);

        cpu.set_flag_h(true);
        assert!(cpu.flag_h());
        assert_eq!(cpu.f, 0xE0);

        cpu.set_flag_c(true);
        assert!(cpu.flag_c());
        assert_eq!(cpu.f, 0xF0);

        cpu.set_flag_z(false);
        assert!(!cpu.flag_z());
        assert_eq!(cpu.f, 0x70);
    }

    #[test]
    fn test_set_af_masks_lower_nibble() {
        let mut cpu = Cpu::new();
        cpu.set_af(0x123F); // F's 4 lower bits must be 0x0F
        assert_eq!(cpu.af(), 0x1230); // they must be set to 0x30!
    }

    #[test]
    fn test_alu_add_and_adc() {
        let mut cpu = Cpu::new();
        cpu.a = 0x0F;
        cpu.alu_add(0x01);
        assert_eq!(cpu.a, 0x10);
        assert!(!cpu.flag_z());
        assert!(!cpu.flag_n());
        assert!(cpu.flag_h());
        assert!(!cpu.flag_c());

        // ADC with carry set
        cpu.set_flag_c(true);
        cpu.a = 0x00;
        cpu.alu_adc(0x00);
        assert_eq!(cpu.a, 0x01);
        assert!(!cpu.flag_z());
        assert!(!cpu.flag_c());
    }

    #[test]
    fn test_alu_sub_sbc_and_cp() {
        let mut cpu = Cpu::new();
        cpu.a = 0x10;
        cpu.alu_sub(0x10);
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.flag_z());
        assert!(cpu.flag_n());
        assert!(!cpu.flag_h());
        assert!(!cpu.flag_c());

        // Underflow / Borrow (3 - 5)
        cpu.a = 0x03;
        cpu.alu_sub(0x05);
        assert_eq!(cpu.a, 0xFE); // 254
        assert!(!cpu.flag_z());
        assert!(cpu.flag_n());
        assert!(cpu.flag_h()); // 3 < 5
        assert!(cpu.flag_c()); // 3 < 5 (borrow)

        // CP does not change A
        cpu.a = 0x42;
        cpu.alu_cp(0x42);
        assert_eq!(cpu.a, 0x42);
        assert!(cpu.flag_z());
        assert!(cpu.flag_n());
    }

    #[test]
    fn test_alu_bitwise_ops() {
        let mut cpu = Cpu::new();
        cpu.a = 0x0F;
        cpu.alu_and(0xF0);
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.flag_z());
        assert!(!cpu.flag_n());
        assert!(cpu.flag_h()); // AND always sets H to 1!
        assert!(!cpu.flag_c());

        cpu.a = 0xAA;
        cpu.alu_xor(0xAA);
        assert_eq!(cpu.a, 0x00);
        assert!(cpu.flag_z());

        cpu.a = 0x0F;
        cpu.alu_or(0xF0);
        assert_eq!(cpu.a, 0xFF);
        assert!(!cpu.flag_z());
    }

    #[test]
    fn test_alu_inc_and_dec() {
        let mut cpu = Cpu::new();
        cpu.set_flag_c(true); // Carry should NOT be touched by inc/dec

        let res = cpu.alu_inc(0x0F);
        assert_eq!(res, 0x10);
        assert!(!cpu.flag_z());
        assert!(!cpu.flag_n());
        assert!(cpu.flag_h());
        assert!(cpu.flag_c()); // C remains true!

        let res = cpu.alu_dec(0x01);
        assert_eq!(res, 0x00);
        assert!(cpu.flag_z());
        assert!(cpu.flag_n());
        assert!(!cpu.flag_h());
        assert!(cpu.flag_c()); // C remains true!
    }
}