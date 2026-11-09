use super::*;

impl ThumbCpu {
    pub(super) fn set_nz(&mut self, result: u32) {
        self.cpsr = (self.cpsr & !(FLAG_N | FLAG_Z))
            | if result & FLAG_N != 0 { FLAG_N } else { 0 }
            | if result == 0 { FLAG_Z } else { 0 };
    }

    pub(super) fn set_add_flags(&mut self, left: u32, right: u32, result: u32, carry: bool) {
        self.set_nz(result);
        let overflow = (!(left ^ right) & (left ^ result) & FLAG_N) != 0;
        self.cpsr = (self.cpsr & !(FLAG_C | FLAG_V))
            | if carry { FLAG_C } else { 0 }
            | if overflow { FLAG_V } else { 0 };
    }

    pub(super) fn set_sub_flags(&mut self, left: u32, right: u32, result: u32) {
        self.set_nz(result);
        let carry = left >= right;
        let overflow = ((left ^ right) & (left ^ result) & FLAG_N) != 0;
        self.cpsr = (self.cpsr & !(FLAG_C | FLAG_V))
            | if carry { FLAG_C } else { 0 }
            | if overflow { FLAG_V } else { 0 };
    }

    pub(super) fn set_cv(&mut self, carry: bool, overflow: bool) {
        self.cpsr = (self.cpsr & !(FLAG_C | FLAG_V))
            | if carry { FLAG_C } else { 0 }
            | if overflow { FLAG_V } else { 0 };
    }

    pub(super) fn set_arithmetic_flags(
        &mut self,
        subtract: bool,
        left: u32,
        right: u32,
        adjustment: u32,
        result: u32,
    ) {
        self.set_nz(result);
        let (carry, overflow) = if subtract {
            let subtrahend = u64::from(right) + u64::from(adjustment);
            let signed_result =
                i64::from(left as i32) - i64::from(right as i32) - i64::from(adjustment);
            (
                u64::from(left) >= subtrahend,
                signed_result < i64::from(i32::MIN) || signed_result > i64::from(i32::MAX),
            )
        } else {
            let wide_result = u64::from(left) + u64::from(right) + u64::from(adjustment);
            let signed_result =
                i64::from(left as i32) + i64::from(right as i32) + i64::from(adjustment);
            (
                wide_result > u64::from(u32::MAX),
                signed_result < i64::from(i32::MIN) || signed_result > i64::from(i32::MAX),
            )
        };
        self.set_cv(carry, overflow);
    }

    pub(super) fn set_carry(&mut self, carry: bool) {
        self.cpsr = (self.cpsr & !FLAG_C) | if carry { FLAG_C } else { 0 };
    }
}

/// Internal multiply cycles (`m`) from the multiplier's significant bytes.
pub(super) fn multiply_array_cycles(multiplier: u32, signed: bool) -> u32 {
    let significant = |mask: u32| {
        let high = multiplier & mask;
        high == 0 || (signed && high == mask)
    };
    if significant(0xFFFF_FF00) {
        1
    } else if significant(0xFFFF_0000) {
        2
    } else if significant(0xFF00_0000) {
        3
    } else {
        4
    }
}

/// Word load; a misaligned address rotates the aligned word right by the byte offset.
pub(super) fn load_word(memory: &MemoryMap, address: u32) -> u32 {
    memory
        .read_u32(address & !3)
        .rotate_right((address & 3) * 8)
}

/// Halfword load; a misaligned address rotates the aligned halfword right by 8 (ARM7TDMI).
pub(super) fn load_halfword(memory: &MemoryMap, address: u32) -> u32 {
    u32::from(memory.read_u16(address & !1)).rotate_right((address & 1) * 8)
}

pub(super) fn load_signed_byte(memory: &MemoryMap, address: u32) -> u32 {
    i32::from(memory.read_u8(address) as i8) as u32
}

/// Signed halfword load; a misaligned address loads the sign-extended byte instead (ARM7TDMI).
pub(super) fn load_signed_halfword(memory: &MemoryMap, address: u32) -> u32 {
    if address & 1 != 0 {
        load_signed_byte(memory, address)
    } else {
        i32::from(memory.read_u16(address) as i16) as u32
    }
}

pub(super) fn sign_extend(value: u32, bits: u32) -> u32 {
    ((value << (32 - bits)) as i32 >> (32 - bits)) as u32
}

pub(super) fn shift_immediate(
    value: u32,
    shift_type: u32,
    amount: u32,
    carry_in: bool,
) -> (u32, bool) {
    match shift_type {
        0 if amount == 0 => (value, carry_in),
        0 => (value << amount, value & (1 << (32 - amount)) != 0),
        1 if amount == 0 => (0, value & FLAG_N != 0),
        1 => (value >> amount, value & (1 << (amount - 1)) != 0),
        2 if amount == 0 => {
            let result = if value & FLAG_N != 0 { u32::MAX } else { 0 };
            (result, value & FLAG_N != 0)
        }
        2 => {
            let result = ((value as i32) >> amount) as u32;
            (result, value & (1 << (amount - 1)) != 0)
        }
        3 if amount == 0 => {
            let result = (u32::from(carry_in) << 31) | (value >> 1);
            (result, value & 1 != 0)
        }
        3 => {
            let result = value.rotate_right(amount);
            (result, result & FLAG_N != 0)
        }
        _ => unreachable!(),
    }
}

pub(super) fn shift_register(
    value: u32,
    amount_register: u32,
    shift_type: u32,
    carry_in: bool,
) -> (u32, bool) {
    let amount = amount_register & 0xFF;
    if amount == 0 {
        return (value, carry_in);
    }

    match shift_type {
        0 if amount < 32 => (value << amount, value & (1 << (32 - amount)) != 0),
        0 if amount == 32 => (0, value & 1 != 0),
        0 => (0, false),
        1 if amount < 32 => (value >> amount, value & (1 << (amount - 1)) != 0),
        1 if amount == 32 => (0, value & FLAG_N != 0),
        1 => (0, false),
        2 if amount < 32 => {
            let result = ((value as i32) >> amount) as u32;
            (result, value & (1 << (amount - 1)) != 0)
        }
        2 => {
            let negative = value & FLAG_N != 0;
            (if negative { u32::MAX } else { 0 }, negative)
        }
        3 => {
            let rotation = amount & 0x1F;
            if rotation == 0 {
                (value, value & FLAG_N != 0)
            } else {
                let result = value.rotate_right(rotation);
                (result, result & FLAG_N != 0)
            }
        }
        _ => unreachable!(),
    }
}
