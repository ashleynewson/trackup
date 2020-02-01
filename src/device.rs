use std::path::{Path,PathBuf};
use std::fs::File;
use std::io::{Read,Seek,SeekFrom};
use std::ffi::CString;
use libc::{c_uint,dev_t};
use crate::chunk::Chunk;
use crate::control::Config;
use crate::quick_io::{slurp_file_at_path,slurp_and_parse_file_at_path};

pub struct Device {
    pub dev: dev_t,
    pub event_dev: u32,
    pub major: c_uint,
    pub minor: c_uint,
    pub sys_dev_path: PathBuf,
    pub sector_count: u64,
    pub start_sector: u64,
    pub end_sector: u64,
    pub parent: Option<Box<Device>>, // If our device is a partition, this will represent the whole-disk.
}

pub struct DeviceFile {
    path: PathBuf,
    size: u64,
    file: File,
    // fd: RawFd,
}

impl Device {
    pub fn from_file(config: &Config, device_file: &DeviceFile) -> Result<Self, String> {
        let cpath = CString::new(device_file.path.to_str().unwrap()).unwrap();
        let stat_result = {
            let mut stat_result = ::std::mem::MaybeUninit::<libc::stat>::uninit();
            unsafe {
                if libc::stat(cpath.as_ptr(), stat_result.as_mut_ptr()) < 0 {
                    return Err(format!("Could not stat device"));
                }
                stat_result.assume_init()
            }
        };

        let major = unsafe{libc::major(stat_result.st_rdev)};
        let minor = unsafe{libc::minor(stat_result.st_rdev)};

        if major == 0 {
            return Err(format!("Target device does not actually appear to be a device."));
        }

        // dev_t is an opaque datatype. In fact, its size isn't even
        // guaranteed to be consistent. However, we're always going to
        // be getting a u32 in blk events, and that is a specific format
        // which isn't obviously documented. However,in the source code
        // for blktrace/blkparse, the major and minor device numbers are
        // are handled using following macros:
        //   #define MINORBITS   20
        //   #define MINORMASK   ((1U << MINORBITS) - 1)
        //   #define MAJOR(dev)  ((unsigned int) ((dev) >> MINORBITS))
        //   #define MINOR(dev)  ((unsigned int) ((dev) & MINORMASK))
        // Which is about as good a confirmation I can find for the
        // format of blk event device codes.
        if major >= (1 << 12) {
            panic!("Major device number exceeds limits for tracing");
        }
        if minor >= (1 << 20) {
            panic!("Minor device number exceeds limits for tracing");
        }
        Self::from_major_minor(config, major, minor)
    }

    pub fn from_major_minor(config: &Config, major: c_uint, minor: c_uint) -> Result<Self,String> {
        let dev: dev_t = unsafe {libc::makedev(major, minor)};
        let event_dev: u32 = (major << 20) | minor;
        let sys_dev_path = config.sys_path.join("dev/block").join(&format!("{}:{}", major, minor));
        let sector_count = slurp_and_parse_file_at_path(&sys_dev_path.join("size")).unwrap();
        let is_partition = sys_dev_path.join("partition").exists();
        let start_sector: u64 =
            if is_partition {
                // This device doesn't cover the entire lba space (i.e. a partition)
                slurp_and_parse_file_at_path(&sys_dev_path.join("start")).unwrap()
            } else {
                // In theory, this is a device which covers an entire lba space
                0
            };
        let end_sector: u64 = start_sector + sector_count;
        let parent =
            if is_partition {
                let parent_major_minor_buf = slurp_file_at_path(&sys_dev_path.join("../dev")).unwrap();
                let mut parent_major_minor =
                    std::str::from_utf8(&parent_major_minor_buf[0..parent_major_minor_buf.len()-1])
                    .unwrap()
                    .split(":")
                    .map(|string| {string.parse().unwrap()});
                let parent_major: c_uint = parent_major_minor.next().unwrap();
                let parent_minor: c_uint = parent_major_minor.next().unwrap();
                Some(Box::new(Self::from_major_minor(config, parent_major, parent_minor).unwrap()))
            } else {
                None
            };

        Ok(Self{
            dev,
            event_dev,
            major,
            minor,
            sys_dev_path,
            sector_count,
            start_sector,
            end_sector,
            parent,
        })
    }

    /// Return the ultimate ancestor (i.e. the device representing the whole disk)
    pub fn get_base_device<'s>(&'s self) -> &'s Device {
        // Will there ever be more than one level?
        match &self.parent {
            None => {
                self
            },
            Some(parent) => {
                parent.get_base_device()
            },
        }
    }
}

impl std::cmp::PartialEq for Device {
    fn eq(&self, other: &Self) -> bool {
        self.dev == other.dev
    }
}
impl std::cmp::Eq for Device {}
impl std::cmp::Ord for Device {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.dev.cmp(&other.dev)
    }
}
impl std::cmp::PartialOrd for Device {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl std::hash::Hash for Device {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.dev.hash(state);
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
        file.seek(SeekFrom::Start(0)).expect("Could not seek back to beginning of device file");

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
        self.file.seek(SeekFrom::Start(offset)).expect("Could not seek device file");
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
