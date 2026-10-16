use super::i256::i256_cmp;
use crate::{
    interpreter::{
        Word,
        stack::{write_u64, write_word},
    },
    utils::word_to_usize_saturated,
};
use core::cmp::Ordering;
use evm2_macros::instruction;

#[instruction]
pub fn lt([a, b]: [Word]) -> out {
    write_u64(out, (a < b) as u64);
}

#[instruction]
pub fn gt([a, b]: [Word]) -> out {
    write_u64(out, (a > b) as u64);
}

#[instruction]
pub fn slt([a, b]: [Word]) -> out {
    write_u64(out, (i256_cmp(a, b) == Ordering::Less) as u64);
}

#[instruction]
pub fn sgt([a, b]: [Word]) -> out {
    write_u64(out, (i256_cmp(a, b) == Ordering::Greater) as u64);
}

#[instruction]
pub fn eq([a, b]: [Word]) -> out {
    write_u64(out, (a == b) as u64);
}

#[instruction]
pub fn iszero([value]: [Word]) -> out {
    write_u64(out, value.is_zero() as u64);
}

#[instruction]
pub fn bitand([a, b]: [Word]) -> out {
    *out = *a & *b;
}

#[instruction]
pub fn bitor([a, b]: [Word]) -> out {
    *out = *a | *b;
}

#[instruction]
pub fn bitxor([a, b]: [Word]) -> out {
    *out = *a ^ *b;
}

#[instruction]
pub fn not([value]: [Word]) -> out {
    *out = !*value;
}

#[instruction]
pub fn byte([index, value]: [Word]) -> out {
    let index = word_to_usize_saturated(*index);
    *out = if index < 32 { Word::from(value.byte(31 - index)) } else { Word::ZERO };
}

#[instruction]
pub fn shl([shift, value]: [Word]) -> out {
    let shift = word_to_usize_saturated(*shift);
    write_word(out, if shift < 256 { shl_word(value, shift) } else { Word::ZERO });
}

#[instruction]
pub fn shr([shift, value]: [Word]) -> out {
    let shift = word_to_usize_saturated(*shift);
    write_word(out, if shift < 256 { shr_word(value, shift) } else { Word::ZERO });
}

#[instruction]
pub fn sar([shift, value]: [Word]) -> out {
    let shift = word_to_usize_saturated(*shift);
    let value = if shift < 256 {
        sar_word(value, shift)
    } else if value.bit(255) {
        Word::MAX
    } else {
        Word::ZERO
    };
    write_word(out, value);
}

#[instruction]
pub fn clz([value]: [Word]) -> out {
    *out = Word::from(value.leading_zeros());
}

// ruint's shifts index a limb array by a runtime offset, which keeps the result in a stack buffer
// that is then read back with one wide load. These shift each limb with its carry and select whole
// limbs in registers instead.

/// Shifts `value` left by `shift < 256` bits.
#[inline]
fn shl_word(value: &Word, shift: usize) -> Word {
    let x = value.as_limbs();
    let bits = shift % 64;
    // `lo >> (64 - bits)`, or zero when `bits` is zero.
    let carry = |lo: u64| (lo >> 1) >> (63 - bits);
    let t = [
        x[0] << bits,
        (x[1] << bits) | carry(x[0]),
        (x[2] << bits) | carry(x[1]),
        (x[3] << bits) | carry(x[2]),
    ];
    Word::from_limbs(match shift / 64 {
        0 => t,
        1 => [0, t[0], t[1], t[2]],
        2 => [0, 0, t[0], t[1]],
        _ => [0, 0, 0, t[0]],
    })
}

/// Shifts `x` right by `shift < 256` bits, given its shifted top limb and the vacated limb value.
#[inline]
fn shr_word_with(x: &[u64; 4], top: u64, fill: u64, shift: usize) -> Word {
    let bits = shift % 64;
    // `hi << (64 - bits)`, or zero when `bits` is zero.
    let carry = |hi: u64| (hi << 1) << (63 - bits);
    let t = [
        (x[0] >> bits) | carry(x[1]),
        (x[1] >> bits) | carry(x[2]),
        (x[2] >> bits) | carry(x[3]),
        top,
    ];
    Word::from_limbs(match shift / 64 {
        0 => t,
        1 => [t[1], t[2], t[3], fill],
        2 => [t[2], t[3], fill, fill],
        _ => [t[3], fill, fill, fill],
    })
}

/// Shifts `value` right by `shift < 256` bits.
#[inline]
fn shr_word(value: &Word, shift: usize) -> Word {
    let x = value.as_limbs();
    shr_word_with(x, x[3] >> (shift % 64), 0, shift)
}

