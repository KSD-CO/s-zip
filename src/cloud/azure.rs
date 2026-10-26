//! Azure Blob Storage streaming adapters.
//!
//! - [`AzureZipWriter`] streams a ZIP into a block blob: each buffered block is
//!   staged with *Put Block* (several in parallel) and the blob is committed
//!   with *Put Block List* on `finish()`. Nothing becomes visible until the
//!   commit, so a failed or abandoned upload never leaves a truncated blob
//!   (Azure garbage-collects uncommitted blocks).
//! - [`AzureZipReader`] reads a blob with ranged downloads, for use with
//!   `GenericAsyncZipReader`.
//!
//! ## Example
//!
//! ```no_run
//! use s_zip::{AsyncStreamingZipWriter, cloud::AzureZipWriter};
//! use azure_storage_blob::BlobClient;
//! use std::sync::Arc;
//!
//! # async fn example(credential: Arc<dyn azure_core::credentials::TokenCredential>) -> Result<(), Box<dyn std::error::Error>> {
//! let blob = BlobClient::new(
//!     "https://myaccount.blob.core.windows.net/exports/archive.zip".parse()?,
//!     Some(credential),
//!     None,
//! )?;
//!
//! let writer = AzureZipWriter::new(blob.block_blob_client());
//! let mut zip = AsyncStreamingZipWriter::from_writer(writer);
//! zip.add_entry("hello.txt", b"Hello Azure!").await?;
//! zip.finish().await?;
//! # Ok(())
//! # }
//! ```

use crate::error::{Result, SZipError};
use azure_storage_blob::models::{
    BlobClientDownloadOptions, BlobClientGetPropertiesResultHeaders, BlockLookupList, HttpRange,
};
use azure_storage_blob::{BlobClient, BlockBlobClient};
use std::future::Future;
use std::io;
use std::num::NonZero;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{ready, Context, Poll};
use tokio::io::{AsyncRead, AsyncSeek, AsyncWrite};
use tokio::sync::mpsc;
use tokio_util::sync::PollSender;

/// Default block size (8 MiB).
pub const DEFAULT_BLOCK_SIZE: usize = 8 * 1024 * 1024;

/// Maximum number of blocks in a block blob.
pub const MAX_BLOCKS: usize = 50_000;

/// Commands sent to the background upload task.
enum UploadCommand {
    Block { index: usize, data: Vec<u8> },
    Commit { final_data: Option<Vec<u8>> },
}

/// Azure block blob writer implementing `AsyncWrite + AsyncSeek + Unpin`.
///
/// Memory is bounded to roughly `(max_concurrent_uploads + 2) × block_size`:
/// writes wait while the upload task is busy.
pub struct AzureZipWriter {
    upload_tx: PollSender<UploadCommand>,
    upload_task: Option<tokio::task::JoinHandle<Result<()>>>,
    buffer: Vec<u8>,
    block_size: usize,
    blocks_sent: usize,
    position: u64,
    shutdown_initiated: bool,
}

/// Builder for [`AzureZipWriter`].
pub struct AzureZipWriterBuilder {
    client: BlockBlobClient,
    block_size: usize,
    max_concurrent_uploads: usize,
}

impl AzureZipWriter {
    /// Create a writer with default settings (8 MiB blocks, 4 parallel uploads).
    pub fn new(client: BlockBlobClient) -> Self {
        Self::builder(client).build()
    }

    /// Create a builder for custom block size / concurrency.
    pub fn builder(client: BlockBlobClient) -> AzureZipWriterBuilder {
        AzureZipWriterBuilder {
            client,
            block_size: DEFAULT_BLOCK_SIZE,
            max_concurrent_uploads: 4,
        }
    }
}

impl AzureZipWriterBuilder {
    /// Block size in bytes (default 8 MiB). With at most 50 000 blocks per
    /// blob, the block size bounds the maximum archive size.
    pub fn block_size(mut self, block_size: usize) -> Self {
        self.block_size = block_size.max(1);
        self
    }

    /// Number of blocks uploaded in parallel (default 4).
    pub fn max_concurrent_uploads(mut self, max: usize) -> Self {
        self.max_concurrent_uploads = max.max(1);
        self
    }

