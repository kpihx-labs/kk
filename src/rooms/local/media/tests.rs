use super::*;
use crate::media::Codec;
use std::io::Cursor;

#[tokio::test]
async fn graceful_relay_stop_completes_the_active_frame_before_finishing_ipc() {
    let (client, mut server) = UnixStream::pair().unwrap();
    rustix::net::sockopt::set_socket_send_buffer_size(&client, 1024).unwrap();
    let mut bytes = vec![0x80, 97, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
    bytes.resize(config::MEDIA_PACKET_BYTES, 0x55);
    let packet = Packet::parse(Codec::Vp8, bytes).unwrap();
    let mut recording = Vec::new();
    for _ in 0..64 {
        recording.extend((packet.bytes().len() as u16).to_be_bytes());
        recording.extend(packet.bytes());
    }
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let writer = tokio::spawn(async move {
        let mut publication = Publisher {
            stream: client,
            kind: Kind::Screen,
            peers: Vec::new(),
        };
        let relayed = publication
            .relay(&mut Cursor::new(recording), async {
                stopped.await.map_err(|_| Error::Cancelled)
            })
            .await
            .unwrap();
        let reply = publication.finish().await.unwrap();
        (relayed, reply)
    });
    let first = server.read_u8().await.unwrap();
    stop.send(()).unwrap();
    let mut received = vec![first];
    tokio::time::timeout(config::DEFAULT_TIMEOUT, server.read_to_end(&mut received))
        .await
        .unwrap()
        .unwrap();
    let mut packets = 0;
    let mut input = Cursor::new(received);
    while Packet::read(Codec::Vp8, &mut input)
        .await
        .unwrap()
        .is_some()
    {
        packets += 1;
    }
    assert!(packets > 0 && packets < 64);
    ipc::write(
        &mut server,
        &ReplyEvent::Completed(Reply {
            ok: true,
            code: None,
            result: serde_json::json!({"packets":packets}),
        }),
    )
    .await
    .unwrap();
    let ((count, requested), reply) = tokio::time::timeout(config::DEFAULT_TIMEOUT, writer)
        .await
        .unwrap()
        .unwrap();
    assert!(requested && reply.ok);
    assert_eq!(count, packets);
}
