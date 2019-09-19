use std::path::Path;
use std::fs::OpenOptions;
use std::os::raw::c_int;
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
