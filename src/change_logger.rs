use std::cell::Cell;
use std::fs::File;
use std::path::{Path,PathBuf};
use std::sync::mpsc::{Receiver,Sender};
use std::sync::{Arc,Barrier};
use std::os::unix::io::IntoRawFd;
use std::io::Read;
use device::Device;
use quick_io::{append_to_file_at_path,slurp_file_at_path,fd_poll_read};
// use std::ffi::CString;


// This needs to be read directly from a file.
#[repr(C, packed)]
struct BlkEvent {
    magic:    u32, /* MAGIC << 8 | version */
    sequence: u32, /* event number */
    time:     u64, /* in nanoseconds */
    sector:   u64, /* disk offset */
    bytes:    u32, /* transfer length */
    action:   u32, /* what happened */
    pid:      u32, /* who did it */
    device:   u32, /* device identifier (dev_t) */
    cpu:      u32, /* on what cpu did it happen */
    error:    u16, /* completion error */
    pdu_len:  u16, /* length of data after this trace */
}

// struct TraceSetup {
//     trace_enable_path: Path,
//     trace_pipe_path: Path,
//     current_tracer_path: Path,
// }

pub struct ChangeLogger<'d> {
    chunk_size: usize,
    device: &'d Device,
    trace_path: PathBuf,
    // trace_enabled: bool,
}


const MAGIC_NATIVE_ENDIAN: u32 = 0x65617400;
const MAGIC_REVERSE_ENDIAN: u32 = 0x00746165;
const SUPPORTED_VERSION: u8 = 0x07;

impl BlkEvent {
    fn read_from_file(trace_pipe: &mut File) -> BlkEvent {
        // let magic = trace_pipe.read_u32::<BigEndian>()?;
        // let (bigendian, version) =
        //     if magic & 0xffffff00 == magic_native_endian {
        //         (true, magic & 0xff)
        //     } else if (magic & 0x00ffffff == magic_reverse_endian) {
        //         (false, magic >> 24)
        //     } else {
        //         panic!("Incorrect magic number for event");
        //     }

        // if version != supported_version {
        //     panic!("Unsupprted blk event format - only version 0x07 is supported");
        // }

        // if bigendian {
        //     sequence
        // }
        // BlkEvent {
        //     magic,
        //     sequence,
        //     time,
        //     sector,
        //     bytes,
        //     action,
        //     pid,
        //     device,
        //     cpu,
        //     error,
        //     pdu_len,
        // }
        let event_size = ::std::mem::size_of::<BlkEvent>();
        let mut event = unsafe {
            let mut event: BlkEvent = ::std::mem::uninitialized();
            let mut buffer = ::std::slice::from_raw_parts_mut(&mut event as *mut BlkEvent as *mut u8, event_size);
            trace_pipe.read_exact(buffer).expect("Could not read event from trace pipe");
            event
        };
        
        let magic = event.magic;
        let (native_endian, version): (bool, u8) =
            if magic & 0xffffff00 == MAGIC_NATIVE_ENDIAN {
                (true, (magic & 0xff) as u8)
            } else if magic & 0x00ffffff == MAGIC_REVERSE_ENDIAN {
                (false, (magic >> 24) as u8)
            } else {
                panic!("Incorrect magic number for event. Got {:x}", magic);
            };

        if version != SUPPORTED_VERSION {
            panic!("Unsupprted blk event format - only version 0x07 is supported");
        }

        if !native_endian {
            event.swap_endian();
        }

        if event.pdu_len > 0 {
            eprintln!("TEST");
            // Just discard - we don't care.
            let mut discard = vec![0; event.pdu_len as usize];
            trace_pipe.read_exact(&mut discard).expect("Could not read (pdu portion of) event from trace pipe");
        }

        event
    }

