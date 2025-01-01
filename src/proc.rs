// SPDX-FileCopyrightText: 2024 Christoph Sax <c_sax@mailbox.org>
//
// SPDX-License-Identifier: AGPL-3.0-or-later
//

use libc::kill;
use std::io::Error;

#[derive(Debug, PartialEq)]
enum ProcState {
    Alive,
    Dead,
    Unknown,
}

fn proc_alive(pid: u32) -> ProcState {
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

    let errno = Error::last_os_error().raw_os_error().expect("Call to kill() should have set errno.");
    assert_ne!(errno, libc::EINVAL);
    if errno == libc::ESRCH {
        ProcState::Dead
    } else {
        // EPERM: No permission to send signal
        ProcState::Unknown
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
