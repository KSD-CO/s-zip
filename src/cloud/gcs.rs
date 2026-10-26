//! Google Cloud Storage streaming adapter using resumable upload.
//!
//! This module provides `GCSZipWriter` which implements `AsyncWrite + AsyncSeek + Unpin`,
//! enabling `AsyncStreamingZipWriter` to stream ZIP files directly to GCS without loading
//! the entire archive into memory.
//!
//! ## How it Works
//!
//! - Uses GCS resumable upload (chunk size must be multiple of 256KB)
//! - Buffers writes until reaching chunk size threshold (default 8MB)
//! - Streams chunks to GCS from a background Tokio task as they are produced
//! - The object is only committed after `finish()`; dropping the writer early aborts the upload
//! - Tracks virtual position for ZIP central directory (no actual seeking)
//! - Maintains constant memory usage (~8-12MB)
//!
//! ## Example
//!
//! ```ignore
//! use s_zip::{AsyncStreamingZipWriter, cloud::GCSZipWriter};
//! use google_cloud_storage::client::Storage;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let gcs_client = Storage::builder().build().await?;
//!
//! let writer = GCSZipWriter::new(
//!     gcs_client,
//!     "my-bucket",
//!     "exports/data.zip"
//! ).await?;
//!
//! let mut zip = AsyncStreamingZipWriter::from_writer(writer);
//! zip.start_entry("file.txt").await?;
//! zip.write_data(b"Hello GCS!").await?;
//! zip.finish().await?;
//! # Ok(())
//! # }
//! ```

use crate::error::{Result, SZipError};
use google_cloud_storage::client::Storage;
use google_cloud_storage::streaming_source::StreamingSource;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{ready, Context, Poll};
use tokio::io::{AsyncSeek, AsyncWrite};
use tokio::sync::mpsc;
use tokio_util::sync::PollSender;

/// Default chunk size for GCS resumable upload (8MB)
/// Must be multiple of 256KB
pub const DEFAULT_CHUNK_SIZE: usize = 8 * 1024 * 1024;

/// GCS chunk alignment (256KB)
pub const CHUNK_ALIGNMENT: usize = 256 * 1024;

/// GCS ZIP writer that streams directly to GCS using resumable upload.
///
/// This writer implements `AsyncWrite + AsyncSeek + Unpin`, making it compatible
/// with `AsyncStreamingZipWriter`.
pub struct GCSZipWriter {
    /// Upload state (managed by background task)
    upload_tx: PollSender<UploadCommand>,
    upload_task: Option<tokio::task::JoinHandle<Result<()>>>,

    /// Write buffer (accumulates data until chunk_size)
    buffer: Vec<u8>,
    chunk_size: usize,

    /// Virtual position tracking (for ZIP central directory)
    position: u64,

    /// Flag to prevent sending Complete command multiple times
    shutdown_initiated: bool,
}

/// Commands sent to the background upload task
enum UploadCommand {
    /// Upload a chunk with given data
    UploadChunk { data: Vec<u8> },
    /// Finalize the upload with optional final chunk
    Finalize { final_data: Option<Vec<u8>> },
}

/// Builder for `GCSZipWriter` with configuration options.
pub struct GCSZipWriterBuilder {
    client: Option<Storage>,
    bucket: String,
    object: String,
    chunk_size: usize,
}

impl GCSZipWriter {
    /// Create a new GCS ZIP writer with default settings.
    ///
    /// Uses 8MB chunk size (must be multiple of 256KB).
    ///
    /// # Example
    ///
    /// ```ignore
    /// # use s_zip::cloud::GCSZipWriter;
    /// # use google_cloud_storage::client::Storage;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Storage::builder().build().await?;
    ///
    /// let writer = GCSZipWriter::new(
    ///     client,
    ///     "my-bucket",
    ///     "exports/archive.zip"
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(
        client: Storage,
        bucket: impl Into<String>,
        object: impl Into<String>,
    ) -> Result<Self> {
        Self::builder()
            .client(client)
            .bucket(bucket)
            .object(object)
            .build()
            .await
    }

