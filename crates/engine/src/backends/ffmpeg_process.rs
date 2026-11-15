//! Unix child isolation. Windows needs a Job Object before encoding is enabled.
use std::{
    fs::File,
    os::fd::AsRawFd,
    process::{Child, Command},
};

#[cfg(unix)]
pub(super) fn configure(command: &mut Command, output: &File) {
    use std::os::unix::process::CommandExt;
    let fd = output.as_raw_fd();
    command.process_group(0);
    // SAFETY: dup2/fcntl are async-signal-safe; the file remains alive until spawn.
    // Descriptor 3 is private to this child and is the sole encoding destination.
    unsafe {
        command.pre_exec(move || {
            let limit = libc::rlimit {
                rlim_cur: 512 * 1024 * 1024,
                rlim_max: 512 * 1024 * 1024,
            };
            if libc::setrlimit(libc::RLIMIT_FSIZE, &limit) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::dup2(fd, 3) < 0 || libc::fcntl(3, libc::F_SETFD, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

pub(super) fn signal(child: &Child, force: bool) {
    #[cfg(unix)]
    // SAFETY: every encoder is spawned into its own process group.
    unsafe {
        libc::kill(
            -(child.id() as i32),
            if force { libc::SIGKILL } else { libc::SIGTERM },
        );
    }
}
