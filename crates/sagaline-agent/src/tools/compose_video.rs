//! `compose_video` agent tool.
//!
//! Stitches N input mp4s into a single output mp4 by **re-muxing**
//! (no re-encoding) — samples are copied from each input's
//! video track into one output track in the order supplied. The
//! output uses the codec parameters from the **first** input, so all
//! inputs must share codec config (typical when all shots are
//! produced by the same `submit_video` call).
//!
//! ## Limitations
//!
//! - **Re-mux only.** A real composer would re-encode to handle
//!   mixed codecs / resolutions / frame rates. This tool does
//!   not — it copies raw sample bytes through.
//! - **Single video track.** Multi-track input (e.g. multi-angle)
//!   picks the first video track and drops the rest.
//! - **No audio track passthrough (yet).** Future phase will copy
//!   audio samples alongside; today's cut is video-only. The
//!   tool's output mp4 will have a video track but no audio.
//!
//! ## Wire shape
//!
//! Args (JSON):
//!
//! ```json
//! {
//!   "shot_paths":  ["/abs/shot_004.mp4", "/abs/shot_005.mp4"],
//!   "output_path": "/abs/episode_01.mp4"
//! }
//! ```
//!
//! Output:
//!
//! ```json
//! {
//!   "path":             "/abs/episode_01.mp4",
//!   "bytes":            12345678,
//!   "inputs":           2,
//!   "video_samples":    187
//! }
//! ```

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use async_trait::async_trait;
use serde::Serialize;
use serde_json::Value;

use crate::tool::{Tool, ToolDescriptor, ToolError, ToolResult};

/// Tool arguments.
#[derive(Debug, Clone, ::serde::Deserialize, ::serde::Serialize, ::schemars::JsonSchema)]
pub struct ComposeVideoArgs {
    /// Input mp4 paths, in the order they should appear in the
    /// output. Must be non-empty; must be valid mp4 files
    /// sharing codec config.
    pub shot_paths: Vec<PathBuf>,

    /// Where to write the composed mp4. Created (with parents)
    /// if missing.
    pub output_path: PathBuf,
}

#[derive(Debug, Serialize)]
struct ComposeVideoOutput {
    path: String,
    bytes: u64,
    inputs: usize,
    video_samples: u32,
}

/// The tool. Stateless — no registry / store / config dependency.
#[derive(Clone, Default)]
pub struct ComposeVideoTool;

impl ComposeVideoTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl Tool for ComposeVideoTool {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor::for_args::<ComposeVideoArgs>(
            "compose_video",
            "Stitch a sequence of mp4 shots into a single mp4 by re-muxing \
             (no re-encoding). Samples are copied in the order `shot_paths` \
             supplies; the output's codec params come from the first input, \
             so all inputs must share codec config. Returns \
             `{path, bytes, inputs, video_samples}`. Video-only in this \
             first cut; audio passthrough is a future phase.",
            crate::tool::Capability::Execute,
        )
    }

    async fn execute(
        &self,
        _ctx: crate::tool::ToolContext,
        args: Value,
    ) -> Result<ToolResult, ToolError> {
        let parsed: ComposeVideoArgs =
            serde_json::from_value(args).map_err(|e| ToolError::BadArgs {
                name: "compose_video".into(),
                message: e.to_string(),
            })?;

        // The mp4 crate's reader / writer types are not
        // `Send` across `.await`, so the heavy lifting runs
        // inside `spawn_blocking`.
        let join_result = tokio::task::spawn_blocking(move || {
            run_compose(&parsed.shot_paths, &parsed.output_path)
        })
        .await
        .map_err(|e| ToolError::Execution {
            name: "compose_video".into(),
            source: Box::new(std::io::Error::new(std::io::ErrorKind::Other, e)),
        })?;
        let result = join_result.map_err(|e| ToolError::Execution {
            name: "compose_video".into(),
            source: Box::new(std::io::Error::new(std::io::ErrorKind::Other, e)),
        })?;

        serde_json::to_value(&result).map_err(|e| ToolError::Execution {
            name: "compose_video".into(),
            source: Box::new(e),
        })
    }
}

