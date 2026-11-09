//! The lockstep gate's Rust side without Python: run each case of a case
//! table on this board (`famicom-trace`) and compare its trace with the
//! reference's cached one, line for line. scripts/lockstep.py does the same
//! with `--reference-cache`, and also runs the reference when a trace is not
//! cached; here a case without a cached trace fails, loudly.
//!
//!     famicom-lockstep --cases TSV --cache DIR [--jobs N] [--trace BIN] [--only TEXT]
//!
//! A case is `name<TAB>slices<TAB>rom<TAB>arguments`; `$NES_TEST_ROMS`,
//! `$NES_DUMPS` and `$INPUTS` are taken from the environment, with
//! lockstep.py's defaults (the test ROMs at famicom-rust's reference
//! checkout). The cached trace is `DIR/<name>.<slices>.ref`, the name with
//! every run of other characters than `A-Za-z0-9._-` made one `_`, as
//! lockstep.py names it.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

struct Case {
    name: String,
    slices: u64,
    rom: String,
    args: Vec<String>,
}

fn safe(name: &str) -> String {
    let mut out = String::new();
    let mut run = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
            out.push(c);
            run = false;
        } else if !run {
            out.push('_');
            run = true;
        }
    }
    out
}

fn cases(table: &Path) -> Vec<Case> {
    let vars: Vec<(String, String)> = [
        (
            "NES_TEST_ROMS",
            "/data/emu/famicom-rust/external/famicom-python/tests/test_roms".to_owned(),
        ),
        ("NES_DUMPS", "/data/emu/roms/nes".to_owned()),
        ("INPUTS", format!("{ROOT}/scripts/inputs")),
    ]
    .into_iter()
    .map(|(k, d)| (format!("${k}"), std::env::var(k).unwrap_or(d)))
    .collect();
    let expand = |text: &str| {
        vars.iter()
            .fold(text.to_owned(), |t, (k, v)| t.replace(k.as_str(), v))
    };
    let text =
        std::fs::read_to_string(table).unwrap_or_else(|e| panic!("{}: {e}", table.display()));
    text.lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|line| {
            let mut fields = line.split('\t');
            let name = fields.next().unwrap().to_owned();
            let slices = fields.next().expect("slices").parse().expect("slices");
            let rom = expand(fields.next().expect("rom"));
            // The arguments hold no quoted words (checked by the ladder's tables).
            let args = expand(fields.next().unwrap_or(""))
                .split_whitespace()
                .map(str::to_owned)
                .collect();
            Case {
                name,
                slices,
                rom,
                args,
            }
        })
        .collect()
}

fn one(case: &Case, trace: &Path, cache: &Path, scratch: &Path) -> Result<String, String> {
    let safe = safe(&case.name);
    let reference = cache.join(format!("{safe}.{}.ref", case.slices));
    if !reference.is_file() {
        return Err(format!("no cached reference trace {}", reference.display()));
    }
    let ours = scratch.join(format!("{safe}.rust"));
    let began = Instant::now();
    let run = Command::new("nice")
        .arg(trace)
        .arg(&case.rom)
        .arg("--slices")
        .arg(case.slices.to_string())
        .args(&case.args)
        .arg("--out")
        .arg(&ours)
        .output()
        .map_err(|e| format!("{}: {e}", trace.display()))?;
    if !run.status.success() {
        let err = String::from_utf8_lossy(&run.stderr);
        return Err(format!(
            "rust failed: {}",
            &err[err.len().saturating_sub(300)..]
        ));
    }
    let seconds = began.elapsed().as_secs_f64();
    let a = std::fs::read_to_string(&reference).map_err(|e| e.to_string())?;
    let b = std::fs::read_to_string(&ours).map_err(|e| e.to_string())?;
    for (number, (left, right)) in a.lines().zip(b.lines()).enumerate() {
        if left != right {
            return Err(format!(
                "line {number}: reference {left:?}\n      rust      {right:?}"
            ));
        }
    }
    if a.len() != b.len() {
        return Err("one trace is longer".to_owned());
    }
    std::fs::remove_file(&ours).ok();
    Ok(format!(
        "identical, {} slices ({seconds:.0} s)",
        case.slices
    ))
}

fn main() {
    let mut table = None;
    let mut cache = None;
    let mut jobs = 4usize;
    let mut trace = PathBuf::from(format!("{ROOT}/target/release/famicom-trace"));
    let mut only = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--cases" => table = args.next().map(PathBuf::from),
            "--cache" => cache = args.next().map(PathBuf::from),
            "--jobs" => jobs = args.next().unwrap().parse().unwrap(),
            "--trace" => trace = PathBuf::from(args.next().unwrap()),
            "--only" => only = args.next(),
            _ => panic!("unknown argument {arg}"),
        }
    }
    let table = table.expect("--cases TSV");
    let cache = cache.expect("--cache DIR");
    let mut todo = cases(&table);
    if let Some(only) = &only {
        todo.retain(|c| c.name.contains(only.as_str()));
    }
    // The longest first, so the pool does not end on one long case.
    todo.sort_by_key(|c| std::cmp::Reverse(c.slices));
    let scratch = PathBuf::from(format!("{ROOT}/external/traces"));
    std::fs::create_dir_all(&scratch).unwrap();
    let todo = Arc::new(todo);
    let next = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(Mutex::new(0usize));
    let workers: Vec<_> = (0..jobs.max(1))
        .map(|_| {
            let (todo, next, failed) = (todo.clone(), next.clone(), failed.clone());
            let (trace, cache, scratch) = (trace.clone(), cache.clone(), scratch.clone());
            std::thread::spawn(move || loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some(case) = todo.get(i) else { break };
                match one(case, &trace, &cache, &scratch) {
                    Ok(text) => println!("{}: {text}", case.name),
                    Err(text) => {
                        println!("{}: {text}", case.name);
                        *failed.lock().unwrap() += 1;
                    }
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    let failed = *failed.lock().unwrap();
    println!(
        "{} of {} cases identical ({})",
        todo.len() - failed,
        todo.len(),
        table.display()
    );
    std::process::exit(i32::from(failed > 0));
}
