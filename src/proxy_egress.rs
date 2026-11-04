//! Loopback-only CONNECT出口。宛先は全DNS候補を検証してからIPへ固定する。
use std::{
    collections::HashSet,
    io,
    net::IpAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
    task::JoinSet,
    time::timeout,
};
use tokio_util::sync::CancellationToken;

pub(crate) fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            a > 0
                && a < 224
                && a != 10
                && a != 127
                && !(a == 100 && (64..=127).contains(&b))
                && !(a == 169 && b == 254)
                && !(a == 172 && (16..=31).contains(&b))
                && !(a == 192
                    && ((b == 0 && (c == 0 || c == 2)) || (b == 88 && c == 99) || b == 168))
                && !(a == 198 && (b == 18 || b == 19 || (b == 51 && c == 100)))
                && !(a == 203 && b == 0 && c == 113)
        }
        IpAddr::V6(ip) => {
            let [a, b, ..] = ip.segments();
            (0x2000..=0x3fff).contains(&a)
                && a != 0x2002
                && a != 0x3fff
                && !(a == 0x2001 && (b <= 0x01ff || b == 0x0db8))
        }
    }
}

pub(crate) fn target(header: &[u8]) -> Option<String> {
    if header
        .iter()
        .any(|&b| b > 126 || (b < 32 && ![9, 10, 13].contains(&b)))
    {
        return None;
    }
    let text = std::str::from_utf8(header).ok()?;
    let mut lines = text.split("\r\n");
    let first = lines.next()?;
    let fields: Vec<_> = first.split(' ').collect();
    if fields.len() != 3 || fields[0] != "CONNECT" || !["HTTP/1.0", "HTTP/1.1"].contains(&fields[2])
    {
        return None;
    }
    let host = fields[1].strip_suffix(":443")?.to_ascii_lowercase();
    if host.len() > 253
        || host.parse::<IpAddr>().is_ok()
        || !host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && label.as_bytes()[0].is_ascii_alphanumeric()
                && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
        })
        || !["x.com", "twitter.com", "twimg.com"]
            .iter()
            .any(|d| host == *d || host.ends_with(&format!(".{d}")))
    {
        return None;
    }
    let mut seen = HashSet::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let (name, value) = line.split_once(':')?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
        {
            return None;
        }
        let name = name.to_ascii_lowercase();
        if !seen.insert(name.clone())
            || name == "content-length"
            || name == "transfer-encoding"
            || (name == "host" && value.trim().to_ascii_lowercase() != format!("{host}:443"))
        {
            return None;
        }
    }
    Some(host)
}

async fn pump<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    mut read: R,
    mut write: W,
    activity: tokio::sync::watch::Sender<tokio::time::Instant>,
) -> io::Result<()> {
    let mut bytes = [0; 16 * 1024];
    loop {
        let n = read.read(&mut bytes).await?;
        if n == 0 {
            write.shutdown().await?;
            return Ok(());
        }
        activity.send_replace(tokio::time::Instant::now());
        let mut offset = 0;
        while offset < n {
            let written = write.write(&bytes[offset..n]).await?;
            if written == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }
            offset += written;
            activity.send_replace(tokio::time::Instant::now());
        }
    }
}

pub(crate) async fn relay<A, B>(left: &mut A, right: &mut B) -> io::Result<()>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    relay_prefixed(left, right, &[]).await
}
async fn relay_prefixed<A, B>(left: &mut A, right: &mut B, prefix: &[u8]) -> io::Result<()>
where
    A: AsyncRead + AsyncWrite + Unpin,
    B: AsyncRead + AsyncWrite + Unpin,
{
    let (lr, lw) = tokio::io::split(left);
    let lr = std::io::Cursor::new(prefix).chain(lr);
    let (rr, rw) = tokio::io::split(right);
    let (activity, mut clock) = tokio::sync::watch::channel(tokio::time::Instant::now());
    timeout(Duration::from_secs(600), async {
        tokio::select!{
            result=async{tokio::try_join!(pump(lr,rw,activity.clone()),pump(rr,lw,activity))?;Ok(())}=>result,
            _=async{loop{
                let deadline=*clock.borrow()+Duration::from_secs(60);
                tokio::select!{
                    _=tokio::time::sleep_until(deadline)=>{if tokio::time::Instant::now()>=*clock.borrow()+Duration::from_secs(60){return;}},
                    changed=clock.changed()=>{if changed.is_err(){std::future::pending::<()>().await;}}
                }
            }}=>Err(io::ErrorKind::TimedOut.into()),
        }
    })
    .await?
}

