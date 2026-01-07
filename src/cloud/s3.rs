//! AWS S3 streaming adapter using multipart upload.
//!
//! This module provides `S3ZipWriter` which implements `AsyncWrite + AsyncSeek + Unpin`,
//! enabling `AsyncStreamingZipWriter` to stream ZIP files directly to S3 without loading
//! the entire archive into memory.
//!
//! ## How it Works
//!
//! - Uses S3 multipart upload (minimum 5MB per part, except the last part)
//! - Buffers writes until reaching part size threshold
//! - Uploads parts in the background using Tokio tasks
//! - Tracks virtual position for ZIP central directory (no actual seeking)
//! - Maintains constant memory usage (~5-10MB)
//!
//! ## Example
//!
//! ```no_run
//! use s_zip::{AsyncStreamingZipWriter, cloud::S3ZipWriter};
//! use aws_sdk_s3::Client;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = aws_config::load_from_env().await;
//! let s3_client = Client::new(&config);
//!
//! let writer = S3ZipWriter::new(s3_client, "my-bucket", "exports/data.zip").await?;
//! let mut zip = AsyncStreamingZipWriter::from_writer(writer);
//!
//! zip.start_entry("file.txt").await?;
//! zip.write_data(b"Hello S3!").await?;
//! zip.finish().await?;
//! # Ok(())
//! # }
//! ```

use crate::error::{Result, SZipError};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart};
use aws_sdk_s3::Client;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::io::{AsyncSeek, AsyncWrite};
use tokio::sync::mpsc;

/// Default part size for S3 multipart upload (5MB - S3 minimum)
pub const DEFAULT_PART_SIZE: usize = 5 * 1024 * 1024;

/// Maximum part size (5GB - S3 maximum)
pub const MAX_PART_SIZE: usize = 5 * 1024 * 1024 * 1024;

/// Maximum number of parts (S3 limit)
pub const MAX_PARTS: usize = 10_000;

/// S3 ZIP writer that streams directly to S3 using multipart upload.
///
/// This writer implements `AsyncWrite + AsyncSeek + Unpin`, making it compatible
/// with `AsyncStreamingZipWriter`.
pub struct S3ZipWriter {
    /// Upload state (managed by background task)
    upload_tx: mpsc::UnboundedSender<UploadCommand>,
    upload_task: Option<tokio::task::JoinHandle<Result<()>>>,

    /// Write buffer (accumulates data until part_size)
    buffer: Vec<u8>,
    part_size: usize,

    /// Virtual position tracking (for ZIP central directory)
    position: u64,

    /// Current part number
    current_part_number: usize,

    /// Flag to prevent sending Complete command multiple times
    shutdown_initiated: bool,
}

/// Commands sent to the background upload task
enum UploadCommand {
    /// Upload a part with given data
    UploadPart { part_number: usize, data: Vec<u8> },
    /// Complete the upload with optional final part
    Complete { final_data: Option<Vec<u8>> },
}

/// Builder for `S3ZipWriter` with configuration options.
pub struct S3ZipWriterBuilder {
    client: Client,
    bucket: String,
    key: String,
    part_size: usize,
}

impl S3ZipWriter {
    /// Create a new S3 ZIP writer with default settings.
    ///
    /// Uses 5MB part size (S3 minimum).
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use s_zip::cloud::S3ZipWriter;
    /// # use aws_sdk_s3::Client;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = aws_config::load_from_env().await;
    /// let client = Client::new(&config);
    ///
    /// let writer = S3ZipWriter::new(
    ///     client,
    ///     "my-bucket",
    ///     "exports/archive.zip"
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(
        client: Client,
        bucket: impl Into<String>,
        key: impl Into<String>,
    ) -> Result<Self> {
        Self::builder()
            .client(client)
            .bucket(bucket)
            .key(key)
            .build()
            .await
    }

    /// Create a builder for configuring the S3 writer.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use s_zip::cloud::S3ZipWriter;
    /// # use aws_sdk_s3::Client;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Client::new(&aws_config::load_from_env().await);
    ///
    /// let writer = S3ZipWriter::builder()
    ///     .client(client)
    ///     .bucket("my-bucket")
    ///     .key("large-archive.zip")
    ///     .part_size(100 * 1024 * 1024)  // 100MB parts for huge files
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn builder() -> S3ZipWriterBuilder {
        S3ZipWriterBuilder {
            client: Client::from_conf(aws_sdk_s3::Config::builder().build()),
            bucket: String::new(),
            key: String::new(),
            part_size: DEFAULT_PART_SIZE,
        }
    }
}