    fn swap_endian(&mut self) {
        self.magic    = self.magic.swap_bytes();
        self.sequence = self.sequence.swap_bytes();
        self.time     = self.time.swap_bytes();
        self.sector   = self.sector.swap_bytes();
        self.bytes    = self.bytes.swap_bytes();
        self.action   = self.action.swap_bytes();
        self.pid      = self.pid.swap_bytes();
        self.device   = self.device.swap_bytes();
        self.cpu      = self.cpu.swap_bytes();
        self.error    = self.error.swap_bytes();
        self.pdu_len  = self.pdu_len.swap_bytes();
    }
}


// RAII-based do something then undo it.
struct DoUndo<'u> {
    undoer: Option<Box<FnOnce() + 'u>>,
}

impl<'u> DoUndo<'u> {
    pub fn new<D: FnOnce(), U: FnOnce() + 'u>(doer: D, undoer: U) -> Self {
        doer();
        Self {
            undoer: Some(Box::new(undoer)),
        }
    }
}
impl<'u> Drop for DoUndo<'u> {
    fn drop(&mut self) {
        (self.undoer.take().unwrap())();
    }
}

// impl TraceSetup {
//     pub fn new(ftrace_path: Path) -> Self {
//         let trace_pipe_path = ftrace_path.join("trace_pipe");
//         let trace_options_path = ftrace_path.join("trace_options");
//         let current_tracer_path = ftrace_path.join("current_tracer");

//         append_to_file_with_path(trace_pipe_path, 

//         TraceSetup {
//             ftrace_path,
//             trace_pipe_path,
//             trace_options_path,
//             current_tracer_path,
//         }
//     }

//     fn 
// }

// impl Drop for TraceSetup {
//     fn drop(&mut self) {
        
//     }
// }


impl<'d> ChangeLogger<'d> {
    pub fn new(chunk_size: usize, device: &'d Device, trace_path: &Path) -> Self {
        // let trace_enable_path = Path::new("/sys/dev/block/").join(format!("{}:{}", device.major, device.minor)).join("trace/enable");


        Self{
            chunk_size,
            device,
            trace_path: trace_path.to_path_buf(),
        }
    }

    // pub fn set_trace_enabled(&mut self, enable: bool) {
    //     let file = match File::create(self.trace_enable_path) {
    //         Ok(x) => x,
    //         Err(_) => {
    //             return Err("Could not open trace enable file");
    //         },
    //     };
    //     file.write_all(if enable {b"1"} else {b"0"}).expect("Could not enable tracing");
    // }