    /// Create a builder for configuring the GCS writer.
    ///
    /// # Example
    ///
    /// ```ignore
    /// # use s_zip::cloud::GCSZipWriter;
    /// # use google_cloud_storage::client::Storage;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Storage::builder().build().await?;
    ///
    /// let writer = GCSZipWriter::builder()
    ///     .client(client)
    ///     .bucket("my-bucket")
    ///     .object("large-archive.zip")
    ///     .chunk_size(16 * 1024 * 1024)  // 16MB chunks
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn builder() -> GCSZipWriterBuilder {
        // Note: Builder requires client to be set explicitly
        // We can't create a default Client without async context
        GCSZipWriterBuilder {
            client: None,
            bucket: String::new(),
            object: String::new(),
            chunk_size: DEFAULT_CHUNK_SIZE,
        }
    }
}

impl GCSZipWriterBuilder {
    /// Set the GCS client.
    pub fn client(mut self, client: Storage) -> Self {
        self.client = Some(client);
        self
    }

    /// Set the GCS bucket name.
    pub fn bucket(mut self, bucket: impl Into<String>) -> Self {
        self.bucket = bucket.into();
        self
    }

    /// Set the GCS object name (path).
    pub fn object(mut self, object: impl Into<String>) -> Self {
        self.object = object.into();
        self
    }

    /// Set the chunk size for resumable upload.
    ///
    /// Must be a multiple of 256KB. Larger chunks reduce the number
    /// of API calls but increase memory usage.
    ///
    /// # Panics
    ///
    /// Panics if chunk_size is not a multiple of 256KB.
    pub fn chunk_size(mut self, chunk_size: usize) -> Self {
        assert!(
            chunk_size.is_multiple_of(CHUNK_ALIGNMENT),
            "Chunk size must be a multiple of 256KB"
        );
        self.chunk_size = chunk_size;
        self
    }

    /// Build the GCS writer and start the background upload task.
    pub async fn build(self) -> Result<GCSZipWriter> {
        let client = self
            .client
            .ok_or_else(|| SZipError::InvalidFormat("GCS client must be set".to_string()))?;

        // Capacity 1 keeps at most ~2 chunks queued besides what the SDK is sending
        let (tx, rx) = mpsc::channel(1);

        // Spawn background task for uploading chunks
        let upload_task = tokio::spawn(upload_worker(client, self.bucket, self.object, rx));

        Ok(GCSZipWriter {
            upload_tx: PollSender::new(tx),
            upload_task: Some(upload_task),
            buffer: Vec::with_capacity(self.chunk_size),
            chunk_size: self.chunk_size,
            position: 0,
            shutdown_initiated: false,
        })
    }
}

impl AsyncWrite for GCSZipWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        // Hand a full buffer to the upload task first; wait (backpressure) while
        // the task is busy so memory stays bounded when the network is slower
        if self.buffer.len() >= self.chunk_size {
            if ready!(self.upload_tx.poll_reserve(cx)).is_err() {
                return Poll::Ready(Err(upload_task_gone()));
            }
            let chunk_size = self.chunk_size;
            let data = std::mem::replace(&mut self.buffer, Vec::with_capacity(chunk_size));
            if self
                .upload_tx
                .send_item(UploadCommand::UploadChunk { data })
                .is_err()
            {
                return Poll::Ready(Err(upload_task_gone()));
            }
        }

        self.buffer.extend_from_slice(buf);
        self.position += buf.len() as u64;
        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Flushing is handled by the background task
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Only send Complete command once
        if !self.shutdown_initiated {
            if ready!(self.upload_tx.poll_reserve(cx)).is_err() {
                return Poll::Ready(Err(upload_task_gone()));
            }
            self.shutdown_initiated = true;
            // Send final chunk (if any) and finalize upload
            let final_data = if !self.buffer.is_empty() {
                Some(std::mem::take(&mut self.buffer))
            } else {
                None
            };

            // Send finalize command
            if self
                .upload_tx
                .send_item(UploadCommand::Finalize { final_data })
                .is_err()
            {
                return Poll::Ready(Err(upload_task_gone()));
            }
        }
        // Wait for background task to complete
        if let Some(task) = self.upload_task.as_mut() {
            match Pin::new(task).poll(cx) {
                Poll::Ready(Ok(Ok(()))) => Poll::Ready(Ok(())),
                Poll::Ready(Ok(Err(e))) => {
                    Poll::Ready(Err(io::Error::other(format!("GCS upload failed: {}", e))))
                }
                Poll::Ready(Err(e)) => Poll::Ready(Err(io::Error::other(format!(
                    "Upload task panicked: {}",
                    e
                )))),
                Poll::Pending => Poll::Pending,
            }
        } else {
            Poll::Ready(Ok(()))
        }
    }
}

