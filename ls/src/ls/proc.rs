// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: EUPL-1.2
//

#[cfg(any(windows, unix))]
use std::io::Error;

#[cfg(unix)]
use libc::kill;

#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{HANDLE, STILL_ACTIVE},
    System::Threading::{GetExitCodeProcess, OpenProcess},
};

#[derive(Debug, PartialEq)]
pub enum ProcState {
    Alive,

    #[cfg(any(windows, unix))]
    Dead,

    #[cfg(any(windows, unix))]
    Unknown,
}

cfg_select! {
    unix => {
        pub fn proc_alive(pid: u32) -> ProcState {
            let proc = pid.try_into();
            // pid is out of range
            if let Err(_) = proc {
                return ProcState::Unknown;
            }

            unsafe {
                if kill(proc.unwrap(), 0) == 0 {
                    return ProcState::Alive;
                }
            }

            let errno = Error::last_os_error()
                .raw_os_error()
                .expect("Call to kill() should have set errno.");
            assert_ne!(errno, libc::EINVAL);
            if errno == libc::ESRCH {
                ProcState::Dead
            } else {
                // EPERM: No permission to send signal
                ProcState::Unknown
            }
        }
    }
    windows => {
        pub fn proc_alive(pid: u32) -> ProcState {
            const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
            const FALSE: i32 = 0;

            let handle = unsafe {
                let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, pid);
                if handle.is_null() {
                    return ProcState::Unknown;
                }
                h
            };

            let exit_code: u32 = 0;
            unsafe {
                if GetExitCodeProcess(handle, exit_code.as_mut_ptr()) == 0 {
                    return ProcState::Unknown;
                }
            }

            if exit_code == STILL_ACTIVE {
                ProcState::Alive
            } else {
                ProcState::Dead
            }
        }
    }
    all(target_os = "wasi", target_env = "p1") => {
        pub fn proc_alive(_pid: u32) -> ProcState {
            ProcState::Alive
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::process::Command;

    #[test]
    fn detects_pid_alive() {
        let mut child = Command::new("sleep").arg("2").spawn().unwrap();

        assert_eq!(proc_alive(child.id()), ProcState::Alive);

        child.kill().unwrap();
    }

    #[test]
    fn detects_pid_dead() {
        let mut child = Command::new("sleep").arg("0.1").spawn().unwrap();

        let pid = child.id();
        child.wait().unwrap();

        assert_eq!(proc_alive(pid), ProcState::Dead);
    }
}
