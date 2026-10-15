//! Unwired byte transport, not channel authorization. PRIVACY.md: no logs,
//! raw errors, environment, identifiers or private IPC in diagnostics.
//! FD3: one <4096-byte JSON config, terminated by EOF. FD4: one closed JSON
//! result, terminated by EOF. stdin/stdout: opaque application bytes after READY.
//! The official unmodified tor-socksproto decoder intentionally ignores RSV.
use serde::Deserialize;
use std::{
    io,
    net::{IpAddr, SocketAddr},
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, unix::AsyncFd},
    net::TcpStream,
};
use tor_socksproto::{
    Buffer, Handshake, NextStep, SocksAddr, SocksAuth, SocksClientHandshake, SocksCmd,
    SocksRequest, SocksStatus, SocksVersion,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    proxy_host: String,
    proxy_port: u16,
    host: String,
    port: u16,
    username: Option<String>,
    password: Option<String>,
    timeout_ms: u64,
}

// Each inherited descriptor is adopted exactly once. AsyncFd also supports real
// pipes, without Tokio's blocking-stdin task surviving a cancelled byte pump.
struct Pipe(AsyncFd<OwnedFd>);
impl Pipe {
    fn adopt(fd: i32) -> io::Result<Self> {
        // SAFETY: the launcher gives this process sole ownership of these FDs.
        let owned = unsafe { OwnedFd::from_raw_fd(fd) };
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: OwnedFd stays open and unchanged until AsyncFd drops it.
        Ok(Self(
            unsafe { AsyncFd::register(owned) }.map_err(io::Error::from)?,
        ))
    }
    async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            let mut ready = self.0.readable().await?;
            match ready.try_io(|fd| {
                // SAFETY: buf is writable for buf.len(), FD remains owned.
                let n = unsafe {
                    libc::read(fd.get_ref().as_raw_fd(), buf.as_mut_ptr().cast(), buf.len())
                };
                if n < 0 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(n as usize)
                }
            }) {
                Ok(result) => return result,
                Err(_) => continue,
            }
        }
    }
    async fn write_all(&self, mut buf: &[u8]) -> io::Result<()> {
        while !buf.is_empty() {
            let mut ready = self.0.writable().await?;
            match ready.try_io(|fd| {
                // SAFETY: buf is readable for buf.len(), FD remains owned.
                let n = unsafe {
                    libc::write(fd.get_ref().as_raw_fd(), buf.as_ptr().cast(), buf.len())
                };
                if n < 0 {
                    Err(io::Error::last_os_error())
                } else if n == 0 {
                    Err(io::ErrorKind::WriteZero.into())
                } else {
                    Ok(n as usize)
                }
            }) {
                Ok(result) => buf = &buf[result?..],
                Err(_) => continue,
            }
        }
        Ok(())
    }
}

async fn config(input: &Pipe) -> Result<Config, &'static str> {
    let mut bytes = [0u8; 4096];
    let mut len = 0;
    loop {
        if len == bytes.len() {
            return Err("configuration");
        }
        let n = input
            .read(&mut bytes[len..])
            .await
            .map_err(|_| "configuration")?;
        if n == 0 {
            break;
        }
        len += n;
    }
    serde_json::from_slice(&bytes[..len]).map_err(|_| "configuration")
}

fn request(c: &Config) -> Result<(SocketAddr, SocksRequest), &'static str> {
    let ip: IpAddr = c.proxy_host.parse().map_err(|_| "configuration")?;
    if !ip.is_loopback()
        || c.proxy_port == 0
        || c.port == 0
        || !(1..=300_000).contains(&c.timeout_ms)
        || c.host.is_empty()
        || !c.host.is_ascii()
        || c.host.bytes().any(|b| b <= 32 || b >= 127)
    {
        return Err("configuration");
    }
    let auth = match (&c.username, &c.password) {
        (None, None) => SocksAuth::NoAuth,
        (Some(u), Some(p)) if !u.is_empty() && !p.is_empty() => {
            SocksAuth::Username(u.as_bytes().to_vec(), p.as_bytes().to_vec())
        }
        _ => return Err("configuration"),
    };
    // No destination resolution: even numeric destination text uses Hostname.
    let host = c.host.clone().try_into().map_err(|_| "configuration")?;
    let req = SocksRequest::new(
        SocksVersion::V5,
        SocksCmd::CONNECT,
        SocksAddr::Hostname(host),
        c.port,
        auth,
    )
    .map_err(|_| "configuration")?;
    Ok((SocketAddr::new(ip, c.proxy_port), req))
}

