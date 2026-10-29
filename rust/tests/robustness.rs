//! Hostile-input checks for the browser-facing API.
//!
//! The release build uses `panic = "abort"`, so a panic in the core kills the WASM instance
//! and the page with it. Arbitrary cartridges, register writes, and save-state bytes must
//! only ever produce `Err` values. These tests drive the core with seeded pseudo-random input
//! (no external crates, fully reproducible) and report the seed of any panic.
//!
//! The default case counts keep `cargo test` fast. For a deeper search, multiply them:
//! `GBA_FUZZ_SCALE=50 RUSTFLAGS="-C overflow-checks=on" cargo test --release --test robustness`.

use gba_wasm::GbaCore;
use std::panic::{AssertUnwindSafe, catch_unwind};

const HEADER_END: usize = 0xC0;
const FRAMES_PER_CASE: usize = 2;

/// xorshift64*: small, deterministic, and good enough to spread bits across opcodes.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn next_u32(&mut self) -> u32 {
        (self.next_u64() >> 32) as u32
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next_u64() % bound as u64) as usize
    }
}

/// The valid header of a vendored MIT test ROM, followed by `body_size` random bytes. The
/// header's entry branch jumps to 0xC0, so execution starts in the random code.
fn random_cartridge(rng: &mut Rng, body_size: usize) -> Vec<u8> {
    let mut rom = include_bytes!("fixtures/gba-tests/arm.gba")[..HEADER_END].to_vec();
    rom.extend((0..body_size).map(|_| rng.next_u64() as u8));
    rom
}

/// Runs `case` for every seed and fails with the seeds that panicked.
fn assert_no_panics(cases: u64, case: impl Fn(&mut Rng)) {
    let scale: u64 = std::env::var("GBA_FUZZ_SCALE")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    let failures: Vec<u64> = (0..cases * scale)
        .filter(|&seed| catch_unwind(AssertUnwindSafe(|| case(&mut Rng::new(seed)))).is_err())
        .collect();
    assert!(failures.is_empty(), "panicked for seeds {failures:?}");
}

fn run_frames(core: &mut GbaCore, rng: &mut Rng) {
    for _ in 0..FRAMES_PER_CASE {
        core.set_pressed_keys(rng.next_u32() as u16 & 0x3FF);
        if core.step_frame().is_err() {
            return;
        }
        let _ = core.memory.apu.take_samples();
    }
}

#[test]
fn random_cartridge_code_never_panics() {
    assert_no_panics(48, |rng| {
        let mut core = GbaCore::new();
        let size = 0x400 << rng.below(6);
        core.load_rom(&random_cartridge(rng, size)).unwrap();
        run_frames(&mut core, rng);
    });
}

#[test]
fn random_io_and_memory_writes_never_panic() {
    assert_no_panics(24, |rng| {
        let mut core = GbaCore::new();
        // An ARM `B .` loop, so frames advance while devices run on the written state.
        let mut rom = random_cartridge(rng, 0);
        rom.extend_from_slice(&0xEAFF_FFFEu32.to_le_bytes());
        core.load_rom(&rom).unwrap();
        for _ in 0..4096 {
            // Mostly I/O registers, where device side effects live; the rest anywhere.
            let address = if rng.below(4) != 0 {
                0x0400_0000 | rng.below(0x400) as u32
            } else {
                rng.next_u32()
            };
            match rng.below(3) {
                0 => core.memory.write_u8(address, rng.next_u32() as u8),
                1 => core.memory.write_u16(address, rng.next_u32() as u16),
                _ => core.memory.write_u32(address, rng.next_u32()),
            }
            let _ = core.memory.read_u32(rng.next_u32());
        }
        run_frames(&mut core, rng);
    });
}

#[test]
fn corrupted_save_states_are_rejected_without_panicking() {
    let rom = include_bytes!("fixtures/gba-tests/arm.gba");
    let mut source = GbaCore::new();
    source.load_rom(rom).unwrap();
    for _ in 0..2 {
        source.step_frame().unwrap();
    }
    let state = source.save_state();

    assert_no_panics(64, |rng| {
        let mut core = GbaCore::new();
        core.load_rom(rom).unwrap();
        let mut corrupted = state.clone();
        match rng.below(3) {
            0 => corrupted.truncate(rng.below(state.len())),
            1 => {
                for _ in 0..1 + rng.below(16) {
                    let index = rng.below(corrupted.len());
                    corrupted[index] = rng.next_u64() as u8;
                }
            }
            _ => {
                // Overwrite a 4-byte window with an extreme value, the shape of a bad length
                // or index field.
                let index = rng.below(corrupted.len() - 4);
                let value = [0, u32::MAX, 0x8000_0000, rng.next_u32()][rng.below(4)];
                corrupted[index..index + 4].copy_from_slice(&value.to_le_bytes());
            }
        }
        // Accepted or not, the core must keep running afterwards.
        let _ = core.load_state(&corrupted);
        run_frames(&mut core, rng);
    });
}
