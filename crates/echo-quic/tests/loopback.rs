//! Echtes QUIC ueber localhost mit dem Provider von echo: Handschlag,
//! Steuerstream, Datagramme und der Fingerabdruck des Servers.

// Hilfsfunktionen ausserhalb von #[test] zaehlen fuer clippy nicht als Test.
#![allow(clippy::unwrap_used)]

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

use echo_quic::{
    HandshakeData, HostCertificate, client_config, endpoint_config, server_config,
    server_fingerprint,
};
use quinn::{Endpoint, TokioRuntime};

const ALPN: &[u8] = b"echo";
const TIMEOUT: Duration = Duration::from_secs(10);

fn endpoint(server: Option<quinn::ServerConfig>) -> Endpoint {
    let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    Endpoint::new(
        endpoint_config().unwrap(),
        server,
        socket,
        Arc::new(TokioRuntime),
    )
    .unwrap()
}

struct Pair {
    server: Endpoint,
    client: Endpoint,
    address: SocketAddr,
}

fn pair(certificate: &HostCertificate) -> Pair {
    let server = endpoint(Some(server_config(certificate, ALPN).unwrap()));
    let mut client = endpoint(None);
    client.set_default_client_config(client_config(ALPN).unwrap());
    let address = server.local_addr().unwrap();
    Pair {
        server,
        client,
        address,
    }
}

async fn connect(pair: &Pair) -> (quinn::Connection, quinn::Connection) {
    let accept = async {
        let incoming = pair.server.accept().await.unwrap();
        incoming.await.unwrap()
    };
    let dial = async {
        pair.client
            .connect(pair.address, "echo")
            .unwrap()
            .await
            .unwrap()
    };
    tokio::time::timeout(TIMEOUT, async { tokio::join!(accept, dial) })
        .await
        .unwrap()
}

#[tokio::test]
async fn handshake_shows_the_server_fingerprint_and_alpn() {
    let certificate = HostCertificate::generate("Testserver").unwrap();
    let pair = pair(&certificate);
    let (server, client) = connect(&pair).await;

    assert_eq!(server_fingerprint(&client), Some(certificate.fingerprint()));
    assert_eq!(server_fingerprint(&server), None);

    let data = client.handshake_data().unwrap();
    let data = data.downcast_ref::<HandshakeData>().unwrap();
    assert_eq!(data.protocol.as_deref(), Some(ALPN));
    let data = server.handshake_data().unwrap();
    let data = data.downcast_ref::<HandshakeData>().unwrap();
    assert_eq!(data.server_name.as_deref(), Some("echo"));
}

#[tokio::test]
async fn control_stream_carries_data_both_ways() {
    let pair = pair(&HostCertificate::generate("Testserver").unwrap());
    let (server, client) = connect(&pair).await;

    let echo = tokio::spawn(async move {
        let (mut send, mut recv) = server.accept_bi().await.unwrap();
        let request = recv.read_to_end(1024).await.unwrap();
        send.write_all(&[b"zurueck: ".as_slice(), &request].concat())
            .await
            .unwrap();
        send.finish().unwrap();
        // Offen halten, bis der Client gelesen hat.
        server.closed().await;
    });

    let (mut send, mut recv) = client.open_bi().await.unwrap();
    send.write_all(b"hallo").await.unwrap();
    send.finish().unwrap();
    let answer = tokio::time::timeout(TIMEOUT, recv.read_to_end(1024))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(answer, b"zurueck: hallo");

    client.close(0u32.into(), b"fertig");
    echo.await.unwrap();
}

#[tokio::test]
async fn datagrams_carry_voice_frames_both_ways() {
    let pair = pair(&HostCertificate::generate("Testserver").unwrap());
    let (server, client) = connect(&pair).await;
    assert!(client.max_datagram_size().is_some());

    // Ein 20-ms-Opus-Frame mit 32 kbit/s sind 80 Byte.
    let frame = vec![0x5a; 80];
    client.send_datagram(frame.clone().into()).unwrap();
    let received = tokio::time::timeout(TIMEOUT, server.read_datagram())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(received, frame);

    server.send_datagram(b"zurueck".to_vec().into()).unwrap();
    let received = tokio::time::timeout(TIMEOUT, client.read_datagram())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&received[..], b"zurueck");
}

#[tokio::test]
async fn client_refuses_a_server_with_another_alpn() {
    let certificate = HostCertificate::generate("Testserver").unwrap();
    let server = endpoint(Some(server_config(&certificate, b"anderes").unwrap()));
    let mut client = endpoint(None);
    client.set_default_client_config(client_config(ALPN).unwrap());
    let address = server.local_addr().unwrap();

    let accept = tokio::spawn(async move {
        if let Some(incoming) = server.accept().await {
            let _ = incoming.await;
        }
    });
    let result = tokio::time::timeout(TIMEOUT, client.connect(address, "echo").unwrap())
        .await
        .unwrap();
    assert!(result.is_err());
    accept.abort();
}

#[tokio::test]
async fn serves_the_certificate_of_the_csharp_server() {
    let fixtures =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/csharp/host");
    let certificate =
        HostCertificate::load(&fixtures.join("host.crt"), &fixtures.join("host.key")).unwrap();
    let expected = std::fs::read_to_string(fixtures.join("fingerprint.txt")).unwrap();
    assert_eq!(certificate.fingerprint(), expected.trim());

    let pair = pair(&certificate);
    let (_server, client) = connect(&pair).await;
    assert_eq!(
        server_fingerprint(&client).as_deref(),
        Some(expected.trim())
    );
}