fn sdk_failure(error: tor_socksproto::Error) -> &'static str {
    use tor_socksproto::Error as E;
    match error {
        E::Syntax | E::Decode(_) | E::BadProtocol(_) => "protocol",
        E::AuthRejected => "authentication",
        E::NotImplemented(_) => "unsupported",
        E::MessageTooLong { .. } => "handshake_bound",
        E::UnexpectedEof => "truncated",
        _ => "handshake_unknown",
    }
}

async fn establish(addr: SocketAddr, req: SocksRequest) -> Result<TcpStream, &'static str> {
    let mut socket = TcpStream::connect(addr).await.map_err(|_| "proxy")?;
    let mut hs = SocksClientHandshake::new(req);
    // Supported precise-read API leaves all application bytes on the socket.
    let mut buf = Buffer::new_precise();
    loop {
        match hs.step(&mut buf).map_err(sdk_failure)? {
            NextStep::Send(bytes) => socket.write_all(&bytes).await.map_err(|_| "handshake_io")?,
            NextStep::Recv(mut recv) => {
                let n = socket.read(recv.buf()).await.map_err(|_| "handshake_io")?;
                recv.note_received(n).map_err(sdk_failure)?;
            }
            NextStep::Finished(finished) => {
                let reply = finished.into_output().map_err(|_| "handshake_unknown")?;
                if reply.status() != SocksStatus::SUCCEEDED {
                    return Err("tunnel");
                }
                return Ok(socket);
            }
        }
    }
}

async fn bridge(input: &Pipe, output: &Pipe, socket: &mut TcpStream) -> io::Result<()> {
    let (mut rx, mut tx) = socket.split();
    let upload = async {
        let mut buf = [0u8; 16 * 1024];
        loop {
            let n = input.read(&mut buf).await?;
            if n == 0 {
                return Ok(());
            }
            tx.write_all(&buf[..n]).await?;
        }
    };
    let download = async {
        let mut buf = [0u8; 16 * 1024];
        loop {
            let n = rx.read(&mut buf).await?;
            if n == 0 {
                return Ok(());
            }
            output.write_all(&buf[..n]).await?;
        }
    };
    // EOF or error in either direction cancels the other, then the owner drops
    // the entire TCP socket. Backpressure alone is NOT a disconnect/deadline.
    tokio::select! { result = upload => result, result = download => result }
}

async fn run(status: Pipe) -> Result<(), &'static str> {
    let input = Pipe::adopt(0).map_err(|_| "ipc_unknown")?;
    let output = Pipe::adopt(1).map_err(|_| "ipc_unknown")?;
    let config_fd = Pipe::adopt(3).map_err(|_| "configuration")?;
    let setup = async {
        let c = tokio::time::timeout(Duration::from_secs(30), config(&config_fd))
            .await
            .map_err(|_| "deadline")??;
        drop(config_fd);
        let (addr, req) = request(&c)?;
        tokio::time::timeout(Duration::from_millis(c.timeout_ms), establish(addr, req))
            .await
            .map_err(|_| "deadline")?
    };
    let mut probe = [0u8; 1];
    let result = tokio::select! {
        result = setup => result,
        observed = input.read(&mut probe) => match observed {
            Ok(0) => Err("cancelled"),
            Ok(_) => Err("early_data"),
            Err(_) => Err("ipc_unknown"),
        },
    };
    let mut socket = match result {
        Ok(socket) => socket,
        Err(kind) => {
            let record = format!("{{\"failure\":\"{kind}\"}}\n");
            let _ = status.write_all(record.as_bytes()).await;
            return Err(kind);
        }
    };
    status
        .write_all(b"{\"ready\":true}\n")
        .await
        .map_err(|_| "ipc_unknown")?;
    drop(status);
    let result = bridge(&input, &output, &mut socket).await;
    drop(socket); // Closes both halves, even while one copy was backpressured.
    result.map_err(|_| "bridge_unknown")
}

fn main() {
    // No panic payload, SDK cause or configuration is emitted on any descriptor.
    std::panic::set_hook(Box::new(|_| {}));
    // A broken output pipe is an ordinary owned-copy failure, not SIGPIPE loss.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let result = std::panic::catch_unwind(|| {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| ())?;
        rt.block_on(async {
            let status = Pipe::adopt(4).map_err(|_| ())?;
            run(status).await.map_err(|_| ())
        })
    });
    if !matches!(result, Ok(Ok(()))) {
        std::process::exit(1);
    }
}
