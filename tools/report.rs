use std::ffi::OsStr;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use wpd::api::{Decoder, Metadata};

pub fn is_stdout(path: &OsStr) -> bool {
    ["-", "/dev/stdout", "/dev/fd/1", "/proc/self/fd/1"]
        .iter()
        .any(|alias| path == OsStr::new(alias))
}

fn same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    let resolve = |p: &Path| {
        p.canonicalize().or_else(|e| {
            if e.kind() != io::ErrorKind::NotFound {
                return Err(e);
            }
            let parent = p
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let name = p.file_name().ok_or(e)?;

            Ok(parent.canonicalize()?.join(name))
        })
    };
    if let (Ok(a), Ok(b)) = (resolve(a), resolve(b)) {
        if a == b {
            return true;
        }
    }
    #[cfg(unix)]
    if let (Ok(a), Ok(b)) = (fs::metadata(a), fs::metadata(b)) {
        use std::os::unix::fs::MetadataExt;

        return a.dev() == b.dev() && a.ino() == b.ino();
    }
    false
}

#[cfg(unix)]
fn same_handle(path: &Path, handle: &impl std::os::fd::AsFd) -> bool {
    use std::os::unix::fs::MetadataExt;

    let Ok(fd) = handle.as_fd().try_clone_to_owned() else {
        return false;
    };
    let file = File::from(fd);

    if let (Ok(a), Ok(b)) = (fs::metadata(path), file.metadata()) {
        a.dev() == b.dev() && a.ino() == b.ino()
    } else {
        false
    }
}

fn stdout_alias(path: &Path) -> bool {
    if same_file(path, Path::new("/dev/stdout")) {
        return true;
    }
    #[cfg(unix)]
    if same_handle(path, &io::stdout()) {
        return true;
    }
    false
}

fn input_alias(path: &Path, input: &Path) -> bool {
    if same_file(path, input) {
        return true;
    }
    #[cfg(unix)]
    if input == Path::new("/dev/stdin") && same_handle(path, &io::stdin()) {
        return true;
    }
    false
}

fn destination(path: &Path) -> io::Result<PathBuf> {
    match fs::metadata(path) {
        Ok(meta) if meta.is_file() => path.canonicalize(),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "metadata output must name a regular file",
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // Refuse dangling links rather than replace the link itself.
            if fs::symlink_metadata(path).is_ok() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "metadata output is a dangling link",
                ));
            }
            let parent = path
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let name = path.file_name().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "metadata output requires a file name",
                )
            })?;

            Ok(parent.canonicalize()?.join(name))
        }
        Err(e) => Err(e),
    }
}

pub fn validate_paths(
    input: &OsStr,
    output: Option<&OsStr>,
    json: bool,
    metadata: &mut [Option<PathBuf>; 3],
) -> io::Result<()> {
    let stdout = Path::new("/dev/stdout");
    let input = if input == OsStr::new("-") {
        Path::new("/dev/stdin")
    } else {
        Path::new(input)
    };
    let output = output.map(|p| if is_stdout(p) { stdout } else { Path::new(p) });

    if json && output.is_some_and(stdout_alias) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "JSON info and decoded output cannot both use stdout",
        ));
    }
    if output.is_some_and(|p| input_alias(p, input)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "decoded output collides with input",
        ));
    }
    let mut destinations: [Option<PathBuf>; 3] = Default::default();

    for (i, path) in metadata.iter().enumerate() {
        let Some(path) = path else { continue };
        let dest = destination(path)?;

        if input_alias(&dest, input)
            || output.is_some_and(|p| same_file(&dest, p))
            || stdout_alias(&dest)
            || same_file(&dest, Path::new("/dev/stderr"))
            || destinations.iter().flatten().any(|p| same_file(&dest, p))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "metadata output collides with input or another output",
            ));
        }
        #[cfg(unix)]
        if same_handle(&dest, &io::stderr()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "metadata output collides with an input or output stream",
            ));
        }
        destinations[i] = Some(dest);
    }
    *metadata = destinations;
    Ok(())
}

fn json_info(decoder: &mut Decoder<'_>) -> io::Result<Vec<u8>> {
    let image = decoder.info().map_err(io::Error::other)?;
    let mut json = Vec::new();

    write!(json, "{{\"width\":{},\"height\":{},\"coding\":\"{}\",\"is_animation\":{},\"frame_count\":{},\"loop_count\":{},\"has_alpha\":{},\"has_icc\":{},\"has_exif\":{},\"has_xmp\":{},\"durations_ms\":[",
        image.width, image.height, image.coding.name(), image.is_animation,
        image.frame_count, image.loop_count, image.has_alpha,
        decoder.metadata(Metadata::Iccp).is_some(),
        decoder.metadata(Metadata::Exif).is_some(),
        decoder.metadata(Metadata::Xmp).is_some())?;
    for i in 0..image.frame_count {
        let duration = if image.is_animation {
            decoder.frame_info(i).map_err(io::Error::other)?.duration
        } else {
            0
        };

        write!(json, "{}{duration}", if i == 0 { "" } else { "," })?;
    }
    json.extend_from_slice(b"]}\n");
    Ok(json)
}

struct Pending {
    path: PathBuf,
    dest: PathBuf,
}

impl Pending {
    fn new(dest: &Path, data: &[u8]) -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);

        for _ in 0..64 {
            let mut name = dest.file_name().unwrap().to_os_string();

            name.push(format!(
                ".{}.{}.part",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let path = dest.with_file_name(name);
            let file = OpenOptions::new().write(true).create_new(true).open(&path);

            let mut file: File = match file {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            };
            let pending = Self {
                path,
                dest: dest.to_owned(),
            };

            file.write_all(data)?;
            file.flush()?;
            if let Ok(meta) = fs::metadata(dest) {
                file.set_permissions(meta.permissions())?;
            }
            return Ok(pending);
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "cannot create metadata temporary file",
        ))
    }

    fn publish(&self) -> io::Result<()> {
        fs::rename(&self.path, &self.dest)
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

pub fn write(
    decoder: &mut Decoder<'_>,
    json: bool,
    metadata: &[Option<PathBuf>; 3],
    input: &OsStr,
    output: Option<&OsStr>,
) -> io::Result<()> {
    // A newly created pixel file can reveal aliases that did not exist at
    // argument validation, including case-insensitive filesystem names.
    let mut metadata = metadata.clone();

    validate_paths(input, output, json, &mut metadata)?;
    let json = if json {
        Some(json_info(decoder)?)
    } else {
        None
    };
    let mut pending = Vec::new();

    for (kind, path) in [Metadata::Iccp, Metadata::Exif, Metadata::Xmp]
        .into_iter()
        .zip(&metadata)
    {
        if let Some(path) = path {
            pending.push(Pending::new(
                path,
                decoder.metadata(kind).unwrap_or_default(),
            )?);
        }
    }
    for (i, file) in pending.iter().enumerate() {
        if pending[..i]
            .iter()
            .any(|previous| same_file(&file.dest, &previous.dest))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "metadata outputs resolve to the same file",
            ));
        }
        file.publish()?;
    }
    if let Some(json) = json {
        let mut stdout = io::stdout().lock();

        stdout.write_all(&json)?;
        stdout.flush()?;
    }
    Ok(())
}
