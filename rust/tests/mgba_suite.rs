//! Scores the MIT-licensed mGBA test suite vendored under `tests/fixtures/mgba-suite`.
//!
//! The ROM shows a menu of suites and has no run-all mode. For each suite this harness boots a
//! fresh core with the opt-in mGBA debug-log device on, presses DOWN to the suite and A to run
//! it, and waits for the `END: <passed>/<total>` line the suite logs through that device.
//!
//! The pass counts are a ratchet, not an all-or-nothing check: a suite fails when its count
//! drops below `baseline`, and prints a reminder when it rises. Several suites compare against
//! values the real BIOS or exact bus timing produces, so the HLE core is not expected to
//! pass them all (see `doc/rust-core/testing-strategy.md`). Run with `--nocapture` to see the
//! counts.

use pecten::GbaCore;

const SUITE_ROM: &[u8] = include_bytes!("fixtures/mgba-suite/suite.gba");

const KEY_A: u16 = 1 << 0;
const KEY_DOWN: u16 = 1 << 7;

/// Frames the ROM needs to draw its menu and finish startup before it reads the keys.
const STARTUP_FRAMES: usize = 10;
/// A key must be released between presses: the ROM acts on newly pressed keys only.
const RELEASE_FRAMES: usize = 3;

struct Suite {
    /// The name the suite logs in its `BEGIN:` line, in menu order.
    name: &'static str,
    /// Number of results the suite reports, which is fixed by the pinned ROM. (BIOS math's
    /// 615 is 123 tests times five registers, counted in the source: the core stops with an
    /// error before the suite logs its total.)
    total: u32,
    /// Lowest pass count the core is allowed to score.
    baseline: u32,
    /// Frames to wait for the `END:` line after pressing A.
    budget: usize,
}

/// In the order of the ROM's menu. The menu's last entry, "Video tests", is not listed: it has
/// no automatic run, logs no `BEGIN:`/`END:` lines, and reports no counts. Its screens can only
/// be judged by eye.
const SUITES: [Suite; 13] = [
    Suite {
        name: "Memory tests",
        total: 1552,
        baseline: 1344,
        budget: 1000,
    },
    Suite {
        name: "I/O read tests",
        total: 130,
        baseline: 52,
        budget: 1000,
    },
    Suite {
        name: "Timing tests",
        total: 2020,
        baseline: 310,
        budget: 1000,
    },
    Suite {
        name: "Timer count-up tests",
        total: 936,
        baseline: 353,
        budget: 1000,
    },
    Suite {
        name: "Timer IRQ tests",
        total: 90,
        baseline: 0,
        budget: 1000,
    },
    Suite {
        name: "Shifter tests",
        total: 140,
        baseline: 140,
        budget: 1000,
    },
    Suite {
        name: "Carry tests",
        total: 93,
        baseline: 93,
        budget: 1000,
    },
    Suite {
        name: "Multiply long tests",
        total: 72,
        baseline: 52,
        budget: 1000,
    },
    Suite {
        name: "BIOS math tests",
        total: 615,
        baseline: 0,
        budget: 1000,
    },
    Suite {
        name: "DMA tests",
        total: 1244,
        baseline: 1092,
        budget: 1000,
    },
    Suite {
        name: "SIO register R/W tests",
        total: 90,
        baseline: 65,
        budget: 1000,
    },
    Suite {
        name: "SIO timing tests",
        total: 4,
        baseline: 0,
        budget: 1000,
    },
    Suite {
        name: "Misc. edge case tests",
        total: 12,
        baseline: 3,
        budget: 1000,
    },
];

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Finished {
        passed: u32,
        total: u32,
        frames: usize,
    },
    /// The core stopped with an error before the suite finished.
    CoreError { frame: usize, error: String },
    /// The suite did not log `END:` within its frame budget.
    OutOfFrames,
}

fn parse_end(text: &str) -> Option<(u32, u32)> {
    let (passed, total) = text.strip_prefix("END: ")?.split_once('/')?;
    Some((passed.trim().parse().ok()?, total.trim().parse().ok()?))
}

/// Boots the suite ROM and runs the suite at menu position `index`.
fn run_suite(index: usize, budget: usize) -> Outcome {
    let mut core = GbaCore::new();
    core.set_debug_log_enabled(true);
    core.load_rom(SUITE_ROM)
        .expect("suite.gba is a valid cartridge");

    // (keys held, frames to hold them) in order.
    let mut script = vec![(0, STARTUP_FRAMES)];
    for _ in 0..index {
        script.push((KEY_DOWN, 1));
        script.push((0, RELEASE_FRAMES));
    }
    script.push((KEY_A, 1));
    script.push((0, budget));

    let name = SUITES[index].name;
    let mut began = false;
    let mut frame = 0;
    for (keys, frames) in script {
        for _ in 0..frames {
            core.set_pressed_keys(keys);
            if let Err(error) = core.step_frame() {
                return Outcome::CoreError {
                    frame,
                    error: error.to_string(),
                };
            }
            frame += 1;
            for line in core.take_debug_log() {
                match line.text.strip_prefix("BEGIN: ") {
                    Some(began_name) => {
                        assert_eq!(
                            began_name, name,
                            "menu position {index} ran a different suite"
                        );
                        began = true;
                    }
                    None if began => {
                        if let Some((passed, total)) = parse_end(&line.text) {
                            return Outcome::Finished {
                                passed,
                                total,
                                frames: frame,
                            };
                        }
                    }
                    None => {}
                }
            }
        }
    }
    Outcome::OutOfFrames
}

fn check(index: usize) {
    let suite = &SUITES[index];
    let outcome = run_suite(index, suite.budget);
    println!("{}: {outcome:?} (baseline {})", suite.name, suite.baseline);

    let passed = match outcome {
        Outcome::Finished { passed, total, .. } => {
            assert_eq!(
                total, suite.total,
                "{}: the suite now reports {total} tests; the pinned ROM reports {}",
                suite.name, suite.total
            );
            passed
        }
        // A suite that cannot finish scores nothing; only a nonzero baseline makes it fail.
        Outcome::CoreError { .. } | Outcome::OutOfFrames => 0,
    };
    assert!(
        passed >= suite.baseline,
        "{}: {passed} passed, below the baseline of {} ({outcome:?})",
        suite.name,
        suite.baseline
    );
    if passed > suite.baseline {
        println!(
            "{}: {passed} passed, above the baseline of {}. Raise the baseline in rust/tests/mgba_suite.rs.",
            suite.name, suite.baseline
        );
    }
}

macro_rules! suite_tests {
    ($($test:ident = $index:expr),* $(,)?) => {
        $(
            #[test]
            fn $test() {
                check($index);
            }
        )*
    };
}

suite_tests! {
    memory = 0,
    io_read = 1,
    timing = 2,
    timers = 3,
    timer_irq = 4,
    shifter = 5,
    carry = 6,
    multiply_long = 7,
    bios_math = 8,
    dma = 9,
    sio_read = 10,
    sio_timing = 11,
    misc_edge_case = 12,
}

#[test]
fn end_lines_parse_into_counts() {
    assert_eq!(parse_end("END: 12/34"), Some((12, 34)));
    assert_eq!(parse_end("END: 0/1"), Some((0, 1)));
    assert_eq!(parse_end("BEGIN: Memory"), None);
    assert_eq!(parse_end("END: x/1"), None);
}
