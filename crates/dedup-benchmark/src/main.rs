//! Rust port of the Swift executable `DedupBenchmark`: synthetic, deterministic resolver timing.
//! No real calendar data is read. This is a benchmark, not Core, so it may read the clock.
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::process::ExitCode;
use std::time::Instant;

use calendar_core::model::CalendarEvent;
use calendar_core::time::add_seconds;
use jiff::Timestamp;
use timetug_core::TimeTugCalendarEvent;
use timetug_core::dedup::lesson_book::LessonBook;
use timetug_core::dedup::resolver::DuplicateResolver;

/// The same linear congruential generator as the Swift benchmark.
struct Generator {
    state: u64,
}

impl Generator {
    fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }
}

fn events(count: usize, shape: &str) -> Vec<TimeTugCalendarEvent> {
    let mut random = Generator { state: 0x5eed };
    let origin = Timestamp::from_second(1_800_000_000).unwrap_or(Timestamp::UNIX_EPOCH);
    (0..count)
        .map(|index| {
            let (slot, title) = match shape {
                "sparse" => (index * 60, format!("Unique {index}")),
                "dense" => (
                    usize::try_from(random.next() % 60).unwrap_or(0),
                    format!("Meeting {index}"),
                ),
                _ => (index / 4, format!("Duplicate {}", index / 4)),
            };
            let start = add_seconds(origin, (slot * 60) as f64);
            let event = CalendarEvent::new(
                format!("e{index}"),
                format!("c{}", index % 4),
                title,
                start,
                add_seconds(start, 1800.0),
            );
            TimeTugCalendarEvent::new(event, "synthetic")
        })
        .collect()
}

const SHAPES: [&str; 3] = ["sparse", "dense", "duplicates"];
const COUNTS: [usize; 4] = [100, 500, 1_000, 5_000];
const USAGE: &str = "Usage: dedup-benchmark [sparse|dense|duplicates [100|500|1000|5000]]";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let requested_count = if arguments.len() == 2 {
        arguments[1].parse::<usize>().ok()
    } else {
        None
    };
    let valid = arguments.len() <= 2
        && (arguments.is_empty() || SHAPES.contains(&arguments[0].as_str()))
        && (arguments.len() < 2 || requested_count.is_some_and(|c| COUNTS.contains(&c)));
    if !valid {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    }
    let repetitions = 5;
    let lessons = LessonBook::default();
    println!("shape,count,median_ms,output_events");
    for shape in SHAPES {
        if !arguments.is_empty() && arguments[0] != shape {
            continue;
        }
        for count in COUNTS {
            if requested_count.is_some_and(|r| r != count) {
                continue;
            }
            let input = events(count, shape);
            let mut times: Vec<f64> = Vec::new();
            let mut output_count = 0;
            for repetition in 0..=repetitions {
                let start = Instant::now();
                let output = DuplicateResolver::resolve(&input, &[], &lessons, None);
                let elapsed = start.elapsed();
                output_count = output.events.len();
                if repetition > 0 {
                    times.push(elapsed.as_secs_f64() * 1000.0);
                }
            }
            times.sort_by(f64::total_cmp);
            println!(
                "{shape},{count},{:.3},{output_count}",
                times[times.len() / 2]
            );
        }
    }
    ExitCode::SUCCESS
}
