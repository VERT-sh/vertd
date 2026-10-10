use std::fs::File;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use log::{info, warn};
use serde::{Deserialize, Serialize};
use strum_macros::{Display, EnumString};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, EnumString, Display)]
#[serde(rename_all = "camelCase")]
pub enum SequenceKind {
    // image sequence -> video / animated images
    #[strum(serialize = "frameSequence")]
    FrameSequence,
    // DASH stream (e.g. steam recordings - mpd + m4s) -> video / animated images
    #[strum(serialize = "dash")]
    Dash,
}

const JUNK_NAMES: &[&str] = &[".DS_Store", "Thumbs.db", "desktop.ini"];
const JUNK_PREFIXES: &[&str] = &["._", "__MACOSX"];

fn is_junk(name: &str) -> bool {
    let name = name.rsplit('/').next().unwrap_or(name);
    JUNK_NAMES.contains(&name) || JUNK_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
}

#[derive(Debug, thiserror::Error)]
pub enum SequenceError {
    #[error("failed to read zip archive: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("filesystem error: {0}")]
    Io(#[from] std::io::Error),
    #[error("zip archive is empty")]
    Empty,
    #[error("zip archive root contains a file without an extension: {0}")]
    NonMediaFile(String),
    #[error("zip archive contains images with mixed extensions: {0}")]
    MixedExtensions(String),
    #[error("zip archive contains no media files at its root")]
    NoMedia,
    #[error("DASH sequence is missing its .mpd manifest and has no .m4s segments")]
    MissingDashManifest,
    #[error("decompressed contents ({size} bytes) exceed the allowed limit of {limit} bytes")]
    TooLarge { size: u64, limit: u64 },
}

#[derive(Debug, Clone)]
pub struct PreparedSequence {
    pub kind: SequenceKind,
    // file path relative to input/
    pub entry: String,
    /// inner extension, used as the job's `from` (e.g. `png`, `mpd`)
    pub extension: String,
    /// known frame count (image sequences only)
    pub frame_count: Option<u64>,
}

pub const CONCAT_MANIFEST: &str = "sequence.ffconcat";

// fallback file for DASH sequences without a manifest (.mpd)
pub const CONCAT_MEDIA: &str = "sequence.mp4";

// e.g. 00001.png
fn frame_name(index: u64, extension: &str) -> String {
    format!("{:05}.{extension}", index)
}

struct SequenceFile {
    index: usize,
    name: String,
    extension: String,
}

fn root_files(archive: &mut ZipArchive<File>) -> Result<Vec<SequenceFile>, SequenceError> {
    let mut candidates: Vec<(usize, String, Vec<String>)> = Vec::new();

    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;

        // skip dirs and symlinks
        if !entry.is_file() {
            continue;
        }

        let Some(path) = entry.enclosed_name() else {
            warn!("skipping zip entry with unsafe path at index {}", index);
            continue;
        };

        let mut components = Vec::new();
        let mut utf8 = true;
        for component in path.components() {
            match component.as_os_str().to_str() {
                Some(component) => components.push(component.to_string()),
                None => {
                    utf8 = false;
                    break;
                }
            }
        }

        if !utf8 {
            warn!("skipping zip entry with non-utf8 path at index {}", index);
            continue;
        }

        let Some(name) = components.last().cloned() else {
            continue;
        };

        if is_junk(&name) {
            continue;
        }

        candidates.push((index, name, components));
    }

    // strip the single top-level directory if all files are under it
    let strip = candidates
        .first()
        .map(|(_, _, first)| {
            let top = first.first();
            let all_under_top = candidates
                .iter()
                .all(|(_, _, components)| components.len() >= 2 && components.first() == top);
            let all_flat_after_strip = candidates
                .iter()
                .all(|(_, _, components)| components.len() == 2);
            usize::from(all_under_top && all_flat_after_strip)
        })
        .unwrap_or(0);

    let files = candidates
        .into_iter()
        .filter(|(_, _, components)| components.len() == 1 + strip)
        .map(|(index, name, _)| {
            let extension = name
                .rsplit_once('.')
                .map(|(_, extension)| extension.to_lowercase())
                .unwrap_or_default();

            SequenceFile {
                index,
                name,
                extension,
            }
        })
        .collect();

    Ok(files)
}

pub fn extract_sequence(
    zip_path: &Path,
    dest_dir: &Path,
    kind: SequenceKind,
    max_uncompressed: Option<u64>,
) -> Result<PreparedSequence, SequenceError> {
    let file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;

    if let Some(limit) = max_uncompressed {
        if let Some(size) = archive.decompressed_size() {
            if size > limit as u128 {
                return Err(SequenceError::TooLarge {
                    size: size as u64,
                    limit,
                });
            }
        }
    }

    let files = root_files(&mut archive)?;
    if files.is_empty() {
        return Err(SequenceError::Empty);
    }

    std::fs::create_dir_all(dest_dir)?;

    match kind {
        SequenceKind::FrameSequence => prepare_frames(&mut archive, &files, dest_dir),
        SequenceKind::Dash => prepare_dash(&mut archive, &files, dest_dir),
    }
}