impl S3ZipWriterBuilder {
    /// Set the S3 client.
    pub fn client(mut self, client: Client) -> Self {
        self.client = client;
        self
    }

    /// Set the S3 bucket name.
    pub fn bucket(mut self, bucket: impl Into<String>) -> Self {
        self.bucket = bucket.into();
        self
    }

    /// Set the S3 object key (path).
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = key.into();
        self
    }

    /// Set the part size for multipart upload.
    ///
    /// Must be at least 5MB (except the final part). Larger parts reduce the number
    /// of API calls but increase memory usage.
    ///
    /// # Panics
    ///
    /// Panics if part_size < 5MB or > 5GB.
    pub fn part_size(mut self, part_size: usize) -> Self {
        assert!(
            part_size >= DEFAULT_PART_SIZE,
            "Part size must be at least 5MB"
        );
        assert!(part_size <= MAX_PART_SIZE, "Part size must not exceed 5GB");
        self.part_size = part_size;
        self
    }

    /// Build the S3 writer and start the background upload task.
    pub async fn build(self) -> Result<S3ZipWriter> {
        let (tx, rx) = mpsc::unbounded_channel();

        // Spawn background task for uploading parts
        let upload_task = tokio::spawn(upload_worker(self.client, self.bucket, self.key, rx));

        Ok(S3ZipWriter {
            upload_tx: tx,
            upload_task: Some(upload_task),
            buffer: Vec::with_capacity(self.part_size),
            part_size: self.part_size,
            position: 0,
            current_part_number: 0,
            shutdown_initiated: false,
        })
    }
}

