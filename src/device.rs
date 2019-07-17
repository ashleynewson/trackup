use std::path::{Path,PathBuf};
use std::fs::File;
use libc::{c_uint,dev_t};
use std::io::{Read,Seek,SeekFrom};
use std::ffi::CString;
use chunk::Chunk;

pub struct Device {
    pub dev: dev_t,
    pub major: c_uint,
    pub minor: c_uint,
}

pub struct DeviceFile {
    path: PathBuf,
    size: u64,
    file: File,
    // fd: RawFd,
}

impl Device {
    pub fn from_file(device_file: &DeviceFile) -> Result<Self, String> {
        let cpath = CString::new(device_file.path.to_str().unwrap()).unwrap();
        let stat_result = unsafe {
            let mut stat_result: libc::stat = ::std::mem::uninitialized();
            if libc::stat(cpath.as_ptr(), &mut stat_result as *mut libc::stat) < 0 {
                return Err(format!("Could not stat device"));
            }
            stat_result
        };

        let major = unsafe{libc::major(stat_result.st_dev)};
        let minor = unsafe{libc::minor(stat_result.st_dev)};

        if major == 0 {
            return Err(format!("Target device does not actually appear to be a device"));
        }

        Ok(Self{
            dev: stat_result.st_dev,
            major,
            minor,
        })
    }
}

impl DeviceFile {
    pub fn from_path(path: &Path) -> Result<Self, String> {
        let mut file = match File::open(path) {
            Ok(x) => x,
            Err(_) => {
                return Err(format!("Could not open device file"));
            },
        };
        // let fd = file.try_clone().unwrap().into_raw_fd();

        let size = file.seek(SeekFrom::End(0)).expect("Could not determine device size");
        file.seek(SeekFrom::Start(0)).expect("Could not seek back to beginning of device file");;

        Ok(Self{
            path: path.to_path_buf(),
            size,
            file,
            // fd,
        })
    }

    pub fn get_chunk(&mut self, offset: u64, size: usize) -> Chunk {
        if offset >= self.size {
            panic!("Offset is out of bounds for device");
        }
        let capped_size: usize =
            if offset + (size as u64) > self.size {
                (self.size - offset) as usize
            } else {
                size
            };
        let mut data: Vec<u8> = Vec::with_capacity(capped_size);
        unsafe{data.set_len(capped_size)};
        self.file.seek(SeekFrom::Start(offset)).expect("Could not seek device file");;
        self.file.read_exact(&mut data).expect("Error reading from device file");
        Chunk {
            offset,
            data,
        }
    }

    pub fn get_path(&self) -> &Path {
        self.path.as_path()
    }

    pub fn get_size(&self) -> u64 {
        self.size
    }
}

// impl Drop for Device {
//     fn drop(&mut self) {
//         self.set_trace_enabled(false);
//     }
// }