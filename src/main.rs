use async_channel::{self, Receiver};
use sha256::try_digest;

use std::env::args;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn scan_path(path: &Path, to_generator: &mut async_channel::Sender<PathBuf>) -> io::Result<()> {
    if path.is_file() {
        // End of recursion: normalize the filename and pass it to the digest generator
        let filename = fs::canonicalize(path)?;
        to_generator.send_blocking(filename).map_err(|err| {
            eprintln!("Failed to send filename {:?}. {}", path, err);
            io::Error::new(io::ErrorKind::Other, err.to_string())
        })?;
    } else if path.is_dir() {
        // Recursion step: visit each entry and repeat the process
        path.read_dir()
            .map_err(|err| {
                eprintln!("Failed to read {:?}. {}", path, err);
                err
            })?
            .flatten()
            .for_each(|entry| {
                if let Err(err) = scan_path(&entry.path(), to_generator) {
                    eprintln!("Failed to scan {:?}.{}", entry, err);
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
        let thread = std::thread::spawn(move || generate_digest(local_rx.clone()));
        threads.push(thread);
    }

    for thread in threads {
        let _ = thread.join();
    }
}
