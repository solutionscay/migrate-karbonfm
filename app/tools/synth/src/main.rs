//! `karbon-synth-export <out-dir> [--scale F] [--seed N] [--clean]`: writes the synthetic FileMaker export
//! (karbon_synth) into <out-dir>. `--clean` leaves out the dirty rows. Never commit the output.

use karbon_synth::{generate, Config};
use std::path::PathBuf;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::default();
    let mut out = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--scale" => config.scale = args.next().ok_or("--scale needs a number")?.parse()?,
            "--seed" => config.seed = args.next().ok_or("--seed needs a number")?.parse()?,
            "--clean" => config.dirty = false,
            "-h" | "--help" => {
                println!("usage: karbon-synth-export <out-dir> [--scale F] [--seed N] [--clean]");
                return Ok(());
            }
            dir => out = Some(PathBuf::from(dir)),
        }
    }
    let out = out.ok_or("usage: karbon-synth-export <out-dir> [--scale F] [--seed N] [--clean]")?;
    let t = Instant::now();
    let s = generate(config, &out)?;
    for (table, n) in &s.rows {
        println!("{n:>7}  {table}.csv");
    }
    println!(
        "{} rows in {} files, {} container files, {:.1} MB, in {:.2?} -> {}",
        s.total_rows(),
        s.rows.len(),
        s.container_files,
        s.bytes as f64 / 1e6,
        t.elapsed(),
        out.display()
    );
    Ok(())
}
