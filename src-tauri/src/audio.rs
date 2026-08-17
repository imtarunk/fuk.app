use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, StreamConfig};
use parking_lot::Mutex;
use serde::Serialize;
use thiserror::Error;

const TARGET_HZ: u32 = 16_000;
const SILENCE_THRESH: f32 = 0.01;
const SILENCE_PAD_MS: u32 = 80;
const LEVEL_HZ: f32 = 20.0;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("no input device available")]
    NoDevice,
    #[error("unsupported sample format: {0:?}")]
    UnsupportedFormat(SampleFormat),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Clone, Serialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

struct SampleBuf {
    samples: Vec<f32>,
    sample_rate: u32,
    last_level_at: Instant,
}

/// cpal marks Stream !Send on every platform because of Android AAudio.
/// Desktop hosts are safe to move the stream when we only touch it via a mutex.
struct SendStream(cpal::Stream);
unsafe impl Send for SendStream {}

struct CaptureInner {
    stream: Option<SendStream>,
    buf: Arc<Mutex<SampleBuf>>,
}

pub struct CaptureController {
    inner: Mutex<CaptureInner>,
}

impl CaptureController {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(CaptureInner {
                stream: None,
                buf: Arc::new(Mutex::new(SampleBuf {
                    samples: Vec::new(),
                    sample_rate: TARGET_HZ,
                    last_level_at: Instant::now(),
                })),
            }),
        }
    }

    pub fn start(
        &self,
        device_id: Option<&str>,
        on_level: Arc<dyn Fn(f32) + Send + Sync>,
    ) -> Result<()> {
        let mut inner = self.inner.lock();
        inner.stream = None;

        let device = find_input_device(device_id)?;
        let supported = device
            .default_input_config()
            .context("default input config")?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.clone().into();
        let sample_rate = config.sample_rate.0;
        let channels = config.channels as usize;

        {
            let mut buf = inner.buf.lock();
            buf.samples.clear();
            buf.sample_rate = sample_rate;
            buf.last_level_at = Instant::now();
        }

        let buf = Arc::clone(&inner.buf);
        let stream = match sample_format {
            SampleFormat::F32 => build_stream::<f32>(&device, &config, buf, channels, on_level)?,
            SampleFormat::I16 => build_stream::<i16>(&device, &config, buf, channels, on_level)?,
            SampleFormat::U16 => build_stream::<u16>(&device, &config, buf, channels, on_level)?,
            SampleFormat::I32 => build_stream::<i32>(&device, &config, buf, channels, on_level)?,
            SampleFormat::U32 => build_stream::<u32>(&device, &config, buf, channels, on_level)?,
            SampleFormat::I64 => build_stream::<i64>(&device, &config, buf, channels, on_level)?,
            SampleFormat::U64 => build_stream::<u64>(&device, &config, buf, channels, on_level)?,
            SampleFormat::F64 => build_stream::<f64>(&device, &config, buf, channels, on_level)?,
            SampleFormat::U8 => build_stream::<u8>(&device, &config, buf, channels, on_level)?,
            SampleFormat::I8 => build_stream::<i8>(&device, &config, buf, channels, on_level)?,
            other => return Err(AudioError::UnsupportedFormat(other).into()),
        };

        stream.play().context("play input stream")?;
        inner.stream = Some(SendStream(stream));
        Ok(())
    }

    pub fn stop(&self) -> (Vec<f32>, u32) {
        let mut inner = self.inner.lock();
        if let Some(SendStream(stream)) = inner.stream.take() {
            let _ = stream.pause();
            drop(stream);
        }
        let mut buf = inner.buf.lock();
        let samples = std::mem::take(&mut buf.samples);
        let rate = buf.sample_rate;
        (samples, rate)
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    buf: Arc<Mutex<SampleBuf>>,
    channels: usize,
    on_level: Arc<dyn Fn(f32) + Send + Sync>,
) -> Result<cpal::Stream>
where
    T: SizedSample + Send + 'static,
    f32: FromSample<T>,
{
    let channels = channels.max(1);
    let stream = device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let mut level = None;
                {
                    let mut b = buf.lock();
                    let rate = b.sample_rate.max(1);
                    if channels == 1 {
                        b.samples
                            .extend(data.iter().copied().map(Sample::to_sample::<f32>));
                    } else {
                        for frame in data.chunks(channels) {
                            if frame.is_empty() {
                                continue;
                            }
                            let sum: f32 = frame.iter().copied().map(Sample::to_sample::<f32>).sum();
                            b.samples.push(sum / frame.len() as f32);
                        }
                    }
                    let interval = 1.0 / LEVEL_HZ;
                    if b.last_level_at.elapsed().as_secs_f32() >= interval {
                        let window = ((rate as f32 / LEVEL_HZ) as usize).max(1);
                        let start = b.samples.len().saturating_sub(window);
                        let slice = &b.samples[start..];
                        let rms = rms(slice);
                        b.last_level_at = Instant::now();
                        level = Some((rms * 4.0).clamp(0.0, 1.0));
                    }
                }
                if let Some(v) = level {
                    on_level(v);
                }
            },
            |e| log::error!("audio stream error: {e}"),
            None,
        )
        .context("build input stream")?;
    Ok(stream)
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f32 = samples.iter().map(|s| s * s).sum();
    (sum / samples.len() as f32).sqrt()
}

