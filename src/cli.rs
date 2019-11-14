use clap::{App,Arg};

pub fn get_app() -> App<'static, 'static> {
    App::new(env!("CARGO_PKG_NAME"))
        .version(env!("CARGO_PKG_VERSION"))
        .author(env!("CARGO_PKG_AUTHORS"))
        .about("Snapshotless device backup")
        .arg(
            Arg::with_name("tracing-path")
                .short("t")
                .long("tracing-path")
                .value_name("TRACING_PATH")
                .help("Path to kernel tracing directory within a debugfs")
                .takes_value(true)
                .default_value("/sys/kernel/debug/tracing")
        )
        .arg(
            Arg::with_name("sys-path")
                .short("s")
                .long("sys-path")
                .value_name("SYS_PATH")
                .help("Path to sysfs")
                .takes_value(true)
                .default_value("/sys")
        )
        .arg(
            Arg::with_name("trace-buffer-size")
                .short("b")
                .long("trace-buffer-size")
                .value_name("BUFFER_SIZE_KB")
                .help("Per-CPU size of kernel tracing buffer in KB")
                .takes_value(true)
                .default_value("8192")
        )
        .arg(
            Arg::with_name("progress-period")
                .short("p")
                .long("progress-period")
                .value_name("SECONDS")
                .help("Time in seconds between progress updates")
                .takes_value(true)
        )
        .arg(
            Arg::with_name("max-diagram-size")
                .short("d")
                .long("max-diagram-size")
                .value_name("SECONDS")
                .help("Maximum number of characters to use for progress diagrams")
                .takes_value(true)
        )
        .arg(
            Arg::with_name("exclusive-progress-updates")
                .short("x")
                .long("exclusive-progress-updates")
                .help("Clear screen before each progress update")
                .takes_value(false)
        )
        .arg(
            Arg::with_name("color")
                .short("C")
                .long("color")
                .help("Display diagrams in color")
                .takes_value(false)
        )
        .arg(
            Arg::with_name("copy")
                .short("f")
                .long("copy")
                .value_names(&["SOURCE", "DESTINATION"])
                .help("Input block device and output file or block device to copy. Can be specified multiple times to copy multiple block devices.")
                .takes_value(true)
                .number_of_values(2)
                .multiple(true)
                .required_unless("daemon")
                .conflicts_with("manifest")
        )
        .arg(
            Arg::with_name("chunk-size")
                .short("c")
                .long("chunk-size")
                .value_name("CHUNK_SIZE")
                .help("Granularity of modification tracking")
                .takes_value(true)
                .required(true)
                .conflicts_with("manifest")
        )
        .arg(
            Arg::with_name("reuse")
                .short("r")
                .long("reuse")
                .help("Write over an existing output file/device (requires the file to be present). By default, any existing file will be deleted, and a new file will be pre-allocated.")
                .takes_value(false)
                .conflicts_with("manifest")
        )
        .arg(
            Arg::with_name("management-socket")
                .short("m")
                .long("management-socket")
                .value_name("SOCKET_PATH")
                .help("Unix socket to use for management")
                .takes_value(true)
        )
        .arg(
            Arg::with_name("daemon")
                .short("D")
                .long("daemon")
                .help("Start a backup daemon")
                .takes_value(false)
                .requires("management-socket")
        )
        .arg(
            Arg::with_name("config")
                .long("config")
                .help("Specify config file")
                .takes_value(true)
        )
        .arg(
            Arg::with_name("manifest")
                .long("manifest")
                .help("Specify manifest file")
                .takes_value(true)
        )
}
