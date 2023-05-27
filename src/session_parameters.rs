use std::mem::{transmute, MaybeUninit};
use std::ptr::null;

use ash::prelude::VkResult;
use ash::vk;
use log::error;

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
    format: vk::Format,
    extent: vk::Extent2D,
    allocator: Option<&vk::AllocationCallbacks>,
) -> VkResult<vk::VideoSessionParametersKHR> {
    let bitdepth = 8;
    //let flags = MaybeUninit::zeroed();
    //let vui = vk::native::StdVideoH264SequenceParameterSetVui {
    //flags,
    //aspect_ratio_idc: todo!(),
    //sar_width: todo!(),
    //sar_height: todo!(),
    //video_format: todo!(),
    //colour_primaries: todo!(),
    //transfer_characteristics: todo!(),
    //matrix_coefficients: todo!(),
    //num_units_in_tick: todo!(),
    //time_scale: todo!(),
    //max_num_reorder_frames: todo!(),
    //max_dec_frame_buffering: todo!(),
    //chroma_sample_loc_type_top_field: todo!(),
    //chroma_sample_loc_type_bottom_field: todo!(),
    //reserved1: 0,
    //pHrdParameters: null(),
    //};
    assert_eq!(format, vk::Format::G8_B8R8_2PLANE_420_UNORM);

    let flags = MaybeUninit::zeroed();
    let flags = unsafe { flags.assume_init() };
    let mut sps = vec![vk::native::StdVideoH264SequenceParameterSet {
        flags,
        profile_idc: vk::native::StdVideoH264ProfileIdc_STD_VIDEO_H264_PROFILE_IDC_MAIN,
        level_idc: vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_5_2,
        chroma_format_idc:
            vk::native::StdVideoH264ChromaFormatIdc_STD_VIDEO_H264_CHROMA_FORMAT_IDC_420,
        seq_parameter_set_id: 0,
        bit_depth_luma_minus8: bitdepth - 8,
        bit_depth_chroma_minus8: bitdepth - 8,
        log2_max_frame_num_minus4: 255,
        pic_order_cnt_type: 0,
        offset_for_non_ref_pic: 0,
        offset_for_top_to_bottom_field: 0,
        log2_max_pic_order_cnt_lsb_minus4: 4, // pic order count 0-255
        num_ref_frames_in_pic_order_cnt_cycle: 0,
        max_num_ref_frames: 0,
        reserved1: 0,
        pic_width_in_mbs_minus1: (extent.width + 15) / 16 - 1, //extent.width.div_ceil(16) - 1, // with unstable feature int_roundings
        pic_height_in_map_units_minus1: (extent.height + 15) / 16 - 1,
        frame_crop_left_offset: 0,
        frame_crop_right_offset: extent.width % 16,
        frame_crop_top_offset: 0,
        frame_crop_bottom_offset: extent.height % 16,
        reserved2: 0,
        pOffsetForRefFrame: null(),
        pScalingLists: null(),
        pSequenceParameterSetVui: null(), //&vui, will break CodecParameters when needing
        //self-referential structs
    }];
    if sps[0].frame_crop_right_offset != 0 || sps[0].frame_crop_bottom_offset != 0 {
        sps[0].flags.set_frame_cropping_flag(1);
    }
    let flags = MaybeUninit::zeroed();
    let mut flags: vk::native::StdVideoH264PpsFlags = unsafe { flags.assume_init() };
    flags.set_transform_8x8_mode_flag(1);
    flags.set_entropy_coding_mode_flag(1);
    flags.set_deblocking_filter_control_present_flag(1);
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
    let mut add_info = vk::VideoEncodeH264SessionParametersAddInfoEXT::default()
        .std_sp_ss(&sps)
        .std_pp_ss(&pps);
    let mut codec_info = vk::VideoEncodeH264SessionParametersCreateInfoEXT::default()
        .max_std_sps_count(sps.len() as u32)
        .max_std_pps_count(pps.len() as u32);
    unsafe {
        codec_info.p_next = transmute(&mut add_info);
        let info = vk::VideoSessionParametersCreateInfoKHR::default();
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
