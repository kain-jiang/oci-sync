//! tar.gz packing and unpacking (stdlib `tar` + `flate2`).
//!
//! Format contract (compatible with the Go version):
//! - Directories are archived with their top-level folder name as the root
//!   entry, preserving the full sub-tree.
//! - A single file is archived as `<basename>`.
//! - Unpacking rejects any entry whose resolved path escapes the destination
//!   (path-traversal defense). Symlinks and special files are skipped.

use std::fs;
use std::io::Cursor;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path};

use anyhow::{Context, Result, bail};
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use tar::{Builder, EntryType, Header};

/// Pack a file or directory into an in-memory tar.gz archive.
pub fn pack(src: &Path) -> Result<Vec<u8>> {
    let meta = fs::metadata(src).with_context(|| format!("stat {}", src.display()))?;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    {
        let mut builder = Builder::new(&mut encoder);
        if meta.is_dir() {
            let root_name = src
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "root".to_string());
            pack_dir(&mut builder, src, &root_name)?;
        } else {
            pack_file(&mut builder, src, &meta)?;
        }
        builder.finish().context("finish tar archive")?;
    }
    let bytes = encoder.finish().context("finish gzip stream")?;
    Ok(bytes)
}

fn pack_dir(
    builder: &mut Builder<&mut GzEncoder<Vec<u8>>>,
    dir: &Path,
    root_name: &str,
) -> Result<()> {
    let mut entries: Vec<(String, std::fs::DirEntry)> = Vec::new();
    collect_entries(Path::new(""), dir, &mut entries)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    // Root directory entry first (matching Go's behavior of archiving the
    // top-level directory itself).
    let root_meta = fs::metadata(dir).context("stat root dir")?;
    let mut root_header = Header::new_gnu();
    root_header.set_entry_type(EntryType::Directory);
    root_header.set_mode(0o755);
    root_header.set_size(0);
    root_header.set_mtime(
        root_meta
            .modified()
            .ok()
            .map(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            })
            .unwrap_or(0),
    );
    root_header
        .set_path(Path::new(root_name))
        .context("set root entry name")?;
    root_header.set_cksum();
    builder
        .append(&root_header, std::io::empty())
        .context("append root dir entry")?;

    for (name, entry) in entries {
        let meta = fs::symlink_metadata(entry.path())
            .with_context(|| format!("metadata of {}", entry.path().display()))?;
        let header_name = format!("{root_name}/{name}");
        if meta.is_dir() {
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Directory);
            header.set_mode(meta.permissions().mode() & 0o777);
            header.set_size(0);
            header.set_mtime(
                meta.modified()
                    .ok()
                    .map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0)
                    })
                    .unwrap_or(0),
            );
            header
                .set_path(Path::new(&header_name))
                .with_context(|| format!("set name for dir entry {header_name}"))?;
            header.set_cksum();
            builder
                .append(&header, std::io::empty())
                .with_context(|| format!("append dir entry {header_name}"))?;
        } else if meta.is_file() {
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Regular);
            header.set_mode(meta.permissions().mode() & 0o777);
            header.set_size(meta.len());
            header.set_mtime(
                meta.modified()
                    .ok()
                    .map(|t| {
                        t.duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_secs())
                            .unwrap_or(0)
                    })
                    .unwrap_or(0),
            );
            header
                .set_path(Path::new(&header_name))
                .with_context(|| format!("set name for file entry {header_name}"))?;
            header.set_cksum();
            let file = fs::File::open(entry.path())
                .with_context(|| format!("open {}", entry.path().display()))?;
            builder
                .append(&header, file)
                .with_context(|| format!("append file entry {header_name}"))?;
        }
        // skip symlinks/sockets/etc.
    }
    Ok(())
}

