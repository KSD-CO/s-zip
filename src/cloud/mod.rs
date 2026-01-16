//! Cloud storage adapters for streaming ZIP files directly to S3, GCS, etc.
//!
//! This module provides adapters that implement `AsyncWrite + AsyncSeek + Unpin`
//! to enable `AsyncStreamingZipWriter` to stream directly to cloud storage without
//! loading entire ZIPs into memory.
//!
//! ## Available Adapters
//!
//! - **S3** - AWS S3 multipart upload (requires `cloud-s3` feature)
//! - **GCS** - Google Cloud Storage resumable upload (requires `cloud-gcs` feature)
//!
//! ## S3-Compatible Services
//!
//! The S3 adapter supports MinIO, Cloudflare R2, DigitalOcean Spaces, Backblaze B2,
//! Linode Object Storage, and other S3-compatible services via custom endpoint URLs.
//!
//! ## Example Usage
//!
//! ### AWS S3
//!
//! ```no_run
//! # #[cfg(feature = "cloud-s3")]
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use s_zip::{AsyncStreamingZipWriter, cloud::S3ZipWriter};
//! use aws_sdk_s3::Client;
//!
//! let s3_client = Client::new(&aws_config::load_from_env().await);
//! let writer = S3ZipWriter::new(s3_client, "my-bucket", "output.zip").await?;
//!
//! let mut zip = AsyncStreamingZipWriter::from_writer(writer);
//! zip.start_entry("hello.txt").await?;
//! zip.write_data(b"Hello S3!").await?;
//! zip.finish().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### MinIO / S3-Compatible
//!
//! ```no_run
//! # #[cfg(feature = "cloud-s3")]
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! use s_zip::{AsyncStreamingZipWriter, cloud::S3ZipWriter};
//!
//! // Write to MinIO
//! let writer = S3ZipWriter::builder()
//!     .endpoint_url("http://localhost:9000")
//!     .region("us-east-1")
//!     .bucket("my-bucket")
//!     .key("output.zip")
//!     .build()
//!     .await?;
//!
//! let mut zip = AsyncStreamingZipWriter::from_writer(writer);
//! zip.start_entry("hello.txt").await?;
//! zip.write_data(b"Hello MinIO!").await?;
//! zip.finish().await?;
//! # Ok(())
//! # }
//! ```

#[cfg(feature = "cloud-s3")]
pub mod s3;

#[cfg(feature = "cloud-gcs")]
pub mod gcs;

#[cfg(feature = "cloud-s3")]
pub use s3::{S3ZipReader, S3ZipReaderBuilder, S3ZipWriter, S3ZipWriterBuilder};

#[cfg(feature = "cloud-gcs")]
pub use gcs::GCSZipWriter;
