use std::convert::TryInto;
use std::path::Path;
use std::fs::OpenOptions;
use std::os::raw::c_int;
use std::os::unix::io::AsRawFd;
use std::io::{Read,Write};


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