impl AsyncWrite for S3ZipWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        // Add data to buffer
        self.buffer.extend_from_slice(buf);
        self.position += buf.len() as u64;

        // Check if we should flush a part
        if self.buffer.len() >= self.part_size {
            let part_size = self.part_size;
            let data = std::mem::replace(&mut self.buffer, Vec::with_capacity(part_size));
            self.current_part_number += 1;

            // Send to background task (non-blocking)
            if self
                .upload_tx
                .send(UploadCommand::UploadPart {
                    part_number: self.current_part_number,
                    data,
                })
                .is_err()
            {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Upload task terminated unexpectedly",
                )));
            }
        }

        Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Flushing is handled by the background task
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Only send Complete command once
        if !self.shutdown_initiated {
            self.shutdown_initiated = true;

            // Send final part (if any) and complete upload
            let final_data = if !self.buffer.is_empty() {
                Some(std::mem::take(&mut self.buffer))
            } else {
                None
            };

            // Send completion command
            if self
                .upload_tx
                .send(UploadCommand::Complete { final_data })
                .is_err()
            {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "Upload task terminated unexpectedly",
                )));
            }
        }

        // Wait for background task to complete
        if let Some(task) = self.upload_task.as_mut() {
            match Pin::new(task).poll(cx) {
                Poll::Ready(Ok(Ok(()))) => Poll::Ready(Ok(())),
                Poll::Ready(Ok(Err(e))) => {
                    Poll::Ready(Err(io::Error::other(format!("S3 upload failed: {}", e))))
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

impl AsyncSeek for S3ZipWriter {
    fn start_seek(self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        // S3 doesn't support seeking - we only track virtual position
        match position {
            io::SeekFrom::Current(0) => Ok(()), // Query current position (allowed)
            _ => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "S3 writer does not support seeking",
            )),
        }
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        // Return tracked virtual position
        Poll::Ready(Ok(self.position))
    }
}

impl Unpin for S3ZipWriter {}

/// Background worker that handles S3 multipart upload operations.
async fn upload_worker(
    client: Client,
    bucket: String,
    key: String,
    mut rx: mpsc::UnboundedReceiver<UploadCommand>,
) -> Result<()> {
    let mut upload_id: Option<String> = None;
    let mut parts: Vec<CompletedPart> = Vec::new();

    while let Some(cmd) = rx.recv().await {
        match cmd {
            UploadCommand::UploadPart { part_number, data } => {
                // Initialize multipart upload if first part
                if upload_id.is_none() {
                    let response = client
                        .create_multipart_upload()
                        .bucket(&bucket)
                        .key(&key)
                        .send()
                        .await
                        .map_err(|e| {
                            SZipError::Io(io::Error::other(format!(
                                "Failed to create multipart upload: {}",
                                e
                            )))
                        })?;

                    upload_id = Some(
                        response
                            .upload_id()
                            .ok_or_else(|| {
                                SZipError::Io(io::Error::other("No upload_id returned from S3"))
                            })?
                            .to_string(),
                    );
                }

                // Upload part
                let response = client
                    .upload_part()
                    .bucket(&bucket)
                    .key(&key)
                    .upload_id(upload_id.as_ref().unwrap())
                    .part_number(part_number as i32)
                    .body(ByteStream::from(data))
                    .send()
                    .await
                    .map_err(|e| {
                        SZipError::Io(io::Error::other(format!(
                            "Failed to upload part {}: {}",
                            part_number, e
                        )))
                    })?;

                let etag = response
                    .e_tag()
                    .ok_or_else(|| {
                        SZipError::Io(io::Error::other(format!(
                            "No ETag returned for part {}",
                            part_number
                        )))
                    })?
                    .to_string();

                parts.push(
                    CompletedPart::builder()
                        .part_number(part_number as i32)
                        .e_tag(etag)
                        .build(),
                );
            }
            UploadCommand::Complete { final_data } => {
                // Upload final part if any data remains
                if let Some(data) = final_data {
                    if !data.is_empty() {
                        // Initialize upload if this is the only part
                        if upload_id.is_none() {
                            let response = client
                                .create_multipart_upload()
                                .bucket(&bucket)
                                .key(&key)
                                .send()
                                .await
                                .map_err(|e| {
                                    SZipError::Io(io::Error::other(format!(
                                        "Failed to create multipart upload: {}",
                                        e
                                    )))
                                })?;

                            upload_id = Some(
                                response
                                    .upload_id()
                                    .ok_or_else(|| {
                                        SZipError::Io(io::Error::other(
                                            "No upload_id returned from S3",
                                        ))
                                    })?
                                    .to_string(),
                            );
                        }

                        let part_number = parts.len() + 1;
                        let response = client
                            .upload_part()
                            .bucket(&bucket)
                            .key(&key)
                            .upload_id(upload_id.as_ref().unwrap())
                            .part_number(part_number as i32)
                            .body(ByteStream::from(data))
                            .send()
                            .await
                            .map_err(|e| {
                                SZipError::Io(io::Error::other(format!(
                                    "Failed to upload final part: {}",
                                    e
                                )))
                            })?;

                        let etag = response
                            .e_tag()
                            .ok_or_else(|| {
                                SZipError::Io(io::Error::other("No ETag returned for final part"))
                            })?
                            .to_string();

                        parts.push(
                            CompletedPart::builder()
                                .part_number(part_number as i32)
                                .e_tag(etag)
                                .build(),
                        );
                    }
                }

                // Complete multipart upload
                if let Some(id) = upload_id {
                    client
                        .complete_multipart_upload()
                        .bucket(&bucket)
                        .key(&key)
                        .upload_id(&id)
                        .multipart_upload(
                            CompletedMultipartUpload::builder()
                                .set_parts(Some(parts))
                                .build(),
                        )
                        .send()
                        .await
                        .map_err(|e| {
                            SZipError::Io(io::Error::other(format!(
                                "Failed to complete multipart upload: {}",
                                e
                            )))
                        })?;
                }

                break;
            }
        }
    }

    Ok(())
}

impl Drop for S3ZipWriter {
    fn drop(&mut self) {
        // If the writer is dropped without calling finish(), we should try to abort
        // the multipart upload to avoid orphaned parts
        // However, we can't easily abort from Drop since it's not async
        // Users should ensure finish() is called properly
    }
}

// ============================================================================
// S3 ZIP Reader
// ============================================================================

use tokio::io::AsyncRead;

/// S3 ZIP reader that reads ZIP files directly from S3.
///
/// This reader implements `AsyncRead + AsyncSeek + Unpin + Send`, making it compatible
/// with `GenericAsyncZipReader`.
///
/// ## Example
///
/// ```no_run
/// use s_zip::{GenericAsyncZipReader, cloud::S3ZipReader};
/// use aws_sdk_s3::Client;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = aws_config::load_from_env().await;
/// let s3_client = Client::new(&config);
///
/// let reader = S3ZipReader::new(s3_client, "my-bucket", "archive.zip").await?;
/// let mut zip = GenericAsyncZipReader::new(reader).await?;
///
/// // List entries
/// for entry in zip.entries() {
///     println!("{}: {} bytes", entry.name, entry.uncompressed_size);
/// }
///
/// // Read a file
/// let data = zip.read_entry_by_name("file.txt").await?;
/// # Ok(())
/// # }
/// ```
pub struct S3ZipReader {
    client: Client,
    bucket: String,
    key: String,
    position: u64,
    size: u64,
    #[allow(clippy::type_complexity)]
    read_future: Option<Pin<Box<dyn Future<Output = io::Result<Vec<u8>>> + Send>>>,
}

impl S3ZipReader {
    /// Create a new S3 ZIP reader.
    ///
    /// # Arguments
    ///
    /// * `client` - AWS S3 client
    /// * `bucket` - S3 bucket name
    /// * `key` - S3 object key (path)
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use s_zip::cloud::S3ZipReader;
    /// # use aws_sdk_s3::Client;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = aws_config::load_from_env().await;
    /// let client = Client::new(&config);
    ///
    /// let reader = S3ZipReader::new(
    ///     client,
    ///     "my-bucket",
    ///     "exports/archive.zip"
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(
        client: Client,
        bucket: impl Into<String>,
        key: impl Into<String>,
    ) -> Result<Self> {
        let bucket = bucket.into();
        let key = key.into();

        // Get object metadata to determine size
        let head = client
            .head_object()
            .bucket(&bucket)
            .key(&key)
            .send()
            .await
            .map_err(|e| {
                SZipError::Io(io::Error::other(format!(
                    "Failed to get S3 object metadata: {}",
                    e
                )))
            })?;

        let size = head
            .content_length()
            .ok_or_else(|| SZipError::Io(io::Error::other("S3 object has no content length")))?
            as u64;

        Ok(Self {
            client,
            bucket,
            key,
            position: 0,
            size,
            read_future: None,
        })
    }

    /// Get the total size of the S3 object.
    pub fn size(&self) -> u64 {
        self.size
    }
}

impl AsyncRead for S3ZipReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        // If we already have a pending future, poll it
        if let Some(fut) = self.read_future.as_mut() {
            match fut.as_mut().poll(cx) {
                Poll::Ready(Ok(bytes)) => {
                    let n = bytes.len().min(buf.remaining());
                    buf.put_slice(&bytes[..n]);
                    self.position += n as u64;
                    self.read_future = None;
                    return Poll::Ready(Ok(()));
                }
                Poll::Ready(Err(e)) => {
                    self.read_future = None;
                    return Poll::Ready(Err(e));
                }
                Poll::Pending => return Poll::Pending,
            }
        }

        // Calculate byte range to read
        let start = self.position;
        let end = (start + buf.remaining() as u64 - 1).min(self.size - 1);

        if start >= self.size {
            return Poll::Ready(Ok(())); // EOF
        }

        let range = format!("bytes={}-{}", start, end);

        // Create future for reading from S3
        let client = self.client.clone();
        let bucket = self.bucket.clone();
        let key = self.key.clone();

        let fut = Box::pin(async move {
            let response = client
                .get_object()
                .bucket(&bucket)
                .key(&key)
                .range(range)
                .send()
                .await
                .map_err(|e| io::Error::other(format!("S3 GetObject failed: {}", e)))?;

            let bytes = response
                .body
                .collect()
                .await
                .map_err(|e| io::Error::other(format!("Failed to read S3 body: {}", e)))?;

            Ok::<_, io::Error>(bytes.into_bytes().to_vec())
        });

        // Store the future and poll it
        self.read_future = Some(fut);

        // Re-enter poll_read to poll the new future
        self.poll_read(cx, buf)
    }
}

impl AsyncSeek for S3ZipReader {
    fn start_seek(mut self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        let new_pos = match position {
            io::SeekFrom::Start(pos) => pos as i64,
            io::SeekFrom::End(offset) => self.size as i64 + offset,
            io::SeekFrom::Current(offset) => self.position as i64 + offset,
        };

        if new_pos < 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid seek position",
            ));
        }

        self.position = new_pos as u64;
        Ok(())
    }

    fn poll_complete(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<u64>> {
        Poll::Ready(Ok(self.position))
    }
}

impl Unpin for S3ZipReader {}

unsafe impl Send for S3ZipReader {}
