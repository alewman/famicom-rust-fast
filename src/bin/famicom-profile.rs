//! A sampling profiler for the board, since `perf` is not allowed here:
//! SIGPROF 1,000 times a second of CPU time, the interrupted instruction's
//! address recorded, then `scripts/profile.py` maps the addresses to source
//! lines (inlined frames included) with `addr2line`.
//!
//!     cargo run --release --features profile --bin famicom-profile -- ROM [--frames N] [--out FILE]

use famicom_rust::joypad::StandardController;
use famicom_rust::System;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

const CAPACITY: usize = 1 << 20;
static mut SAMPLES: [usize; CAPACITY] = [0; CAPACITY];
static COUNT: AtomicUsize = AtomicUsize::new(0);

extern "C" fn on_prof(_: libc::c_int, _: *mut libc::siginfo_t, context: *mut libc::c_void) {
    // SAFETY: the kernel passes a ucontext_t; only the RIP is read, and each
    // slot is written once, by this handler, on this thread.
    unsafe {
        let ucontext = &*(context as *const libc::ucontext_t);
        let rip = ucontext.uc_mcontext.gregs[libc::REG_RIP as usize] as usize;
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        if n < CAPACITY {
            let samples = &raw mut SAMPLES;
            (*samples)[n] = rip;
        }
    }
}

fn timer(usec: libc::suseconds_t) {
    let tick = libc::timeval {
        tv_sec: 0,
        tv_usec: usec,
    };
    let timer = libc::itimerval {
        it_interval: tick,
        it_value: tick,
    };
    // SAFETY: setting this process's profiling interval timer.
    unsafe {
        libc::setitimer(libc::ITIMER_PROF, &timer, std::ptr::null_mut());
    }
}

fn main() {
    let mut frames = 600u64;
    let mut out = "external/profile.txt".to_owned();
    let mut roms = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--frames" => frames = args.next().expect("N").parse().expect("a number"),
            "--out" => out = args.next().expect("FILE"),
            _ => roms.push(arg),
        }
    }
    assert!(
        !roms.is_empty(),
        "usage: famicom-profile ROM... [--frames N] [--out FILE]"
    );
    let mut systems: Vec<System> = roms
        .iter()
        .map(|rom| {
            let data = famicom_rust::cartridge::read_rom_file(
                Path::new(rom),
                famicom_rust::cartridge::ROM_SUFFIXES,
            )
            .unwrap_or_else(|e| panic!("{e}"));
            let machine = System::machine_for(&data).unwrap_or_else(|e| panic!("{e}"));
            System::from_rom(&data, machine, None).unwrap_or_else(|e| panic!("{e}"))
        })
        .collect();
    // SAFETY: installing a handler for this process's SIGPROF.
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = on_prof as *const () as usize;
        action.sa_flags = libc::SA_SIGINFO | libc::SA_RESTART;
        libc::sigaction(libc::SIGPROF, &action, std::ptr::null_mut());
    }
    timer(1000);
    for system in systems.iter_mut() {
        for frame in 0..frames {
            // Start now and then, as the benchmark presses it.
            let buttons = if frame % 120 < 6 && frame >= 120 {
                0x08
            } else {
                0
            };
            if let Some(pad) = system.board().pads[0]
                .as_any_mut()
                .downcast_mut::<StandardController>()
            {
                pad.buttons = buttons;
            }
            system.run_frames(1);
            system.take_samples();
        }
    }
    timer(0);
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
    let exe = std::fs::read_link("/proc/self/exe").unwrap();
    let base = maps
        .lines()
        .find(|line| line.ends_with(exe.to_str().unwrap()))
        .and_then(|line| usize::from_str_radix(line.split('-').next().unwrap(), 16).ok())
        .expect("the executable's mapping");
    let n = COUNT.load(Ordering::Relaxed).min(CAPACITY);
    let mut text = format!("{}\n", exe.display());
    let samples = &raw const SAMPLES;
    for i in 0..n {
        // SAFETY: the timer is stopped, nothing writes SAMPLES any more.
        let rip = unsafe { (*samples)[i] };
        text.push_str(&format!("{:x}\n", rip.wrapping_sub(base)));
    }
    std::fs::write(&out, text).unwrap();
    eprintln!("{n} samples to {out}");
}
