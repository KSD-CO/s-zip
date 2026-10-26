//! End-to-end tests against local cloud emulators. Ignored by default; run with
//!
//! ```sh
//! docker run -d -p 9000:9090 -e initialBuckets=szip adobe/s3mock
//! docker run -d -p 10000:10000 mcr.microsoft.com/azure-storage/azurite \
//!     azurite-blob --blobHost 0.0.0.0 --loose --skipApiVersionCheck
//! # The GCS SDK sends `Host: localhost` without a port, so fake-gcs-server must
//! # be reachable on port 80 for its resumable-upload URLs to work
//! docker run -d -p 80:4443 fsouza/fake-gcs-server -scheme http -port 4443 \
//!     -external-url http://localhost
//! cargo test --all-features --test cloud_e2e -- --ignored
//! ```

#![allow(dead_code, unused_imports)]

use s_zip::{AsyncStreamingZipWriter, GenericAsyncZipReader};

/// ~12 MiB of incompressible data, so uploads span several parts/blocks/chunks.
fn payload() -> Vec<u8> {
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    (0..12 * 1024 * 1024)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x as u8
        })
        .collect()
}

async fn write_zip<W>(writer: W, data: &[u8])
where
    W: tokio::io::AsyncWrite + tokio::io::AsyncSeek + Unpin,
{
    let mut zip = AsyncStreamingZipWriter::from_writer(writer);
    zip.add_entry("small.txt", b"hello cloud").await.unwrap();
    zip.add_entry("big.bin", data).await.unwrap();
    zip.finish().await.unwrap();
}

async fn verify_zip<R>(reader: R, data: &[u8])
where
    R: tokio::io::AsyncRead + tokio::io::AsyncSeek + Unpin + Send,
{
    let mut zip = GenericAsyncZipReader::new_with_buffer_size(reader, None)
        .await
        .unwrap();
    assert_eq!(zip.entries().len(), 2);
    assert_eq!(
        zip.read_entry_by_name("small.txt").await.unwrap(),
        b"hello cloud"
    );
    assert_eq!(zip.read_entry_by_name("big.bin").await.unwrap(), data);
}

#[cfg(feature = "cloud-s3")]
#[tokio::test]
#[ignore = "needs an S3 emulator on localhost:9000"]
async fn test_s3_roundtrip() {
    use s_zip::cloud::{S3ZipReader, S3ZipWriter};
    std::env::set_var("AWS_ACCESS_KEY_ID", "test");
    std::env::set_var("AWS_SECRET_ACCESS_KEY", "test");

    let config = aws_config::from_env()
        .endpoint_url("http://localhost:9000")
        .region(aws_config::Region::new("us-east-1"))
        .load()
        .await;
    let client = aws_sdk_s3::Client::from_conf(
        aws_sdk_s3::config::Builder::from(&config)
            .force_path_style(true)
            .build(),
    );
    let _ = client.create_bucket().bucket("szip").send().await;

    let data = payload();
    let writer = S3ZipWriter::builder()
        .client(client.clone())
        .bucket("szip")
        .key("e2e.zip")
        .part_size(5 * 1024 * 1024)
        .max_concurrent_uploads(3)
        .build()
        .await
        .unwrap();
    write_zip(writer, &data).await;

    let reader = S3ZipReader::new(client, "szip", "e2e.zip").await.unwrap();
    verify_zip(reader, &data).await;
}

#[cfg(feature = "cloud-azure")]
mod azure {
    use super::*;
    use hmac::{KeyInit, Mac};

    const ACCOUNT: &str = "devstoreaccount1";
    // Azurite's well-known development key (public, not a secret)
    const KEY: &str =
        "Eby8vdM02xNOcqFlqUwJPLlmEtlCDXJ1OUzFT50uSRZ6IFsuFq2UVErCz4I6tq/K1SZFPTOtr/KBHBeksoGMGw==";

    /// Account SAS query string for Azurite.
    fn account_sas() -> String {
        let (sp, ss, srt, se, spr, sv) = (
            "rwdlac",
            "b",
            "sco",
            "2030-01-01T00:00:00Z",
            "https,http",
            "2021-08-06",
        );
        let to_sign = format!("{ACCOUNT}\n{sp}\n{ss}\n{srt}\n\n{se}\n\n{spr}\n{sv}\n\n");
        let key = azure_core::base64::decode(KEY).unwrap();
        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(&key).unwrap();
        mac.update(to_sign.as_bytes());
        let sig = azure_core::base64::encode(mac.finalize().into_bytes());
        let sig: String = sig
            .chars()
            .map(|c| match c {
                '+' => "%2B".to_string(),
                '/' => "%2F".to_string(),
                '=' => "%3D".to_string(),
                c => c.to_string(),
            })
            .collect();
        format!("sv={sv}&ss={ss}&srt={srt}&sp={sp}&se={se}&spr={spr}&sig={sig}")
    }

    #[tokio::test]
    #[ignore = "needs Azurite on localhost:10000"]
    async fn test_azure_roundtrip() {
        use azure_storage_blob::{BlobClient, BlobContainerClient};
        use s_zip::cloud::{AzureZipReader, AzureZipWriter};

        let base = format!("http://127.0.0.1:10000/{ACCOUNT}/szip");
        let sas = account_sas();
        let container =
            BlobContainerClient::new(format!("{base}?{sas}").parse().unwrap(), None, None).unwrap();
        let _ = container.create(None).await;

        let blob_url = format!("{base}/e2e.zip?{sas}");
        let blob = BlobClient::new(blob_url.parse().unwrap(), None, None).unwrap();

        let data = payload();
        let writer = AzureZipWriter::builder(blob.block_blob_client())
            .block_size(4 * 1024 * 1024)
            .max_concurrent_uploads(3)
            .build();
        write_zip(writer, &data).await;

        let blob = BlobClient::new(blob_url.parse().unwrap(), None, None).unwrap();
        let reader = AzureZipReader::new(blob).await.unwrap();
        verify_zip(reader, &data).await;
    }
}

#[cfg(feature = "cloud-gcs")]
#[tokio::test]
#[ignore = "needs fake-gcs-server on localhost:80"]
async fn test_gcs_roundtrip() {
    use google_cloud_storage::client::Storage;
    use s_zip::cloud::{GCSZipReader, GCSZipWriter};

    // Create the bucket through the emulator's JSON API
    let status = std::process::Command::new("curl")
        .args([
            "-s",
            "-o",
            "/dev/null",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "-d",
            r#"{"name":"szip"}"#,
            "http://localhost/storage/v1/b?project=test",
        ])
        .status()
        .unwrap();
    assert!(status.success());

    let client = Storage::builder()
        .with_endpoint("http://localhost")
        .with_credentials(google_cloud_auth::credentials::anonymous::Builder::new().build())
        .build()
        .await
        .unwrap();

    let data = payload();
    let writer = GCSZipWriter::builder()
        .client(client.clone())
        .bucket("szip")
        .object("e2e.zip")
        .chunk_size(4 * 1024 * 1024)
        .build()
        .await
        .unwrap();
    write_zip(writer, &data).await;

    let reader = GCSZipReader::new(client, "szip", "e2e.zip").await.unwrap();
    verify_zip(reader, &data).await;
}
