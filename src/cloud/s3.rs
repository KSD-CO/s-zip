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
use std::sync::Arc;
use std::task::{ready, Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncSeek, AsyncWrite};
use tokio::sync::mpsc;
use tokio_util::sync::PollSender;

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
    upload_tx: PollSender<UploadCommand>,
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

    /// Maximum concurrent part uploads (default: 4)
    #[allow(dead_code)]
    max_concurrent_uploads: usize,
}

/// Commands sent to the background upload task
enum UploadCommand {
    /// Upload a part with given data
    UploadPart { part_number: usize, data: Vec<u8> },
    /// Complete the upload with optional final part
    Complete { final_data: Option<Vec<u8>> },
}

/// Builder for `S3ZipWriter` with configuration options.
///
/// Supports MinIO and other S3-compatible storage services by allowing
/// custom endpoint URLs.
///
/// ## Using with MinIO
///
/// ```no_run
/// # use s_zip::cloud::S3ZipWriter;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let writer = S3ZipWriter::builder()
///     .endpoint_url("http://localhost:9000")
///     .region("us-east-1")
///     .bucket("my-bucket")
///     .key("archive.zip")
///     .build()
///     .await?;
/// # Ok(())
/// # }
/// ```
///
/// ## Using with Cloudflare R2
///
/// ```no_run
/// # use s_zip::cloud::S3ZipWriter;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let writer = S3ZipWriter::builder()
///     .endpoint_url("https://<account_id>.r2.cloudflarestorage.com")
///     .bucket("my-bucket")
///     .key("archive.zip")
///     .build()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct S3ZipWriterBuilder {
    client: Option<Client>,
    bucket: String,
    key: String,
    part_size: usize,
    endpoint_url: Option<String>,
    region: Option<String>,
    force_path_style: bool,
    max_concurrent_uploads: usize,
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
    ///     .max_concurrent_uploads(8)      // Upload 8 parts in parallel
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn builder() -> S3ZipWriterBuilder {
        S3ZipWriterBuilder {
            client: None,
            bucket: String::new(),
            key: String::new(),
            part_size: DEFAULT_PART_SIZE,
            endpoint_url: None,
            region: None,
            force_path_style: false,
            max_concurrent_uploads: 4, // Default: 4 concurrent uploads
        }
    }
}

