//! `s-zip` command-line tool: create, extract, list and verify ZIP archives.
//!
//! Build with `cargo install s-zip --features cli`.

use clap::{Parser, Subcommand, ValueEnum};
use s_zip::{
    AesStrength, CompressionMethod, EntryOptions, ExtractLimits, StreamingZipReader,
    StreamingZipWriter,
};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "s-zip",
    version,
    about = "Streaming ZIP tool built on the s-zip library"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create an archive from files and directories
    Pack {
        /// Archive to create
        archive: PathBuf,
        /// Files or directories to add (directories are added recursively)
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Compression method
        #[arg(short, long, value_enum, default_value_t = Method::Deflate)]
        method: Method,
        /// Compression level (0-9 for deflate, 1-21 for zstd)
        #[arg(short, long, default_value_t = 6)]
        level: u32,
        /// Encrypt entries with AES-256 (or set S_ZIP_PASSWORD)
        #[arg(short, long, env = "S_ZIP_PASSWORD", hide_env_values = true)]
        password: Option<String>,
        /// Archive comment
        #[arg(long)]
        comment: Option<String>,
    },
    /// Extract an archive
    Unpack {
        /// Archive to extract
        archive: PathBuf,
        /// Destination directory
        #[arg(short = 'd', long, default_value = ".")]
        dest: PathBuf,
        /// Password for encrypted entries (or set S_ZIP_PASSWORD)
        #[arg(short, long, env = "S_ZIP_PASSWORD", hide_env_values = true)]
        password: Option<String>,
        /// Disable ZIP bomb limits (by default ExtractLimits::strict() applies)
        #[arg(long)]
        no_limits: bool,
    },
    /// List the entries of an archive
    List {
        /// Archive to list
        archive: PathBuf,
    },
    /// Decompress every entry and verify its CRC-32 / authentication code
    Test {
        /// Archive to test
        archive: PathBuf,
        /// Password for encrypted entries (or set S_ZIP_PASSWORD)
        #[arg(short, long, env = "S_ZIP_PASSWORD", hide_env_values = true)]
        password: Option<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Method {
    Deflate,
    Zstd,
    Store,
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> ExitCode {
    let result = match Cli::parse().command {
        Command::Pack {
            archive,
            paths,
            method,
            level,
            password,
            comment,
        } => pack(&archive, &paths, method, level, password, comment),
        Command::Unpack {
            archive,
            dest,
            password,
            no_limits,
        } => unpack(&archive, &dest, password, no_limits),
        Command::List { archive } => list(&archive),
        Command::Test { archive, password } => test(&archive, password),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("s-zip: {e}");
            ExitCode::FAILURE
        }
    }
}

fn open_reader(archive: &Path, password: Option<String>) -> Result<StreamingZipReader> {
    let mut reader = StreamingZipReader::open(archive)?;
    if let Some(pw) = password {
        reader.set_password(pw);
    }
    Ok(reader)
}

fn pack(
    archive: &Path,
    paths: &[PathBuf],
    method: Method,
    level: u32,
    password: Option<String>,
    comment: Option<String>,
) -> Result<()> {
    let method = match method {
        Method::Deflate => CompressionMethod::Deflate,
        Method::Zstd => CompressionMethod::Zstd,
        Method::Store => CompressionMethod::Stored,
    };
    let file = std::fs::File::create(archive)?;
    let mut writer =
        StreamingZipWriter::from_writer_with_method(std::io::BufWriter::new(file), method, level)?;
    if let Some(pw) = password {
        writer
            .set_password(pw)
            .set_encryption_strength(AesStrength::Aes256);
    }
    if let Some(c) = comment {
        writer.set_comment(c)?;
    }

    let archive_abs = std::fs::canonicalize(archive)?;
    let mut count = 0usize;
    for path in paths {
        // Store entries relative to the parent of each argument, like `zip -r`
        let base = path.parent().unwrap_or(Path::new(""));
        add_path(&mut writer, base, path, &archive_abs, &mut count)?;
    }
    let (_, stats) = writer.finish_with_stats()?;
    println!(
        "{}: {} entries, {} -> {} bytes",
        archive.display(),
        count,
        stats.total_uncompressed_bytes,
        stats.total_compressed_bytes
    );
    Ok(())
}

fn add_path<W: std::io::Write + std::io::Seek>(
    writer: &mut StreamingZipWriter<W>,
    base: &Path,
    path: &Path,
    archive_abs: &Path,
    count: &mut usize,
) -> Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() {
        let mut children: Vec<_> = std::fs::read_dir(path)?.collect::<std::io::Result<_>>()?;
        children.sort_by_key(|e| e.file_name());
        for child in children {
            add_path(writer, base, &child.path(), archive_abs, count)?;
        }
        return Ok(());
    }
    if !meta.is_file() {
        eprintln!("s-zip: skipping {} (not a regular file)", path.display());
        return Ok(());
    }
    // Don't add the archive being written to itself
    if std::fs::canonicalize(path).ok().as_deref() == Some(archive_abs) {
        return Ok(());
    }

    let name = path
        .strip_prefix(base)
        .unwrap_or(path)
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    #[cfg(unix)]
    let unix_mode = {
        use std::os::unix::fs::PermissionsExt;
        Some(meta.permissions().mode())
    };
    #[cfg(not(unix))]
    let unix_mode = None;
    let options = EntryOptions {
        mtime: meta.modified().ok(),
        unix_mode,
        comment: None,
    };

    writer.start_entry_with_options(&name, options)?;
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = std::io::Read::read(&mut file, &mut buf)?;
        if n == 0 {
            break;
        }
        writer.write_data(&buf[..n])?;
    }
    *count += 1;
    Ok(())
}