/// Recursively collect all entries under `dir` with their paths relative to
/// `dir` (accumulated in `rel`), so nested sub-directories keep their full
/// relative path (e.g. `sub/deep/c.txt`).
fn collect_entries(
    rel: &Path,
    dir: &Path,
    out: &mut Vec<(String, std::fs::DirEntry)>,
) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("read dir {}", dir.display()))? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        // symlink_metadata: do not follow symlinks (matches Go's filepath.Walk
        // semantics — symlinks are treated as non-directory entries and skipped)
        let meta = fs::symlink_metadata(entry.path())?;
        let is_dir = meta.is_dir();
        let entry_rel = rel.join(&name);
        let path = entry.path();
        out.push((entry_rel.to_string_lossy().into_owned(), entry));
        if is_dir {
            collect_entries(&entry_rel, &path, out)?;
        }
    }
    Ok(())
}

fn pack_file(
    builder: &mut Builder<&mut GzEncoder<Vec<u8>>>,
    file: &Path,
    meta: &fs::Metadata,
) -> Result<()> {
    let name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| anyhow::anyhow!("cannot determine file name for {}", file.display()))?;
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Regular);
    header.set_mode(meta.permissions().mode() & 0o777);
    header.set_size(meta.len());
    header.set_mtime(
        meta.modified()
            .ok()
            .map(|t| {
                t.duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0)
            })
            .unwrap_or(0),
    );
    header
        .set_path(Path::new(&name))
        .with_context(|| format!("set name for {name}"))?;
    header.set_cksum();
    let f = fs::File::open(file).with_context(|| format!("open {}", file.display()))?;
    builder
        .append(&header, f)
        .with_context(|| format!("append file entry {name}"))?;
    Ok(())
}

/// Extract a tar.gz archive into `dest` (created if missing).
pub fn unpack(data: &[u8], dest: &Path) -> Result<()> {
    let abs_dest = absolutize(dest)?;
    fs::create_dir_all(&abs_dest)
        .with_context(|| format!("create dest dir {}", abs_dest.display()))?;

    let decoder = GzDecoder::new(Cursor::new(data));
    let mut archive = tar::Archive::new(decoder);

    let entries = archive.entries().context("read tar entries")?;
    for entry in entries {
        let mut entry = entry.context("read tar entry")?;
        let path = entry.path().context("read entry path")?.into_owned();

        // Security: normalize lexically (resolve "." and "..") BEFORE the
        // containment check. `Path::join` does not clean "..", so a naive
        // starts_with check would be bypassable (e.g. "sub/../../evil").
        let target = normalize_lexically(&abs_dest.join(&path));
        if !target.starts_with(&abs_dest) {
            bail!("illegal file path in archive: {}", path.display());
        }

        let entry_type = entry.header().entry_type();
        match entry_type {
            EntryType::Directory => {
                fs::create_dir_all(&target)
                    .with_context(|| format!("create dir {}", target.display()))?;
            }
            EntryType::Regular => {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)
                        .with_context(|| format!("create parent dir {}", parent.display()))?;
                }
                let mode = entry.header().mode().unwrap_or(0o644) & 0o777;
                let mut f = fs::File::create(&target)
                    .with_context(|| format!("create file {}", target.display()))?;
                std::io::copy(&mut entry, &mut f)
                    .with_context(|| format!("write file {}", target.display()))?;
                let _ = fs::set_permissions(&target, fs::Permissions::from_mode(mode));
            }
            _ => {
                // skip symlinks, devices, etc. (same as Go implementation)
                continue;
            }
        }
    }
    Ok(())
}

/// Absolute path without resolving symlinks beyond the base (lexical).
fn absolutize(path: &Path) -> Result<std::path::PathBuf> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .context("get current dir")?
            .join(path)
    };
    Ok(normalize_lexically(&path))
}

