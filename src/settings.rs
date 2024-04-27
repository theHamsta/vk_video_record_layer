use std::{fmt::Display, path::PathBuf};

use log::{debug, error, info};
use regex::Regex;

use crate::dpb::PictureType;

#[derive(Debug, Eq, PartialEq, Default, Copy, Clone)]
pub enum Codec {
    #[default]
    H264,
    H265,
    AV1,
}

#[derive(Debug, Eq, PartialEq, Default, Copy, Clone)]
pub enum RateControlMode {
    #[default]
    Cbr,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub codec: Codec,
    pub output_folder: PathBuf,
    pub use_nvpro: bool,
    pub gop_size: u64,
    pub idr_period: u64,
    pub max_consecutive_b_frames: u64,
    pub last_frame_type: PictureType,
    pub max_bitrate: u64,
    pub average_bitrate: u64,
    pub rate_control_mode: RateControlMode,
    pub frame_rate_numerator: u32,
    pub frame_rate_denominator: u32,
    pub vbv_size_in_ms: u32,
    pub initial_vbv_size_in_ms: u32,
    pub quality_level: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            codec: Codec::default(),
            output_folder: "".into(),
            use_nvpro: false,
            gop_size: 16,
            idr_period: 16,
            max_consecutive_b_frames: 0,
            last_frame_type: PictureType::P,
            initial_vbv_size_in_ms: 0,
            vbv_size_in_ms: 1000,
            max_bitrate: 10 * 1024 * 1024,
            average_bitrate: 8 * 1024 * 1024,
            rate_control_mode: RateControlMode::Cbr,
            frame_rate_numerator: 60,
            frame_rate_denominator: 1,
            quality_level: 1,
        }
    }
}

impl Settings {
    pub(crate) fn new_from_env() -> Self {
        let mut settings = Settings::default();
        let settings_file = get_settings_file();
        debug!("Trying to load {settings_file:?} to read layer configuration...");

        let settings_string = std::fs::read_to_string(&settings_file);
        match settings_string {
            Ok(settings_string) => {
                let regex = Regex::new(r"^thehamsta_video_record.(\w*)\s*=\s*(.*)$").unwrap();
                for line in settings_string.lines() {
                    for cap in regex.captures_iter(line) {
                        info!(
                            "Parsed thehamsta_video_record.{} = \"{}\"",
                            &cap[1], &cap[2]
                        );
                        match &cap[1] {
                            "video_output_folder" => settings.output_folder = cap[2].into(),
                            "codec" => settings.codec = cap[2].into(),
                            "rate_control_mode" => settings.rate_control_mode = cap[2].into(),
                            "use_nvpro" => settings.use_nvpro = cap[2].parse().unwrap_or(false),
                            "gop_size" => settings.gop_size = cap[2].parse().unwrap_or(16),
                            "idr_period" => settings.gop_size = cap[2].parse().unwrap_or(16),
                            "last_frame_type" => settings.last_frame_type = cap[2].into(),
                            "max_consecutive_b_frames" => {
                                settings.max_consecutive_b_frames = cap[2].parse().unwrap_or(16)
                            }
                            "frame_rate_numerator" => {
                                settings.frame_rate_numerator = cap[2].parse().unwrap_or(60)
                            }
                            "frame_rate_denominator" => {
                                settings.frame_rate_denominator = cap[2].parse().unwrap_or(1)
                            }
                            "average_bitrate" => {
                                settings.average_bitrate = cap[2].parse().unwrap_or(8 * 1024 * 1024)
                            }
                            "max_bitrate" => {
                                settings.max_bitrate = cap[2].parse().unwrap_or(8 * 1024 * 1024)
                            }
                            "vbv_size_in_ms" => {
                                settings.vbv_size_in_ms = cap[2].parse().unwrap_or(8 * 1024 * 1024)
                            }
                            "initial_vbv_size_in_ms" => {
                                settings.initial_vbv_size_in_ms =
                                    cap[2].parse().unwrap_or(8 * 1024 * 1024)
                            }
                            "quality_level" => settings.quality_level = cap[2].parse().unwrap_or(1),
                            _ => error!("Could not parse unknown key {}", &cap[1]),
                        }
                    }
                }
            }
            Err(err) => error!("Failed to read settings file {settings_file:?}: {err}"),
        }

        if let Ok(output_file) = std::env::var("VK_VIDEO_RECORD_OUTPUT_FOLDER") {
            settings.output_folder = output_file.into();
        }
        if let Ok(codec) = std::env::var("VK_VIDEO_RECORD_CODEC") {
            settings.codec = codec.into();
        }
        info!("{:?}", settings);
        settings
    }
}

impl<T> From<T> for Codec
where
    T: AsRef<str> + Display,
{
    fn from(value: T) -> Self {
        match value.as_ref() {
            "H264" => Codec::H264,
            "H265" => Codec::H265,
            "AV1" => Codec::AV1,
            _ => {
                error!(
                    "Could not parse value \"{}\" for VK_VIDEO_RECORD_CODEC! Falling back to {:?}",
                    value,
                    Codec::default()
                );
                Codec::default()
            }
        }
    }
}

impl<T> From<T> for RateControlMode
where
    T: AsRef<str> + Display,
{
    fn from(value: T) -> Self {
        match value.as_ref() {
            "CBR" => RateControlMode::Cbr,
            _ => {
                error!(
                    "Could not parse value \"{}\" for rate control mode! Falling back to {:?}",
                    value,
                    RateControlMode::default()
                );
                RateControlMode::default()
            }
        }
    }
}

impl<T> From<T> for PictureType
where
    T: AsRef<str> + Display,
{
    fn from(value: T) -> Self {
        match value.as_ref() {
            "I" => PictureType::I,
            "IDR" => PictureType::Idr,
            "P" => PictureType::P,
            "B" => PictureType::B,
            _ => {
                error!(
                    "Could not parse value \"{}\" for VK_VIDEO_RECORD_CODEC! Falling back to {:?}",
                    value,
                    PictureType::P
                );
                PictureType::P
            }
        }
    }
}

pub(crate) fn get_settings_file() -> PathBuf {
    if let Some(data_dir) = dirs::data_local_dir() {
        let local_vk_dir = data_dir.join("vulkan/settings.d/vk_layer_settings.txt");
        if local_vk_dir.is_file() {
            return local_vk_dir;
        }
        if let Ok(overide_path) = std::env::var("VK_LAYER_SETTINGS_PATH") {
            let overide_file = PathBuf::from(overide_path).join("vk_layer_settings.txt");
            if overide_file.is_file() {
                return overide_file;
            }
        }
    }

    "vk_layer_settings.txt".into()
}
