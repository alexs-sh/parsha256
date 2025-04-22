use async_channel::{self, Receiver};
use sha256::try_digest;

use std::env::args;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn should_scan(path: &Path) -> bool {
    // Let's skip some runtime paths
    let skip_prefixes = ["/dev/", "/proc/", "/sys/"];
    path.to_str()
        .map(|p| !skip_prefixes.iter().any(|prefix| p.starts_with(prefix)))
        .unwrap_or(false)
}

fn scan_path(path: &Path, to_generator: &mut async_channel::Sender<PathBuf>) -> io::Result<()> {
    let file_type = fs::metadata(path)?.file_type();
    if file_type.is_file() {
        // End of recursion: normalize the filename and pass it to the digest generator
        let filename = fs::canonicalize(path)?;
        to_generator.send_blocking(filename).map_err(|err| {
            eprintln!("Failed to send filename {:?}. {}", path, err);
            io::Error::new(io::ErrorKind::Other, err.to_string())
        })?;
    } else if file_type.is_dir() {
        // Recursion step: visit each entry and repeat the process
        path.read_dir()
            .map_err(|err| {
                eprintln!("Failed to read {:?}. {}", path, err);
                err
            })?
            .flatten()
            .for_each(|entry| {
                let next_path = entry.path();
                if should_scan(&next_path) {
                    if let Err(err) = scan_path(&next_path, to_generator) {
                        eprintln!("Failed to scan {:?}.{}", entry, err);
                    }
                }
            });
    }
    Ok(())
}

fn generate_digest(receiver: Receiver<PathBuf>) {
    while let Ok(path) = receiver.recv_blocking() {
        let path = path.as_path();
        match try_digest(path) {
            Ok(hash) => println!("{} {}", hash, path.to_str().unwrap()),
            Err(e) => eprintln!("Failed to create digest {:?}: {}", path, e),
        }
    }
}

fn main() {
    let nthreads = std::thread::available_parallelism().map_or(1, |x| x.get());
    let root: String = args().nth(1).unwrap_or(".".to_owned());

    let mut threads = Vec::new();
    let (tx, rx) = async_channel::unbounded::<PathBuf>();
    let thread = std::thread::spawn(move || {
        let mut tx = tx;
        let _ = scan_path(Path::new(&root), &mut tx);
    });

    threads.push(thread);

    for _ in 0..nthreads {
        let local_rx = rx.clone();
        let thread = std::thread::spawn(move || generate_digest(local_rx));
        threads.push(thread);
    }

    for thread in threads {
        let _ = thread.join();
    }
}
