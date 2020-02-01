use std::convert::TryInto;
use std::path::Path;
use std::fs::OpenOptions;
use std::os::raw::c_int;
use std::os::unix::io::AsRawFd;
use std::io::{Read,Write};
use byteorder::ByteOrder;


pub fn append_to_file_at_path(path: &Path, buf: &[u8]) -> Result<(),String> {
    let mut file = match OpenOptions::new().write(true).append(true).open(path) {
    // let mut file = match OpenOptions::new().write(true).append(true).open(path) {
        Ok(x) => x,
        Err(_) => {
            return Err(format!("Could not open '{}' for appending", path.display()));
        },
    };

    if let Err(_) = file.write_all(buf) {
        return Err(format!("Could not append data to '{}'", path.display()));
    }
    if let Err(_) = file.flush() {
        return Err(format!("Could not append data to '{}' (flush failed)", path.display()));
    }

    eprintln!("Appending to file '{}':\n{}", path.display(), std::str::from_utf8(buf).unwrap());
    Ok(())
}

pub fn slurp_file_at_path(path: &Path) -> Result<Vec<u8>, String> {
    let mut file = match OpenOptions::new().read(true).open(path) {
        Ok(x) => x,
        Err(_) => {
            return Err(format!("Could not open '{}' for reading", path.display()));
        },
    };

    let mut buf = Vec::new();
    if let Err(_) = file.read_to_end(&mut buf) {
        return Err(format!("Could not read data from '{}'", path.display()));
    }

    eprintln!("Slurped file '{}':\n{}", path.display(), std::str::from_utf8(&buf).unwrap());
    Ok(buf)
}

pub fn slurp_and_parse_file_at_path<T: std::str::FromStr>(path: &Path) -> Result<T, String> {
    let buf = slurp_file_at_path(path)?;
    match std::str::from_utf8(&buf[0..buf.len()-1]) {
        Ok(s) => {
            match s.parse::<T>() {
                Ok(p) => {
                    Ok(p)
                },
                Err(_) => {
                    Err(format!("Data '{}' from file '{}' could not be parsed", s, path.display()))
                }
            }
        },
        Err(_) => {
            Err(format!("Slurped file '{}' is not valid utf8", path.display()))
        }
    }
}

pub fn fd_poll_read(fd: c_int, timeout_ms: c_int) -> bool {
    let mut pollfd = libc::pollfd{
        fd: fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let poll_ret = unsafe {
        libc::poll(
            &mut pollfd as *mut libc::pollfd,
            1,
            timeout_ms,
        )
    };
    match poll_ret {
        0 => {
            false
        },
        1 => {
            pollfd.revents & libc::POLLIN != 0
        },
        _ => {
            panic!("Unexpected poll return status");
        }
    }
}
pub fn poll_read(file: &dyn AsRawFd, timeout: std::time::Duration) -> bool {
    fd_poll_read(file.as_raw_fd(), timeout.as_millis().try_into().unwrap())
}

pub fn assert_read(read: &mut dyn Read, expected: &[u8]) -> Result<(),()> {
    let mut actual: Vec<u8> = vec![0; expected.len()];
    if let Err(e) = read.read_exact(&mut actual) {
        eprintln!("Error trying to read an expected value: {:?}", e);
        return Err(());
    }
    if actual == expected {
        Ok(())
    } else {
        Err(())
    }
}

pub fn read_be_u64<R: Read>(read: &mut R) -> Result<u64,String> {
    let mut buf: [u8; 8] = [0; 8];
    if let Err(e) = read.read_exact(&mut buf) {
        return Err(format!("Could not read 8 bytes (for big endian u64): {:?}", e));
    }
    Ok(byteorder::BigEndian::read_u64(&buf))
}

pub fn write_be_u64<W: Write>(write: &mut W, value: u64) -> Result<(),String> {
    let mut buf: [u8; 8] = [0; 8];
    byteorder::BigEndian::write_u64(&mut buf, value);
    if let Err(e) = write.write_all(&buf) {
        return Err(format!("Could not write 8 bytes (for big endian u64): {:?}", e));
    }
    Ok(())
}

/// Read a skip-run format sparse data collection
///
/// skip_run_collection = skip_run_section*;
/// skip_run_section = skip_length, run_length, record*;
///
/// skip_length = big_endian_u64;
/// run_length  = big_endian_u64;
///
/// There are run_length records in a skip_run_section. The sum of all skips
/// and runs must equal the specified limit. Reading stops as soon as the
/// limit is reached. There should not be any redundant or fragmented
/// skip_run_sections, such as sections with both skip and run lengths of
/// zero, or two consecutive sections where the both the skip or both the run
/// are zero.
///
/// run_func will be called to read data for positions that are runs.
/// skip_func will be called for all positions that are skips.
pub fn read_skip_run<R: Read, F: FnMut(&mut R, u64)->Result<(),String>, G: FnMut(u64)->Result<(),String>>(read: &mut R, limit: u64, mut run_func: F, mut skip_func: G) -> Result<(),String> {
    let mut position: u64 = 0;
    while position < limit {
        let mut skip: u64 = read_be_u64(read)?;
        let mut run : u64 = read_be_u64(read)?;
        if skip.checked_add(run).expect("Skip+run overflow") > limit - position {
            return Err(format!("Skip-run values exceed final position"));
        }
        while skip > 0 {
            skip_func(position)?;
            position = position + 1;
            skip = skip - 1;
        }
        while run > 0 {
            run_func(read, position)?;
            position = position + 1;
            run = run - 1;
        }
    }
    Ok(())
}

/// Write a skip-run format sparse data collection (see read_skip_run for format)
///
/// run_func will be called to write data for positions that are selected by test_func.
pub fn write_skip_run<W: Write, F: FnMut(&mut W, u64)->Result<(),String>, T: FnMut(u64)->Result<bool,String>>(write: &mut W, limit: u64, mut run_func: F, mut test_func: T) -> Result<(),String> {
    let mut position: u64 = 0;
    while position < limit {
        let mut skip: u64 = 0;
        let mut run : u64  = 0;
        while position < limit && !test_func(position)? {
            skip = skip + 1;
            position = position + 1;
        }
        while position + run < limit && test_func(position + run)? {
            run = run + 1;
        }
        write_be_u64(write, skip)?;
        write_be_u64(write, run)?;
        while run > 0 {
            run_func(write, position)?;
            position = position + 1;
            run = run - 1;
        }
    }
    Ok(())
}

pub struct CountedWrite<W: Write> {
    write: W,
    count: u64,
}
impl<W: Write> CountedWrite<W> {
    pub fn new(write: W) -> Self {
        Self {
            write,
            count: 0,
        }
    }
    pub fn reset(&mut self) -> u64 {
        let count = self.count;
        self.count = 0;
        count
    }
    pub fn get_count(&self) -> u64 {
        self.count
    }
}
impl<W: Write> Write for CountedWrite<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let written = self.write.write(buf)?;
        self.count = self.count + (written as u64);
        Ok(written)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.write.flush()
    }
}
