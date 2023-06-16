use crate::bitstream::{write_h264_pps, write_h264_sps};
use ash::prelude::VkResult;
use ash::vk;
use log::error;
use std::io::Write;
use std::mem::{transmute, MaybeUninit};
use std::ptr::null;

//// TODO: handle vui with valid pointers
//pub enum CodecParameters {
//H264Parameters {
//sps: vk::native::StdVideoH264SequenceParameterSet,
//pps: vk::native::StdVideoH264PictureParameterSet,
//},
//H265Parameters {
//sps: vk::native::StdVideoH265SequenceParameterSet,
//pps: vk::native::StdVideoH265PictureParameterSet,
//vps: vk::native::StdVideoH265VideoParameterSet,
//},
//}

//pub struct VideoSessionParameters {
//parameters: vk::VideoSessionParametersKHR,
//codec_parameters: CodecParameters,
//}

//impl VideoSessionParameters {
//pub fn parameters(&self) -> vk::VideoSessionParametersKHR {
//self.parameters
//}

//pub fn codec_parameters(&self) -> &CodecParameters {
//&self.codec_parameters
//}
//}

pub fn make_h264_video_session_parameters(
    device: &ash::Device,
    video_queue_fn: &vk::KhrVideoQueueFn,
    video_session: vk::VideoSessionKHR,
    format: vk::Format,
    extent: vk::Extent2D,
    output_file: Option<impl Write>,
    allocator: Option<&vk::AllocationCallbacks>,
) -> VkResult<vk::VideoSessionParametersKHR> {
    let bitdepth = 8;
    let flags = unsafe { MaybeUninit::zeroed().assume_init() };
    let _vui = vk::native::StdVideoH264SequenceParameterSetVui {
        flags,
        aspect_ratio_idc:
            ash::vk::native::StdVideoH265AspectRatioIdc_STD_VIDEO_H265_ASPECT_RATIO_IDC_SQUARE,
        sar_width: 0,
        sar_height: 0,
        video_format: 0,
        colour_primaries: 0,
        transfer_characteristics: 0,
        matrix_coefficients: 0,
        num_units_in_tick: 1000,
        time_scale: 0,
        max_num_reorder_frames: 0,
        max_dec_frame_buffering: 0,
        chroma_sample_loc_type_top_field: 0,
        chroma_sample_loc_type_bottom_field: 0,
        reserved1: 0,
        pHrdParameters: null(),
    };
    assert_eq!(format, vk::Format::G8_B8R8_2PLANE_420_UNORM);

    let flags = MaybeUninit::zeroed();
    let mut flags: vk::native::StdVideoH264SpsFlags = unsafe { flags.assume_init() };
    // Use whatever ffmpeg uses for h264 nvenc
    flags.set_frame_mbs_only_flag(1);
    flags.set_direct_8x8_inference_flag(1);
    //https://registry.khronos.org/vulkan/specs/1.3-extensions/html/vkspec.html#decode-h264-sps
    let mut sps = vec![vk::native::StdVideoH264SequenceParameterSet {
        flags,
        profile_idc: vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_MAIN,
        level_idc: vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_5_1,
        chroma_format_idc:
            vk::native::StdVideoH264ChromaFormatIdc_STD_VIDEO_H264_CHROMA_FORMAT_IDC_420,
        seq_parameter_set_id: 0,
        bit_depth_luma_minus8: bitdepth - 8,
        bit_depth_chroma_minus8: bitdepth - 8,
        log2_max_frame_num_minus4: 10 - 4,
        pic_order_cnt_type: 0,
        offset_for_non_ref_pic: 0,
        offset_for_top_to_bottom_field: 0,
        log2_max_pic_order_cnt_lsb_minus4: 8 - 4, // pic order count 0-255
        num_ref_frames_in_pic_order_cnt_cycle: 0,
        max_num_ref_frames: 0,
        reserved1: 0,
        pic_width_in_mbs_minus1: (extent.width + 15) / 16 - 1, //extent.width.div_ceil(16) - 1, // with unstable feature int_roundings
        pic_height_in_map_units_minus1: (extent.height + 15) / 16 - 1,
        frame_crop_left_offset: 0,
        frame_crop_right_offset: (16 - extent.width % 16) / 2,
        frame_crop_top_offset: 0,
        frame_crop_bottom_offset: (16 - extent.height % 16) / 2,
        reserved2: 0,
        pOffsetForRefFrame: null(),
        pScalingLists: null(),
        pSequenceParameterSetVui: null(), //&vui (requires vui_is_present_flag)
    }];
    if sps[0].frame_crop_right_offset != 0 || sps[0].frame_crop_bottom_offset != 0 {
        sps[0].flags.set_frame_cropping_flag(1);
    }
    let flags = MaybeUninit::zeroed();
    let mut flags: vk::native::StdVideoH264PpsFlags = unsafe { flags.assume_init() };
    flags.set_entropy_coding_mode_flag(1);
    flags.set_deblocking_filter_control_present_flag(1);
    //https://registry.khronos.org/vulkan/specs/1.3-extensions/html/vkspec.html#decode-h264-pps
    let pps = vec![vk::native::StdVideoH264PictureParameterSet {
        flags,
        seq_parameter_set_id: 0,
        pic_parameter_set_id: 0,
        num_ref_idx_l0_default_active_minus1: 0,
        num_ref_idx_l1_default_active_minus1: 0,
        weighted_bipred_idc: 0,
        pic_init_qp_minus26: 0,
        pic_init_qs_minus26: 0,
        chroma_qp_index_offset: 0,
        second_chroma_qp_index_offset: 0,
        pScalingLists: null(),
    }];
    let add_info = vk::VideoEncodeH264SessionParametersAddInfoEXT::default()
        .std_sp_ss(&sps)
        .std_pp_ss(&pps);
    let mut codec_info = vk::VideoEncodeH264SessionParametersCreateInfoEXT::default()
        .max_std_sps_count(sps.len() as u32)
        .max_std_pps_count(pps.len() as u32)
        .parameters_add_info(&add_info);

    if let Some(mut output_file) = output_file {
        write_h264_sps(&mut output_file, &sps[0]).map_err(|e| {
            error!("Error writing sps: {e}!");
            vk::Result::ERROR_INITIALIZATION_FAILED
        })?;
        write_h264_pps(&mut output_file, &sps[0], &pps[0]).map_err(|e| {
            error!("Error writing pps: {e}!");
            vk::Result::ERROR_INITIALIZATION_FAILED
        })?;
        output_file.flush().map_err(|e| {
            error!("Failed flushing output file: {e}!");
            vk::Result::ERROR_INITIALIZATION_FAILED
        })?;
    }
    unsafe {
        let mut info =
            vk::VideoSessionParametersCreateInfoKHR::default().video_session(video_session);
        info = info.push_next(&mut codec_info);
        let mut parameters = MaybeUninit::zeroed();
        let res = (video_queue_fn.create_video_session_parameters_khr)(
            device.handle(),
            &info,
            transmute(allocator),
            parameters.as_mut_ptr(),
        );
        if res != vk::Result::SUCCESS {
            error!("Failed to create H264 session parameters: {res}");
        }
        res.result_with_success(parameters.assume_init())
        //res.result_with_success(VideoSessionParameters {
        //parameters: parameters.assume_init(),
        //codec_parameters: CodecParameters::H264Parameters {
        //sps: sps[0],
        //pps: pps[0],
        //},
        //})
    }
}