fn find_input_device(id: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if let Some(id) = id {
        if let Ok(devices) = host.input_devices() {
            for device in devices {
                if device.name().ok().as_deref() == Some(id) {
                    return Ok(device);
                }
            }
        }
        log::warn!("input device `{id}` not found, falling back to default");
    }
    host.default_input_device()
        .ok_or_else(|| AudioError::NoDevice.into())
}

pub fn list_input_devices() -> Result<Vec<AudioDevice>> {
    let host = cpal::default_host();
    let default_name = host
        .default_input_device()
        .and_then(|d| d.name().ok());
    let mut out = Vec::new();
    let devices = host
        .input_devices()
        .map_err(|e| anyhow!("list input devices: {e}"))?;
    for device in devices {
        let name = match device.name() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let is_default = default_name.as_ref() == Some(&name);
        out.push(AudioDevice {
            id: name.clone(),
            name,
            is_default,
        });
    }
    Ok(out)
}

pub fn resample_to_16k(samples: &[f32], from_hz: u32) -> Vec<f32> {
    if samples.is_empty() || from_hz == 0 || from_hz == TARGET_HZ {
        return samples.to_vec();
    }
    let ratio = from_hz as f64 / TARGET_HZ as f64;
    let out_len = ((samples.len() as f64) / ratio).floor() as usize;
    if out_len == 0 {
        return Vec::new();
    }
    let last = samples.len() - 1;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src = i as f64 * ratio;
        let i0 = (src.floor() as usize).min(last);
        let i1 = (i0 + 1).min(last);
        let frac = (src - i0 as f64) as f32;
        out.push(samples[i0] * (1.0 - frac) + samples[i1] * frac);
    }
    out
}

pub fn trim_silence(samples: &[f32]) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    let pad = ((TARGET_HZ as u64 * SILENCE_PAD_MS as u64) / 1000) as usize;
    let first = samples.iter().position(|s| s.abs() >= SILENCE_THRESH);
    let last = samples.iter().rposition(|s| s.abs() >= SILENCE_THRESH);
    match (first, last) {
        (Some(f), Some(l)) if l >= f => {
            let start = f.saturating_sub(pad);
            let end = (l + 1 + pad).min(samples.len());
            samples[start..end].to_vec()
        }
        _ => Vec::new(),
    }
}

/// Probe the default input by opening a short-lived stream (triggers TCC on macOS).
pub fn probe_microphone() -> bool {
    let host = cpal::default_host();
    let Some(device) = host.default_input_device() else {
        return false;
    };
    let Ok(supported) = device.default_input_config() else {
        return false;
    };
    let config: StreamConfig = supported.clone().into();
    let format = supported.sample_format();
    let dummy = |_data: &[f32], _: &cpal::InputCallbackInfo| {};
    let err = |_| {};
    let stream = match format {
        SampleFormat::F32 => device.build_input_stream(&config, dummy, err, None).ok(),
        SampleFormat::I16 => device
            .build_input_stream(
                &config,
                |data: &[i16], _| {
                    let _ = data;
                },
                err,
                None,
            )
            .ok(),
        _ => device
            .build_input_stream(
                &config,
                |data: &[u8], _| {
                    let _ = data;
                },
                err,
                None,
            )
            .ok(),
    };
    if let Some(stream) = stream {
        let _ = stream.play();
        std::thread::sleep(std::time::Duration::from_millis(80));
        drop(stream);
        true
    } else {
        host.default_input_device().is_some()
    }
}

pub fn microphone_available() -> bool {
    cpal::default_host().default_input_device().is_some()
}
