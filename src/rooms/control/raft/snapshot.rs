//! Bounded aggregate snapshot reception, not just bounded individual RPC chunks.
use crate::config;
use std::{
    io::{self, Cursor, SeekFrom},
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf};

#[derive(Default)]
pub struct SnapshotBuffer {
    inner: Cursor<Vec<u8>>,
}
impl SnapshotBuffer {
    pub fn new(bytes: Vec<u8>) -> io::Result<Self> {
        if bytes.len() > config::ROOM_CONTROL_SNAPSHOT_BYTES {
            return Err(limit());
        }
        Ok(Self {
            inner: Cursor::new(bytes),
        })
    }
    pub fn into_bytes(self) -> Vec<u8> {
        self.inner.into_inner()
    }
}
fn limit() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "public snapshot exceeds resource bound",
    )
}
impl AsyncRead for SnapshotBuffer {
    fn poll_read(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(context, buffer)
    }
}
impl AsyncWrite for SnapshotBuffer {
    fn poll_write(
        self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this
            .inner
            .position()
            .checked_add(bytes.len() as u64)
            .is_none_or(|size| size > config::ROOM_CONTROL_SNAPSHOT_BYTES as u64)
        {
            return Poll::Ready(Err(limit()));
        }
        Pin::new(&mut this.inner).poll_write(context, bytes)
    }
    fn poll_flush(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(context)
    }
    fn poll_shutdown(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(context)
    }
}
impl AsyncSeek for SnapshotBuffer {
    fn start_seek(self: Pin<&mut Self>, position: SeekFrom) -> io::Result<()> {
        let this = self.get_mut();
        let next = match position {
            SeekFrom::Start(value) => i128::from(value),
            SeekFrom::Current(value) => i128::from(this.inner.position()) + i128::from(value),
            SeekFrom::End(value) => this.inner.get_ref().len() as i128 + i128::from(value),
        };
        if next < 0 || next > config::ROOM_CONTROL_SNAPSHOT_BYTES as i128 {
            return Err(limit());
        }
        Pin::new(&mut this.inner).start_seek(position)
    }
    fn poll_complete(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Pin::new(&mut self.get_mut().inner).poll_complete(context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncSeekExt, AsyncWriteExt};
    #[tokio::test]
    async fn repeated_chunks_cannot_grow_past_the_total_limit() {
        let mut buffer = SnapshotBuffer::default();
        let block = vec![0; 65_536];
        for _ in 0..config::ROOM_CONTROL_SNAPSHOT_BYTES / block.len() {
            buffer.write_all(&block).await.unwrap();
        }
        assert!(buffer.write_all(&[1]).await.is_err());
        assert!(buffer.seek(SeekFrom::Start(u64::MAX)).await.is_err());
        assert_eq!(
            buffer.into_bytes().len(),
            config::ROOM_CONTROL_SNAPSHOT_BYTES
        );
    }
}
