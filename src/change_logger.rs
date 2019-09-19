use std::cell::Cell;
use std::sync::mpsc::{Receiver,Sender};
use std::sync::{Arc,Barrier};
use config::Config;
use device::Device;
use quick_io::{append_to_file_at_path,slurp_file_at_path,fd_poll_read};
use libc::{c_int,c_void,ssize_t,size_t};
use std::ffi::CString;


// This needs to be read directly from a file.
#[repr(C, packed)]
struct BlkEvent {
    magic:    u32, /* MAGIC << 8 | version */
    sequence: u32, /* event number */
    time:     u64, /* in nanoseconds */
    sector:   u64, /* disk offset */
    bytes:    u32, /* transfer length */
    action:   u32, /* what happened (a 16 high-bit field for action and then a 16 low-bit enum for category) */
    pid:      u32, /* who did it */
    device:   u32, /* device identifier (major is 12 high-bits then minor is 20 low-bits */
    cpu:      u32, /* on what cpu did it happen */
    error:    u16, /* completion error */
    pdu_len:  u16, /* length of data after this trace */
}

pub struct ChangeLogger<'c, 'd> {
    config: &'c Config<'c>,
    device: &'d Device,
}


const MAGIC_NATIVE_ENDIAN: u32 = 0x65617400;
const MAGIC_REVERSE_ENDIAN: u32 = 0x00746165;
const SUPPORTED_VERSION: u8 = 0x07;

