use super::*;

pub(super) fn program(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

pub(super) fn arm_word(opcode: u32) -> Vec<u8> {
    opcode.to_le_bytes().to_vec()
}

/// ARM IRQ handler installed at 0x03007FFC: acknowledges VBlank in `IF` and reports it
/// to the BIOS through the interrupt flags at 0x03007FF8, as games do.
pub(super) const VBLANK_HANDLER: [u32; 8] = [
    0xE3A0_3301, // MOV r3, #0x04000000
    0xE283_3C02, // ADD r3, r3, #0x200
    0xE3A0_2001, // MOV r2, #1
    0xE1C3_20B2, // STRH r2, [r3, #2]     (IF: acknowledge VBlank)
    0xE3A0_3403, // MOV r3, #0x03000000
    0xE283_3C7F, // ADD r3, r3, #0x7F00
    0xE1C3_2FB8, // STRH r2, [r3, #0xF8]  (BIOS interrupt flags)
    0xE12F_FF1E, // BX lr
];

pub(super) fn install_vblank_handler(memory: &mut MemoryMap, rom: &mut Vec<u8>, offset: usize) {
    rom.resize(offset, 0);
    for word in VBLANK_HANDLER {
        rom.extend(word.to_le_bytes());
    }
    memory.write_u32(0x0300_7FFC, ROM_BASE + offset as u32);
}
