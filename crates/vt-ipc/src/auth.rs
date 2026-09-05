//! Peer authentication for the Unix socket.
//!
//! The socket lives in a `0700` directory, which already keeps other users
//! out; `getpeereid` is the second lock so a misconfigured directory cannot
//! silently open the daemon to another account. The loopback HTTP listener
//! uses a Keychain-held bearer token instead (M2).

use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;

use crate::error::IpcError;

/// Our effective uid.
pub fn own_uid() -> u32 {
    // SAFETY: geteuid cannot fail and touches no memory.
    unsafe { libc::geteuid() }
}

/// The peer's effective uid, via `getpeereid(2)`.
pub fn peer_uid(stream: &UnixStream) -> std::io::Result<u32> {
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: valid fd and out-pointers.
    let r = unsafe { libc::getpeereid(stream.as_raw_fd(), &raw mut uid, &raw mut gid) };
    if r == 0 {
        Ok(uid)
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Reject any peer that is not us.
pub fn check_peer(stream: &UnixStream) -> Result<(), IpcError> {
    let peer = peer_uid(stream)?;
    let owner = own_uid();
    if peer == owner {
        Ok(())
    } else {
        Err(IpcError::Unauthorized {
            peer_uid: peer,
            owner_uid: owner,
        })
    }
}
