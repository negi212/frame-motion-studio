use anyhow::{anyhow, Result};
use opencv::{
    core::{Mat, Size, Scalar, CV_8UC1},
    imgcodecs, imgproc,
    videoio::{VideoCapture, CAP_ANY, CAP_PROP_FPS, CAP_PROP_FRAME_COUNT, CAP_PROP_FRAME_HEIGHT, CAP_PROP_FRAME_WIDTH},
    prelude::*,
};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub width: i32,
    pub height: i32,
    pub fps: f64,
    pub frame_count: i32,
    pub duration_sec: f64,
}

impl VideoInfo {
    pub fn resolution_str(&self) -> String {
        if self.width > 0 && self.height > 0 {
            format!("{} x {}", self.width, self.height)
        } else {
            "--".to_string()
        }
    }
    pub fn fps_str(&self) -> String {
        if self.fps.is_finite() && self.fps > 0.0 {
            format!("{:.2}", self.fps)
        } else {
            "--".to_string()
        }
    }
    pub fn frame_count_str(&self) -> String {
        if self.frame_count > 0 {
            format!("{}", self.frame_count)
        } else {
            "--".to_string()
        }
    }
    pub fn duration_str(&self) -> String {
        if self.duration_sec.is_finite() && self.duration_sec > 0.0 {
            format!("{:.2}s", self.duration_sec)
        } else {
            "--".to_string()
        }
    }
}

pub fn get_video_info<P: AsRef<Path>>(path: P) -> Result<VideoInfo> {
    let path_str = path.as_ref().to_string_lossy().to_string();
    #[allow(unused_mut)]
    let mut cap = VideoCapture::from_file(&path_str, CAP_ANY)
        .map_err(|e| anyhow!("動画を開けません: {} ({})", path_str, e))?;
    if !cap.is_opened().map_err(|e| anyhow!("VideoCapture open check failed: {}", e))? {
        return Err(anyhow!("動画を開けません: {}", path_str));
    }
    let fps = cap.get(CAP_PROP_FPS).unwrap_or(0.0);
    let frame_count = cap.get(CAP_PROP_FRAME_COUNT).unwrap_or(0.0) as i32;
    let width = cap.get(CAP_PROP_FRAME_WIDTH).unwrap_or(0.0) as i32;
    let height = cap.get(CAP_PROP_FRAME_HEIGHT).unwrap_or(0.0) as i32;
    let duration_sec = if fps > 1e-6 && frame_count > 0 {
        frame_count as f64 / fps
    } else {
        0.0
    };
    // validate FPS
    // If fps is 0, treat as error but still return info with --? Caller handles.
    Ok(VideoInfo {
        width,
        height,
        fps,
        frame_count,
        duration_sec,
    })
}

pub fn extract_first_frame_mat<P: AsRef<Path>>(path: P) -> Result<Mat> {
    let path_str = path.as_ref().to_string_lossy().to_string();
    let mut cap = VideoCapture::from_file(&path_str, CAP_ANY)
        .map_err(|e| anyhow!("動画を開けません: {} ({})", path_str, e))?;
    if !cap.is_opened().map_err(|e| anyhow!("open check: {}", e))? {
        return Err(anyhow!("動画を開けません: {}", path_str));
    }
    let mut frame = Mat::default();
    let ok = cap.read(&mut frame).map_err(|e| anyhow!("フレーム読込失敗: {}", e))?;
    if !ok || frame.empty() {
        return Err(anyhow!("先頭フレームを読み込めません: {}", path_str));
    }
    Ok(frame)
}

/// Mat (BGR) -> egui ColorImage (RGBA)
pub fn mat_to_color_image(mat: &Mat) -> Result<egui::ColorImage> {
    // Convert BGR -> RGBA
    let mut rgba = Mat::default();
    imgproc::cvt_color(mat, &mut rgba, imgproc::COLOR_BGR2RGBA, 0)
        .map_err(|e| anyhow!("cvtColor failed: {}", e))?;
    let rows = rgba.rows() as usize;
    let cols = rgba.cols() as usize;
    // Ensure continuous
    // Get bytes
    let bytes = rgba.data_bytes().map_err(|e| anyhow!("data_bytes: {}", e))?.to_vec();
    // bytes is RGBA order already (since we cvt to RGBA)
    // egui expects RGBA unmultiplied
    Ok(egui::ColorImage::from_rgba_unmultiplied([cols, rows], &bytes))
}

/// Mat -> PNG bytes (for saving preview to texture via image crate fallback)
pub fn mat_to_png_bytes(mat: &Mat) -> Result<Vec<u8>> {
    let mut buf = opencv::core::Vector::<u8>::new();
    let params = opencv::core::Vector::<i32>::new();
    imgcodecs::imencode(".png", mat, &mut buf, &params)
        .map_err(|e| anyhow!("imencode png: {}", e))?;
    Ok(buf.to_vec())
}

pub fn is_supported_video_extension(path: &Path) -> bool {
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext = ext.to_ascii_lowercase();
        matches!(
            ext.as_str(),
            "mp4" | "avi" | "mov" | "mkv" | "m4v" | "webm" | "wmv" | "flv" | "mpg" | "mpeg"
        )
    } else {
        false
    }
}

pub fn default_output_path(input: &Path, ext: &str) -> std::path::PathBuf {
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
    let ext = ext.trim_start_matches('.');
    let ext = if ext.is_empty() { "png" } else { ext };
    parent.join(format!("{}_composite.{}", stem, ext))
}

pub fn ensure_extension(path: &Path, default_ext: &str) -> std::path::PathBuf {
    if path.extension().is_some() {
        path.to_path_buf()
    } else {
        let mut p = path.to_path_buf();
        let ext = default_ext.trim_start_matches('.');
        p.set_extension(ext);
        p
    }
}