impl BlkEvent {
    fn try_read_from_file(trace_pipe_fd: c_int) -> Option<BlkEvent> {
        let event_size = ::std::mem::size_of::<BlkEvent>();
        // Wait 1ms for something
        if !fd_poll_read(trace_pipe_fd, 1) {
            return None;
        }
        let mut event = unsafe {
            let mut event: BlkEvent = ::std::mem::uninitialized();
            let buffer = ::std::slice::from_raw_parts_mut(&mut event as *mut BlkEvent as *mut u8, event_size);
            let bytes_read = libc::read(trace_pipe_fd, buffer.as_mut_ptr() as *mut c_void, event_size as size_t);
            if bytes_read == event_size as ssize_t {
                event
            } else if bytes_read == 0 {
                return None;
            } else if bytes_read < 0 {
                let errno = *libc::__errno_location();
                if errno == libc::EAGAIN || errno == libc::EWOULDBLOCK {
                    // Not got anything to read right now.
                    return None;
                } else {
                    panic!("Could not read from trace pipe");
                }
            } else {
                panic!("Read an incorrect number of bytes for a blk event. Wanted {}, read {}", event_size, bytes_read);
            }
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
            // Just discard - we don't care.
            let mut discard: Vec<u8> = vec![0; event.pdu_len as usize];
            unsafe {
                if libc::read(trace_pipe_fd, discard.as_mut_ptr() as *mut c_void, event.pdu_len as size_t) != event.pdu_len as ssize_t {
                    panic!("Could not read (pdu portion of) event from trace pipe");
                }
            }
        }

        Some(event)
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
    undoer: Option<Box<dyn FnOnce() + 'u>>,
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


impl<'c, 'd> ChangeLogger<'c, 'd> {
    pub fn new(config: &'c Config, device: &'d Device) -> Self {
        Self{
            config,
            device,
        }
    }

    pub fn run(&self, log_channel: Sender<usize>, sync_barrier_channel: Receiver<Arc<Barrier>>) {
        {
            let events_enabled = slurp_file_at_path(&self.config.tracing_path.join("events/enable")).unwrap();
            if std::str::from_utf8(&events_enabled).unwrap() != "0\n" {
                panic!("Some tracing events are already enabled");
            }
        }

        let old_current_tracer = slurp_file_at_path(&self.config.tracing_path.join("current_tracer")).unwrap();
        let _current_tracer_setup = DoUndo::new(
            || {append_to_file_at_path(&self.config.tracing_path.join("current_tracer"), b"blk\n").unwrap();},
            || {append_to_file_at_path(&self.config.tracing_path.join("current_tracer"), &old_current_tracer).unwrap();},
        );

        let old_tracer_option_bin = slurp_file_at_path(&self.config.tracing_path.join("options/bin")).unwrap();
        let _tracer_option_bin_setup = DoUndo::new(
            || {append_to_file_at_path(&self.config.tracing_path.join("options/bin"), b"1\n").unwrap();},
            || {append_to_file_at_path(&self.config.tracing_path.join("options/bin"), &old_tracer_option_bin).unwrap();},
        );

        let old_tracer_option_context = slurp_file_at_path(&self.config.tracing_path.join("options/context-info")).unwrap();
        let _tracer_option_context = DoUndo::new(
            || {append_to_file_at_path(&self.config.tracing_path.join("options/context-info"), b"0\n").unwrap();},
            || {append_to_file_at_path(&self.config.tracing_path.join("options/context-info"), &old_tracer_option_context).unwrap();},
        );

        let old_buffer_size = slurp_file_at_path(&self.config.tracing_path.join("buffer_size_kb")).unwrap();
        let _buffer_size = DoUndo::new(
            || {append_to_file_at_path(&self.config.tracing_path.join("buffer_size_kb"), format!("{}\n", self.config.trace_buffer_size).as_bytes()).unwrap();},
            || {append_to_file_at_path(&self.config.tracing_path.join("buffer_size_kb"), &old_buffer_size).unwrap();},
        );

        let trace_pipe_fd = unsafe {
            libc::open(CString::new(self.config.tracing_path.join("trace_pipe").to_str().unwrap()).unwrap().as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK)
        };
        if trace_pipe_fd < 0 {
            panic!("Could not open trace pipe");
        }

        // Flush anything in the trace_pipe first so we know we're only going
        // to get blk data.
        {
            let trace_pipe_file = unsafe{
                libc::fdopen(trace_pipe_fd, b"rb\0".as_ptr() as *const i8)
            };
            // I'm assuming the stream can't ever half-write a data structure.
            unsafe{
                while libc::fgetc(trace_pipe_file) >= 0 {
                };
            };
        }

        let block_trace_enable_path = self.config.sys_path.join("dev/block").join(&format!("{}:{}", self.device.major, self.device.minor)).join("trace/enable");
        let old_block_trace_enable = slurp_file_at_path(&block_trace_enable_path).unwrap();
        let _block_trace_enable_setup = DoUndo::new(
            || {append_to_file_at_path(&block_trace_enable_path, b"1\n").unwrap();},
            || {append_to_file_at_path(&block_trace_enable_path, &old_block_trace_enable).unwrap();},
        );

        let continuing = Cell::new(true);

        // Returns bool for whether or not something was read.
        let consume_event = || {
            match BlkEvent::try_read_from_file(trace_pipe_fd) {
                None => {
                    false
                },
                Some(event) => {
                    // I could potentially filter by category (low bits of 
                    // action), but I'm unsure about race conditions.
                    if event.action & 0x00020000 == 0 {
                        // Was not a write operation, so we don't care.
                        return false;
                    }
                    if event.sector == 0 || event.bytes == 0 {
                        // There is no data location associated, so skip.
                        return false;
                    }
                    if event.device != self.device.event_dev {
                        // We're not looking at this device.
                        return false;
                    }

                    let first_byte: u64 = event.sector * 512; // I think a sector is always 512 on Linux?
                    let last_byte: u64 = first_byte + (event.bytes as u64) - 1;
                    let first_chunk: usize = (first_byte / (self.config.chunk_size as u64)) as usize;
                    let last_chunk: usize = (last_byte / (self.config.chunk_size as u64)) as usize;
                    for change_index in first_chunk..(last_chunk+1) {
                        if let Err(_) = log_channel.send(change_index) {
                            continuing.set(false);
                            return false;
                        }
                    }

                    true
                },
            }
        };
        while continuing.get() {
            match sync_barrier_channel.try_recv() {
                Ok(barrier) => {
                    eprintln!("Syncing...");
                    while consume_event() {}
                    barrier.wait();
                },
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // Does not block
                    if !consume_event() {
                        std::thread::yield_now();
                    }
                },
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    continuing.set(false);
                }
            };
        }
    }
}
