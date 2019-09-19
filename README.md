# TrackUp

Create crash-consistent whole disk or raw partition backups without having to
use a copy-on-write snapshot.

Please read the advantages and especially disadvantages listed below before
attempting to use this software.

## How it works

TrackUp keeps track of modifications to block devices as they are being
copied. Data which has been modified during the copy process is marked as
dirty and will be copied again later. This process is repeated until the copy
holds an up-to-date mirror image of the original block device.

## Is TrackUp right for me?

### Advantages

- Can create a crash-consistent backup of a live running system.

- No copy-on-write snapshot file or partition is required, so trackup can be
  run regardless of available disk space or partitioning.

- Compatible with most modern Linux setups out-of-the-box. TrackUp uses the
  commonly available sysfs and kernel debugfs filesystems to track changes on
  block devices, so no additional kernel drivers are required.

- Back up to a file or another block device.

### Disadvantages (WARNING)

- Experimental software! Not tested as thoroughly as more mainstream backup
  solutions. It is a rather hacky program, and is best suited to casual use. I
  know that does not sound great for a backup program, but it seems to work
  for me.

- Cannot run concurrently with other instances of TrackUp, or any other
  program which uses kernel tracing features via debugfs
  (e.g. blktrace). Running multiple instances of this program at once will
  likely result in non-crash-consistent backups!

- Unlike other backup solutions, the backup is crash-consistent at the time
  the backup completes, not when it starts.

- Backups might never complete if there is a continual high rate of
  modifications and the copying process cannot keep up.

- No compression or incremental backups.
