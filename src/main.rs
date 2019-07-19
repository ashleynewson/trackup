extern crate trackup;
extern crate clap;

use std::path::Path;

fn main() {
    let matches = clap::App::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about("Snapshotless device backup")
        .arg(
            clap::Arg::with_name("in")
                .short("i")
                .long("in")
                .value_name("SOURCE")
                .help("Input block device")
                .takes_value(true)
                .required(true)
        )
        .arg(
            clap::Arg::with_name("out")
                .short("o")
                .long("out")
                .value_name("DESTINATION")
                .help("Output file (or block device)")
                .takes_value(true)
                .required(true)
        )
        .arg(
            clap::Arg::with_name("chunk-size")
                .short("c")
                .long("chunk-size")
                .value_name("CHUNK_SIZE")
                .help("Granularity of modification tracking")
                .takes_value(true)
                .required(true)
        )
        .get_matches();

    let source      = matches.value_of("in").unwrap();
    let destination = matches.value_of("out").unwrap();
    let chunk_size: usize = matches.value_of("chunk-size").unwrap().parse().unwrap();
    if let Err(_) = trackup::backup_device(chunk_size, Path::new(source), Path::new(destination)) {
        eprintln!("Backup failed");
    }
}
