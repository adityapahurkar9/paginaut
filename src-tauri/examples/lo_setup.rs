//! Sets up the LibreOffice pack from an installer already downloaded (the app downloads it itself):
//! cargo run --release --example lo_setup -- <LibreOffice_x_Win_x86-64.msi> <version> <pack dir> <profile dir>
use paginaut_lib::office::libre;
use std::path::Path;
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let t = Instant::now();
    let r = libre::set_up(Path::new(&a[1]), &a[2], "", Path::new(&a[3]), Path::new(&a[4]), &mut |stage, _, _| {
        println!("{:?} {}", t.elapsed(), serde_json::to_string(&stage).unwrap());
    });
    match r {
        Ok(lo) => println!("OK {} in {:?}, {} MB", lo.soffice.display(), t.elapsed(), libre::size_of(Path::new(&a[3])) >> 20),
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(1);
        }
    }
}