/// Lexically normalize a path (resolve `.` and `..` without touching the fs).
fn normalize_lexically(path: &Path) -> std::path::PathBuf {
    let mut result = std::path::PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    result.push(comp.as_os_str());
                }
            }
            other => result.push(other.as_os_str()),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oci-sync-archive-test-{tag}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn pack_unpack_single_file_roundtrip() {
        let dir = tempdir("single");
        let file = dir.join("hello.txt");
        fs::write(&file, "hello oci-sync").unwrap();

        let data = pack(&file).unwrap();
        assert!(!data.is_empty());

        let out = dir.join("out");
        unpack(&data, &out).unwrap();
        let restored = out.join("hello.txt");
        assert_eq!(fs::read_to_string(&restored).unwrap(), "hello oci-sync");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn pack_unpack_directory_roundtrip() {
        let dir = tempdir("dir");
        let src = dir.join("mydir");
        fs::create_dir_all(src.join("sub/deep")).unwrap();
        fs::write(src.join("a.txt"), "AAA").unwrap();
        fs::write(src.join("sub/b.txt"), "BBB").unwrap();
        fs::write(src.join("sub/deep/c.txt"), "CCC").unwrap();

        let data = pack(&src).unwrap();

        let out = dir.join("out");
        unpack(&data, &out).unwrap();
        assert!(out.join("mydir").join("a.txt").is_file());
        assert!(out.join("mydir").join("sub/deep/c.txt").is_file());
        assert_eq!(
            fs::read_to_string(out.join("mydir/sub/b.txt")).unwrap(),
            "BBB"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unpack_empty_directory() {
        let dir = tempdir("empty");
        let src = dir.join("emptydir");
        fs::create_dir_all(&src).unwrap();
        let data = pack(&src).unwrap();
        let out = dir.join("out");
        unpack(&data, &out).unwrap();
        assert!(out.join("emptydir").is_dir());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unpack_rejects_path_traversal() {
        let dir = tempdir("traversal");
        // Craft a malicious tar manually with a "../evil.txt" entry name,
        // bypassing the tar crate's path sanitization.
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        {
            let mut builder = Builder::new(&mut encoder);
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Regular);
            header.set_mode(0o644);
            header.set_size(3);
            let name_bytes = b"../evil.txt";
            header.as_old_mut().name[..name_bytes.len()].copy_from_slice(name_bytes);
            header.set_cksum();
            builder.append(&header, &b"bad"[..]).unwrap();
        }
        let data = encoder.finish().unwrap();

        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        let res = unpack(&data, &out);
        assert!(res.is_err());
        assert!(!dir.join("evil.txt").exists());
        assert!(!out.join("evil.txt").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unpack_rejects_nested_traversal() {
        let dir = tempdir("nested-traversal");
        // "sub/../../evil.txt" must also be rejected after lexical cleanup.
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        {
            let mut builder = Builder::new(&mut encoder);
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Regular);
            header.set_mode(0o644);
            header.set_size(3);
            let name_bytes = b"sub/../../evil.txt";
            header.as_old_mut().name[..name_bytes.len()].copy_from_slice(name_bytes);
            header.set_cksum();
            builder.append(&header, &b"bad"[..]).unwrap();
        }
        let data = encoder.finish().unwrap();

        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        assert!(unpack(&data, &out).is_err());
        assert!(!dir.join("evil.txt").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unpack_rejects_absolute_path_entry() {
        let dir = tempdir("absolute");
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        {
            let mut builder = Builder::new(&mut encoder);
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Regular);
            header.set_mode(0o644);
            header.set_size(3);
            let name_bytes = b"/tmp/oci-sync-abs-evil.txt";
            header.as_old_mut().name[..name_bytes.len()].copy_from_slice(name_bytes);
            header.set_cksum();
            builder.append(&header, &b"bad"[..]).unwrap();
        }
        let data = encoder.finish().unwrap();

        let out = dir.join("out");
        fs::create_dir_all(&out).unwrap();
        assert!(unpack(&data, &out).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn unpack_rejects_invalid_gzip() {
        let dir = tempdir("invalid");
        let out = dir.join("out");
        let res = unpack(b"this is not gzip data at all, definitely not", &out);
        assert!(res.is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
