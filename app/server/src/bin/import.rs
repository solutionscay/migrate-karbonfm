//! `karbon-import <export-dir> --data-dir <dir> [--me <user id>]`: loads a FileMaker export (one CSV per table,
//! OD-03-01) into a NEW database `<dir>/karbonfm.db` (OP-IMPORT-01, BR-IMPORT-01) and writes the report next to it.
//! The folder must not hold a database yet. Point the dev server at it with `karbon-dev-server <port> --data-dir <dir>`.

use karbon_server::import::{import_export, Options};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let usage = "usage: karbon-import <export-dir> --data-dir <dir> [--me <user id>]";
    let (mut export, mut data_dir, mut opts) = (None, None, Options::default());
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--data-dir" => data_dir = Some(PathBuf::from(args.next().ok_or(usage)?)),
            "--me" => opts.me_user_id = Some(args.next().ok_or(usage)?),
            "-h" | "--help" => {
                println!("{usage}");
                return Ok(());
            }
            dir => export = Some(PathBuf::from(dir)),
        }
    }
    let (export, data_dir) = (export.ok_or(usage)?, data_dir.ok_or(usage)?);
    let r = import_export(&export, &data_dir, &opts)?;
    println!(
        "{} rows loaded from {} files in {} ms: {} rejected, {} values changed, {} containers copied ({} missing)",
        r.loaded(),
        r.files.len(),
        r.elapsed_ms,
        r.rejected.len(),
        r.changed.len(),
        r.containers_copied,
        r.containers_missing
    );
    for x in &r.rejected {
        println!("  rejected {} {}: {}", x.source_table, x.source_key.as_deref().unwrap_or("-"), x.reason);
    }
    println!("database: {}\nreport:   {}", r.database.display(), r.report_path.display());
    Ok(())
}