    /// Build the writer and start the background upload task.
    pub fn build(self) -> AzureZipWriter {
        let (tx, rx) = mpsc::channel(1);
        let upload_task = tokio::spawn(upload_worker(
            Arc::new(self.client),
            rx,
            self.max_concurrent_uploads,
        ));
        AzureZipWriter {
            upload_tx: PollSender::new(tx),
            upload_task: Some(upload_task),
            buffer: Vec::with_capacity(self.block_size),
            block_size: self.block_size,
            blocks_sent: 0,
            position: 0,
            shutdown_initiated: false,
        }
    }
}

fn upload_task_gone() -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        "Upload task terminated unexpectedly",
    )
}

impl AsyncWrite for AzureZipWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        // Hand a full buffer to the upload task first; wait (backpressure) while it is busy
        if self.buffer.len() >= self.block_size {
            if self.blocks_sent + 1 >= MAX_BLOCKS {
                return Poll::Ready(Err(io::Error::other(
                    "Azure block blob limit (50 000 blocks) reached; increase block_size",
                )));
            }
            if ready!(self.upload_tx.poll_reserve(cx)).is_err() {
                return Poll::Ready(Err(upload_task_gone()));
            }
            let block_size = self.block_size;
            let data = std::mem::replace(&mut self.buffer, Vec::with_capacity(block_size));
            let index = self.blocks_sent;
            self.blocks_sent += 1;
            if self
                .upload_tx
                .send_item(UploadCommand::Block { index, data })
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
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if !self.shutdown_initiated {
            if ready!(self.upload_tx.poll_reserve(cx)).is_err() {
                return Poll::Ready(Err(upload_task_gone()));
            }
            self.shutdown_initiated = true;
            let final_data = Some(std::mem::take(&mut self.buffer)).filter(|d| !d.is_empty());
            if self
                .upload_tx
                .send_item(UploadCommand::Commit { final_data })
                .is_err()
            {
                return Poll::Ready(Err(upload_task_gone()));
            }
        }

        match self.upload_task.as_mut() {
            Some(task) => match ready!(Pin::new(task).poll(cx)) {
                Ok(Ok(())) => Poll::Ready(Ok(())),
                Ok(Err(e)) => {
                    Poll::Ready(Err(io::Error::other(format!("Azure upload failed: {}", e))))
                }
                Err(e) => Poll::Ready(Err(io::Error::other(format!(
                    "Upload task panicked: {}",
                    e
                )))),
            },
            None => Poll::Ready(Ok(())),
        }
    }
}

impl AsyncSeek for AzureZipWriter {
    fn start_seek(self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        // Blob uploads are append-only; only position queries are supported
        match position {
            io::SeekFrom::Current(0) => Ok(()),
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Azure writer does not support seeking",
            )),
        }
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(self.position))
    }
}

/// Block ids must all have the same length within a blob.
fn block_id(index: usize) -> Vec<u8> {
    format!("{:016x}", index).into_bytes()
}

async fn stage_block(client: Arc<BlockBlobClient>, index: usize, data: Vec<u8>) -> Result<usize> {
    let len = data.len() as u64;
    client
        .stage_block(&block_id(index), len, bytes::Bytes::from(data).into(), None)
        .await
        .map_err(|e| {
            SZipError::Io(io::Error::other(format!(
                "Failed to stage block {}: {}",
                index, e
            )))
        })?;
    Ok(index)
}

/// Stages up to `max_concurrent` blocks in parallel, accepting a new block
/// from the writer only when a slot is free, then commits the block list.
async fn upload_worker(
    client: Arc<BlockBlobClient>,
    mut rx: mpsc::Receiver<UploadCommand>,
    max_concurrent: usize,
) -> Result<()> {
    use futures_util::stream::{FuturesUnordered, StreamExt};

    let mut in_flight = FuturesUnordered::new();
    let mut staged = 0usize;

    loop {
        tokio::select! {
            Some(result) = in_flight.next(), if !in_flight.is_empty() => {
                result?;
                staged += 1;
            }
            cmd = rx.recv(), if in_flight.len() < max_concurrent => match cmd {
                Some(UploadCommand::Block { index, data }) => {
                    in_flight.push(stage_block(client.clone(), index, data));
                }
                Some(UploadCommand::Commit { final_data }) => {
                    let total = staged + in_flight.len() + usize::from(final_data.is_some());
                    if let Some(data) = final_data {
                        in_flight.push(stage_block(client.clone(), total - 1, data));
                    }
                    while let Some(result) = in_flight.next().await {
                        result?;
                    }

                    let blocks = BlockLookupList {
                        latest: Some((0..total).map(block_id).collect()),
                        ..Default::default()
                    };
                    let body = blocks.try_into().map_err(|e: azure_core::Error| {
                        SZipError::Io(io::Error::other(e.to_string()))
                    })?;
                    client.commit_block_list(body, None).await.map_err(|e| {
                        SZipError::Io(io::Error::other(format!(
                            "Failed to commit block list: {}",
                            e
                        )))
                    })?;
                    return Ok(());
                }
                // Writer dropped before finish(): leave the blob uncommitted
                None => {
                    return Err(SZipError::Io(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Azure writer dropped before finish()",
                    )));
                }
            },
        }
    }
}

