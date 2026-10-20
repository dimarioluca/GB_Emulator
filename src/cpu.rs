//! Sharp SM83 (LR35902) CPU core for the Nintendo Game Boy.
//!
//! This module implements the 8-bit processor, including:
//! - 8-bit registers (`A`, `B`, `C`, `D`, `E`, `H`, `L`) and flags register (`F`)
//! - 16-bit register pairs (`AF`, `BC`, `DE`, `HL`), Stack Pointer (`SP`), and Program Counter (`PC`)
//! - Complete 8-bit and 16-bit ALU operations (arithmetic, logic, BCD decimal adjustment)
//! - All 256 standard opcodes and 256 prefix-CB opcodes with cycle-accurate timing
//! - Hardware interrupt controller with priority servicing and HALT bug emulation

use crate::bus::Bus;

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
    pub halt_bug: bool,       // Glitch when HALT is executed with IME=false and pending interrupts
    pub ime_scheduled: u8,   // Delay counter for EI (interrupts enabled after the next instruction)
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
            halt_bug: false,
            ime_scheduled: 0,
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

    /// Fetches an 8-bit byte from memory at the current PC and advances PC by 1.
    /// If `halt_bug` is active, PC is not incremented (simulating the hardware bug where the next byte is fetched twice).
    fn fetch_byte(&mut self, bus: &Bus) -> u8 {
        let byte = bus.read_byte(self.pc);
        if self.halt_bug {
            self.halt_bug = false;
        } else {
            self.pc = self.pc.wrapping_add(1);
        }
        byte
    }

    /// Fetches a 16-bit word from memory at the current PC in Little-Endian format and advances PC by 2.
    fn fetch_word(&mut self, bus: &Bus) -> u16 {
        let low = self.fetch_byte(bus) as u16;
        let high = self.fetch_byte(bus) as u16;
        (high << 8) | low
    }

    /// Pushes a 16-bit word onto the stack (decrements SP by 2, writes Little-Endian word).
    fn push_word(&mut self, bus: &mut Bus, val: u16) {
        self.sp = self.sp.wrapping_sub(2);
        bus.write_word(self.sp, val);
    }

    /// Pops a 16-bit word from the stack (reads Little-Endian word from SP, increments SP by 2).
    fn pop_word(&mut self, bus: &Bus) -> u16 {
        let val = bus.read_word(self.sp);
        self.sp = self.sp.wrapping_add(2);
        val
    }

    /// Checks and services pending hardware interrupts according to their priority.
    pub fn handle_interrupts(&mut self, bus: &mut Bus) -> Option<u32> {
        let pending = bus.ie & bus.if_flag & 0x1F;

        if pending != 0 {self.halted = false;}

        if self.ime && pending != 0 {
            self.ime = false;
            self.push_word(bus, self.pc);

            // we use if and not match because we only want one and we have a priority
            if (pending & 0x01) != 0 {
                bus.if_flag &= !0x01; // VBlank serviced
                self.pc = 0x0040;
            } else if (pending & 0x02) != 0 {
                bus.if_flag &= !0x02; // LCD STAT serviced
                self.pc = 0x0048;
            } else if (pending & 0x04) != 0 {
                bus.if_flag &= !0x04; // Timer serviced
                self.pc = 0x0050;
            } else if (pending & 0x08) != 0 {
                bus.if_flag &= !0x08; // Serial serviced
                self.pc = 0x0058;
            } else if (pending & 0x10) != 0 {
                bus.if_flag &= !0x10; // Joypad serviced
                self.pc = 0x0060;
            }

            Some(20)
        } else {
            None
        }
    }

    /// Adds a 16-bit value to HL and updates H and C flags (Z flag is preserved, N is cleared).
    pub fn add_hl(&mut self, val: u16) {
        let hl = self.hl();
        let res = hl.wrapping_add(val);
        self.set_flag_n(false);
        self.set_flag_h(((hl & 0x0FFF) + (val & 0x0FFF)) > 0x0FFF);
        self.set_flag_c((hl as u32 + val as u32) > 0xFFFF);
        self.set_hl(res);
    }

    /// Decimal Adjust Accumulator (DAA): adjusts register A into Binary-Coded Decimal (BCD).
    pub fn daa(&mut self) {
        let mut adjust = 0;
        let mut carry = false;
        if self.flag_h() || (!self.flag_n() && (self.a & 0x0F) > 0x09) {
            adjust |= 0x06;
        }
        if self.flag_c() || (!self.flag_n() && self.a > 0x99) {
            adjust |= 0x60;
            carry = true;
        }
        if self.flag_n() {
            self.a = self.a.wrapping_sub(adjust);
        } else {
            self.a = self.a.wrapping_add(adjust);
        }
        self.set_flag_z(self.a == 0);
        self.set_flag_h(false);
        self.set_flag_c(carry);
    }

    /// Reads an 8-bit register or memory value `(HL)` using the 3-bit CB opcode register index:
    /// `0 = B`, `1 = C`, `2 = D`, `3 = E`, `4 = H`, `5 = L`, `6 = (HL)`, `7 = A`.
    fn get_reg_cb(&self, bus: &Bus, reg: u8) -> u8 {
        match reg {
            0 => self.b,
            1 => self.c,
            2 => self.d,
            3 => self.e,
            4 => self.h,
            5 => self.l,
            6 => bus.read_byte(self.hl()),
            7 => self.a,
            _ => unreachable!(),
        }
    }

    /// Writes an 8-bit value to a register or memory location `(HL)` using the 3-bit CB opcode register index:
    /// `0 = B`, `1 = C`, `2 = D`, `3 = E`, `4 = H`, `5 = L`, `6 = (HL)`, `7 = A`.
    fn set_reg_cb(&mut self, bus: &mut Bus, reg: u8, val: u8) {
        match reg {
            0 => self.b = val,
            1 => self.c = val,
            2 => self.d = val,
            3 => self.e = val,
            4 => self.h = val,
            5 => self.l = val,
            6 => bus.write_byte(self.hl(), val),
            7 => self.a = val,
            _ => unreachable!(),
        }
    }

    /// Executes a prefix 0xCB instruction and returns the T-cycles elapsed.
    pub fn execute_cb(&mut self, bus: &mut Bus) -> u32 {
        let cb_op = self.fetch_byte(bus);
        let reg_idx = cb_op & 0x07;
        let val = self.get_reg_cb(bus, reg_idx);

        let group = cb_op >> 6;
        let bit = (cb_op >> 3) & 0x07;

        match group {
            0b00 => {
                let res = match bit {
                    0 => {
                        // RLC: Rotate Left Circular
                        let c = (val & 0x80) != 0;
                        let res = (val << 1) | (if c { 1 } else { 0 });
                        self.set_flag_z(res == 0);
                        self.set_flag_n(false);
                        self.set_flag_h(false);
                        self.set_flag_c(c);
                        res
                    }
                    1 => {
                        // RRC: Rotate Right Circular
                        let c = (val & 0x01) != 0;
                        let res = (val >> 1) | (if c { 0x80 } else { 0 });
                        self.set_flag_z(res == 0);
                        self.set_flag_n(false);
                        self.set_flag_h(false);
                        self.set_flag_c(c);
                        res
                    }
                    2 => {
                        // RL: Rotate Left through Carry
                        let old_c = if self.flag_c() { 1 } else { 0 };
                        let new_c = (val & 0x80) != 0;
                        let res = (val << 1) | old_c;
                        self.set_flag_z(res == 0);
                        self.set_flag_n(false);
                        self.set_flag_h(false);
                        self.set_flag_c(new_c);
                        res
                    }
                    3 => {
                        // RR: Rotate Right through Carry
                        let old_c = if self.flag_c() { 0x80 } else { 0 };
                        let new_c = (val & 0x01) != 0;
                        let res = (val >> 1) | old_c;
                        self.set_flag_z(res == 0);
                        self.set_flag_n(false);
                        self.set_flag_h(false);
                        self.set_flag_c(new_c);
                        res
                    }
                    4 => {
                        // SLA: Shift Left Arithmetic
                        let c = (val & 0x80) != 0;
                        let res = val << 1;
                        self.set_flag_z(res == 0);
                        self.set_flag_n(false);
                        self.set_flag_h(false);
                        self.set_flag_c(c);
                        res
                    }
                    5 => {
                        // SRA: Shift Right Arithmetic
                        let c = (val & 0x01) != 0;
                        let res = (val >> 1) | (val & 0x80);
                        self.set_flag_z(res == 0);
                        self.set_flag_n(false);
                        self.set_flag_h(false);
                        self.set_flag_c(c);
                        res
                    }
                    6 => {
                        // SWAP: Swap upper and lower nibbles
                        let res = (val >> 4) | (val << 4);
                        self.set_flag_z(res == 0);
                        self.set_flag_n(false);
                        self.set_flag_h(false);
                        self.set_flag_c(false);
                        res
                    }
                    7 => {
                        // SRL: Shift Right Logical
                        let c = (val & 0x01) != 0;
                        let res = val >> 1;
                        self.set_flag_z(res == 0);
                        self.set_flag_n(false);
                        self.set_flag_h(false);
                        self.set_flag_c(c);
                        res
                    }
                    _ => unreachable!(),
                };
                self.set_reg_cb(bus, reg_idx, res);
                if reg_idx == 6 { 16 } else { 8 }
            }
            0b01 => {
                // BIT b, r: Test bit b
                self.set_flag_z((val & (1 << bit)) == 0);
                self.set_flag_n(false);
                self.set_flag_h(true);
                if reg_idx == 6 { 12 } else { 8 }
            }
            0b10 => {
                // RES b, r: Reset bit b
                let res = val & !(1 << bit);
                self.set_reg_cb(bus, reg_idx, res);
                if reg_idx == 6 { 16 } else { 8 }
            }
            0b11 => {
                // SET b, r: Set bit b
                let res = val | (1 << bit);
                self.set_reg_cb(bus, reg_idx, res);
                if reg_idx == 6 { 16 } else { 8 }
            }
            _ => unreachable!(),
        }
    }

    /// Executes a single CPU instruction or handles a pending interrupt, returning elapsed T-cycles.
    pub fn step(&mut self, bus: &mut Bus) -> u32 {
        if let Some(cycles) = self.handle_interrupts(bus) {
            return cycles;
        }

        // if the CPU is hlted it waits 4 clock cicles
        if self.halted {
            return 4;
        }

        // FETCH: read the next instruction's OPCODE
        let opcode = self.fetch_byte(bus);

        // DECODE & EXECUTE
        let cycles = match opcode {
            0x00 => 4, // NOP: no operations

            // 0x01..0x0F
            0x01 => { let val = self.fetch_word(bus); self.set_bc(val); 12 } // LD BC, d16: Carica valore immediato a 16 bit in BC
            0x02 => { bus.write_byte(self.bc(), self.a); 8 } // LD [BC], A: Scrive A in memoria all'indirizzo BC
            0x03 => { self.set_bc(self.bc().wrapping_add(1)); 8 } // INC BC: Incrementa registro BC a 16 bit
            0x04 => { self.b = self.alu_inc(self.b); 4 } // INC B: Incrementa registro B
            0x05 => { self.b = self.alu_dec(self.b); 4 } // DEC B: Decrementa registro B
            0x06 => { self.b = self.fetch_byte(bus); 8 } // LD B, d8: Carica valore immediato a 8 bit in B
            0x07 => { // RLCA: Ruota A a sinistra (bit 7 nel Carry e nel bit 0)
                let c = (self.a & 0x80) != 0;
                self.a = (self.a << 1) | (if c { 1 } else { 0 });
                self.set_flag_z(false);
                self.set_flag_n(false);
                self.set_flag_h(false);
                self.set_flag_c(c);
                4
            }
            0x08 => { // LD [a16], SP: Scrive il puntatore SP all'indirizzo a 16 bit
                let addr = self.fetch_word(bus);
                bus.write_word(addr, self.sp);
                20
            }
            0x09 => { self.add_hl(self.bc()); 8 } // ADD HL, BC: Somma BC a HL a 16 bit
            0x0A => { self.a = bus.read_byte(self.bc()); 8 } // LD A, [BC]: Legge dalla memoria all'indirizzo BC in A
            0x0B => { self.set_bc(self.bc().wrapping_sub(1)); 8 } // DEC BC: Decrementa registro BC a 16 bit
            0x0C => { self.c = self.alu_inc(self.c); 4 } // INC C: Incrementa registro C
            0x0D => { self.c = self.alu_dec(self.c); 4 } // DEC C: Decrementa registro C
            0x0E => { self.c = self.fetch_byte(bus); 8 } // LD C, d8: Carica valore immediato a 8 bit in C
            0x0F => { // RRCA: Ruota A a destra (bit 0 nel Carry e nel bit 7)
                let c = (self.a & 0x01) != 0;
                self.a = (self.a >> 1) | (if c { 0x80 } else { 0 });
                self.set_flag_z(false);
                self.set_flag_n(false);
                self.set_flag_h(false);
                self.set_flag_c(c);
                4
            }

            // 0x10..0x1F
            0x10 => { self.fetch_byte(bus); 4 } // STOP: Modalità standby a basso consumo
            0x11 => { let val = self.fetch_word(bus); self.set_de(val); 12 } // LD DE, d16: Carica valore immediato a 16 bit in DE
            0x12 => { bus.write_byte(self.de(), self.a); 8 } // LD [DE], A: Scrive A in memoria all'indirizzo DE
            0x13 => { self.set_de(self.de().wrapping_add(1)); 8 } // INC DE: Incrementa registro DE a 16 bit
            0x14 => { self.d = self.alu_inc(self.d); 4 } // INC D: Incrementa registro D
            0x15 => { self.d = self.alu_dec(self.d); 4 } // DEC D: Decrementa registro D
            0x16 => { self.d = self.fetch_byte(bus); 8 } // LD D, d8: Carica valore immediato a 8 bit in D
            0x17 => { // RLA: Ruota A a sinistra attraverso il flag di Carry
                let old_c = if self.flag_c() { 1 } else { 0 };
                let new_c = (self.a & 0x80) != 0;
                self.a = (self.a << 1) | old_c;
                self.set_flag_z(false);
                self.set_flag_n(false);
                self.set_flag_h(false);
                self.set_flag_c(new_c);
                4
            }
            0x18 => { // JR r8: Salto relativo incondizionato con offset relativo con segno
                let offset = self.fetch_byte(bus) as i8;
                self.pc = self.pc.wrapping_add(offset as i16 as u16);
                12
            }
            0x19 => { self.add_hl(self.de()); 8 } // ADD HL, DE: Somma DE a HL a 16 bit
            0x1A => { self.a = bus.read_byte(self.de()); 8 } // LD A, [DE]: Legge dalla memoria all'indirizzo DE in A
            0x1B => { self.set_de(self.de().wrapping_sub(1)); 8 } // DEC DE: Decrementa registro DE a 16 bit
            0x1C => { self.e = self.alu_inc(self.e); 4 } // INC E: Incrementa registro E
            0x1D => { self.e = self.alu_dec(self.e); 4 } // DEC E: Decrementa registro E
            0x1E => { self.e = self.fetch_byte(bus); 8 } // LD E, d8: Carica valore immediato a 8 bit in E
            0x1F => { // RRA: Ruota A a destra attraverso il flag di Carry
                let old_c = if self.flag_c() { 0x80 } else { 0 };
                let new_c = (self.a & 0x01) != 0;
                self.a = (self.a >> 1) | old_c;
                self.set_flag_z(false);
                self.set_flag_n(false);
                self.set_flag_h(false);
                self.set_flag_c(new_c);
                4
            }

            // 0x20..0x2F
            0x20 => { // JR NZ, r8: Salto relativo se il flag Z è false
                let offset = self.fetch_byte(bus) as i8;
                if !self.flag_z() {
                    self.pc = self.pc.wrapping_add(offset as i16 as u16);
                    12
                } else {
                    8
                }
            }
            0x21 => { let val = self.fetch_word(bus); self.set_hl(val); 12 } // LD HL, d16: Carica valore immediato a 16 bit in HL
            0x22 => { // LD [HL+], A: Scrive A in [HL] e incrementa HL
                let hl = self.hl();
                bus.write_byte(hl, self.a);
                self.set_hl(hl.wrapping_add(1));
                8
            }
            0x23 => { self.set_hl(self.hl().wrapping_add(1)); 8 } // INC HL: Incrementa registro HL a 16 bit
            0x24 => { self.h = self.alu_inc(self.h); 4 } // INC H: Incrementa registro H
            0x25 => { self.h = self.alu_dec(self.h); 4 } // DEC H: Decrementa registro H
            0x26 => { self.h = self.fetch_byte(bus); 8 } // LD H, d8: Carica valore immediato a 8 bit in H
            0x27 => { self.daa(); 4 } // DAA: Aggiustamento decimale per cifre BCD in A
            0x28 => { // JR Z, r8: Salto relativo se il flag Z è true
                let offset = self.fetch_byte(bus) as i8;
                if self.flag_z() {
                    self.pc = self.pc.wrapping_add(offset as i16 as u16);
                    12
                } else {
                    8
                }
            }
            0x29 => { self.add_hl(self.hl()); 8 } // ADD HL, HL: Somma HL a HL a 16 bit
            0x2A => { // LD A, [HL+]: Legge da [HL] in A e incrementa HL
                let hl = self.hl();
                self.a = bus.read_byte(hl);
                self.set_hl(hl.wrapping_add(1));
                8
            }
            0x2B => { self.set_hl(self.hl().wrapping_sub(1)); 8 } // DEC HL: Decrementa registro HL a 16 bit
            0x2C => { self.l = self.alu_inc(self.l); 4 } // INC L: Incrementa registro L
            0x2D => { self.l = self.alu_dec(self.l); 4 } // DEC L: Decrementa registro L
            0x2E => { self.l = self.fetch_byte(bus); 8 } // LD L, d8: Carica valore immediato a 8 bit in L
            0x2F => { // CPL: Inverte i bit di A (complemento a 1)
                self.a = !self.a;
                self.set_flag_n(true);
                self.set_flag_h(true);
                4
            }

            // 0x30..0x3F
            0x30 => { // JR NC, r8: Salto relativo se Carry è false
                let offset = self.fetch_byte(bus) as i8;
                if !self.flag_c() {
                    self.pc = self.pc.wrapping_add(offset as i16 as u16);
                    12
                } else {
                    8
                }
            }
            0x31 => { self.sp = self.fetch_word(bus); 12 } // LD SP, d16: Carica valore immediato a 16 bit nello Stack Pointer
            0x32 => { // LD [HL-], A: Scrive A in [HL] e decrementa HL
                let hl = self.hl();
                bus.write_byte(hl, self.a);
                self.set_hl(hl.wrapping_sub(1));
                8
            }
            0x33 => { self.sp = self.sp.wrapping_add(1); 8 } // INC SP: Incrementa Stack Pointer a 16 bit
            0x34 => { // INC [HL]: Incrementa il byte in memoria puntato da HL
                let hl = self.hl();
                let val = self.alu_inc(bus.read_byte(hl));
                bus.write_byte(hl, val);
                12
            }
            0x35 => { // DEC [HL]: Decrementa il byte in memoria puntato da HL
                let hl = self.hl();
                let val = self.alu_dec(bus.read_byte(hl));
                bus.write_byte(hl, val);
                12
            }
            0x36 => { // LD [HL], d8: Scrive valore immediato a 8 bit all'indirizzo HL
                let val = self.fetch_byte(bus);
                bus.write_byte(self.hl(), val);
                12
            }
            0x37 => { // SCF: Imposta a true il flag Carry
                self.set_flag_n(false);
                self.set_flag_h(false);
                self.set_flag_c(true);
                4
            }
            0x38 => { // JR C, r8: Salto relativo se Carry è true
                let offset = self.fetch_byte(bus) as i8;
                if self.flag_c() {
                    self.pc = self.pc.wrapping_add(offset as i16 as u16);
                    12
                } else {
                    8
                }
            }
            0x39 => { self.add_hl(self.sp); 8 } // ADD HL, SP: Somma lo Stack Pointer a HL
            0x3A => { // LD A, [HL-]: Legge da [HL] in A e decrementa HL
                let hl = self.hl();
                self.a = bus.read_byte(hl);
                self.set_hl(hl.wrapping_sub(1));
                8
            }
            0x3B => { self.sp = self.sp.wrapping_sub(1); 8 } // DEC SP: Decrementa Stack Pointer a 16 bit
            0x3C => { self.a = self.alu_inc(self.a); 4 } // INC A: Incrementa registro A
            0x3D => { self.a = self.alu_dec(self.a); 4 } // DEC A: Decrementa registro A
            0x3E => { self.a = self.fetch_byte(bus); 8 } // LD A, d8: Carica valore immediato a 8 bit in A
            0x3F => { // CCF: Inverte il valore del flag Carry
                let c = self.flag_c();
                self.set_flag_n(false);
                self.set_flag_h(false);
                self.set_flag_c(!c);
                4
            }

            // Trasferimenti tra registri a 8-bit (LD r, r') 0x40..0x7F
            0x40 => 4, // LD B, B: Copia B in B
            0x41 => { self.b = self.c; 4 } // LD B, C: Copia C in B
            0x42 => { self.b = self.d; 4 } // LD B, D: Copia D in B
            0x43 => { self.b = self.e; 4 } // LD B, E: Copia E in B
            0x44 => { self.b = self.h; 4 } // LD B, H: Copia H in B
            0x45 => { self.b = self.l; 4 } // LD B, L: Copia L in B
            0x46 => { self.b = bus.read_byte(self.hl()); 8 } // LD B, [HL]: Carica da memoria all'indirizzo HL in B
            0x47 => { self.b = self.a; 4 } // LD B, A: Copia A in B

            0x48 => { self.c = self.b; 4 } // LD C, B: Copia B in C
            0x49 => 4, // LD C, C: Copia C in C
            0x4A => { self.c = self.d; 4 } // LD C, D: Copia D in C
            0x4B => { self.c = self.e; 4 } // LD C, E: Copia E in C
            0x4C => { self.c = self.h; 4 } // LD C, H: Copia H in C
            0x4D => { self.c = self.l; 4 } // LD C, L: Copia L in C
            0x4E => { self.c = bus.read_byte(self.hl()); 8 } // LD C, [HL]: Carica da memoria all'indirizzo HL in C
            0x4F => { self.c = self.a; 4 } // LD C, A: Copia A in C

            0x50 => { self.d = self.b; 4 } // LD D, B: Copia B in D
            0x51 => { self.d = self.c; 4 } // LD D, C: Copia C in D
            0x52 => 4, // LD D, D: Copia D in D
            0x53 => { self.d = self.e; 4 } // LD D, E: Copia E in D
            0x54 => { self.d = self.h; 4 } // LD D, H: Copia H in D
            0x55 => { self.d = self.l; 4 } // LD D, L: Copia L in D
            0x56 => { self.d = bus.read_byte(self.hl()); 8 } // LD D, [HL]: Carica da memoria all'indirizzo HL in D
            0x57 => { self.d = self.a; 4 } // LD D, A: Copia A in D

            0x58 => { self.e = self.b; 4 } // LD E, B: Copia B in E
            0x59 => { self.e = self.c; 4 } // LD E, C: Copia C in E
            0x5A => { self.e = self.d; 4 } // LD E, D: Copia D in E
            0x5B => 4, // LD E, E: Copia E in E
            0x5C => { self.e = self.h; 4 } // LD E, H: Copia H in E
            0x5D => { self.e = self.l; 4 } // LD E, L: Copia L in E
            0x5E => { self.e = bus.read_byte(self.hl()); 8 } // LD E, [HL]: Carica da memoria all'indirizzo HL in E
            0x5F => { self.e = self.a; 4 } // LD E, A: Copia A in E

            0x60 => { self.h = self.b; 4 } // LD H, B: Copia B in H
            0x61 => { self.h = self.c; 4 } // LD H, C: Copia C in H
            0x62 => { self.h = self.d; 4 } // LD H, D: Copia D in H
            0x63 => { self.h = self.e; 4 } // LD H, E: Copia E in H
            0x64 => 4, // LD H, H: Copia H in H
            0x65 => { self.h = self.l; 4 } // LD H, L: Copia L in H
            0x66 => { self.h = bus.read_byte(self.hl()); 8 } // LD H, [HL]: Carica da memoria all'indirizzo HL in H
            0x67 => { self.h = self.a; 4 } // LD H, A: Copia A in H

            0x68 => { self.l = self.b; 4 } // LD L, B: Copia B in L
            0x69 => { self.l = self.c; 4 } // LD L, C: Copia C in L
            0x6A => { self.l = self.d; 4 } // LD L, D: Copia D in L
            0x6B => { self.l = self.e; 4 } // LD L, E: Copia E in L
            0x6C => { self.l = self.h; 4 } // LD L, H: Copia H in L
            0x6D => 4, // LD L, L: Copia L in L
            0x6E => { self.l = bus.read_byte(self.hl()); 8 } // LD L, [HL]: Carica da memoria all'indirizzo HL in L
            0x6F => { self.l = self.a; 4 } // LD L, A: Copia A in L

            0x70 => { bus.write_byte(self.hl(), self.b); 8 } // LD [HL], B: Scrive B in memoria all'indirizzo HL
            0x71 => { bus.write_byte(self.hl(), self.c); 8 } // LD [HL], C: Scrive C in memoria all'indirizzo HL
            0x72 => { bus.write_byte(self.hl(), self.d); 8 } // LD [HL], D: Scrive D in memoria all'indirizzo HL
            0x73 => { bus.write_byte(self.hl(), self.e); 8 } // LD [HL], E: Scrive E in memoria all'indirizzo HL
            0x74 => { bus.write_byte(self.hl(), self.h); 8 } // LD [HL], H: Scrive H in memoria all'indirizzo HL
            0x75 => { bus.write_byte(self.hl(), self.l); 8 } // LD [HL], L: Scrive L in memoria all'indirizzo HL
            0x76 => { // HALT: Mette la CPU in attesa di un interrupt
                if !self.ime && (bus.ie & bus.if_flag & 0x1F) != 0 {
                    self.halt_bug = true;
                } else {
                    self.halted = true;
                }
                4
            }
            0x77 => { bus.write_byte(self.hl(), self.a); 8 } // LD [HL], A: Scrive A in memoria all'indirizzo HL

            0x78 => { self.a = self.b; 4 } // LD A, B: Copia B in A
            0x79 => { self.a = self.c; 4 } // LD A, C: Copia C in A
            0x7A => { self.a = self.d; 4 } // LD A, D: Copia D in A
            0x7B => { self.a = self.e; 4 } // LD A, E: Copia E in A
            0x7C => { self.a = self.h; 4 } // LD A, H: Copia H in A
            0x7D => { self.a = self.l; 4 } // LD A, L: Copia L in A
            0x7E => { self.a = bus.read_byte(self.hl()); 8 } // LD A, [HL]: Carica da memoria all'indirizzo HL in A
            0x7F => 4, // LD A, A: Copia A in A

            // Operazioni ALU a 8-bit con registri o [HL] 0x80..0xBF
            0x80 => { self.alu_add(self.b); 4 } // ADD A, B: Somma B ad A
            0x81 => { self.alu_add(self.c); 4 } // ADD A, C: Somma C ad A
            0x82 => { self.alu_add(self.d); 4 } // ADD A, D: Somma D ad A
            0x83 => { self.alu_add(self.e); 4 } // ADD A, E: Somma E ad A
            0x84 => { self.alu_add(self.h); 4 } // ADD A, H: Somma H ad A
            0x85 => { self.alu_add(self.l); 4 } // ADD A, L: Somma L ad A
            0x86 => { let val = bus.read_byte(self.hl()); self.alu_add(val); 8 } // ADD A, [HL]: Somma il byte in [HL] ad A
            0x87 => { self.alu_add(self.a); 4 } // ADD A, A: Somma A ad A

            0x88 => { self.alu_adc(self.b); 4 } // ADC A, B: Somma B e il Carry ad A
            0x89 => { self.alu_adc(self.c); 4 } // ADC A, C: Somma C e il Carry ad A
            0x8A => { self.alu_adc(self.d); 4 } // ADC A, D: Somma D e il Carry ad A
            0x8B => { self.alu_adc(self.e); 4 } // ADC A, E: Somma E e il Carry ad A
            0x8C => { self.alu_adc(self.h); 4 } // ADC A, H: Somma H e il Carry ad A
            0x8D => { self.alu_adc(self.l); 4 } // ADC A, L: Somma L e il Carry ad A
            0x8E => { let val = bus.read_byte(self.hl()); self.alu_adc(val); 8 } // ADC A, [HL]: Somma [HL] e il Carry ad A
            0x8F => { self.alu_adc(self.a); 4 } // ADC A, A: Somma A e il Carry ad A

            0x90 => { self.alu_sub(self.b); 4 } // SUB B: Sottrae B da A
            0x91 => { self.alu_sub(self.c); 4 } // SUB C: Sottrae C da A
            0x92 => { self.alu_sub(self.d); 4 } // SUB D: Sottrae D da A
            0x93 => { self.alu_sub(self.e); 4 } // SUB E: Sottrae E da A
            0x94 => { self.alu_sub(self.h); 4 } // SUB H: Sottrae H da A
            0x95 => { self.alu_sub(self.l); 4 } // SUB L: Sottrae L da A
            0x96 => { let val = bus.read_byte(self.hl()); self.alu_sub(val); 8 } // SUB [HL]: Sottrae il byte in [HL] da A
            0x97 => { self.alu_sub(self.a); 4 } // SUB A: Sottrae A da A (A = 0)

            0x98 => { self.alu_sbc(self.b); 4 } // SBC A, B: Sottrae B e il Carry da A
            0x99 => { self.alu_sbc(self.c); 4 } // SBC A, C: Sottrae C e il Carry da A
            0x9A => { self.alu_sbc(self.d); 4 } // SBC A, D: Sottrae D e il Carry da A
            0x9B => { self.alu_sbc(self.e); 4 } // SBC A, E: Sottrae E e il Carry da A
            0x9C => { self.alu_sbc(self.h); 4 } // SBC A, H: Sottrae H e il Carry da A
            0x9D => { self.alu_sbc(self.l); 4 } // SBC A, L: Sottrae L e il Carry da A
            0x9E => { let val = bus.read_byte(self.hl()); self.alu_sbc(val); 8 } // SBC A, [HL]: Sottrae [HL] e il Carry da A
            0x9F => { self.alu_sbc(self.a); 4 } // SBC A, A: Sottrae A e il Carry da A

            0xA0 => { self.alu_and(self.b); 4 } // AND B: AND logico bit a bit tra A e B
            0xA1 => { self.alu_and(self.c); 4 } // AND C: AND logico bit a bit tra A e C
            0xA2 => { self.alu_and(self.d); 4 } // AND D: AND logico bit a bit tra A e D
            0xA3 => { self.alu_and(self.e); 4 } // AND E: AND logico bit a bit tra A e E
            0xA4 => { self.alu_and(self.h); 4 } // AND H: AND logico bit a bit tra A e H
            0xA5 => { self.alu_and(self.l); 4 } // AND L: AND logico bit a bit tra A e L
            0xA6 => { let val = bus.read_byte(self.hl()); self.alu_and(val); 8 } // AND [HL]: AND logico bit a bit tra A e [HL]
            0xA7 => { self.alu_and(self.a); 4 } // AND A: AND logico bit a bit tra A e A

            0xA8 => { self.alu_xor(self.b); 4 } // XOR B: XOR logico bit a bit tra A e B
            0xA9 => { self.alu_xor(self.c); 4 } // XOR C: XOR logico bit a bit tra A e C
            0xAA => { self.alu_xor(self.d); 4 } // XOR D: XOR logico bit a bit tra A e D
            0xAB => { self.alu_xor(self.e); 4 } // XOR E: XOR logico bit a bit tra A e E
            0xAC => { self.alu_xor(self.h); 4 } // XOR H: XOR logico bit a bit tra A e H
            0xAD => { self.alu_xor(self.l); 4 } // XOR L: XOR logico bit a bit tra A e L
            0xAE => { let val = bus.read_byte(self.hl()); self.alu_xor(val); 8 } // XOR [HL]: XOR logico bit a bit tra A e [HL]
            0xAF => { self.alu_xor(self.a); 4 } // XOR A: XOR logico bit a bit tra A e A (A = 0)

            0xB0 => { self.alu_or(self.b); 4 } // OR B: OR logico bit a bit tra A e B
            0xB1 => { self.alu_or(self.c); 4 } // OR C: OR logico bit a bit tra A e C
            0xB2 => { self.alu_or(self.d); 4 } // OR D: OR logico bit a bit tra A e D
            0xB3 => { self.alu_or(self.e); 4 } // OR E: OR logico bit a bit tra A e E
            0xB4 => { self.alu_or(self.h); 4 } // OR H: OR logico bit a bit tra A e H
            0xB5 => { self.alu_or(self.l); 4 } // OR L: OR logico bit a bit tra A e L
            0xB6 => { let val = bus.read_byte(self.hl()); self.alu_or(val); 8 } // OR [HL]: OR logico bit a bit tra A e [HL]
            0xB7 => { self.alu_or(self.a); 4 } // OR A: OR logico bit a bit tra A e A

            0xB8 => { self.alu_cp(self.b); 4 } // CP B: Confronta A con B
            0xB9 => { self.alu_cp(self.c); 4 } // CP C: Confronta A con C
            0xBA => { self.alu_cp(self.d); 4 } // CP D: Confronta A con D
            0xBB => { self.alu_cp(self.e); 4 } // CP E: Confronta A con E
            0xBC => { self.alu_cp(self.h); 4 } // CP H: Confronta A con H
            0xBD => { self.alu_cp(self.l); 4 } // CP L: Confronta A con L
            0xBE => { let val = bus.read_byte(self.hl()); self.alu_cp(val); 8 } // CP [HL]: Confronta A con il byte in [HL]
            0xBF => { self.alu_cp(self.a); 4 } // CP A: Confronta A con A

            // Salti, chiamate di funzione, ritorni e stack 0xC0..0xFF
            0xC0 => { // RET NZ: Ritorna da funzione se il flag Z è false
                if !self.flag_z() {
                    self.pc = self.pop_word(bus);
                    20
                } else {
                    8
                }
            }
            0xC1 => { let val = self.pop_word(bus); self.set_bc(val); 12 } // POP BC: Estrae dallo stack in BC
            0xC2 => { // JP NZ, a16: Salta ad indirizzo se il flag Z è false
                let addr = self.fetch_word(bus);
                if !self.flag_z() {
                    self.pc = addr;
                    16
                } else {
                    12
                }
            }
            0xC3 => { self.pc = self.fetch_word(bus); 16 } // JP a16: Salto incondizionato a indirizzo a 16 bit
            0xC4 => { // CALL NZ, a16: Chiama subroutine se il flag Z è false
                let addr = self.fetch_word(bus);
                if !self.flag_z() {
                    self.push_word(bus, self.pc);
                    self.pc = addr;
                    24
                } else {
                    12
                }
            }
            0xC5 => { self.push_word(bus, self.bc()); 16 } // PUSH BC: Inserisce BC nello stack
            0xC6 => { let val = self.fetch_byte(bus); self.alu_add(val); 8 } // ADD A, d8: Somma valore immediato a 8 bit ad A
            0xC7 => { self.push_word(bus, self.pc); self.pc = 0x0000; 16 } // RST 00h: Restart saltando all'indirizzo 0x0000
            0xC8 => { // RET Z: Ritorna da funzione se il flag Z è true
                if self.flag_z() {
                    self.pc = self.pop_word(bus);
                    20
                } else {
                    8
                }
            }
            0xC9 => { self.pc = self.pop_word(bus); 16 } // RET: Ritorno incondizionato da subroutine
            0xCA => { // JP Z, a16: Salta ad indirizzo se il flag Z è true
                let addr = self.fetch_word(bus);
                if self.flag_z() {
                    self.pc = addr;
                    16
                } else {
                    12
                }
            }
            0xCB => self.execute_cb(bus), // PREFIX CB: Istruzioni su singoli bit, shift e rotazioni
            0xCC => { // CALL Z, a16: Chiama subroutine se il flag Z è true
                let addr = self.fetch_word(bus);
                if self.flag_z() {
                    self.push_word(bus, self.pc);
                    self.pc = addr;
                    24
                } else {
                    12
                }
            }
            0xCD => { // CALL a16: Chiama subroutine salvando il PC nello stack
                let addr = self.fetch_word(bus);
                self.push_word(bus, self.pc);
                self.pc = addr;
                24
            }
            0xCE => { let val = self.fetch_byte(bus); self.alu_adc(val); 8 } // ADC A, d8: Somma valore immediato e Carry ad A
            0xCF => { self.push_word(bus, self.pc); self.pc = 0x0008; 16 } // RST 08h: Restart saltando all'indirizzo 0x0008

            0xD0 => { // RET NC: Ritorna da funzione se Carry è false
                if !self.flag_c() {
                    self.pc = self.pop_word(bus);
                    20
                } else {
                    8
                }
            }
            0xD1 => { let val = self.pop_word(bus); self.set_de(val); 12 } // POP DE: Estrae dallo stack in DE
            0xD2 => { // JP NC, a16: Salta ad indirizzo se Carry è false
                let addr = self.fetch_word(bus);
                if !self.flag_c() {
                    self.pc = addr;
                    16
                } else {
                    12
                }
            }
            0xD4 => { // CALL NC, a16: Chiama subroutine se Carry è false
                let addr = self.fetch_word(bus);
                if !self.flag_c() {
                    self.push_word(bus, self.pc);
                    self.pc = addr;
                    24
                } else {
                    12
                }
            }
            0xD5 => { self.push_word(bus, self.de()); 16 } // PUSH DE: Inserisce DE nello stack
            0xD6 => { let val = self.fetch_byte(bus); self.alu_sub(val); 8 } // SUB d8: Sottrae valore immediato a 8 bit da A
            0xD7 => { self.push_word(bus, self.pc); self.pc = 0x0010; 16 } // RST 10h: Restart saltando all'indirizzo 0x0010
            0xD8 => { // RET C: Ritorna da funzione se Carry è true
                if self.flag_c() {
                    self.pc = self.pop_word(bus);
                    20
                } else {
                    8
                }
            }
            0xD9 => { // RETI: Ritorna da interrupt e riabilita gli interrupt (IME = true)
                self.pc = self.pop_word(bus);
                self.ime = true;
                16
            }
            0xDA => { // JP C, a16: Salta ad indirizzo se Carry è true
                let addr = self.fetch_word(bus);
                if self.flag_c() {
                    self.pc = addr;
                    16
                } else {
                    12
                }
            }
            0xDC => { // CALL C, a16: Chiama subroutine se Carry è true
                let addr = self.fetch_word(bus);
                if self.flag_c() {
                    self.push_word(bus, self.pc);
                    self.pc = addr;
                    24
                } else {
                    12
                }
            }
            0xDE => { let val = self.fetch_byte(bus); self.alu_sbc(val); 8 } // SBC A, d8: Sottrae valore immediato e Carry da A
            0xDF => { self.push_word(bus, self.pc); self.pc = 0x0018; 16 } // RST 18h: Restart saltando all'indirizzo 0x0018

            0xE0 => { // LDH [a8], A: Scrive A nell'area I/O veloce (0xFF00 + a8)
                let offset = self.fetch_byte(bus) as u16;
                bus.write_byte(0xFF00 + offset, self.a);
                12
            }
            0xE1 => { let val = self.pop_word(bus); self.set_hl(val); 12 } // POP HL: Estrae dallo stack in HL
            0xE2 => { // LD [C], A: Scrive A nell'area I/O veloce (0xFF00 + C)
                bus.write_byte(0xFF00 + self.c as u16, self.a);
                8
            }
            0xE5 => { self.push_word(bus, self.hl()); 16 } // PUSH HL: Inserisce HL nello stack
            0xE6 => { let val = self.fetch_byte(bus); self.alu_and(val); 8 } // AND d8: AND logico tra A e valore immediato a 8 bit
            0xE7 => { self.push_word(bus, self.pc); self.pc = 0x0020; 16 } // RST 20h: Restart saltando all'indirizzo 0x0020
            0xE8 => { // ADD SP, r8: Somma offset relativo con segno allo Stack Pointer
                let e = self.fetch_byte(bus) as i8;
                self.set_flag_z(false);
                self.set_flag_n(false);
                self.set_flag_h(((self.sp & 0x0F) + (e as u8 as u16 & 0x0F)) > 0x0F);
                self.set_flag_c(((self.sp & 0xFF) + (e as u8 as u16)) > 0xFF);
                self.sp = self.sp.wrapping_add(e as i16 as u16);
                16
            }
            0xE9 => { self.pc = self.hl(); 4 } // JP HL: Salta all'indirizzo contenuto nel registro HL
            0xEA => { // LD [a16], A: Scrive A in memoria all'indirizzo a 16 bit
                let addr = self.fetch_word(bus);
                bus.write_byte(addr, self.a);
                16
            }
            0xEE => { let val = self.fetch_byte(bus); self.alu_xor(val); 8 } // XOR d8: XOR logico tra A e valore immediato a 8 bit
            0xEF => { self.push_word(bus, self.pc); self.pc = 0x0028; 16 } // RST 28h: Restart saltando all'indirizzo 0x0028

            0xF0 => { // LDH A, [a8]: Legge dall'area I/O veloce (0xFF00 + a8) in A
                let offset = self.fetch_byte(bus) as u16;
                self.a = bus.read_byte(0xFF00 + offset);
                12
            }
            0xF1 => { let val = self.pop_word(bus); self.set_af(val); 12 } // POP AF: Estrae dallo stack in AF (bit bassi di F azzerati)
            0xF2 => { // LD A, [C]: Legge dall'area I/O veloce (0xFF00 + C) in A
                self.a = bus.read_byte(0xFF00 + self.c as u16);
                8
            }
            0xF3 => { self.ime = false; self.ime_scheduled = 0; 4 } // DI: Disabilita globalmente gli interrupt subito
            0xF5 => { self.push_word(bus, self.af()); 16 } // PUSH AF: Inserisce AF nello stack
            0xF6 => { let val = self.fetch_byte(bus); self.alu_or(val); 8 } // OR d8: OR logico tra A e valore immediato a 8 bit
            0xF7 => { self.push_word(bus, self.pc); self.pc = 0x0030; 16 } // RST 30h: Restart saltando all'indirizzo 0x0030
            0xF8 => { // LD HL, SP+r8: Calcola SP + offset relativo con segno e carica in HL
                let e = self.fetch_byte(bus) as i8;
                self.set_flag_z(false);
                self.set_flag_n(false);
                self.set_flag_h(((self.sp & 0x0F) + (e as u8 as u16 & 0x0F)) > 0x0F);
                self.set_flag_c(((self.sp & 0xFF) + (e as u8 as u16)) > 0xFF);
                self.set_hl(self.sp.wrapping_add(e as i16 as u16));
                12
            }
            0xF9 => { self.sp = self.hl(); 8 } // LD SP, HL: Carica il valore di HL nello Stack Pointer
            0xFA => { // LD A, [a16]: Legge dalla memoria all'indirizzo a 16 bit in A
                let addr = self.fetch_word(bus);
                self.a = bus.read_byte(addr);
                16
            }
            0xFB => { self.ime_scheduled = 2; 4 } // EI: Abilita gli interrupt con ritardo di un'istruzione
            0xFE => { let val = self.fetch_byte(bus); self.alu_cp(val); 8 } // CP d8: Confronta A con valore immediato a 8 bit
            0xFF => { self.push_word(bus, self.pc); self.pc = 0x0038; 16 } // RST 38h: Restart saltando all'indirizzo 0x0038

            // Opcodes illegali: istruzioni non presenti nell'ISA dello Sharp SM83 (DMG)
            0xD3 | 0xDB | 0xDD | 0xE3 | 0xE4 | 0xEB | 0xEC | 0xED | 0xF4 | 0xFC | 0xFD => {
                panic!("Opcode DMG non valido: {:#04X} a PC: {:#06X}", opcode, self.pc.wrapping_sub(1));
            }
        };

        if self.ime_scheduled > 0 {
            self.ime_scheduled -= 1;
            if self.ime_scheduled == 0 {
                self.ime = true;
            }
        }

        cycles
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

    fn make_test_bus() -> Bus {
        use crate::cartridge::Cartridge;
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
    fn test_add_hl() {
        let mut cpu = Cpu::new();
        cpu.set_flag_z(true); // Z flag should be preserved
        cpu.set_hl(0x0FFF);
        cpu.add_hl(0x0001);

        assert_eq!(cpu.hl(), 0x1000);
        assert!(cpu.flag_z()); // Preserved
        assert!(!cpu.flag_n());
        assert!(cpu.flag_h()); // Half-carry from bit 11 to 12
        assert!(!cpu.flag_c());

        // Test carry overflow
        cpu.set_hl(0xFFFF);
        cpu.add_hl(0x0001);
        assert_eq!(cpu.hl(), 0x0000);
        assert!(cpu.flag_c());
    }

    #[test]
    fn test_daa_addition_and_subtraction() {
        let mut cpu = Cpu::new();

        // 45 + 38 = 83 in BCD
        cpu.a = 0x45;
        cpu.alu_add(0x38); // 0x45 + 0x38 = 0x7D (binary)
        cpu.daa();
        assert_eq!(cpu.a, 0x83);
        assert!(!cpu.flag_c());
        assert!(!cpu.flag_z());

        // 83 - 38 = 45 in BCD
        cpu.a = 0x83;
        cpu.alu_sub(0x38); // 0x83 - 0x38 = 0x4B (binary)
        cpu.daa();
        assert_eq!(cpu.a, 0x45);
        assert!(!cpu.flag_c());
        assert!(!cpu.flag_z());
    }

    #[test]
    fn test_execute_cb() {
        let mut bus = make_test_bus();
        let mut cpu = Cpu::new();

        // Test SWAP A (0xCB 0x37)
        bus.write_byte(0xC000, 0xCB);
        bus.write_byte(0xC001, 0x37);
        cpu.pc = 0xC000;
        cpu.a = 0x3A;
        let cycles = cpu.step(&mut bus);
        assert_eq!(cycles, 8);
        assert_eq!(cpu.a, 0xA3);

        // Test BIT 3, B (0xCB 0x58)
        bus.write_byte(0xC002, 0xCB);
        bus.write_byte(0xC003, 0x58);
        cpu.pc = 0xC002;
        cpu.b = 0x08; // bit 3 is 1
        let cycles = cpu.step(&mut bus);
        assert_eq!(cycles, 8);
        assert!(!cpu.flag_z()); // bit is 1, so Z is false
        assert!(cpu.flag_h());

        // Test SET 7, C (0xCB 0xF9)
        bus.write_byte(0xC004, 0xCB);
        bus.write_byte(0xC005, 0xF9);
        cpu.pc = 0xC004;
        cpu.c = 0x00;
        let cycles = cpu.step(&mut bus);
        assert_eq!(cycles, 8);
        assert_eq!(cpu.c, 0x80);

        // Test RES 7, C (0xCB 0xB9)
        bus.write_byte(0xC006, 0xCB);
        bus.write_byte(0xC007, 0xB9);
        cpu.pc = 0xC006;
        cpu.c = 0x80;
        let cycles = cpu.step(&mut bus);
        assert_eq!(cycles, 8);
        assert_eq!(cpu.c, 0x00);
    }

    #[test]
    fn test_step_loads_and_stack() {
        let mut bus = make_test_bus();
        let mut cpu = Cpu::new();

        // 1. LD BC, 0x1234 (0x01, 0x34, 0x12)
        bus.write_byte(0xC000, 0x01);
        bus.write_byte(0xC001, 0x34);
        bus.write_byte(0xC002, 0x12);

        // 2. PUSH BC (0xC5)
        bus.write_byte(0xC003, 0xC5);

        // 3. POP DE (0xD1)
        bus.write_byte(0xC004, 0xD1);

        cpu.pc = 0xC000;
        cpu.sp = 0xFFFE;

        let c1 = cpu.step(&mut bus);
        assert_eq!(c1, 12);
        assert_eq!(cpu.bc(), 0x1234);

        let c2 = cpu.step(&mut bus);
        assert_eq!(c2, 16);
        assert_eq!(cpu.sp, 0xFFFC);

        let c3 = cpu.step(&mut bus);
        assert_eq!(c3, 12);
        assert_eq!(cpu.de(), 0x1234);
        assert_eq!(cpu.sp, 0xFFFE);
    }

    #[test]
    fn test_interrupt_service() {
        let mut bus = make_test_bus();
        let mut cpu = Cpu::new();

        cpu.pc = 0x0100;
        cpu.ime = true;
        bus.ie = 0x01; // Enable VBlank interrupt
        bus.if_flag = 0x01; // Request VBlank interrupt

        let cycles = cpu.step(&mut bus);
        assert_eq!(cycles, 20); // 5 M-cycles (20 T-cycles)
        assert_eq!(cpu.pc, 0x0040); // Jumped to VBlank vector
        assert!(!cpu.ime); // IME reset to false
        assert_eq!(bus.if_flag & 0x01, 0); // VBlank flag cleared in IF
        assert_eq!(cpu.sp, 0xFFFC); // Old PC (0x0100) pushed to stack
    }

    #[test]
    fn test_halt_bug() {
        let mut bus = make_test_bus();
        let mut cpu = Cpu::new();

        cpu.ime = false; // IME is disabled
        bus.ie = 0x01;   // VBlank enabled in IE
        bus.if_flag = 0x01; // VBlank pending in IF

        // Write HALT (0x76) followed by NOP (0x00)
        bus.write_byte(0xC000, 0x76);
        bus.write_byte(0xC001, 0x00);
        cpu.pc = 0xC000;

        cpu.step(&mut bus); // Executes HALT with pending interrupt and IME=false
        assert!(cpu.halt_bug);
        assert!(!cpu.halted); // Does NOT enter halt state
        assert_eq!(cpu.pc, 0xC001);

        // Next fetch should trigger the bug: read byte at 0xC001 without incrementing PC
        let byte = cpu.fetch_byte(&bus);
        assert_eq!(byte, 0x00);
        assert_eq!(cpu.pc, 0xC001); // PC was NOT incremented!
        assert!(!cpu.halt_bug);

        // Next fetch should increment PC normally
        let byte2 = cpu.fetch_byte(&bus);
        assert_eq!(byte2, 0x00);
        assert_eq!(cpu.pc, 0xC002);
    }

    #[test]
    fn test_ei_delay() {
        let mut bus = make_test_bus();
        let mut cpu = Cpu::new();

        cpu.ime = false;
        // 0xC000: EI (0xFB)
        // 0xC001: NOP (0x00)
        // 0xC002: NOP (0x00)
        bus.write_byte(0xC000, 0xFB);
        bus.write_byte(0xC001, 0x00);
        bus.write_byte(0xC002, 0x00);
        cpu.pc = 0xC000;

        // Step 1: EI executed -> ime should still be false
        cpu.step(&mut bus);
        assert!(!cpu.ime);
        assert_eq!(cpu.ime_scheduled, 1);

        // Step 2: NOP executed -> at the end of this instruction, ime becomes true
        cpu.step(&mut bus);
        assert!(cpu.ime);
        assert_eq!(cpu.ime_scheduled, 0);
    }

    #[test]
    fn test_conditional_branch_timing() {
        let mut bus = make_test_bus();
        let mut cpu = Cpu::new();

        // 0xC000: JR NZ, +2 (0x20, 0x02)
        bus.write_byte(0xC000, 0x20);
        bus.write_byte(0xC001, 0x02);

        // Condition TAKEN (Z = false)
        cpu.pc = 0xC000;
        cpu.set_flag_z(false);
        let cycles_taken = cpu.step(&mut bus);
        assert_eq!(cycles_taken, 12);
        assert_eq!(cpu.pc, 0xC004); // 0xC002 + 2

        // Condition NOT TAKEN (Z = true)
        cpu.pc = 0xC000;
        cpu.set_flag_z(true);
        let cycles_not_taken = cpu.step(&mut bus);
        assert_eq!(cycles_not_taken, 8);
        assert_eq!(cpu.pc, 0xC002); // 0xC000 + 2 (just fetched opcode + offset)
    }

    #[test]
    fn test_add_sp_e_flags() {
        let mut bus = make_test_bus();
        let mut cpu = Cpu::new();

        // 0xC000: ADD SP, 1 (0xE8, 0x01)
        bus.write_byte(0xC000, 0xE8);
        bus.write_byte(0xC001, 0x01);

        cpu.pc = 0xC000;
        cpu.sp = 0x000F;
        cpu.step(&mut bus);

        assert_eq!(cpu.sp, 0x0010);
        assert!(!cpu.flag_z()); // Always 0
        assert!(!cpu.flag_n()); // Always 0
        assert!(cpu.flag_h());  // 0x0F + 0x01 > 0x0F
        assert!(!cpu.flag_c()); // 0x0F + 0x01 <= 0xFF
    }
}