impl S3ZipWriterBuilder {
    /// Set a pre-configured S3 client.
    ///
    /// If not set, a client will be created automatically using environment
    /// credentials and any configured endpoint/region.
    pub fn client(mut self, client: Client) -> Self {
        self.client = Some(client);
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

    /// Set a custom endpoint URL for S3-compatible services.
    ///
    /// Use this for MinIO, Cloudflare R2, DigitalOcean Spaces, Backblaze B2,
    /// Linode Object Storage, and other S3-compatible services.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use s_zip::cloud::S3ZipWriter;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// // MinIO
    /// let writer = S3ZipWriter::builder()
    ///     .endpoint_url("http://localhost:9000")
    ///     .bucket("my-bucket")
    ///     .key("archive.zip")
    ///     .build()
    ///     .await?;
    ///
    /// // Cloudflare R2
    /// let writer = S3ZipWriter::builder()
    ///     .endpoint_url("https://account_id.r2.cloudflarestorage.com")
    ///     .bucket("my-bucket")
    ///     .key("archive.zip")
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn endpoint_url(mut self, url: impl Into<String>) -> Self {
        self.endpoint_url = Some(url.into());
        self
    }

    /// Set the AWS region.
    ///
    /// For MinIO and some S3-compatible services, you may need to set this
    /// to a specific value (e.g., "us-east-1").
    pub fn region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Force path-style URLs instead of virtual-hosted style.
    ///
    /// Required for MinIO and some S3-compatible services that don't support
    /// virtual-hosted style URLs (e.g., `http://endpoint/bucket/key` instead of
    /// `http://bucket.endpoint/key`).
    ///
    /// Default: `false` (uses virtual-hosted style for AWS S3)
    pub fn force_path_style(mut self, force: bool) -> Self {
        self.force_path_style = force;
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

    /// Set maximum number of concurrent part uploads.
    ///
    /// Higher values increase throughput but use more network connections.
    /// Default is 4, which provides good balance between speed and resource usage.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use s_zip::cloud::S3ZipWriter;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let writer = S3ZipWriter::builder()
    ///     .bucket("my-bucket")
    ///     .key("archive.zip")
    ///     .max_concurrent_uploads(8)  // 8 parts in parallel for faster uploads
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn max_concurrent_uploads(mut self, max: usize) -> Self {
        assert!(max > 0, "max_concurrent_uploads must be at least 1");
        assert!(max <= 20, "max_concurrent_uploads should not exceed 20");
        self.max_concurrent_uploads = max;
        self
    }

    /// Build the S3 writer and start the background upload task.
    ///
    /// If no client was provided, one will be created using environment credentials
    /// and any configured endpoint/region settings.
    pub async fn build(self) -> Result<S3ZipWriter> {
        let client = match self.client {
            Some(c) => c,
            None => {
                // Build client from configuration
                let mut config_loader = aws_config::from_env();

                if let Some(ref endpoint) = self.endpoint_url {
                    config_loader = config_loader.endpoint_url(endpoint);
                }

                if let Some(ref region) = self.region {
                    config_loader = config_loader.region(aws_config::Region::new(region.clone()));
                }

                let sdk_config = config_loader.load().await;

                let mut s3_config = aws_sdk_s3::config::Builder::from(&sdk_config);

                if self.force_path_style {
                    s3_config = s3_config.force_path_style(true);
                }

                Client::from_conf(s3_config.build())
            }
        };

        // Capacity 1 + in-flight uploads bounds memory to ~(max_concurrent + 2) parts
        let (tx, rx) = mpsc::channel(1);

        // Spawn background task for uploading parts with concurrent support
        let max_concurrent = self.max_concurrent_uploads;
        let upload_task = tokio::spawn(upload_worker_concurrent(
            client,
            self.bucket,
            self.key,
            rx,
            max_concurrent,
        ));

        Ok(S3ZipWriter {
            upload_tx: PollSender::new(tx),
            upload_task: Some(upload_task),
            buffer: Vec::with_capacity(self.part_size),
            part_size: self.part_size,
            position: 0,
            current_part_number: 0,
            shutdown_initiated: false,
            max_concurrent_uploads: self.max_concurrent_uploads,
        })
    }
}

impl AsyncWrite for S3ZipWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        // Hand a full buffer to the upload task first; wait (backpressure) while
        // the task is busy so memory stays bounded when the network is slower
        if self.buffer.len() >= self.part_size {
            if ready!(self.upload_tx.poll_reserve(cx)).is_err() {
                return Poll::Ready(Err(upload_task_gone()));
            }
            let part_size = self.part_size;
            let data = std::mem::replace(&mut self.buffer, Vec::with_capacity(part_size));
            self.current_part_number += 1;
            let part_number = self.current_part_number;
            if self
                .upload_tx
                .send_item(UploadCommand::UploadPart { part_number, data })
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

            // Send final part (if any) and complete upload
            let final_data = if !self.buffer.is_empty() {
                Some(std::mem::take(&mut self.buffer))
            } else {
                None
            };

            if self
                .upload_tx
                .send_item(UploadCommand::Complete { final_data })
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

fn upload_task_gone() -> io::Error {
    io::Error::new(
        io::ErrorKind::BrokenPipe,
        "Upload task terminated unexpectedly",
    )
}

/// Concurrent upload worker with retry logic and parallel uploads
///
/// Uploads up to `max_concurrent` parts in parallel and only accepts a new part
/// from the writer when a slot is free, which (together with the bounded
/// channel) gives the writer backpressure. Includes automatic retry with
/// exponential backoff for transient failures. If the writer is dropped before
/// `finish()`, the multipart upload is aborted so no orphaned parts remain.
async fn upload_worker_concurrent(
    client: Client,
    bucket: String,
    key: String,
    mut rx: mpsc::Receiver<UploadCommand>,
    max_concurrent: usize,
) -> Result<()> {
    use futures_util::stream::{FuturesUnordered, StreamExt};

    let client = Arc::new(client);
    let bucket = Arc::new(bucket);
    let key = Arc::new(key);
    let mut upload_id: Option<String> = None;
    let mut completed_parts: Vec<(usize, CompletedPart)> = Vec::new();
    let mut upload_futures = FuturesUnordered::new();

    let result: Result<()> = async {
        loop {
            tokio::select! {
                Some(result) = upload_futures.next(), if !upload_futures.is_empty() => {
                    completed_parts.push(result?);
                }
                cmd = rx.recv(), if upload_futures.len() < max_concurrent => {
                    match cmd {
                        Some(UploadCommand::UploadPart { part_number, data }) => {
                            let id = ensure_multipart_upload(&client, &bucket, &key, &mut upload_id).await?;
                            upload_futures.push(upload_part_with_retry(
                                client.clone(),
                                bucket.clone(),
                                key.clone(),
                                id,
                                part_number,
                                data,
                            ));
                        }
                        Some(UploadCommand::Complete { final_data }) => {
                            if let Some(data) = final_data.filter(|d| !d.is_empty()) {
                                let id = ensure_multipart_upload(&client, &bucket, &key, &mut upload_id).await?;
                                let part_number = completed_parts.len() + upload_futures.len() + 1;
                                upload_futures.push(upload_part_with_retry(
                                    client.clone(),
                                    bucket.clone(),
                                    key.clone(),
                                    id,
                                    part_number,
                                    data,
                                ));
                            }

                            // Wait for all remaining uploads to complete
                            while let Some(result) = upload_futures.next().await {
                                completed_parts.push(result?);
                            }

                            // Sort parts by part number (S3 requires sequential order)
                            completed_parts.sort_by_key(|(part_num, _)| *part_num);
                            let parts: Vec<_> =
                                std::mem::take(&mut completed_parts).into_iter().map(|(_, p)| p).collect();

                            // Complete multipart upload
                            if let Some(id) = upload_id.take() {
                                client
                                    .complete_multipart_upload()
                                    .bucket(bucket.as_ref())
                                    .key(key.as_ref())
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
                            return Ok(());
                        }
                        // Writer dropped before finish()
                        None => {
                            return Err(SZipError::Io(io::Error::new(
                                io::ErrorKind::UnexpectedEof,
                                "S3 writer dropped before finish()",
                            )));
                        }
                    }
                }
            }
        }
    }
    .await;

    // On any failure, abort the multipart upload so S3 does not keep (and bill) orphaned parts
    if result.is_err() {
        if let Some(id) = upload_id {
            let _ = client
                .abort_multipart_upload()
                .bucket(bucket.as_ref())
                .key(key.as_ref())
                .upload_id(id)
                .send()
                .await;
        }
    }
    result
}

/// Start the multipart upload on first use and return its id.
async fn ensure_multipart_upload(
    client: &Client,
    bucket: &str,
    key: &str,
    upload_id: &mut Option<String>,
) -> Result<String> {
    if let Some(id) = upload_id {
        return Ok(id.clone());
    }
    let response = client
        .create_multipart_upload()
        .bucket(bucket)
        .key(key)
        .send()
        .await
        .map_err(|e| {
            SZipError::Io(io::Error::other(format!(
                "Failed to create multipart upload: {}",
                e
            )))
        })?;
    let id = response
        .upload_id()
        .ok_or_else(|| SZipError::Io(io::Error::other("No upload_id returned from S3")))?
        .to_string();
    *upload_id = Some(id.clone());
    Ok(id)
}

/// Upload a single part with exponential backoff retry
async fn upload_part_with_retry(
    client: Arc<Client>,
    bucket: Arc<String>,
    key: Arc<String>,
    upload_id: String,
    part_number: usize,
    data: Vec<u8>,
) -> Result<(usize, CompletedPart)> {
    const MAX_RETRIES: u32 = 3;
    const BASE_DELAY_MS: u64 = 100;

    let mut retries = 0;

    loop {
        match client
            .upload_part()
            .bucket(bucket.as_ref())
            .key(key.as_ref())
            .upload_id(&upload_id)
            .part_number(part_number as i32)
            .body(ByteStream::from(data.clone()))
            .send()
            .await
        {
            Ok(response) => {
                let etag = response
                    .e_tag()
                    .ok_or_else(|| {
                        SZipError::Io(io::Error::other(format!(
                            "No ETag returned for part {}",
                            part_number
                        )))
                    })?
                    .to_string();

                let completed_part = CompletedPart::builder()
                    .part_number(part_number as i32)
                    .e_tag(etag)
                    .build();

                return Ok((part_number, completed_part));
            }
            Err(_e) if retries < MAX_RETRIES => {
                retries += 1;
                let delay = BASE_DELAY_MS * 2_u64.pow(retries - 1);
                tokio::time::sleep(Duration::from_millis(delay)).await;
                // Continue loop to retry
            }
            Err(e) => {
                return Err(SZipError::Io(io::Error::other(format!(
                    "Failed to upload part {} after {} retries: {}",
                    part_number, MAX_RETRIES, e
                ))));
            }
        }
    }
}

impl Drop for S3ZipWriter {
    fn drop(&mut self) {
        // Dropping the sender makes the upload task abort the multipart upload
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
/// let mut zip = GenericAsyncZipReader::new_with_buffer_size(reader, None).await?;
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

/// Builder for `S3ZipReader` with configuration options.
///
/// Supports MinIO and other S3-compatible storage services.
///
/// ## Using with MinIO
///
/// ```no_run
/// # use s_zip::cloud::S3ZipReader;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let reader = S3ZipReader::builder()
///     .endpoint_url("http://localhost:9000")
///     .region("us-east-1")
///     .bucket("my-bucket")
///     .key("archive.zip")
///     .build()
///     .await?;
/// # Ok(())
/// # }
/// ```
pub struct S3ZipReaderBuilder {
    client: Option<Client>,
    bucket: String,
    key: String,
    endpoint_url: Option<String>,
    region: Option<String>,
    force_path_style: bool,
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
        Self::builder()
            .client(client)
            .bucket(bucket)
            .key(key)
            .build()
            .await
    }

    /// Create a builder for configuring the S3 reader.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use s_zip::cloud::S3ZipReader;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// // Read from MinIO
    /// let reader = S3ZipReader::builder()
    ///     .endpoint_url("http://localhost:9000")
    ///     .bucket("my-bucket")
    ///     .key("archive.zip")
    ///     .build()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn builder() -> S3ZipReaderBuilder {
        S3ZipReaderBuilder {
            client: None,
            bucket: String::new(),
            key: String::new(),
            endpoint_url: None,
            region: None,
            force_path_style: false,
        }
    }

    /// Get the total size of the S3 object.
    pub fn size(&self) -> u64 {
        self.size
    }
}

impl S3ZipReaderBuilder {
    /// Set a pre-configured S3 client.
    pub fn client(mut self, client: Client) -> Self {
        self.client = Some(client);
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

    /// Set a custom endpoint URL for S3-compatible services.
    ///
    /// Use this for MinIO, Cloudflare R2, DigitalOcean Spaces, etc.
    pub fn endpoint_url(mut self, url: impl Into<String>) -> Self {
        self.endpoint_url = Some(url.into());
        self
    }

    /// Set the AWS region.
    pub fn region(mut self, region: impl Into<String>) -> Self {
        self.region = Some(region.into());
        self
    }

    /// Force path-style URLs instead of virtual-hosted style.
    pub fn force_path_style(mut self, force: bool) -> Self {
        self.force_path_style = force;
        self
    }

    /// Build the S3 reader.
    pub async fn build(self) -> Result<S3ZipReader> {
        let client = match self.client {
            Some(c) => c,
            None => {
                let mut config_loader = aws_config::from_env();

                if let Some(ref endpoint) = self.endpoint_url {
                    config_loader = config_loader.endpoint_url(endpoint);
                }

                if let Some(ref region) = self.region {
                    config_loader = config_loader.region(aws_config::Region::new(region.clone()));
                }

                let sdk_config = config_loader.load().await;

                let mut s3_config = aws_sdk_s3::config::Builder::from(&sdk_config);

                if self.force_path_style {
                    s3_config = s3_config.force_path_style(true);
                }

                Client::from_conf(s3_config.build())
            }
        };

        // Get object metadata to determine size
        let head = client
            .head_object()
            .bucket(&self.bucket)
            .key(&self.key)
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

        Ok(S3ZipReader {
            client,
            bucket: self.bucket,
            key: self.key,
            position: 0,
            size,
            read_future: None,
        })
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

// SAFETY: Every field of `S3ZipReader` is `Send`: `Client` (aws-sdk-s3) is
// `Send + Sync` (internally `Arc`-backed), `String`/`u64` are trivially `Send`,
// and `read_future` is explicitly typed as
// `Pin<Box<dyn Future<Output = io::Result<Vec<u8>>> + Send>>`. This impl should
// in fact be derivable automatically; it is written explicitly to guarantee
// `Send` remains enforced even if a future field addition would otherwise make
// it non-obvious to the compiler (e.g. behind another `dyn Trait`).
unsafe impl Send for S3ZipReader {}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[test]
    fn test_writer_applies_backpressure() {
        let (tx, mut rx) = mpsc::channel(1);
        let mut writer = S3ZipWriter {
            upload_tx: PollSender::new(tx),
            upload_task: None,
            buffer: Vec::new(),
            part_size: 4,
            position: 0,
            current_part_number: 0,
            shutdown_initiated: false,
            max_concurrent_uploads: 1,
        };

        // Nobody drains the channel: after one queued part the writer must stall
        let data = [0u8; 4];
        let mut task = tokio_test::task::spawn(async {
            for _ in 0..4 {
                writer.write_all(&data).await.unwrap();
            }
        });
        assert!(task.poll().is_pending());

        // Draining one part lets the writer make progress again
        assert!(matches!(
            rx.try_recv(),
            Ok(UploadCommand::UploadPart { part_number: 1, .. })
        ));
        assert!(task.is_woken());
        assert!(task.poll().is_pending());
        assert!(matches!(
            rx.try_recv(),
            Ok(UploadCommand::UploadPart { part_number: 2, .. })
        ));
    }
}
