//! Passing a file descriptor over a Unix socket (`SCM_RIGHTS`).
//!
//! Used once per attach between `vtermd` and a session's fd holder; never on
//! the byte path.

use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;

/// Send `fd` together with `payload` (at least one byte is required by the
/// kernel; the payload is the header line the receiver parses).
pub fn send_fd(sock: &UnixStream, fd: &impl AsRawFd, payload: &[u8]) -> io::Result<()> {
    if payload.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "SCM_RIGHTS needs at least one payload byte",
        ));
    }
    let mut iov = libc::iovec {
        iov_base: payload.as_ptr().cast_mut().cast(),
        iov_len: payload.len(),
    };
    let space = cmsg_space();
    let mut cbuf = vec![0u8; space];
    let raw_fd: libc::c_int = fd.as_raw_fd();
    // SAFETY: all pointers refer to live buffers sized per CMSG_SPACE/CMSG_LEN.
    unsafe {
        let mut msg: libc::msghdr = std::mem::zeroed();
        msg.msg_iov = &raw mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = cbuf.as_mut_ptr().cast();
        msg.msg_controllen = cmsg_len_u32(space);
        let cmsg = libc::CMSG_FIRSTHDR(&raw const msg);
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len = cmsg_len_u32(libc::CMSG_LEN(fd_size()) as usize);
        std::ptr::copy_nonoverlapping(
            (&raw const raw_fd).cast::<u8>(),
            libc::CMSG_DATA(cmsg),
            fd_size() as usize,
        );
        let n = libc::sendmsg(sock.as_raw_fd(), &raw const msg, 0);
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        if usize::try_from(n).ok() != Some(payload.len()) {
            return Err(io::Error::new(io::ErrorKind::WriteZero, "short sendmsg"));
        }
    }
    Ok(())
}

/// Receive one fd and up to `max_payload` bytes of payload.
pub fn recv_fd(sock: &UnixStream, max_payload: usize) -> io::Result<(OwnedFd, Vec<u8>)> {
    let mut payload = vec![0u8; max_payload.max(1)];
    let mut iov = libc::iovec {
        iov_base: payload.as_mut_ptr().cast(),
        iov_len: payload.len(),
    };
    let space = cmsg_space();
    let mut cbuf = vec![0u8; space];
    // SAFETY: as in send_fd; the kernel fills the buffers we sized.
    let (n, fd) = unsafe {
        let mut msg: libc::msghdr = std::mem::zeroed();
        msg.msg_iov = &raw mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = cbuf.as_mut_ptr().cast();
        msg.msg_controllen = cmsg_len_u32(space);
        let n = libc::recvmsg(sock.as_raw_fd(), &raw mut msg, 0);
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "peer closed before sending the fd",
            ));
        }
        let cmsg = libc::CMSG_FIRSTHDR(&raw const msg);
        if cmsg.is_null()
            || (*cmsg).cmsg_level != libc::SOL_SOCKET
            || (*cmsg).cmsg_type != libc::SCM_RIGHTS
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "message carried no SCM_RIGHTS fd",
            ));
        }
        let mut raw: libc::c_int = -1;
        std::ptr::copy_nonoverlapping(
            libc::CMSG_DATA(cmsg),
            (&raw mut raw).cast::<u8>(),
            fd_size() as usize,
        );
        if raw < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid fd received",
            ));
        }
        (usize::try_from(n).unwrap_or(0), OwnedFd::from_raw_fd(raw))
    };
    payload.truncate(n);
    Ok((fd, payload))
}

const fn fd_size() -> u32 {
    // c_int is 4 bytes on every target we build for; checked at compile time.
    const { assert!(std::mem::size_of::<libc::c_int>() == 4) };
    4
}

fn cmsg_space() -> usize {
    // SAFETY: pure arithmetic macro.
    unsafe { libc::CMSG_SPACE(fd_size()) as usize }
}

#[allow(clippy::cast_possible_truncation)]
fn cmsg_len_u32(len: usize) -> libc::socklen_t {
    len as libc::socklen_t
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::fd::AsFd;

    #[test]
    fn passes_a_pipe_end_across_a_socketpair() {
        let (a, b) = UnixStream::pair().unwrap();
        let (mut reader, writer) = std::io::pipe().unwrap();
        send_fd(&a, &writer.as_fd(), b"{\"hello\":1}\n").unwrap();
        let (fd, payload) = recv_fd(&b, 64).unwrap();
        assert_eq!(payload, b"{\"hello\":1}\n");
        // The received fd is a working duplicate of the pipe's write end.
        let mut dup = std::fs::File::from(fd);
        dup.write_all(b"via passed fd").unwrap();
        drop(dup);
        drop(writer);
        let mut got = String::new();
        reader.read_to_string(&mut got).unwrap();
        assert_eq!(got, "via passed fd");
    }
}
