extern crate trackup;
extern crate clap;

use std::path::PathBuf;
use std::time::Duration;
use trackup::config::Config;

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
        .arg(
            clap::Arg::with_name("tracing-path")
                .short("t")
                .long("tracing-path")
                .value_name("TRACING_PATH")
                .help("Path to kernel tracing directory within a debugfs")
                .takes_value(true)
                .default_value("/sys/kernel/debug/tracing")
        )
        .arg(
            clap::Arg::with_name("sys-path")
                .short("s")
                .long("sys-path")
                .value_name("SYS_PATH")
                .help("Path to sysfs")
                .takes_value(true)
                .default_value("/sys")
        )
        .arg(
            clap::Arg::with_name("trace-buffer-size")
                .short("b")
                .long("trace-buffer-size")
                .value_name("BUFFER_SIZE_KB")
                .help("Per-CPU size of kernel tracing buffer in KB")
                .takes_value(true)
                .default_value("8192")
        )
        .arg(
            clap::Arg::with_name("progress-period")
                .short("p")
                .long("progress-period")
                .value_name("SECONDS")
                .help("Time in seconds between progress updates")
                .takes_value(true)
                .default_value("5")
        )
        .arg(
            clap::Arg::with_name("max-diagram-size")
                .short("d")
                .long("max-diagram-size")
                .value_name("SECONDS")
                .help("Maximum number of characters to use for progress diagram")
                .takes_value(true)
                .default_value("1024")
        )
        .arg(
            clap::Arg::with_name("exclusive-progress-updates")
                .short("x")
                .long("exclusive-progress-updates")
                .help("Clear screen before each progress update")
                .takes_value(false)
        )
        .get_matches();

    let source      = PathBuf::from(matches.value_of("in").unwrap());
    let destination = PathBuf::from(matches.value_of("out").unwrap());
    let chunk_size: usize = matches.value_of("chunk-size").unwrap().parse().unwrap();
    let tracing_path = PathBuf::from(matches.value_of("tracing-path").unwrap());
    let sys_path = PathBuf::from(matches.value_of("sys-path").unwrap());
    let trace_buffer_size: usize = matches.value_of("trace-buffer-size").unwrap().parse().unwrap();
    let progress_update_period = Duration::from_secs(matches.value_of("progress-period").unwrap().parse().unwrap());
    let exclusive_progress_updates = matches.is_present("exclusive-progress-updates");
    let max_diagram_size: usize = matches.value_of("max-diagram-size").unwrap().parse().unwrap();

    let config = Config {
        source: source.as_path(),
        destination: destination.as_path(),
        chunk_size,
        tracing_path: tracing_path.as_path(),
        sys_path: sys_path.as_path(),
        trace_buffer_size,
        progress_update_period,
        exclusive_progress_updates,
        max_diagram_size,
    };

    if let Err(_) = trackup::backup_device(&config) {
        eprintln!("Backup failed");
    }
}