impl AsyncSeek for GCSZipWriter {
    fn start_seek(self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        // GCS doesn't support seeking - we only track virtual position
        match position {
            io::SeekFrom::Current(0) => Ok(()), // Query current position (allowed)
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "GCS writer does not support seeking",
            )),
        }
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        // Return tracked virtual position
        Poll::Ready(Ok(self.position))
    }
}

impl Unpin for GCSZipWriter {}

fn upload_task_gone() -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        "Upload task terminated unexpectedly",
    )
}

/// Background worker that streams buffered chunks to GCS via resumable upload.
async fn upload_worker(
    client: Storage,
    bucket: String,
    object: String,
    rx: mpsc::Receiver<UploadCommand>,
) -> Result<()> {
    client
        .write_object(
            format!("projects/_/buckets/{}", bucket),
            object,
            ChunkSource {
                rx,
                finished: false,
            },
        )
        .send_buffered()
        .await
        .map_err(|e| SZipError::Io(io::Error::other(format!("Failed to upload to GCS: {}", e))))?;

    Ok(())
}

/// Adapts the upload command channel into a GCS streaming payload.
struct ChunkSource {
    rx: mpsc::Receiver<UploadCommand>,
    finished: bool,
}

impl StreamingSource for ChunkSource {
    type Error = io::Error;

    async fn next(&mut self) -> Option<std::result::Result<bytes::Bytes, Self::Error>> {
        if self.finished {
            return None;
        }
        match self.rx.recv().await {
            Some(UploadCommand::UploadChunk { data }) => Some(Ok(data.into())),
            Some(UploadCommand::Finalize { final_data }) => {
                self.finished = true;
                final_data.map(|data| Ok(data.into()))
            }
            // Writer dropped before finish(): fail so a truncated object is never committed
            None => {
                self.finished = true;
                Some(Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "GCS writer dropped before finish()",
                )))
            }
        }
    }
}

impl Drop for GCSZipWriter {
    fn drop(&mut self) {
        // If the writer is dropped without calling finish(), we should try to clean up
        // However, we can't easily do this from Drop since it's not async
        // Users should ensure finish() is called properly
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[test]
    fn test_writer_applies_backpressure() {
        let (tx, mut rx) = mpsc::channel(1);
        let mut writer = GCSZipWriter {
            upload_tx: PollSender::new(tx),
            upload_task: None,
            buffer: Vec::new(),
            chunk_size: 4,
            position: 0,
            shutdown_initiated: false,
        };

        // Nobody drains the channel: after one queued chunk the writer must stall
        let data = [0u8; 4];
        let mut task = tokio_test::task::spawn(async {
            for _ in 0..4 {
                writer.write_all(&data).await.unwrap();
            }
        });
        assert!(task.poll().is_pending());

        assert!(matches!(
            rx.try_recv(),
            Ok(UploadCommand::UploadChunk { .. })
        ));
        assert!(task.is_woken());
    }

    #[tokio::test]
    async fn test_chunk_source_errors_when_writer_dropped() {
        let (tx, rx) = mpsc::channel(1);
        let mut source = ChunkSource {
            rx,
            finished: false,
        };
        tx.send(UploadCommand::UploadChunk { data: vec![1, 2] })
            .await
            .unwrap();
        drop(tx);
        assert_eq!(source.next().await.unwrap().unwrap().as_ref(), &[1, 2]);
        assert!(source.next().await.unwrap().is_err());
        assert!(source.next().await.is_none());
    }
}
