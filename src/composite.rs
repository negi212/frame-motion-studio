use anyhow::{anyhow, Result};
#[cfg(feature = "opencv")]
use opencv::{
    core::{self, Mat, Point, Scalar, Size, Vector, CV_8UC1, BORDER_CONSTANT},
    imgcodecs, imgproc,
    prelude::*,
    video,
    video::BackgroundSubtractorTrait,
    videoio::{VideoCapture, CAP_ANY, CAP_PROP_FPS, CAP_PROP_FRAME_COUNT, CAP_PROP_POS_FRAMES},
};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
        Arc,
    },
};

#[derive(Debug, Clone)]
pub struct CompositeParams {
    pub interval_sec: f64,
    pub min_area: f64,
    pub threshold: f64,
    pub history: i32,
}

impl Default for CompositeParams {
    fn default() -> Self {
        Self {
            interval_sec: 0.25,
            min_area: 200.0,
            threshold: 16.0,
            history: 200,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompositeRequest {
    pub input_path: PathBuf,
    pub output_path: PathBuf,
    pub params: CompositeParams,
}

#[derive(Debug, Clone)]
pub struct Progress {
    pub percent: f32,
    pub current_sec: f32,
    pub total_sec: f32,
}

#[derive(Debug)]
pub enum CompositeEvent {
    Progress(Progress),
    Done(PathBuf),
    Error(String),
    Cancelled,
}

#[cfg(feature = "opencv")]
pub fn run_composite(
    req: CompositeRequest,
    tx: Sender<CompositeEvent>,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    let input_str = req.input_path.to_string_lossy().to_string();
    let mut cap = VideoCapture::from_file(&input_str, CAP_ANY)
        .map_err(|e| anyhow!("動画を開けません: {} ({})", input_str, e))?;
    if !cap.is_opened().map_err(|e| anyhow!("open check: {}", e))? {
        let _ = tx.send(CompositeEvent::Error(format!("動画を開けません: {}", input_str)));
        return Err(anyhow!("動画を開けません: {}", input_str));
    }

    let fps = cap.get(CAP_PROP_FPS).unwrap_or(0.0);
    if fps <= 1e-6 || !fps.is_finite() {
        let msg = "FPSを取得できません".to_string();
        let _ = tx.send(CompositeEvent::Error(msg.clone()));
        return Err(anyhow!(msg));
    }
    let total_frames_f = cap.get(CAP_PROP_FRAME_COUNT).unwrap_or(0.0);
    let total_frames = total_frames_f as i32;
    if total_frames <= 0 {
        let msg = "総フレーム数を取得できません".to_string();
        let _ = tx.send(CompositeEvent::Error(msg.clone()));
        return Err(anyhow!(msg));
    }
    let total_sec = total_frames as f64 / fps;

    let mut first_frame = Mat::default();
    let ok = cap.read(&mut first_frame).map_err(|e| anyhow!("先頭フレーム読込失敗: {}", e))?;
    if !ok || first_frame.empty() {
        let msg = "先頭フレームを読み込めません".to_string();
        let _ = tx.send(CompositeEvent::Error(msg.clone()));
        return Err(anyhow!(msg));
    }
    let mut composite = first_frame.clone();

    let mut mog2 = video::create_background_subtractor_mog2(req.params.history, req.params.threshold, false)
        .map_err(|e| anyhow!("MOG2生成失敗: {}", e))?;

    {
        let mut tmp_mask = Mat::default();
        let _ = BackgroundSubtractorTrait::apply(&mut mog2, &first_frame, &mut tmp_mask, -1.0);
    }

    let _ = cap.set(CAP_PROP_POS_FRAMES, 0.0);

    let kernel = imgproc::get_structuring_element(
        imgproc::MORPH_ELLIPSE,
        Size::new(5, 5),
        Point::new(-1, -1),
    )
    .map_err(|e| anyhow!("kernel生成失敗: {}", e))?;

    let mut frame = Mat::default();
    let mut fg_mask = Mat::default();
    let mut opened = Mat::default();
    let mut closed = Mat::default();
    let mut clean_mask = Mat::default();

    let mut frame_idx: i32 = 0;
    let mut next_sample_time = req.params.interval_sec;

    while !cancel.load(Ordering::Relaxed) {
        let read_ok = cap.read(&mut frame).map_err(|e| anyhow!("フレーム読込失敗: {}", e))?;
        if !read_ok || frame.empty() {
            break;
        }

        let current_sec = frame_idx as f64 / fps;
        let percent = ((frame_idx as f32 / total_frames as f32) * 100.0).clamp(0.0, 100.0);
        let _ = tx.send(CompositeEvent::Progress(Progress {
            percent,
            current_sec: current_sec as f32,
            total_sec: total_sec as f32,
        }));

        BackgroundSubtractorTrait::apply(&mut mog2, &frame, &mut fg_mask, -1.0)
            .map_err(|e| anyhow!("MOG2 apply失敗: {}", e))?;

        if current_sec + 1e-9 >= next_sample_time {
            imgproc::morphology_ex(
                &fg_mask,
                &mut opened,
                imgproc::MORPH_OPEN,
                &kernel,
                Point::new(-1, -1),
                1,
                BORDER_CONSTANT,
                imgproc::morphology_default_border_value().unwrap_or(Scalar::default()),
            )
            .map_err(|e| anyhow!("morph open失敗: {}", e))?;

            imgproc::morphology_ex(
                &opened,
                &mut closed,
                imgproc::MORPH_CLOSE,
                &kernel,
                Point::new(-1, -1),
                1,
                BORDER_CONSTANT,
                imgproc::morphology_default_border_value().unwrap_or(Scalar::default()),
            )
            .map_err(|e| anyhow!("morph close失敗: {}", e))?;

            let mut contours: Vector<Vector<Point>> = Vector::new();
            imgproc::find_contours(
                &closed,
                &mut contours,
                imgproc::RETR_EXTERNAL,
                imgproc::CHAIN_APPROX_SIMPLE,
                Point::new(0, 0),
            )
            .map_err(|e| anyhow!("findContours失敗: {}", e))?;

            clean_mask = Mat::zeros(frame.rows(), frame.cols(), CV_8UC1)
                .map_err(|e| anyhow!("zeros失敗: {}", e))?
                .to_mat()
                .map_err(|e| anyhow!("to_mat失敗: {}", e))?;

            let mut has_valid_contour = false;
            for cnt in contours.iter() {
                let area = imgproc::contour_area(&cnt, false)
                    .map_err(|e| anyhow!("contourArea失敗: {}", e))?;
                if area >= req.params.min_area {
                    has_valid_contour = true;
                    let mut poly: Vector<Vector<Point>> = Vector::new();
                    poly.push(cnt.clone());
                    imgproc::fill_poly(
                        &mut clean_mask,
                        &poly,
                        Scalar::all(255.0),
                        imgproc::LINE_8,
                        0,
                        Point::new(0, 0),
                    )
                    .map_err(|e| anyhow!("fillPoly失敗: {}", e))?;
                }
            }

            if has_valid_contour {
                core::copy_to(&frame, &mut composite, &clean_mask)
                    .map_err(|e| anyhow!("copyTo失敗: {}", e))?;
            }

            while next_sample_time <= current_sec + 1e-9 {
                next_sample_time += req.params.interval_sec;
            }
        }

        frame_idx += 1;

        if frame_idx >= total_frames {
            let _ = tx.send(CompositeEvent::Progress(Progress {
                percent: 100.0,
                current_sec: total_sec as f32,
                total_sec: total_sec as f32,
            }));
        }
    }

    if cancel.load(Ordering::Relaxed) {
        let _ = tx.send(CompositeEvent::Cancelled);
        return Ok(());
    }

    if let Some(parent) = req.output_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow!("出力ディレクトリ作成失敗: {} ({})", parent.display(), e))?;
        }
    }

