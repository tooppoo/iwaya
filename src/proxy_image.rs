//! Local, lazy construction of the proxy sidecar image.
//!
//! The image is assembled from the running iwaya binary and the shared
//! objects it has loaded, on top of `FROM scratch`: no base image, so no
//! registry or network access is ever required
//! (docs/adr/20260820T162206Z_proxy-backed-secret-delivery.md, "The proxy
//! image is iwaya-owned and built locally"). The recipe is embedded here and
//! never written into the user's project.

use std::fmt;
use std::fmt::Write as _;
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sha2::{Digest, Sha256};

/// The entire recipe. `rootfs/` carries the binary and, for a dynamically
/// linked iwaya, its loaded shared objects at their original absolute paths;
/// a statically linked iwaya ships alone. The proxy is the iwaya binary in
/// its `proxy` execution mode, not a separately distributed executable.
const DOCKERFILE: &str = "FROM scratch\nCOPY rootfs/ /\nENTRYPOINT [\"/iwaya\", \"proxy\"]\n";

/// One file of the image: where it lands in the image filesystem, the open
/// handle it is hashed and copied from, and the path for diagnostics. The
/// handle pins one inode, so the digest and the copied bytes cannot diverge
/// even if the path is replaced mid-build.
struct Material {
    destination: String,
    path: PathBuf,
    file: fs::File,
}

/// Ensures a proxy image matching the running iwaya exists locally and
/// returns its tag. An existing image with the same tag is reused; otherwise
/// the image is built from a temporary context assembled out of the running
/// binary and its loaded objects. The context lives under the system temp
/// directory only for the duration of the build.
pub fn ensure_proxy_image(runtime: &str) -> Result<String, ProxyImageError> {
    let mut material = build_material()?;
    let tag = image_tag(&mut material)?;
    if image_exists(runtime, &tag)? {
        return Ok(tag);
    }
    let context = TempContext::create()?;
    write_build_context(&context.path, &mut material).map_err(ProxyImageError::Context)?;
    build_image(runtime, &tag, &context.path)?;
    Ok(tag)
}

/// Collects the files the image needs: the running binary itself at
/// `/iwaya`, its ELF program interpreter at the path the binary requests,
/// and every shared object currently mapped into this process. Loaded
/// objects are the dependency closure the dynamic linker actually resolved,
/// so nothing has to predict what the binary needs.
fn build_material() -> Result<Vec<Material>, ProxyImageError> {
    let exe_path = std::env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(ProxyImageError::Exe)?;
    // The handle comes from `/proc/self/exe`, which names the inode this
    // process is executing, not whatever the path holds now: a rebuild can
    // replace the file on disk mid-run, and the sidecar must run the same
    // binary as the supervisor that builds it.
    let mut exe = fs::File::open("/proc/self/exe").map_err(ProxyImageError::Exe)?;
    let interpreter = program_interpreter(&mut exe)?;
    let mut material = vec![Material {
        destination: "/iwaya".to_string(),
        path: exe_path.clone(),
        file: exe,
    }];
    if let Some(interpreter) = interpreter {
        material.push(open_material(interpreter)?);
    }
    for object in loaded_shared_objects(&exe_path)? {
        material.push(open_material(object)?);
    }
    material.sort_by(|a, b| a.destination.cmp(&b.destination));
    material.dedup_by(|a, b| a.destination == b.destination);
    Ok(material)
}

/// Opens one image file whose destination equals its host path.
fn open_material(path: String) -> Result<Material, ProxyImageError> {
    let file = fs::File::open(&path).map_err(|source| ProxyImageError::Material {
        path: PathBuf::from(&path),
        source,
    })?;
    Ok(Material {
        path: PathBuf::from(&path),
        destination: path,
        file,
    })
}

/// Every file-backed `.so` mapping of this process except the binary itself,
/// read from `/proc/self/maps`. The mapping paths are where the dynamic
/// linker found each object, so copying to the same paths reproduces the
/// lookup inside the image.
fn loaded_shared_objects(exe: &Path) -> Result<Vec<String>, ProxyImageError> {
    let maps = fs::read_to_string("/proc/self/maps").map_err(ProxyImageError::Maps)?;
    let mut objects: Vec<String> = maps
        .lines()
        .filter_map(mapped_pathname)
        .filter(|path| path.starts_with('/') && path.contains(".so"))
        .filter(|path| Path::new(path) != exe)
        .map(str::to_string)
        .collect();
    objects.sort();
    objects.dedup();
    Ok(objects)
}

