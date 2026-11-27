//! Connecting to a port on this machine, where a port nothing listens on is refused at once.
//!
//! On macOS and Linux a connect to a closed local port is refused at once. Windows instead
//! retries the refused connect twice before it gives up, about two seconds in all, so a helper
//! that finds its server gone would wait two seconds on every request. Here Windows is told
//! not to retry (`SIO_TCP_INITIAL_RTO` with no SYN retransmissions), so it is refused at once
//! there too.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

/// Connects to `127.0.0.1:port`, waiting at most `within`. A port nothing listens on fails at
/// once with [`io::ErrorKind::ConnectionRefused`] on every platform.
///
/// Example: `connect_local(4100, Duration::from_secs(5))` reaches a server listening on port
/// 4100; once that server has ended, the same call fails at once, refused.
pub fn connect_local(port: u16, within: Duration) -> io::Result<TcpStream> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    connect_without_retries(&address, within)
}

#[cfg(not(windows))]
fn connect_without_retries(address: &SocketAddr, within: Duration) -> io::Result<TcpStream> {
    TcpStream::connect_timeout(address, within)
}

#[cfg(windows)]
fn connect_without_retries(address: &SocketAddr, within: Duration) -> io::Result<TcpStream> {
    use socket2::{Domain, Protocol, Socket, Type};
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{
        SIO_TCP_INITIAL_RTO, TCP_INITIAL_RTO_NO_SYN_RETRANSMISSIONS, TCP_INITIAL_RTO_PARAMETERS,
        WSAIoctl,
    };

    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
    let no_retries = TCP_INITIAL_RTO_PARAMETERS {
        Rtt: u16::MAX, // TCP_INITIAL_RTO_UNSPECIFIED_RTT: the system's own first timeout
        // The C header's ((UCHAR)-2): the constant is declared wider than the field it fills.
        MaxSynRetransmissions: TCP_INITIAL_RTO_NO_SYN_RETRANSMISSIONS as u8,
    };
    let mut returned = 0u32;
    // SAFETY: the socket is open and owned here; the input buffer is a live value of the size
    // given; there is no output buffer, overlapped structure or completion routine.
    let _ = unsafe {
        WSAIoctl(
            socket.as_raw_socket() as usize,
            SIO_TCP_INITIAL_RTO,
            std::ptr::from_ref(&no_retries).cast(),
            size_of::<TCP_INITIAL_RTO_PARAMETERS>() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
            None,
        )
    }; // if the system refuses the option, the connect still works, only slower when refused
    socket.connect_timeout(&(*address).into(), within)?;
    Ok(socket.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::time::Instant;

    #[test]
    fn a_port_nothing_listens_on_is_refused_at_once() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let started = Instant::now();
        let refused = connect_local(port, Duration::from_secs(5)).unwrap_err();
        assert_eq!(
            refused.kind(),
            io::ErrorKind::ConnectionRefused,
            "{refused}"
        );
        // Windows' retries take about two seconds; one refusal takes milliseconds
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn a_listening_port_is_reached() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let echo = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&stream).read_line(&mut line).unwrap();
            (&stream).write_all(line.as_bytes()).unwrap();
        });
        let mut stream = connect_local(port, Duration::from_secs(5)).unwrap();
        writeln!(stream, "hello").unwrap();
        let mut line = String::new();
        BufReader::new(&stream).read_line(&mut line).unwrap();
        assert_eq!(line, "hello\n");
        echo.join().unwrap();
    }
}
