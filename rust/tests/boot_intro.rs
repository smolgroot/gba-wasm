//! The HLE BIOS boot animation, run in front of the jsmolka/gba-tests `arm.gba` ROM.

use gba_wasm::GbaCore;

const ROM: &[u8] = include_bytes!("fixtures/gba-tests/arm.gba");
const INTRO_FRAMES: usize = 116;
const GAME_FRAMES: usize = 30;

fn core(boot_intro: bool) -> GbaCore {
    let mut core = GbaCore::new();
    core.load_rom(ROM).unwrap();
    if boot_intro {
        core.start_boot_intro();
    }
    core
}

fn pixel(frame: &[u8], x: usize, y: usize) -> [u8; 3] {
    let offset = (y * 240 + x) * 4;
    [frame[offset], frame[offset + 1], frame[offset + 2]]
}

#[test]
fn intro_draws_the_logo_and_wordmark_then_fades_to_black() {
    let mut core = core(true);
    let frames: Vec<Vec<u8>> = (0..INTRO_FRAMES)
        .map(|_| core.step_frame().unwrap())
        .collect();

    assert!(
        frames[0].chunks(4).all(|rgba| rgba[..3] == [0, 0, 0]),
        "starts black"
    );
    let hold = &frames[80];
    let [red, green, blue] = pixel(hold, 120, 66);
    assert!(
        red > green && blue > green,
        "logo center is pink, got {red},{green},{blue}"
    );
    let wordmark_lit = |frame: &[u8]| {
        (112..126).any(|y| (70..170).any(|x| pixel(frame, x, y).iter().all(|&c| c > 200)))
    };
    assert!(
        !wordmark_lit(&frames[30]),
        "wordmark is typed after the zoom"
    );
    assert!(wordmark_lit(hold));
    assert!(
        frames[INTRO_FRAMES - 1]
            .chunks(4)
            .all(|rgba| rgba[..3] == [0, 0, 0]),
        "ends black"
    );
    assert!(core.boot_intro_active());
}

#[test]
fn game_runs_after_the_intro_exactly_as_after_a_direct_boot() {
    let mut with_intro = core(true);
    let mut direct = core(false);
    for _ in 0..INTRO_FRAMES {
        with_intro.step_frame().unwrap();
    }

    for frame in 0..GAME_FRAMES {
        assert_eq!(
            with_intro.step_frame().unwrap(),
            direct.step_frame().unwrap(),
            "frame {frame}"
        );
    }
    assert!(!with_intro.boot_intro_active());
    assert_eq!(with_intro.save_state(), direct.save_state());
}

#[test]
fn a_key_press_skips_the_rest_of_the_intro() {
    let mut core = core(true);
    for _ in 0..10 {
        core.step_frame().unwrap();
    }
    core.set_pressed_keys(1 << 3); // Start
    core.step_frame().unwrap();
    assert!(!core.boot_intro_active());
}

#[test]
fn a_state_saved_during_the_intro_is_the_game_start_state() {
    let mut intro = core(true);
    for _ in 0..50 {
        intro.step_frame().unwrap();
    }
    let state = intro.save_state();
    assert_eq!(state, core(false).save_state());

    let mut restored = core(true);
    restored.load_state(&state).unwrap();
    assert!(!restored.boot_intro_active());
}