fn validated_address(addresses: &[std::net::SocketAddr]) -> io::Result<std::net::SocketAddr> {
    if addresses.is_empty()
        || addresses.len() > 64
        || addresses.iter().any(|a| !public_address(a.ip()))
    {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(addresses[0])
}
async fn await_upstream<R, T, F>(client: &mut R, head: &mut Vec<u8>, connecting: F) -> io::Result<T>
where
    R: AsyncRead + Unpin,
    F: std::future::Future<Output = io::Result<T>>,
{
    tokio::pin!(connecting);
    let mut bytes = [0; 1024];
    loop {
        tokio::select! {
            biased;
            read=client.read(&mut bytes)=>{let n=read?;if n==0{return Err(io::ErrorKind::UnexpectedEof.into());}if head.len()+n>65536{return Err(io::ErrorKind::InvalidData.into());}head.extend_from_slice(&bytes[..n]);},
            result=&mut connecting=>return result,
        }
    }
}

async fn read_header<R: AsyncRead + Unpin>(client: &mut R) -> io::Result<(Vec<u8>, usize)> {
    let mut header = Vec::with_capacity(8192);
    loop {
        let mut part = [0; 1024];
        let n = client.read(&mut part).await?;
        if n == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        // 新しい終端は、直前の読込み末尾3バイト以降にしか現れない。
        let start = header.len().saturating_sub(3);
        header.extend_from_slice(&part[..n]);
        if let Some(i) = header[start..].windows(4).position(|x| x == b"\r\n\r\n") {
            let boundary = start + i + 4;
            if boundary > 8192 {
                return Err(io::ErrorKind::InvalidData.into());
            }
            return Ok((header, boundary));
        }
        if header.len() >= 8192 {
            return Err(io::ErrorKind::InvalidData.into());
        }
    }
}

async fn tunnel(client: &mut TcpStream) -> io::Result<()> {
    let prepared = timeout(Duration::from_secs(15), async {
        let (header, boundary) = read_header(client).await?;
        let host = target(&header[..boundary]).ok_or(io::ErrorKind::PermissionDenied)?;
        let mut head = header[boundary..].to_vec();
        let connecting = async {
            let addresses: Vec<_> = tokio::net::lookup_host((host.as_str(), 443))
                .await?
                .take(65)
                .collect();
            TcpStream::connect(validated_address(&addresses)?).await
        };
        let upstream = await_upstream(client, &mut head, connecting).await?;
        Ok::<_, io::Error>((upstream, head))
    })
    .await;
    let (mut upstream, head) = match prepared {
        Ok(Ok(tunnel)) => tunnel,
        _ => {
            let _ = timeout(
                Duration::from_secs(1),
                client.write_all(b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n"),
            )
            .await;
            return Ok(());
        }
    };
    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    relay_prefixed(client, &mut upstream, &head).await
}

pub(crate) async fn serve(
    listener: TcpListener,
    cancel: CancellationToken,
    active: Arc<AtomicUsize>,
) {
    let permits = Arc::new(Semaphore::new(16));
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {},
            accepted = listener.accept() => {
                let Ok((mut client, address)) = accepted else { break };
                if !address.ip().is_loopback() { continue; }
                let Ok(permit) = permits.clone().try_acquire_owned() else { continue };
                let active = active.clone();
                let cancel = cancel.clone();
                tasks.spawn(async move {
                    let _permit = permit;
                    active.fetch_add(1, Ordering::Relaxed);
                    let _count=ActiveConnection(active);
                    tokio::select! { _ = cancel.cancelled() => {}, _ = timeout(Duration::from_secs(600),tunnel(&mut client)) => {}}
                });
            }
        }
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
}

struct ActiveConnection(Arc<AtomicUsize>);
impl Drop for ActiveConnection {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn fragmented_header_keeps_first_delimiter_and_early_payload() {
        let header = b"CONNECT x.com:443 HTTP/1.1\r\nHost: x.com:443\r\n\r\n";
        let payload = b"tls\0payload\r\n\r\nmore";
        for split in header.len() - 3..header.len() {
            let mut remaining = header[split..].to_vec();
            remaining.extend_from_slice(payload);
            let mut client =
                std::io::Cursor::new(&header[..split]).chain(std::io::Cursor::new(remaining));
            let (actual, boundary) = read_header(&mut client).await.unwrap();
            assert_eq!(boundary, header.len());
            assert_eq!(&actual[..boundary], header);
            let mut forwarded = actual[boundary..].to_vec();
            client.read_to_end(&mut forwarded).await.unwrap();
            assert_eq!(forwarded, payload);
        }
        let (mut source, mut client) = tokio::io::duplex(1);
        let writer = tokio::spawn(async move { source.write_all(header).await.unwrap() });
        let (actual, boundary) = read_header(&mut client).await.unwrap();
        assert_eq!(boundary, header.len());
        assert_eq!(actual, header);
        writer.await.unwrap();
    }
    #[tokio::test]
    async fn header_limit_applies_to_first_delimiter_not_early_payload() {
        let payload = b"early\0tls\r\n\r\n";
        for boundary in [8191, 8192, 8193] {
            let mut bytes = vec![b'x'; boundary - 4];
            bytes.extend_from_slice(b"\r\n\r\n");
            bytes.extend_from_slice(payload);
            // 終端の途中で分割し、上限を越えて先読みしたペイロードも確認する。
            let mut client = std::io::Cursor::new(&bytes[..boundary - 2])
                .chain(std::io::Cursor::new(&bytes[boundary - 2..]));
            let result = read_header(&mut client).await;
            if boundary > 8192 {
                assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
            } else {
                let (actual, end) = result.unwrap();
                assert_eq!(end, boundary);
                assert_eq!(actual, bytes);
                assert_eq!(&actual[end..], payload);
            }
        }
        let mut unterminated = std::io::Cursor::new(vec![b'x'; 8192]);
        assert_eq!(
            read_header(&mut unterminated).await.unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let mut truncated = std::io::Cursor::new(b"CONNECT x.com:443 HTTP/1.1\r\n\r");
        assert_eq!(
            read_header(&mut truncated).await.unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }
    #[test]
    fn every_dns_candidate_must_be_public_and_bounded() {
        let public = "8.8.8.8:443".parse().unwrap();
        assert!(validated_address(&[public]).is_ok());
        for forbidden in [
            "127.0.0.1:443",
            "10.0.0.1:443",
            "[::ffff:8.8.8.8]:443",
            "[2001:db8::1]:443",
        ] {
            assert!(validated_address(&[public, forbidden.parse().unwrap()]).is_err());
        }
        assert!(validated_address(&[]).is_err());
        assert!(validated_address(&vec![public; 65]).is_err());
    }
    #[tokio::test]
    async fn disconnected_client_never_advances_pending_dns_to_connect() {
        let (client, mut proxy) = tokio::io::duplex(64);
        drop(client);
        let connected = Arc::new(AtomicUsize::new(0));
        let count = connected.clone();
        let mut head = vec![];
        let result = await_upstream(&mut proxy, &mut head, async move {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
        .await;
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
        assert_eq!(connected.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn early_payload_is_forwarded_exactly_once() {
        let (mut source, mut left) = tokio::io::duplex(1024);
        let (mut right, mut sink) = tokio::io::duplex(1024);
        let task =
            tokio::spawn(async move { relay_prefixed(&mut left, &mut right, b"early").await });
        source.write_all(b"later").await.unwrap();
        source.shutdown().await.unwrap();
        let mut actual = [0; 10];
        sink.read_exact(&mut actual).await.unwrap();
        assert_eq!(&actual, b"earlylater");
        sink.shutdown().await.unwrap();
        task.await.unwrap().unwrap();
    }
    #[tokio::test(start_paused = true)]
    async fn one_way_activity_keeps_tunnel_alive_then_full_idle_expires() {
        let (mut source, mut left) = tokio::io::duplex(64);
        let (mut right, mut sink) = tokio::io::duplex(64);
        let task = tokio::spawn(async move { relay(&mut left, &mut right).await });
        for _ in 0..4 {
            tokio::time::advance(Duration::from_secs(30)).await;
            source.write_all(b"a").await.unwrap();
            let mut b = [0];
            sink.read_exact(&mut b).await.unwrap();
            assert_eq!(b, [b'a']);
            assert!(!task.is_finished());
        }
        tokio::time::advance(Duration::from_secs(59)).await;
        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        tokio::time::advance(Duration::from_secs(2)).await;
        assert_eq!(
            task.await.unwrap().unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
    }
    #[test]
    fn rejects_special_addresses_and_tunnels() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "100.64.0.1",
            "192.0.2.1",
            "198.18.0.1",
            "::1",
            "::ffff:8.8.8.8",
            "2002:808:808::1",
            "2001:db8::1",
        ] {
            assert!(!public_address(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(public_address(ip.parse().unwrap()), "{ip}");
        }
    }
    #[test]
    fn connect_authority_and_headers_have_one_interpretation() {
        assert_eq!(
            target(b"CONNECT api.x.com:443 HTTP/1.1\r\nHost: api.x.com:443\r\n\r\n").as_deref(),
            Some("api.x.com")
        );
        for h in [
            "CONNECT evilx.com:443 HTTP/1.1\r\n\r\n",
            "CONNECT x.com:80 HTTP/1.1\r\n\r\n",
            "CONNECT x.com:443 HTTP/1.1\r\nHost: evil.com:443\r\n\r\n",
            "CONNECT x.com:443 HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
            "CONNECT x.com:443 HTTP/1.1\r\nHost: x.com:443\r\nhost: x.com:443\r\n\r\n",
        ] {
            assert!(target(h.as_bytes()).is_none());
        }
    }
}