/// The pathname field of one `/proc/self/maps` line, or `None` for an
/// anonymous mapping. The field is everything after the first five, not a
/// whitespace token: maps does not escape spaces, so splitting would
/// silently truncate — and thereby drop — a library under a path containing
/// one. The kernel's ` (deleted)` marker is stripped; the material open then
/// fails loudly on such a path instead of the image quietly lacking a file.
fn mapped_pathname(line: &str) -> Option<&str> {
    let mut rest = line;
    for _ in 0..5 {
        let separator = rest.find(char::is_whitespace)?;
        rest = rest[separator..].trim_start();
    }
    let pathname = rest.strip_suffix(" (deleted)").unwrap_or(rest);
    (!pathname.is_empty()).then_some(pathname)
}

/// The `PT_INTERP` path of an ELF64 little-endian executable, or `None` for
/// a static binary. The interpreter path is read from the binary rather than
/// guessed, because the kernel resolves exactly this embedded path when the
/// container starts `/iwaya`. Malformed input is refused with an error, never
/// an abort, so every arithmetic and allocation below is bounded first.
fn program_interpreter(file: &mut fs::File) -> Result<Option<String>, ProxyImageError> {
    const PT_INTERP: u32 = 3;
    // An interpreter path is a filesystem path; anything beyond this bound
    // is a corrupt size field, not a long path.
    const INTERPRETER_PATH_LIMIT: u64 = 4096;
    let mut header = [0u8; 64];
    file.seek(SeekFrom::Start(0))
        .and_then(|_| file.read_exact(&mut header))
        .map_err(ProxyImageError::Exe)?;
    if header[..4] != [0x7f, b'E', b'L', b'F'] || header[4] != 2 || header[5] != 1 {
        // Only the format this build of iwaya can itself be: ELF64 little
        // endian. Anything else means the material discovery does not
        // understand the binary, and building a broken image would be worse
        // than refusing.
        return Err(ProxyImageError::UnsupportedExecutable);
    }
    let ph_offset = u64::from_le_bytes(header[0x20..0x28].try_into().unwrap());
    let ph_entry_size = u16::from_le_bytes(header[0x36..0x38].try_into().unwrap()) as u64;
    let ph_count = u16::from_le_bytes(header[0x38..0x3a].try_into().unwrap()) as u64;
    for index in 0..ph_count {
        let entry_offset = index
            .checked_mul(ph_entry_size)
            .and_then(|table_offset| ph_offset.checked_add(table_offset))
            .ok_or(ProxyImageError::UnsupportedExecutable)?;
        let mut entry = [0u8; 56];
        file.seek(SeekFrom::Start(entry_offset))
            .and_then(|_| file.read_exact(&mut entry))
            .map_err(ProxyImageError::Exe)?;
        if u32::from_le_bytes(entry[0..4].try_into().unwrap()) != PT_INTERP {
            continue;
        }
        let offset = u64::from_le_bytes(entry[0x8..0x10].try_into().unwrap());
        let size = u64::from_le_bytes(entry[0x20..0x28].try_into().unwrap());
        if size == 0 || size > INTERPRETER_PATH_LIMIT {
            return Err(ProxyImageError::UnsupportedExecutable);
        }
        let mut path = vec![0u8; size as usize];
        file.seek(SeekFrom::Start(offset))
            .and_then(|_| file.read_exact(&mut path))
            .map_err(ProxyImageError::Exe)?;
        while path.last() == Some(&0) {
            path.pop();
        }
        return String::from_utf8(path)
            .map(Some)
            .map_err(|_| ProxyImageError::UnsupportedExecutable);
    }
    Ok(None)
}