fn unpack(archive: &Path, dest: &Path, password: Option<String>, no_limits: bool) -> Result<()> {
    let mut reader = open_reader(archive, password)?;
    if !no_limits {
        reader.set_limits(ExtractLimits::strict());
    }
    std::fs::create_dir_all(dest)?;
    reader.extract_all(dest)?;
    println!(
        "{}: extracted {} entries to {}",
        archive.display(),
        reader.entries().len(),
        dest.display()
    );
    Ok(())
}

fn method_name(method: u16) -> String {
    match method {
        0 => "store".into(),
        8 => "deflate".into(),
        12 => "bzip2".into(),
        14 => "lzma".into(),
        93 => "zstd".into(),
        95 => "xz".into(),
        99 => "aes".into(),
        m => format!("method {m}"),
    }
}

fn list(archive: &Path) -> Result<()> {
    let reader = StreamingZipReader::open(archive)?;
    println!(
        "{:>12} {:>12} {:>8}  {:<4} Name",
        "Size", "Compressed", "Method", ""
    );
    let (mut total, mut total_compressed) = (0u64, 0u64);
    for entry in reader.entries() {
        println!(
            "{:>12} {:>12} {:>8}  {:<4} {}{}",
            entry.uncompressed_size,
            entry.compressed_size,
            method_name(entry.compression_method),
            if entry.is_encrypted { "enc" } else { "" },
            entry.name,
            if entry.comment.is_empty() {
                String::new()
            } else {
                format!("  # {}", entry.comment)
            }
        );
        total += entry.uncompressed_size;
        total_compressed += entry.compressed_size;
    }
    println!(
        "{:>12} {:>12}           {} entries",
        total,
        total_compressed,
        reader.entries().len()
    );
    if !reader.comment().is_empty() {
        println!("Comment: {}", reader.comment());
    }
    Ok(())
}

fn test(archive: &Path, password: Option<String>) -> Result<()> {
    let mut reader = open_reader(archive, password)?;
    let entries = reader.entries().to_vec();
    let mut failures = 0usize;
    for entry in &entries {
        if entry.name.ends_with('/') {
            continue;
        }
        let result = if entry.is_encrypted {
            // Full read verifies the AES authentication code / ZipCrypto CRC
            reader.read_entry(entry).map(|_| ())
        } else {
            // Stream (no size cap) and check the CRC-32 ourselves
            (|| -> s_zip::Result<()> {
                let mut stream = reader.read_entry_streaming(entry)?;
                let mut hasher = crc32fast::Hasher::new();
                let mut buf = vec![0u8; 256 * 1024];
                loop {
                    let n = std::io::Read::read(&mut stream, &mut buf)?;
                    if n == 0 {
                        break;
                    }
                    hasher.update(&buf[..n]);
                }
                let crc = hasher.finalize();
                if crc != entry.crc32 {
                    return Err(s_zip::SZipError::InvalidFormat(format!(
                        "CRC-32 mismatch: expected {:#010x}, got {:#010x}",
                        entry.crc32, crc
                    )));
                }
                Ok(())
            })()
        };
        match result {
            Ok(_) => println!("    OK  {}", entry.name),
            Err(e) => {
                failures += 1;
                println!("  FAIL  {}: {}", entry.name, e);
            }
        }
    }
    if failures > 0 {
        return Err(format!("{failures} of {} entries failed", entries.len()).into());
    }
    println!("No errors detected in {}", archive.display());
    Ok(())
}