    let params = Vector::<i32>::new();
    imgcodecs::imwrite(
        &req.output_path.to_string_lossy().to_string(),
        &composite,
        &params,
    )
    .map_err(|e| anyhow!("画像保存失敗: {} ({})", req.output_path.display(), e))?;

    let _ = tx.send(CompositeEvent::Done(req.output_path.clone()));
    Ok(())
}

#[cfg(not(feature = "opencv"))]
pub fn run_composite(
    req: CompositeRequest,
    tx: Sender<CompositeEvent>,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    // Fallback for Windows without OpenCV: just copy first frame via ffmpeg or show message
    // Try to use ffmpeg to extract first frame, or just return error
    let _ = tx.send(CompositeEvent::Progress(Progress { percent: 0.0, current_sec: 0.0, total_sec: 1.0 }));
    if cancel.load(Ordering::Relaxed) {
        let _ = tx.send(CompositeEvent::Cancelled);
        return Ok(());
    }
    // Try ffmpeg
    let input = req.input_path.to_string_lossy().to_string();
    let output = req.output_path.to_string_lossy().to_string();
    if let Some(parent) = req.output_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    // Use ffmpeg to extract first frame and save as image (simple fallback)
    let status = std::process::Command::new("ffmpeg")
        .args(&["-y", "-i", &input, "-frames:v", "1", "-q:v", "2", &output])
        .output();
    match status {
        Ok(out) if out.status.success() && std::path::Path::new(&output).exists() => {
            let _ = tx.send(CompositeEvent::Progress(Progress { percent: 100.0, current_sec: 1.0, total_sec: 1.0 }));
            let _ = tx.send(CompositeEvent::Done(req.output_path.clone()));
            Ok(())
        }
        _ => {
            // Fallback: just copy input to output if ffmpeg fails? Or error
            let msg = "このビルドはOpenCV無しのため、合成は簡易的に先頭フレームを保存します。ffmpegが必要です。".to_string();
            let _ = tx.send(CompositeEvent::Error(msg.clone()));
            Err(anyhow!(msg))
        }
    }
}