/// The image tag: the iwaya version plus a digest of the recipe and every
/// file that goes into the image. A locally rebuilt binary changes the
/// digest, so a stale image is never silently reused across builds that
/// share a version number.
fn image_tag(material: &mut [Material]) -> Result<String, ProxyImageError> {
    let mut hasher = Sha256::new();
    hasher.update(DOCKERFILE.as_bytes());
    for entry in material.iter_mut() {
        // Each field is length-prefixed so the material-to-bytes encoding is
        // injective: without the prefixes, content containing a separator
        // byte could collide with a different destination/content split.
        hasher.update((entry.destination.len() as u64).to_le_bytes());
        hasher.update(entry.destination.as_bytes());
        let length = entry
            .file
            .metadata()
            .map(|metadata| metadata.len())
            .map_err(|source| ProxyImageError::Material {
                path: entry.path.clone(),
                source,
            })?;
        hasher.update(length.to_le_bytes());
        entry
            .file
            .seek(SeekFrom::Start(0))
            .map_err(|source| ProxyImageError::Material {
                path: entry.path.clone(),
                source,
            })?;
        // Streamed in chunks: the binary is tens of megabytes and never
        // needs to be resident just to be hashed.
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = entry
                .file
                .read(&mut buffer)
                .map_err(|source| ProxyImageError::Material {
                    path: entry.path.clone(),
                    source,
                })?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(12);
    for byte in &digest[..6] {
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    Ok(format!("iwaya-proxy:v{}-{hex}", env!("CARGO_PKG_VERSION")))
}

/// Writes the Dockerfile and copies every material file under `rootfs/` at
/// its destination path. Copies are marked executable wholesale: the set is
/// exactly one binary and shared objects, and the linker also maps those
/// executable.
fn write_build_context(directory: &Path, material: &mut [Material]) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::write(directory.join("Dockerfile"), DOCKERFILE)?;
    let rootfs = directory.join("rootfs");
    for entry in material.iter_mut() {
        let destination = rootfs.join(entry.destination.trim_start_matches('/'));
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        // Copied from the already-open handle, not the path: the digest was
        // computed from this handle's inode, and the copy must be the bytes
        // the tag describes even if the path was replaced in between.
        entry.file.seek(SeekFrom::Start(0))?;
        let mut copy = fs::File::create(&destination)?;
        std::io::copy(&mut entry.file, &mut copy)?;
        fs::set_permissions(&destination, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

/// Whether the runtime already has an image under this tag. Output is
/// discarded: only the answer matters, and a missing image is the expected
/// case, not a diagnostic.
fn image_exists(runtime: &str, tag: &str) -> Result<bool, ProxyImageError> {
    let status = Command::new(runtime)
        .args(["image", "inspect", tag])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|source| ProxyImageError::RuntimeStart {
            runtime: runtime.to_string(),
            source,
        })?;
    Ok(status.success())
}

fn build_image(runtime: &str, tag: &str, context: &Path) -> Result<(), ProxyImageError> {
    let output = Command::new(runtime)
        .arg("build")
        .args(["--tag", tag])
        .arg(context)
        .stdin(Stdio::null())
        .output()
        .map_err(|source| ProxyImageError::RuntimeStart {
            runtime: runtime.to_string(),
            source,
        })?;
    if output.status.success() {
        return Ok(());
    }
    // The build touches no secret, so its stderr is safe to surface and is
    // the only way to see why the runtime refused.
    Err(ProxyImageError::BuildFailed {
        status: output.status.to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim_end().to_string(),
    })
}

/// A build-context directory that removes itself, so neither a completed nor
/// a failed build leaves material copies in the temp directory.
struct TempContext {
    path: PathBuf,
}

impl TempContext {
    fn create() -> Result<TempContext, ProxyImageError> {
        // Random suffix rather than the pid alone: a leftover directory from
        // a crashed run with a recycled pid must not fail the build.
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).map_err(|_| ProxyImageError::TempDir(None))?;
        let mut suffix = String::with_capacity(16);
        for byte in random {
            write!(suffix, "{byte:02x}").expect("writing to a String cannot fail");
        }
        let path = std::env::temp_dir().join(format!("iwaya-proxy-image-{suffix}"));
        fs::create_dir(&path).map_err(|source| ProxyImageError::TempDir(Some(source)))?;
        Ok(TempContext { path })
    }
}

impl Drop for TempContext {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// A failure before or during the local image build. No variant carries
/// secret data: image construction happens before any secret is resolved.
#[derive(Debug)]
pub enum ProxyImageError {
    Exe(std::io::Error),
    Maps(std::io::Error),
    UnsupportedExecutable,
    Material {
        path: PathBuf,
        source: std::io::Error,
    },
    TempDir(Option<std::io::Error>),
    Context(std::io::Error),
    RuntimeStart {
        runtime: String,
        source: std::io::Error,
    },
    BuildFailed {
        status: String,
        stderr: String,
    },
}

impl fmt::Display for ProxyImageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProxyImageError::Exe(source) => {
                write!(f, "cannot read the running iwaya binary: {source}")
            }
            ProxyImageError::Maps(source) => {
                write!(f, "cannot list the loaded shared objects: {source}")
            }
            ProxyImageError::UnsupportedExecutable => {
                write!(
                    f,
                    "the running iwaya binary is not an ELF64 little-endian executable"
                )
            }
            ProxyImageError::Material { path, source } => {
                write!(
                    f,
                    "cannot read proxy image material '{}': {source}",
                    path.display()
                )
            }
            ProxyImageError::TempDir(source) => match source {
                Some(source) => {
                    write!(f, "cannot create the proxy image build context: {source}")
                }
                None => write!(
                    f,
                    "cannot create the proxy image build context: no usable OS entropy source"
                ),
            },
            ProxyImageError::Context(source) => {
                write!(f, "cannot assemble the proxy image build context: {source}")
            }
            ProxyImageError::RuntimeStart { runtime, source } => {
                write!(f, "cannot start runtime '{runtime}': {source}")
            }
            ProxyImageError::BuildFailed { status, stderr } => {
                write!(f, "the proxy image build failed ({status}): {stderr}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("iwaya-image-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A runtime stand-in that appends its argv to `log` and follows
    /// `body` for its exit behavior.
    fn fake_runtime(dir: &Path, log: &Path, body: &str) -> String {
        let path = dir.join("runtime");
        fs::write(
            &path,
            format!("#!/bin/sh\necho \"$@\" >> {}\n{body}\n", log.display()),
        )
        .unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path.to_string_lossy().into_owned()
    }

    fn material_fixture(dir: &Path) -> Vec<Material> {
        let path = dir.join("payload");
        fs::write(&path, b"payload-bytes").unwrap();
        vec![Material {
            destination: "/iwaya".to_string(),
            file: fs::File::open(&path).unwrap(),
            path,
        }]
    }

    #[test]
    fn bundles_the_running_binary_at_the_image_root() {
        let material = build_material().unwrap();
        let exe = fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        assert!(
            material
                .iter()
                .any(|entry| entry.destination == "/iwaya" && entry.path == exe)
        );
    }

    // The test binary is glibc-dynamic in this project's dev and CI
    // environments; a fully static build would legitimately fail this.
    #[test]
    fn bundles_the_loaded_shared_objects_at_their_own_paths() {
        let material = build_material().unwrap();
        let objects: Vec<&Material> = material
            .iter()
            .filter(|entry| entry.destination != "/iwaya")
            .collect();
        assert!(!objects.is_empty());
        assert!(
            objects
                .iter()
                .all(|entry| entry.path.as_path() == Path::new(&entry.destination))
        );
    }

    #[test]
    fn bundles_the_interpreter_the_binary_requests() {
        let mut exe = fs::File::open("/proc/self/exe").unwrap();
        let interpreter = program_interpreter(&mut exe).unwrap().unwrap();
        let material = build_material().unwrap();
        assert!(material.iter().any(|entry| entry.destination == interpreter));
    }

    #[test]
    fn keeps_a_mapped_pathname_containing_spaces_intact() {
        let line = "7f00-7f01 r-xp 00000000 08:01 123    /opt/my libs/libfoo.so";
        assert_eq!(mapped_pathname(line), Some("/opt/my libs/libfoo.so"));
    }

    #[test]
    fn strips_the_deleted_marker_from_a_mapped_pathname() {
        let line = "7f00-7f01 r-xp 00000000 08:01 123    /usr/lib/libbar.so (deleted)";
        assert_eq!(mapped_pathname(line), Some("/usr/lib/libbar.so"));
    }

    #[test]
    fn refuses_a_non_elf_executable() {
        let dir = test_dir("non-elf");
        let path = dir.join("not-elf");
        fs::write(&path, [0u8; 64]).unwrap();
        let mut file = fs::File::open(&path).unwrap();
        assert!(matches!(
            program_interpreter(&mut file),
            Err(ProxyImageError::UnsupportedExecutable)
        ));
    }

    #[test]
    fn treats_an_executable_without_program_headers_as_static() {
        let dir = test_dir("static-elf");
        let path = dir.join("static");
        // A minimal ELF64 little-endian header with zero program headers:
        // the static-binary branch the recipe documents as "the binary
        // ships alone".
        let mut header = [0u8; 64];
        header[..4].copy_from_slice(&[0x7f, b'E', b'L', b'F']);
        header[4] = 2;
        header[5] = 1;
        fs::write(&path, header).unwrap();
        let mut file = fs::File::open(&path).unwrap();
        assert!(program_interpreter(&mut file).unwrap().is_none());
    }

    #[test]
    fn tags_with_the_iwaya_version_and_a_12_hex_digest() {
        let dir = test_dir("tag-format");
        let tag = image_tag(&mut material_fixture(&dir)).unwrap();
        let suffix = tag
            .strip_prefix(&format!("iwaya-proxy:v{}-", env!("CARGO_PKG_VERSION")))
            .expect("the tag starts with the versioned prefix");
        assert!(suffix.len() == 12 && suffix.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn tags_the_same_material_identically() {
        let dir = test_dir("tag-same");
        let mut material = material_fixture(&dir);
        assert_eq!(
            image_tag(&mut material).unwrap(),
            image_tag(&mut material).unwrap()
        );
    }

    #[test]
    fn tags_changed_material_differently() {
        let dir = test_dir("tag-changed");
        let mut material = material_fixture(&dir);
        let before = image_tag(&mut material).unwrap();
        // Same inode as the held handle: `fs::write` truncates in place, so
        // this models a rebuilt file, not a replaced one.
        fs::write(&material[0].path, b"rebuilt-bytes").unwrap();
        assert_ne!(before, image_tag(&mut material).unwrap());
    }

    #[test]
    fn writes_the_embedded_dockerfile_into_the_context() {
        let dir = test_dir("context-dockerfile");
        write_build_context(&dir, &mut material_fixture(&dir)).unwrap();
        assert_eq!(
            fs::read_to_string(dir.join("Dockerfile")).unwrap(),
            DOCKERFILE
        );
    }

    #[test]
    fn copies_material_under_rootfs_at_its_destination_path() {
        let dir = test_dir("context-copy");
        write_build_context(&dir, &mut material_fixture(&dir)).unwrap();
        assert_eq!(
            fs::read(dir.join("rootfs/iwaya")).unwrap(),
            b"payload-bytes"
        );
    }

    #[test]
    fn marks_copied_material_executable() {
        let dir = test_dir("context-mode");
        write_build_context(&dir, &mut material_fixture(&dir)).unwrap();
        let mode = fs::metadata(dir.join("rootfs/iwaya"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    #[test]
    fn reuses_an_existing_image_without_building() {
        let dir = test_dir("reuse");
        let log = dir.join("log");
        let runtime = fake_runtime(&dir, &log, "exit 0");
        let tag = ensure_proxy_image(&runtime).unwrap();
        let calls = fs::read_to_string(&log).unwrap();
        assert_eq!(calls.lines().count(), 1);
        assert_eq!(
            calls.lines().next().unwrap(),
            format!("image inspect {tag}")
        );
    }

    #[test]
    fn builds_when_no_image_matches() {
        let dir = test_dir("build");
        let log = dir.join("log");
        // Inspect misses; a build validating its own context succeeds.
        let runtime = fake_runtime(
            &dir,
            &log,
            r#"case "$1" in
  image) exit 1 ;;
  build) for a; do context=$a; done
         [ -f "$context/Dockerfile" ] && [ -x "$context/rootfs/iwaya" ] && exit 0
         exit 9 ;;
esac"#,
        );
        let tag = ensure_proxy_image(&runtime).unwrap();
        let calls = fs::read_to_string(&log).unwrap();
        assert!(
            calls
                .lines()
                .nth(1)
                .unwrap()
                .starts_with(&format!("build --tag {tag} "))
        );
    }

    #[test]
    fn removes_the_build_context_after_a_build() {
        let dir = test_dir("cleanup");
        let log = dir.join("log");
        let runtime = fake_runtime(
            &dir,
            &log,
            r#"case "$1" in
  image) exit 1 ;;
  build) exit 0 ;;
esac"#,
        );
        ensure_proxy_image(&runtime).unwrap();
        // The logged build argv names the context directory, which must be
        // gone once `ensure_proxy_image` has returned.
        let calls = fs::read_to_string(&log).unwrap();
        let context = calls
            .lines()
            .nth(1)
            .unwrap()
            .split(' ')
            .next_back()
            .unwrap();
        assert!(!Path::new(context).exists());
    }

    #[test]
    fn surfaces_the_runtime_stderr_of_a_failed_build() {
        let dir = test_dir("build-failure");
        let log = dir.join("log");
        let runtime = fake_runtime(
            &dir,
            &log,
            r#"case "$1" in
  image) exit 1 ;;
  build) echo "no space left" >&2; exit 1 ;;
esac"#,
        );
        let rendered = ensure_proxy_image(&runtime).unwrap_err().to_string();
        assert!(rendered.contains("no space left"), "{rendered}");
    }

    #[test]
    fn reports_a_runtime_that_cannot_start() {
        let rendered = ensure_proxy_image("no-such-runtime-zz-iwaya")
            .unwrap_err()
            .to_string();
        assert!(rendered.contains("no-such-runtime-zz-iwaya"), "{rendered}");
    }
}