/// Pick the lowest-id video track id from an [`mp4::Mp4Reader`].
/// Returns `None` if there are no video tracks (audio-only mp4
/// or container with metadata only).
fn first_video_track_id<R: std::io::Read + std::io::Seek>(
    reader: &mp4::Mp4Reader<R>,
) -> Option<u32> {
    let mut ids: Vec<u32> = reader
        .tracks()
        .iter()
        .filter_map(|(id, track)| {
            // `track_type` returns a Result; non-video tracks
            // are skipped (we don't want to crash on a metadata
            // track whose type we can't decode).
            match track.track_type() {
                Ok(mp4::TrackType::Video) => Some(*id),
                _ => None,
            }
        })
        .collect();
    ids.sort();
    ids.into_iter().next()
}

/// Read the video track's `MediaConfig` out of the reader so
/// the output `TrackConfig` matches. The mp4 v0.14 `StsdBox`
/// holds one codec-specific child box (`avc1`, `hev1`, `vp09`,
/// `mp4a`, `tx3g`) — pick the first video one that's `Some`
/// and rebuild a `MediaConfig` from it. Audio passthrough is
/// not implemented in this first cut (the `mp4` crate's audio
/// path requires rebuilding `AacConfig` from `EsdsBox`'s raw
/// fields, which is out of scope).
fn video_media_conf<R: std::io::Read + std::io::Seek>(
    reader: &mp4::Mp4Reader<R>,
    track_id: u32,
) -> Result<mp4::MediaConfig, String> {
    let track = reader
        .tracks()
        .get(&track_id)
        .ok_or_else(|| format!("track {track_id} not in reader"))?;
    let stsd = &track.trak.mdia.minf.stbl.stsd;
    if let Some(avc1) = &stsd.avc1 {
        let sps: Vec<u8> = avc1
            .avcc
            .sequence_parameter_sets
            .first()
            .map(|n| n.bytes.clone())
            .unwrap_or_default();
        let pps: Vec<u8> = avc1
            .avcc
            .picture_parameter_sets
            .first()
            .map(|n| n.bytes.clone())
            .unwrap_or_default();
        Ok(mp4::MediaConfig::AvcConfig(mp4::AvcConfig {
            width: avc1.width,
            height: avc1.height,
            seq_param_set: sps,
            pic_param_set: pps,
        }))
    } else if let Some(hev1) = &stsd.hev1 {
        Ok(mp4::MediaConfig::HevcConfig(mp4::HevcConfig {
            width: hev1.width,
            height: hev1.height,
        }))
    } else if let Some(vp09) = &stsd.vp09 {
        Ok(mp4::MediaConfig::Vp9Config(mp4::Vp9Config {
            width: vp09.width,
            height: vp09.height,
        }))
    } else {
        Err("video track has no recognised video codec box (avc1/hev1/vp09) — \
             audio-only inputs are not supported by compose_video in this phase"
            .into())
    }
}

