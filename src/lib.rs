extern crate libc;
extern crate nix;
extern crate crossbeam;

mod alias_tree;
mod chunk;
pub mod job;
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

    let mut device_files = Vec::new();
    let mut backup_files = Vec::new();
    for job in config.jobs {
        let device_file = device::DeviceFile::from_path(&job.source).expect("Could not open device");
        backup_files.push(
            if config.reuse_output {
                backup_file::BackupFile::use_file(&job.destination, device_file.get_size()).expect("Could not open backup file (reuse)")
            } else {
                backup_file::BackupFile::create_file(&job.destination, device_file.get_size()).expect("Could not open backup file (create)")
            }
        );
        device_files.push(device_file);
    }
    let mut copier = copier::Copier::new(config, &mut device_files, &mut backup_files);
    copier.run()
}