/// Shifts `value` right by `shift < 256` bits, extending the sign bit.
#[inline]
fn sar_word(value: &Word, shift: usize) -> Word {
    let x = value.as_limbs();
    let top = ((x[3] as i64) >> (shift % 64)) as u64;
    shr_word_with(x, top, ((x[3] as i64) >> 63) as u64, shift)
}

#[cfg(test)]
mod tests {
    use crate::{
        interpreter::Word,
        test_utils::{assert_stack, neg},
    };

    #[test]
    fn lt_opcode() {
        assert_stack!(LT(1, 2), 1);
        assert_stack!(LT(2, 1), 0);
        assert_stack!(LT(2, 2), 0);
    }

    #[test]
    fn gt_opcode() {
        assert_stack!(GT(2, 1), 1);
        assert_stack!(GT(1, 2), 0);
        assert_stack!(GT(2, 2), 0);
    }

    #[test]
    fn slt_opcode() {
        assert_stack!(SLT(neg(1), 0), 1);
        assert_stack!(SLT(0, neg(1)), 0);
        assert_stack!(SLT(neg(2), neg(1)), 1);
    }

    #[test]
    fn sgt_opcode() {
        assert_stack!(SGT(0, neg(1)), 1);
        assert_stack!(SGT(neg(1), 0), 0);
        assert_stack!(SGT(neg(1), neg(2)), 1);
    }

    #[test]
    fn eq_opcode() {
        assert_stack!(EQ(3, 3), 1);
        assert_stack!(EQ(3, 4), 0);
        assert_stack!(EQ(neg(1), neg(1)), 1);
    }

    #[test]
    fn iszero_opcode() {
        assert_stack!(ISZERO(0), 1);
        assert_stack!(ISZERO(1), 0);
        assert_stack!(ISZERO(neg(1)), 0);
    }

    #[test]
    fn and_opcode() {
        assert_stack!(AND(0b1100, 0b1010), 0b1000);
        assert_stack!(AND(neg(1), 0x55), 0x55);
        assert_stack!(AND(0, neg(1)), 0);
    }

    #[test]
    fn or_opcode() {
        assert_stack!(OR(0b1100, 0b1010), 0b1110);
        assert_stack!(OR(0, 0x55), 0x55);
        assert_stack!(OR(neg(1), 0), neg(1));
    }

    #[test]
    fn xor_opcode() {
        assert_stack!(XOR(0b1100, 0b1010), 0b0110);
        assert_stack!(XOR(neg(1), neg(1)), 0);
        assert_stack!(XOR(0, 0x55), 0x55);
    }

    #[test]
    fn not_opcode() {
        assert_stack!(NOT(0), neg(1));
        assert_stack!(NOT(neg(1)), 0);
        assert_stack!(NOT(0xff), neg(1) - Word::from(0xff));
    }

    #[test]
    fn byte_opcode() {
        assert_stack!(BYTE(31, 0x1234), 0x34);
        assert_stack!(BYTE(30, 0x1234), 0x12);
        assert_stack!(BYTE(32, 0x1234), 0);
    }

    #[test]
    fn shl_opcode() {
        assert_stack!(SHL(8, 1), 256);
        assert_stack!(SHL(0, 7), 7);
        assert_stack!(SHL(256, 1), 0);
        assert_stack!(SHL(65, 0x8000_0000_0000_0001u64), Word::from_limbs([0, 2, 1, 0]));
    }

    #[test]
    fn shr_opcode() {
        assert_stack!(SHR(8, 256), 1);
        assert_stack!(SHR(0, 7), 7);
        assert_stack!(SHR(256, neg(1)), 0);
        assert_stack!(SHR(65, Word::from_limbs([0, 2, 1, 0])), 0x8000_0000_0000_0001u64);
    }

    #[test]
    fn sar_opcode() {
        assert_stack!(SAR(1, neg(1) - Word::from(1)), neg(1));
        assert_stack!(SAR(1, 4), 2);
        assert_stack!(SAR(256, neg(1)), neg(1));
        assert_stack!(
            SAR(129, Word::from_limbs([0, 0, 0, 1 << 63])),
            Word::from_limbs([0, 0xc000_0000_0000_0000, u64::MAX, u64::MAX])
        );
    }

    #[test]
    fn clz_opcode() {
        assert_stack!(CLZ(1), 255);
        assert_stack!(CLZ(0), 256);
        assert_stack!(CLZ(neg(1)), 0);
    }
}