// ============================================================================
// Azure ZIP Reader
// ============================================================================

/// Azure blob reader implementing `AsyncRead + AsyncSeek + Unpin + Send`.
///
/// Reads are conditional on the ETag seen at open time, so modifying the blob
/// mid-read fails instead of mixing two versions.
///
/// ```no_run
/// use s_zip::{GenericAsyncZipReader, cloud::AzureZipReader};
/// # async fn example(blob: azure_storage_blob::BlobClient) -> Result<(), Box<dyn std::error::Error>> {
/// let reader = AzureZipReader::new(blob).await?;
/// let mut zip = GenericAsyncZipReader::new_with_buffer_size(reader, None).await?;
/// let data = zip.read_entry_by_name("file.txt").await?;
/// # Ok(())
/// # }
/// ```
pub struct AzureZipReader {
    client: Arc<BlobClient>,
    etag: Option<azure_core::http::Etag>,
    position: u64,
    size: u64,
    #[allow(clippy::type_complexity)]
    read_future: Option<Pin<Box<dyn Future<Output = io::Result<Vec<u8>>> + Send>>>,
}

impl AzureZipReader {
    /// Open the blob for reading.
    pub async fn new(client: BlobClient) -> Result<Self> {
        let props = client.get_properties(None).await.map_err(|e| {
            SZipError::Io(io::Error::other(format!(
                "Failed to get Azure blob properties: {}",
                e
            )))
        })?;
        let size =
            props.content_length().ok().flatten().ok_or_else(|| {
                SZipError::Io(io::Error::other("Azure blob has no content length"))
            })?;
        let etag = props.etag().ok().flatten();
        Ok(Self {
            client: Arc::new(client),
            etag,
            position: 0,
            size,
            read_future: None,
        })
    }

    /// Total blob size in bytes.
    pub fn size(&self) -> u64 {
        self.size
    }
}

impl AsyncRead for AzureZipReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.read_future.is_none() {
            if self.position >= self.size || buf.remaining() == 0 {
                return Poll::Ready(Ok(())); // EOF
            }
            let start = self.position;
            let count = (buf.remaining() as u64).min(self.size - start);
            let client = self.client.clone();
            let etag = self.etag.clone();
            self.read_future = Some(Box::pin(async move {
                let options = BlobClientDownloadOptions {
                    range: Some(HttpRange::new(start, count)),
                    if_match: etag,
                    parallel: NonZero::new(1),
                    ..Default::default()
                };
                let response = client
                    .download(Some(options))
                    .await
                    .map_err(|e| io::Error::other(format!("Azure download failed: {}", e)))?;
                let bytes = response
                    .body
                    .collect()
                    .await
                    .map_err(|e| io::Error::other(format!("Azure download failed: {}", e)))?;
                Ok(bytes.to_vec())
            }));
        }

        let fut = self.read_future.as_mut().expect("read future set above");
        let result = ready!(fut.as_mut().poll(cx));
        self.read_future = None;
        let bytes = result?;
        let n = bytes.len().min(buf.remaining());
        buf.put_slice(&bytes[..n]);
        self.position += n as u64;
        Poll::Ready(Ok(()))
    }
}

impl AsyncSeek for AzureZipReader {
    fn start_seek(mut self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        let new_pos = match position {
            io::SeekFrom::Start(pos) => Some(pos),
            io::SeekFrom::End(offset) => self.size.checked_add_signed(offset),
            io::SeekFrom::Current(offset) => self.position.checked_add_signed(offset),
        };
        self.position = new_pos
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "Invalid seek position"))?;
        // A seek invalidates any in-flight read for the old position
        self.read_future = None;
        Ok(())
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(self.position))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_ids_have_equal_length() {
        assert_eq!(block_id(0).len(), block_id(MAX_BLOCKS).len());
        assert!(block_id(0) < block_id(1));
    }
}
