//! Generated HLE BIOS image mapped at `0x00000000`.
//!
//! SWI services are executed in Rust (see `ThumbCpu::execute_bios_call`), so the
//! image only carries the exception vectors and the IRQ dispatcher. The dispatcher
//! uses the same instruction sequence as the hardware BIOS: it saves the scratch
//! registers on the IRQ stack, calls the handler stored at `0x03007FFC`, and
//! returns with `SUBS pc, lr, #4`.

pub const BIOS_SIZE: usize = 16 * 1024;
pub const IRQ_VECTOR: u32 = 0x0000_0018;
const IRQ_DISPATCHER: u32 = 0x0000_0128;
const IDLE_LOOP: u32 = 0xEAFF_FFFE; // B .

const IRQ_DISPATCHER_CODE: [u32; 6] = [
    0xE92D_500F, // STMFD sp!, {r0-r3, r12, lr}
    0xE3A0_0301, // MOV r0, #0x04000000
    0xE28F_E000, // ADD lr, pc, #0
    0xE510_F004, // LDR pc, [r0, #-4]   (handler pointer mirrored at 0x03007FFC)
    0xE8BD_500F, // LDMFD sp!, {r0-r3, r12, lr}
    0xE25E_F004, // SUBS pc, lr, #4
];

pub fn hle_image() -> Vec<u8> {
    let mut image = vec![0; BIOS_SIZE];
    let mut write_word = |address: u32, value: u32| {
        let offset = address as usize;
        image[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    };

    // Unmodeled vectors (reset, undefined, SWI, aborts, FIQ) park the CPU in place.
    for vector in (0x00..0x20).step_by(4) {
        write_word(vector, IDLE_LOOP);
    }
    let branch_offset = (IRQ_DISPATCHER - IRQ_VECTOR - 8) >> 2;
    write_word(IRQ_VECTOR, 0xEA00_0000 | branch_offset);
    for (index, instruction) in IRQ_DISPATCHER_CODE.iter().enumerate() {
        write_word(IRQ_DISPATCHER + index as u32 * 4, *instruction);
    }
    image
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn irq_vector_branches_to_the_dispatcher() {
        let image = hle_image();
        let vector = u32::from_le_bytes(image[0x18..0x1C].try_into().unwrap());
        let target = 0x18 + 8 + ((vector & 0x00FF_FFFF) << 2);

        assert_eq!(vector >> 24, 0xEA);
        assert_eq!(target, IRQ_DISPATCHER);
        assert_eq!(
            u32::from_le_bytes(image[0x128..0x12C].try_into().unwrap()),
            IRQ_DISPATCHER_CODE[0]
        );
    }
}