fn prepare_frames(
    archive: &mut ZipArchive<File>,
    files: &[SequenceFile],
    dest_dir: &Path,
) -> Result<PreparedSequence, SequenceError> {
    if let Some(file) = files.iter().find(|file| file.extension.is_empty()) {
        return Err(SequenceError::NonMediaFile(file.name.clone()));
    }

    let extension = files[0].extension.clone();
    if let Some(file) = files.iter().find(|file| file.extension != extension) {
        return Err(SequenceError::MixedExtensions(format!(
            "{} and {}",
            extension, file.extension
        )));
    }

    // natural sorting (e.g. frame2 < frame10)
    let mut ordered: Vec<&SequenceFile> = files.iter().collect();
    ordered.sort_by(|a, b| natord::compare_ignore_case(&a.name, &b.name));

    let mut frame_count = 0u64;
    let mut manifest = String::from("ffconcat version 1.0\n");
    for file in &ordered {
        frame_count += 1;
        let name = frame_name(frame_count, &extension);

        let mut entry = archive.by_index(file.index)?;
        let mut frame = File::create(dest_dir.join(&name))?;
        std::io::copy(&mut entry, &mut frame)?;

        manifest.push_str(&format!("file '{name}'\n"));
    }

    std::fs::write(dest_dir.join(CONCAT_MANIFEST), manifest)?;

    info!(
        "prepared frame sequence: {} frame(s) of .{}",
        frame_count, extension
    );

    Ok(PreparedSequence {
        kind: SequenceKind::FrameSequence,
        entry: CONCAT_MANIFEST.to_string(),
        extension,
        frame_count: Some(frame_count),
    })
}

// use .mpd as manifest, else concatenate .m4s segments (init first) into a single .mp4 file
// TODO: support multiple resolutions (e.g. 1080p, 720p) in a single DASH sequence, so that we don't need to re-encode for each resolution if the user requests an exact resolution via settings
fn prepare_dash(
    archive: &mut ZipArchive<File>,
    files: &[SequenceFile],
    dest_dir: &Path,
) -> Result<PreparedSequence, SequenceError> {
    for file in files {
        let mut entry = archive.by_index(file.index)?;
        let mut out = File::create(dest_dir.join(&file.name))?;
        std::io::copy(&mut entry, &mut out)?;
    }

    // check for manifest
    if let Some(manifest) = files.iter().find(|file| file.extension == "mpd") {
        info!("prepared DASH sequence with manifest {}", manifest.name);
        return Ok(PreparedSequence {
            kind: SequenceKind::Dash,
            entry: manifest.name.clone(),
            extension: "mpd".to_string(),
            frame_count: None,
        });
    }

    // else if no manifest
    let mut segments: Vec<&SequenceFile> = files
        .iter()
        .filter(|file| file.extension == "m4s" || file.extension == "m4d")
        .collect();

    if segments.is_empty() {
        return Err(SequenceError::MissingDashManifest);
    }

    segments.sort_by(|a, b| {
        let a_init = a.name.to_lowercase().contains("init");
        let b_init = b.name.to_lowercase().contains("init");
        b_init
            .cmp(&a_init)
            .then_with(|| natord::compare_ignore_case(&a.name, &b.name))
    });

    let concat_path = dest_dir.join(CONCAT_MEDIA);
    let mut out = File::create(&concat_path)?;
    for segment in &segments {
        let mut entry = archive.by_index(segment.index)?;
        std::io::copy(&mut entry, &mut out)?;
    }
    out.flush()?;

    log::info!(
        "concatenated {} DASH segment(s) into {}",
        segments.len(),
        concat_path.display()
    );

    Ok(PreparedSequence {
        kind: SequenceKind::Dash,
        entry: CONCAT_MEDIA.to_string(),
        extension: "mp4".to_string(),
        frame_count: None,
    })
}

// zip for permanent storage (e.g. admin downloads)
pub fn zip_sequence(src_dir: &Path, dest_path: &Path) -> Result<(), SequenceError> {
    let file = File::create(dest_path)?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

    let mut frames: Vec<PathBuf> = std::fs::read_dir(src_dir)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_file())
        .collect();
    frames.sort();

    let mut written = 0u64;
    for frame in &frames {
        let Some(name) = frame.file_name().and_then(|name| name.to_str()) else {
            warn!("skipping non-utf8 frame path {}", frame.display());
            continue;
        };

        if name == CONCAT_MANIFEST || name == CONCAT_MEDIA {
            continue;
        }

        writer.start_file(name, options)?;
        let mut frame_file = File::open(frame)?;
        std::io::copy(&mut frame_file, &mut writer)?;
        written += 1;
    }

    writer.finish()?;

    if written == 0 {
        return Err(SequenceError::NoMedia);
    }

    info!(
        "wrote {} file(s) from {} into {}",
        written,
        src_dir.display(),
        dest_path.display()
    );

    Ok(())
}
