use std::path::Path;

extern crate libc;
extern crate nix;
extern crate crossbeam;

mod alias_tree;
mod chunk;
mod device;
mod backup_file;
mod chunk_tracker;
mod change_logger;
mod writer;
mod copier;
mod quick_io;

pub fn backup_device(chunk_size: usize, source: &Path, destination: &Path) -> Result<(),()> {
    nix::sys::mman::mlockall(nix::sys::mman::MlockAllFlags::all()).expect("Could not mlock pages in RAM. (Are you root?)");

    let mut device_file = device::DeviceFile::from_path(source).expect("Could not open device");
    let mut backup_file = backup_file::BackupFile::create_file(destination, device_file.get_size()).expect("Could not open backup file");
    let mut copier = copier::Copier::new(chunk_size, &mut device_file, &mut backup_file);
    copier.run()
}
