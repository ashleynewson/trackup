use std::cell::Cell;
use std::collections::{HashMap,BTreeSet};
use std::sync::mpsc::{Receiver,Sender};
use std::sync::{Arc,Barrier};
use std::ffi::CString;
use libc::{c_char,c_int,c_void,ssize_t,size_t};
use crate::device::Device;
use crate::quick_io::{append_to_file_at_path,slurp_file_at_path,fd_poll_read};
use crate::control::{Config,Manifest};

trait WarnIfErr {
    fn warn_if_err(&self);
}
impl<T,E: std::fmt::Debug> WarnIfErr for Result<T,E> {
    fn warn_if_err(&self) {
        if let Err(e) = self {
            eprintln!("Warning: {:?}", e);
        }
    }
}


// This needs to be read directly from a file.
#[derive(Copy,Clone,Debug)]
#[repr(C, packed)]
struct BlkEvent {
    magic:    u32, /* MAGIC << 8 | version */
    sequence: u32, /* event number */
    time:     u64, /* in nanoseconds */
    sector:   u64, /* disk offset */
    bytes:    u32, /* transfer length */
    action:   u32, /* what happened (a 16 high-bit field for category and then a 16 low-bit enum for action) */
    pid:      u32, /* who did it */
    device:   u32, /* device identifier (major is 12 high-bits then minor is 20 low-bits */
    cpu:      u32, /* on what cpu did it happen */
    error:    u16, /* completion error */
    pdu_len:  u16, /* length of data after this trace */
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
            let mut event = ::std::mem::MaybeUninit::<BlkEvent>::uninit();
            let buffer = ::std::slice::from_raw_parts_mut(event.as_mut_ptr() as *mut u8, event_size);
            let bytes_read = libc::read(trace_pipe_fd, buffer.as_mut_ptr() as *mut c_void, event_size as size_t);
            if bytes_read == event_size as ssize_t {
                event.assume_init()
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


pub fn run(config: &Config, manifest: &Manifest, devices: &Vec<Device>, log_channel: Sender<(usize, usize)>, sync_barrier_channel: Receiver<Arc<Barrier>>) {
    let mut device_map: HashMap<u32, HashMap<&Device, usize>> = HashMap::new();
    for (i, device) in devices.iter().enumerate() {
        let base_device_event_dev = device.get_base_device().event_dev;
        if !device_map.contains_key(&base_device_event_dev) {
            device_map.insert(base_device_event_dev, HashMap::new());
        }
        if let Some(_) = device_map.get_mut(&base_device_event_dev).unwrap().insert(device, i) {
            panic!("Duplicate device found");
        }
    }
    let device_map = device_map; // Drop mutability
    let whole_disk_devices: Vec<&Device> =
        devices
        .iter()
        .map(|device| {device.get_base_device()})
        .collect::<BTreeSet<&Device>>() // Deduplicate and sort
        .into_iter()
        .collect();

    {
        let events_enabled = slurp_file_at_path(&config.tracing_path.join("events/enable")).unwrap();
        if std::str::from_utf8(&events_enabled).unwrap() != "0\n" {
            panic!("Some tracing events are already enabled");
        }
    }

    let old_current_tracer = slurp_file_at_path(&config.tracing_path.join("current_tracer")).unwrap();
    let _current_tracer_setup = DoUndo::new(
        || {append_to_file_at_path(&config.tracing_path.join("current_tracer"), b"blk\n").unwrap();},
        || {append_to_file_at_path(&config.tracing_path.join("current_tracer"), &old_current_tracer).warn_if_err();},
    );

    let old_tracer_option_bin = slurp_file_at_path(&config.tracing_path.join("options/bin")).unwrap();
    let _tracer_option_bin_setup = DoUndo::new(
        || {append_to_file_at_path(&config.tracing_path.join("options/bin"), b"1\n").unwrap();},
        || {append_to_file_at_path(&config.tracing_path.join("options/bin"), &old_tracer_option_bin).warn_if_err();},
    );

    let old_tracer_option_context = slurp_file_at_path(&config.tracing_path.join("options/context-info")).unwrap();
    let _tracer_option_context = DoUndo::new(
        || {append_to_file_at_path(&config.tracing_path.join("options/context-info"), b"0\n").unwrap();},
        || {append_to_file_at_path(&config.tracing_path.join("options/context-info"), &old_tracer_option_context).warn_if_err();},
    );

    let old_buffer_size = slurp_file_at_path(&config.tracing_path.join("buffer_size_kb")).unwrap();
    let _buffer_size = DoUndo::new(
        || {append_to_file_at_path(&config.tracing_path.join("buffer_size_kb"), format!("{}\n", config.trace_buffer_size).as_bytes()).unwrap();},
        || {append_to_file_at_path(&config.tracing_path.join("buffer_size_kb"), &old_buffer_size).warn_if_err();},
    );

    let trace_pipe_fd = unsafe {
        libc::open(CString::new(config.tracing_path.join("trace_pipe").to_str().unwrap()).unwrap().as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK)
    };
    if trace_pipe_fd < 0 {
        panic!("Could not open trace pipe");
    }

    // Flush anything in the trace_pipe first so we know we're only going
    // to get blk data.
    {
        let trace_pipe_file = unsafe{
            libc::fdopen(trace_pipe_fd, b"rb\0".as_ptr() as *const c_char)
        };
        // I'm assuming the stream can't ever half-write a data structure.
        unsafe{
            while libc::fgetc(trace_pipe_file) >= 0 {
            };
        };
    }

    // Use whole disk devices, as they're unique, and they'll give us good defaults.
    let old_block_trace_enables: Vec<(Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>)> = whole_disk_devices.iter().map(
        |device| {
            (
                slurp_file_at_path(&device.sys_dev_path.join("trace/act_mask")).unwrap(),
                slurp_file_at_path(&device.sys_dev_path.join("trace/start_lba")).unwrap(),
                slurp_file_at_path(&device.sys_dev_path.join("trace/end_lba")).unwrap(),
                slurp_file_at_path(&device.sys_dev_path.join("trace/enable")).unwrap()
            )
        }
    ).collect();
    let _block_trace_enable_setup = DoUndo::new(
        || {
            for device in &whole_disk_devices {
                append_to_file_at_path(&device.sys_dev_path.join("trace/act_mask"), b"queue\n").unwrap();
                append_to_file_at_path(&device.sys_dev_path.join("trace/start_lba"), b"0\n").unwrap();
                append_to_file_at_path(&device.sys_dev_path.join("trace/end_lba"), format!("{}\n", device.end_sector).as_bytes()).unwrap();
                append_to_file_at_path(&device.sys_dev_path.join("trace/enable"), b"1\n").unwrap();
            }
        },
        || {
            for (device, old_block_trace_enable) in whole_disk_devices.iter().zip(&old_block_trace_enables) {
                let (act_mask, start_lba, end_lba, _) = old_block_trace_enable;
                append_to_file_at_path(&device.sys_dev_path.join("trace/end_lba"), &end_lba).warn_if_err();
                append_to_file_at_path(&device.sys_dev_path.join("trace/start_lba"), &start_lba).warn_if_err();
                append_to_file_at_path(&device.sys_dev_path.join("trace/act_mask"), &act_mask).warn_if_err();
            }
            for (device, old_block_trace_enable) in whole_disk_devices.iter().zip(&old_block_trace_enables) {
                let (_, _, _, enable) = old_block_trace_enable;
                append_to_file_at_path(&device.sys_dev_path.join("trace/enable"), &enable).unwrap();
            }
        }
    );

    let continuing = Cell::new(true);

    // Returns bool for whether or not something was read.
    let consume_event = || {
        match BlkEvent::try_read_from_file(trace_pipe_fd) {
            None => {
                false
            },
            Some(event) => {
                let category = event.action >> 16;
                let action = event.action & 0xffff;
                let absolute_sector: u64 = event.sector;
                let bytes: u64 = event.bytes as u64;

                if category & 0x0002 == 0 {
                    // Was not a write operation, so we don't care.
                    return true;
                }
                if action != 1 {
                    // Was not a QUEUE action.
                    return true;
                }
                if bytes == 0 {
                    // There is no data location associated, so skip.
                    return true;
                }
                let event_dev = event.device;

                if let Some(child_devices) = device_map.get(&event_dev) {
                    // child_device may contain both a whole disk AND partitions.
                    for (device, device_number) in child_devices {
                        if device.start_sector <= absolute_sector && absolute_sector < device.end_sector {
                            let chunk_size = manifest.jobs[*device_number].chunk_size as u64;
                            let relative_sector: u64 = absolute_sector - device.start_sector;
                            let first_byte: u64 = relative_sector * 512; // I think a sector is always 512 on Linux?
                            let last_byte: u64 = first_byte + bytes - 1;
                            let first_chunk: usize = (first_byte / chunk_size) as usize;
                            let last_chunk: usize = (last_byte / chunk_size) as usize;

                            if last_byte >= device.sector_count * 512 {
                                // This might be violated if we're tracing a partition whilst a whole disk is modified!
                                // As such, this should not panic, but a warning may be useful.
                                eprintln!("Traced operation extends beyond end of device. This may happen if a device has been extended, or if a whole disk is modified whilst a partition is being traced. Event is from {} to {}, but matched device ({}:{}) is from {} to {}. Event: {:?}", absolute_sector, absolute_sector + bytes/512, device.major, device.minor, device.start_sector, device.end_sector, event);
                            }

                            for change_index in first_chunk..(last_chunk+1) {
                                if let Err(_) = log_channel.send((*device_number, change_index)) {
                                    continuing.set(false);
                                    return true;
                                }
                            }
                            // We might be tracing both a whole disk AND a partition, so don't break!
                        }
                    }
                };
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
