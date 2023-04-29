use std::{fmt::Display, path::PathBuf, sync::RwLock};

use ash::vk;
use log::{debug, error, info};
use once_cell::sync::Lazy;
use regex::Regex;

// TODO: either do object wrapping or hash map dispatch. Until then there can only be a single
// instance/device
#[derive(Default)]
pub struct State {
    pub instance: RwLock<Option<vk::Instance>>,
    pub device: RwLock<Option<vk::Device>>,
    pub instance_get_fn: RwLock<Option<vk::PFN_vkGetInstanceProcAddr>>,
    pub device_get_fn: RwLock<Option<vk::PFN_vkGetDeviceProcAddr>>,
    pub settings: Settings,
}

#[derive(Debug, Eq, PartialEq, Default)]
pub enum Codec {
    #[default]
    H264,
    H265,
    AV1,
}
#[derive(Debug, Default)]
pub struct Settings {
    codec: Codec,
    output_file: PathBuf,
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

pub fn get_state() -> &'static State {
    static STATE: Lazy<State> = Lazy::new(|| {
        let mut state = State::default();
        let settings_file = get_settings_file();
        debug!("Trying to load {settings_file:?} to read layer configuration...");

        let settings_string = std::fs::read_to_string(&settings_file);
        match settings_string {
            Ok(settings_string) => {
                let regex = Regex::new(r"^thehamsta_video_record.(\w*)\s*=\s*(.*)$").unwrap();
                for line in settings_string.lines() {
                    for cap in regex.captures_iter(&line) {
                        info!(
                            "Parsed thehamsta_video_record.{} = \"{}\"",
                            &cap[1], &cap[2]
                        );
                        match &cap[1] {
                            "video_filename" => state.settings.output_file = cap[2].into(),
                            "codec" => state.settings.codec = cap[2].into(),
                            _ => error!("Could not parse unknown key {}", &cap[1]),
                        }
                    }
                }
            }
            Err(err) => error!("Failed to read settings file {settings_file:?}: {err}"),
        }

        if let Ok(output_file) = std::env::var("VK_VIDEO_RECORD_OUTPUT_FILE") {
            state.settings.output_file = output_file.into();
        }
        if let Ok(codec) = std::env::var("VK_VIDEO_RECORD_CODEC") {
            state.settings.codec = codec.into();
        }
        info!("{:?}", state.settings);
        state
    });
    &STATE
}

fn get_settings_file() -> PathBuf {
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
