//! scripts/testrom_compare.py without Python: compare the reference's test
//! ROM outcomes with this board's (`famicom-testrom`'s), case by case.
//!
//!     famicom-testrom-compare REFERENCE.jsonl RUST.jsonl
//!
//! A case agrees when its verdict, CPU cycles, instructions and detail text
//! are the same (for the "spans" protocol the detail prints Python sets, whose
//! order is the interpreter's: there only the rest is compared). Prints each
//! disagreement and a summary per suite; exits 1 unless all agree.

use serde_json::Value;
use std::collections::{BTreeMap, HashMap};

fn lines(path: &str) -> Vec<Value> {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{path}: {e}"))
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let reference = lines(&args[0]);
    let rust: HashMap<String, Value> = lines(&args[1])
        .into_iter()
        .map(|row| (row["label"].as_str().unwrap().to_owned(), row))
        .collect();
    let mut tally: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    for row in &reference {
        let (suite, label) = (row["suite"].as_str().unwrap(), row["label"].as_str().unwrap());
        let Some(want) = row.get("outcome") else { continue };
        let entry = tally.entry(suite.to_owned()).or_default();
        entry.1 += 1;
        entry.2 += usize::from(want["passed"] == Value::Bool(true));
        let Some(mine) = rust.get(label) else {
            println!("{suite} {label}: not run here");
            continue;
        };
        if let Some(error) = mine.get("error") {
            println!("{suite} {label}: rust error {error}");
            continue;
        }
        let got = &mine["outcome"];
        let mut fields = vec!["passed", "cycles", "instructions"];
        if row["protocol"].as_str() != Some("spans") {
            fields.push("detail");
        }
        let different: Vec<_> = fields.into_iter().filter(|f| got[f] != want[f]).collect();
        if different.is_empty() {
            entry.0 += 1;
        } else {
            println!("{suite} {label}: differs in {different:?}");
        }
    }
    let (mut agree, mut total) = (0, 0);
    for (suite, (a, t, p)) in &tally {
        println!("{suite}: {a} of {t} agree; reference passes {p}");
        agree += a;
        total += t;
    }
    println!("all: {agree} of {total} agree");
    std::process::exit(i32::from(agree != total));
}
