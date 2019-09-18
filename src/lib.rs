extern crate libc;
extern crate nix;
extern crate crossbeam;

mod alias_tree;
mod chunk;
pub mod config;
mod device;
mod backup_file;
mod chunk_tracker;
mod change_logger;
mod writer;
mod copier;
mod quick_io;

pub fn backup_device(config: &config::Config) -> Result<(),()> {
    nix::sys::mman::mlockall(nix::sys::mman::MlockAllFlags::all()).expect("Could not mlock pages in RAM. (Are you root?)");

    let mut device_file = device::DeviceFile::from_path(config.source).expect("Could not open device");
    let mut backup_file = backup_file::BackupFile::create_file(config.destination, device_file.get_size()).expect("Could not open backup file");
    let mut copier = copier::Copier::new(config, &mut device_file, &mut backup_file);
    copier.run()
}