    pub fn run(&self, log_channel: Sender<usize>, sync_barrier_channel: Receiver<Arc<Barrier>>) {
        {
            let events_enabled = slurp_file_at_path(&self.trace_path.join("events/enable")).unwrap();
            if std::str::from_utf8(&events_enabled).unwrap() != "0\n" {
                panic!("Some tracing events are already enabled");
            }
        }

        let old_current_tracer = slurp_file_at_path(&self.trace_path.join("current_tracer")).unwrap();
        let _current_tracer_setup = DoUndo::new(
            || {append_to_file_at_path(&self.trace_path.join("current_tracer"), b"blk\n").unwrap();},
            || {append_to_file_at_path(&self.trace_path.join("current_tracer"), &old_current_tracer).unwrap();},
        );

        // let old_tracer_options = slurp_file_at_path(&self.trace_path.join("trace_options")).unwrap();
        // let _tracer_options_setup = DoUndo::new(
        //     || {append_to_file_at_path(&self.trace_path.join("trace_options"), b"bin\nnocontext-info\n").unwrap();},
        //     || {append_to_file_at_path(&self.trace_path.join("trace_options"), &old_tracer_options).unwrap();},
        // );

        let old_tracer_option_bin = slurp_file_at_path(&self.trace_path.join("options/bin")).unwrap();
        let _tracer_option_bin_setup = DoUndo::new(
            || {append_to_file_at_path(&self.trace_path.join("options/bin"), b"1\n").unwrap();},
            || {append_to_file_at_path(&self.trace_path.join("options/bin"), &old_tracer_option_bin).unwrap();},
        );

        let old_tracer_option_context = slurp_file_at_path(&self.trace_path.join("options/context-info")).unwrap();
        let _tracer_option_context = DoUndo::new(
            || {append_to_file_at_path(&self.trace_path.join("options/context-info"), b"0\n").unwrap();},
            || {append_to_file_at_path(&self.trace_path.join("options/context-info"), &old_tracer_option_context).unwrap();},
        );

        let mut trace_pipe = File::open(&self.trace_path.join("trace_pipe")).expect("Could not open trace pipe");
        let trace_pipe_fd = trace_pipe.try_clone().unwrap().into_raw_fd();

        // Flush anything in the trace_pipe first so we know we're only going
        // to get blk data.
        {
            let file_flags = unsafe{
                libc::fcntl(trace_pipe_fd, libc::F_GETFL, 0)
            };
            if file_flags < 0 {
                panic!("Could not get trace_pipe file flags");
            }
            let file = unsafe{
                libc::fdopen(trace_pipe_fd, b"rb\0".as_ptr() as *const i8)
            };
            if file.is_null() {
                panic!("Could not open trace_pipe stream");
            }
            unsafe{
                if libc::fcntl(trace_pipe_fd, libc::F_SETFL, file_flags | libc::O_NONBLOCK) < 0 {
                    panic!("Could not set non-blocking trace_pipe");
                }
            };
            // I'm assuming the stream can't ever half-write a data structure.
            // let mut junked: Vec<u8> = vec![0];
            // while fd_poll_read(trace_pipe_fd) {
            //     trace_pipe.read_exact(&mut junked).expect("Could not pre-consume trace_pipe before tracing");
            // }
            unsafe{
                while libc::fgetc(file) >= 0 {};
            };
            unsafe{
                if libc::fcntl(trace_pipe_fd, libc::F_SETFL, file_flags) < 0 {
                    panic!("Could not restore blocking io on trace_pipe");
                }
            };
            unsafe{
                libc::fclose(file);
            };
        }

        let block_trace_enable_path = Path::new("/sys/dev/block").join(&format!("{}:{}", self.device.major, self.device.minor)).join("trace/enable");
        let old_block_trace_enable = slurp_file_at_path(&block_trace_enable_path).unwrap();
        let _block_trace_enable_setup = DoUndo::new(
            || {append_to_file_at_path(&block_trace_enable_path, b"1\n").unwrap();},
            || {append_to_file_at_path(&block_trace_enable_path, &old_block_trace_enable).unwrap();},
        );

        let continuing = Cell::new(true);

        let mut consume_event = || {
            let event = BlkEvent::read_from_file(&mut trace_pipe);

            if event.action & 0x00000002 == 0 {
                // Was not a write operation, so we don't care.
                return;
            }
            if event.sector == 0 || event.bytes == 0 {
                // There is no data location associated, so skip.
                return;
            }
            if event.device != self.device.dev as u32 {
                // We're not looking at this device.
                return;
            }

            let first_byte: u64 = event.sector * 512; // I think a sector is always 512 on Linux?
            let last_byte: u64 = first_byte + (event.bytes as u64) - 1;
            let first_chunk: usize = (first_byte / (self.chunk_size as u64)) as usize;
            let last_chunk: usize = (last_byte / (self.chunk_size as u64)) as usize;
            // let end_chunk = end_byte / self.chunk_size
            //     + if end_byte % self.chunk_size { 1 } else { 0 };
            for change_index in first_chunk..(last_chunk+1) {
                if let Err(_) = log_channel.send(change_index) {
                    continuing.set(false);
                    return;
                }
            }
        };
        let mut try_consume_event = || {
            // Replace with non-blocking read
            let can_read = fd_poll_read(trace_pipe_fd);
            if can_read {
                consume_event();
            }
            can_read
        };
        while continuing.get() {
            match sync_barrier_channel.try_recv() {
                Ok(barrier) => {
                    while try_consume_event() {}
                    barrier.wait();
                },
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // Does not block
                    try_consume_event();
                },
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    continuing.set(false);
                }
            };
        }
    }
}

// impl Drop for Device {
//     fn drop(&mut self) {
//         if self.trace_enabled {
//             self.set_trace_enabled(false);
//         }
//     }
// }
