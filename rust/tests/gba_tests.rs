//! Runs the MIT-licensed jsmolka/gba-tests ROMs vendored under `tests/fixtures/gba-tests`.
//!
//! Each ROM ends in an ARM `B .` idle loop with the number of the first failed
//! test in its result register (0 when every test passed): r12 for the ARM-state
//! suites and r7 for `thumb.gba`.

use gba_wasm::{GbaCore, SCREEN_WIDTH};

const IDLE_LOOP: u32 = 0xEAFF_FFFE;
const STEP_LIMIT: u32 = 2_000_000;
/// FNV-1a hash of the RGBA frame showing "All tests passed" in Mode 4.
const PASSED_SCREEN_HASH: u64 = 0xe5ab_36d2_fca9_6065;
const FRAMES_TO_RESULT_SCREEN: usize = 10;

fn run_until_idle(rom: &[u8], result_register: usize) -> Result<u32, String> {
    let mut core = GbaCore::new();
    core.load_rom(rom).map_err(|error| error.to_string())?;

    for _ in 0..STEP_LIMIT {
        let pc = core.cpu.registers[15];
        if !core.cpu.is_thumb() && core.memory.read_u32(pc) == IDLE_LOOP {
            return Ok(core.cpu.registers[result_register]);
        }
        core.cpu
            .step_with_cycles(&mut core.memory)
            .map_err(|error| error.to_string())?;
    }
    Err(format!(
        "did not reach the idle loop within {STEP_LIMIT} steps (pc=0x{:08X})",
        core.cpu.registers[15]
    ))
}

fn assert_passes(name: &str, rom: &[u8], result_register: usize) {
    match run_until_idle(rom, result_register) {
        Ok(0) => {}
        Ok(failed) => panic!("{name}: failed test {failed}"),
        Err(error) => panic!("{name}: {error}"),
    }
}

#[test]
fn jsmolka_arm_suite_passes() {
    assert_passes("arm.gba", include_bytes!("fixtures/gba-tests/arm.gba"), 12);
}

#[test]
fn jsmolka_thumb_suite_passes() {
    assert_passes(
        "thumb.gba",
        include_bytes!("fixtures/gba-tests/thumb.gba"),
        7,
    );
}

#[test]
fn jsmolka_memory_suite_passes() {
    assert_passes(
        "memory.gba",
        include_bytes!("fixtures/gba-tests/memory.gba"),
        12,
    );
}

fn frame_hash(frame: &[u8]) -> u64 {
    frame.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Boots through the same frame API the browser uses and checks the rendered result screen.
fn assert_renders_passed_screen(name: &str, rom: &[u8]) {
    let mut core = GbaCore::new();
    core.load_rom(rom).unwrap();
    let mut frame = Vec::new();
    for _ in 0..FRAMES_TO_RESULT_SCREEN {
        frame = core
            .step_frame()
            .unwrap_or_else(|error| panic!("{name}: {error}"));
    }

    // White background with black text confined to the 8-pixel line drawn at y = 76.
    let text_pixels: Vec<(usize, usize)> = frame
        .chunks_exact(4)
        .enumerate()
        .filter(|(_, pixel)| *pixel != [255, 255, 255, 255])
        .map(|(index, _)| (index % SCREEN_WIDTH, index / SCREEN_WIDTH))
        .collect();
    assert!(!text_pixels.is_empty(), "{name}: blank screen");
    assert!(
        text_pixels
            .iter()
            .all(|&(x, y)| (56..184).contains(&x) && (76..84).contains(&y)),
        "{name}: pixels outside the result text line"
    );
    assert_eq!(
        frame_hash(&frame),
        PASSED_SCREEN_HASH,
        "{name}: result screen differs"
    );
}

#[test]
fn jsmolka_suites_render_the_passed_screen_in_mode4() {
    assert_renders_passed_screen("arm.gba", include_bytes!("fixtures/gba-tests/arm.gba"));
    assert_renders_passed_screen("thumb.gba", include_bytes!("fixtures/gba-tests/thumb.gba"));
    assert_renders_passed_screen(
        "memory.gba",
        include_bytes!("fixtures/gba-tests/memory.gba"),
    );
}

/// Pausing is not calling `step_frame`: the core has no wall-clock input, so a run that is
/// paused and resumed must match an uninterrupted run exactly, frame for frame.
#[test]
fn paused_and_resumed_run_matches_an_uninterrupted_run() {
    let rom = include_bytes!("fixtures/gba-tests/arm.gba");
    let mut uninterrupted = GbaCore::new();
    uninterrupted.load_rom(rom).unwrap();
    let mut paused = GbaCore::new();
    paused.load_rom(rom).unwrap();

    let mut expected = Vec::new();
    for _ in 0..FRAMES_TO_RESULT_SCREEN {
        expected.push(uninterrupted.step_frame().unwrap());
    }
    let mut resumed = Vec::new();
    for frame in 0..FRAMES_TO_RESULT_SCREEN {
        if frame == 4 {
            // A paused browser keeps forwarding key releases but does not step.
            paused.set_pressed_keys(0);
        }
        resumed.push(paused.step_frame().unwrap());
    }

    assert_eq!(resumed, expected);
    assert_eq!(paused.cpu.registers, uninterrupted.cpu.registers);
    assert_eq!(
        paused.memory.vblanks_started(),
        FRAMES_TO_RESULT_SCREEN as u64
    );
}