/// Synchronous composition. Run inside `spawn_blocking` because
/// `mp4::Mp4Reader` / `Mp4Writer` hold `!Send` I/O handles.
fn run_compose(
    shot_paths: &[PathBuf],
    output_path: &std::path::Path,
) -> Result<ComposeVideoOutput, String> {
    use mp4::{Mp4Config, Mp4Reader, Mp4Writer, TrackConfig, TrackType};

    if shot_paths.is_empty() {
        return Err("compose_video: shot_paths must be non-empty".into());
    }

    // 1. Open the first input to seed the output track's codec
    //    config. All subsequent inputs must share this config
    //    (re-mux can't transcode).
    let first_path = &shot_paths[0];
    let first_file = File::open(first_path)
        .map_err(|e| format!("open first input {}: {e}", first_path.display()))?;
    let first_size = first_file
        .metadata()
        .map_err(|e| format!("stat first input: {e}"))?
        .len();
    let first_reader = Mp4Reader::read_header(first_file, first_size)
        .map_err(|e| format!("read_header first input: {e}"))?;

    let first_video_id =
        first_video_track_id(&first_reader).ok_or_else(|| {
            format!("first input {} has no video track", first_path.display())
        })?;
    let first_video_cfg = video_media_conf(&first_reader, first_video_id)?;

    let first_track = first_reader
        .tracks()
        .get(&first_video_id)
        .ok_or_else(|| "first input video track vanished".to_string())?;
    let first_timescale = first_track.timescale();
    let first_language = first_track.language().to_string();

    // 2. Open the output writer. `Mp4Config` has no Default impl
    //    in mp4 v0.14, so build one explicitly.
    let output_file = File::create(output_path)
        .map_err(|e| format!("create output {}: {e}", output_path.display()))?;
    let writer_config = Mp4Config {
        major_brand: mp4::FourCC::from(*b"mp42"),
        minor_version: 0,
        compatible_brands: vec![mp4::FourCC::from(*b"mp42"), mp4::FourCC::from(*b"isom")],
        timescale: first_timescale,
    };
    let mut writer =
        Mp4Writer::write_start(BufWriter::new(output_file), &writer_config)
            .map_err(|e| format!("Mp4Writer::write_start: {e}"))?;

    let track_config = TrackConfig {
        track_type: TrackType::Video,
        timescale: first_timescale,
        language: first_language,
        media_conf: first_video_cfg,
    };
    // The crate's `add_track` returns `Result<()>` and doesn't
    // hand back a track id, so we manage one ourselves. mp4 files
    // conventionally start track ids at 1.
    let out_track_id: u32 = 1;
    writer
        .add_track(&track_config)
        .map_err(|e| format!("add_track: {e}"))?;

    // 3. Walk each input, copy video samples into the output
    //    track. `start_time` is accumulated so the timeline is
    //    continuous.
    let mut total_samples: u32 = 0;
    let mut next_start_time: u64 = 0;
    for (idx, shot_path) in shot_paths.iter().enumerate() {
        let file = File::open(shot_path)
            .map_err(|e| format!("open input #{} {}: {e}", idx, shot_path.display()))?;
        let size = file
            .metadata()
            .map_err(|e| format!("stat input #{}: {e}", idx))?
            .len();
        let mut reader = Mp4Reader::read_header(file, size).map_err(|e| {
            format!("read_header input #{} {}: {e}", idx, shot_path.display())
        })?;

        let video_id = first_video_track_id(&reader).ok_or_else(|| {
            format!(
                "input #{} {} has no video track",
                idx,
                shot_path.display()
            )
        })?;
        let n = reader
            .sample_count(video_id)
            .map_err(|e| format!("sample_count input #{idx}: {e}"))?;

        for sample_id in 0..n {
            let Some(mut sample) = reader
                .read_sample(video_id, sample_id)
                .map_err(|e| format!("read_sample input #{idx} #{sample_id}: {e}"))?
            else {
                continue;
            };
            sample.start_time = next_start_time;
            writer
                .write_sample(out_track_id, &sample)
                .map_err(|e| format!("write_sample input #{idx} #{sample_id}: {e}"))?;
            next_start_time = next_start_time.saturating_add(sample.duration as u64);
            total_samples = total_samples.saturating_add(1);
        }
    }

    writer
        .write_end()
        .map_err(|e| format!("Mp4Writer::write_end: {e}"))?;
    drop(writer);

    // 4. Stat the output to report `bytes` honestly.
    let final_bytes = std::fs::metadata(output_path)
        .map_err(|e| format!("stat output: {e}"))?
        .len();

    Ok(ComposeVideoOutput {
        path: output_path.to_string_lossy().into_owned(),
        bytes: final_bytes,
        inputs: shot_paths.len(),
        video_samples: total_samples,
    })
}