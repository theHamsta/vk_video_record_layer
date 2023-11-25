use crate::bitstream::{write_h264_pps, write_h264_sps};
use ash::prelude::VkResult;
use ash::vk;
use log::{error, info, warn};
use std::ffi::c_void;
use std::io::Write;
use std::mem::{transmute, MaybeUninit};
use std::ptr::{null, null_mut};

// TODO: handle vui with valid pointers
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
    encode_queue_fn: &vk::KhrVideoEncodeQueueFn,
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

    let video_session_parameters = unsafe {
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
    };
    if let (Some(mut output_file), Ok(video_session_parameters)) =
        (output_file, video_session_parameters)
    {
        let mut h264_info = vk::VideoEncodeH264SessionParametersGetInfoEXT::default()
            .write_std_sps(true)
            .write_std_pps(true)
            .std_sps_id(0)
            .std_pps_id(0);
        let mut info = vk::VideoEncodeSessionParametersGetInfoKHR::default()
            .video_session_parameters(video_session_parameters);
        info = info.push_next(&mut h264_info);
        let mut h264_feedback = vk::VideoEncodeH264SessionParametersFeedbackInfoEXT::default();
        let feedback = vk::VideoEncodeSessionParametersFeedbackInfoKHR::default();
        let mut feedback = feedback.push_next(&mut h264_feedback);
        let mut size = 0usize;
        let mut data = Vec::new();
        let mut res = unsafe {
            (encode_queue_fn.get_encoded_video_session_parameters_khr)(
                device.handle(),
                &info,
                &mut feedback,
                &mut size,
                null_mut(),
            )
        };
        if res == vk::Result::SUCCESS {
            info!("Resizing array for feedback: {size} bytes");
            data.resize(size, 0);
            res = unsafe {
                (encode_queue_fn.get_encoded_video_session_parameters_khr)(
                    device.handle(),
                    &info,
                    &mut feedback,
                    &mut size,
                    data.as_mut_ptr() as *mut c_void,
                )
            };
        }
        let h264_feedback = unsafe {
            (feedback.p_next as *const vk::VideoEncodeSessionParametersFeedbackInfoKHR).as_ref()
        };
        if res == vk::Result::SUCCESS {
            info!("Received driver feedback: {size} bytes, {feedback:?} {h264_feedback:?}");
            output_file.write(&data).map_err(|e| {
                error!("Failed to write to file: {e}");
                unsafe {
                    (video_queue_fn.destroy_video_session_parameters_khr)(
                        device.handle(),
                        video_session_parameters,
                        allocator
                            .map(|e| e as *const vk::AllocationCallbacks)
                            .unwrap_or(null()),
                    )
                };
                vk::Result::ERROR_INITIALIZATION_FAILED
            })?;
        } else {
            warn!("Failed to retrieve encode video session parameters: {res}. Falling back to own bitstream writer logic. Might not use driver applied overwrites");
            // Own logic to write sps/pps
            write_h264_sps(&mut output_file, &sps[0]).map_err(|e| {
                error!("Error writing sps: {e}!");
                vk::Result::ERROR_INITIALIZATION_FAILED
            })?;
            write_h264_pps(&mut output_file, &sps[0], &pps[0]).map_err(|e| {
                error!("Error writing pps: {e}!");
                vk::Result::ERROR_INITIALIZATION_FAILED
            })?;
        }
        output_file.flush().map_err(|e| {
            error!("Failed flushing output file: {e}!");
            vk::Result::ERROR_INITIALIZATION_FAILED
        })?;
    }

    video_session_parameters
}

pub fn make_h265_video_session_parameters(
    device: &ash::Device,
    video_queue_fn: &vk::KhrVideoQueueFn,
    encode_queue_fn: &vk::KhrVideoEncodeQueueFn,
    video_session: vk::VideoSessionKHR,
    format: vk::Format,
    extent: vk::Extent2D,
    output_file: Option<impl Write>,
    allocator: Option<&vk::AllocationCallbacks>,
) -> VkResult<vk::VideoSessionParametersKHR> {
    let bitdepth = 8;
    let flags = unsafe { MaybeUninit::zeroed().assume_init() };
    let _vui = vk::native::StdVideoH265SequenceParameterSetVui {
        flags,
        aspect_ratio_idc:
            ash::vk::native::StdVideoH265AspectRatioIdc_STD_VIDEO_H265_ASPECT_RATIO_IDC_SQUARE,
        sar_width: 0,
        sar_height: 0,
        video_format: 0,
        colour_primaries: 0,
        transfer_characteristics: 0,
        matrix_coeffs: 0,
        vui_num_units_in_tick: 1000,
        chroma_sample_loc_type_top_field: 0,
        chroma_sample_loc_type_bottom_field: 0,
        reserved1: 0,
        pHrdParameters: null(),
        reserved2: Default::default(),
        def_disp_win_left_offset: 0,
        def_disp_win_right_offset: 0,
        def_disp_win_top_offset: 0,
        def_disp_win_bottom_offset: 0,
        vui_time_scale: 1000,
        vui_num_ticks_poc_diff_one_minus1: 0,
        min_spatial_segmentation_idc: 0,
        reserved3: Default::default(),
        max_bytes_per_pic_denom: 0,
        max_bits_per_min_cu_denom: 0,
        log2_max_mv_length_horizontal: 0,
        log2_max_mv_length_vertical: 0,
    };
    assert_eq!(format, vk::Format::G8_B8R8_2PLANE_420_UNORM);

    let flags = MaybeUninit::zeroed();
    let mut flags: vk::native::StdVideoH265VpsFlags = unsafe { flags.assume_init() };
    // Use whatever ffmpeg uses for h264 nvenc
    // TODO:
    //https://registry.khronos.org/vulkan/specs/1.3-extensions/html/vkspec.html#decode-h264-sps
    let mut vps = vec![vk::native::StdVideoH265VideoParameterSet {
        flags,
        vps_video_parameter_set_id: 0,
        vps_max_sub_layers_minus1: todo!(),
        reserved1: todo!(),
        reserved2: todo!(),
        vps_num_units_in_tick: todo!(),
        vps_time_scale: todo!(),
        vps_num_ticks_poc_diff_one_minus1: todo!(),
        reserved3: todo!(),
        pDecPicBufMgr: null(),
        pHrdParameters: null(),
        pProfileTierLevel: null(),
    }];

    let flags = MaybeUninit::zeroed();
    let mut flags: vk::native::StdVideoH265SpsFlags = unsafe { flags.assume_init() };
    // Use whatever ffmpeg uses for h264 nvenc
    //https://registry.khronos.org/vulkan/specs/1.3-extensions/html/vkspec.html#decode-h264-sps
    let mut sps = vec![vk::native::StdVideoH265SequenceParameterSet {
        flags,
        chroma_format_idc: todo!(),
        pic_width_in_luma_samples: todo!(),
        pic_height_in_luma_samples: todo!(),
        sps_video_parameter_set_id: todo!(),
        sps_max_sub_layers_minus1: todo!(),
        sps_seq_parameter_set_id: todo!(),
        bit_depth_luma_minus8: todo!(),
        bit_depth_chroma_minus8: todo!(),
        log2_max_pic_order_cnt_lsb_minus4: todo!(),
        log2_min_luma_coding_block_size_minus3: todo!(),
        log2_diff_max_min_luma_coding_block_size: todo!(),
        log2_min_luma_transform_block_size_minus2: todo!(),
        log2_diff_max_min_luma_transform_block_size: todo!(),
        max_transform_hierarchy_depth_inter: todo!(),
        max_transform_hierarchy_depth_intra: todo!(),
        num_short_term_ref_pic_sets: todo!(),
        num_long_term_ref_pics_sps: todo!(),
        pcm_sample_bit_depth_luma_minus1: todo!(),
        pcm_sample_bit_depth_chroma_minus1: todo!(),
        log2_min_pcm_luma_coding_block_size_minus3: todo!(),
        log2_diff_max_min_pcm_luma_coding_block_size: todo!(),
        reserved1: todo!(),
        reserved2: todo!(),
        palette_max_size: todo!(),
        delta_palette_max_predictor_size: todo!(),
        motion_vector_resolution_control_idc: todo!(),
        sps_num_palette_predictor_initializers_minus1: todo!(),
        conf_win_left_offset: todo!(),
        conf_win_right_offset: todo!(),
        conf_win_top_offset: todo!(),
        conf_win_bottom_offset: todo!(),
        pProfileTierLevel: todo!(),
        pDecPicBufMgr: todo!(),
        pScalingLists: todo!(),
        pShortTermRefPicSet: todo!(),
        pLongTermRefPicsSps: todo!(),
        pSequenceParameterSetVui: todo!(),
        pPredictorPaletteEntries: todo!(),
    }];
    //if sps[0].frame_crop_right_offset != 0 || sps[0].frame_crop_bottom_offset != 0 {
    //sps[0].flags.set_frame_cropping_flag(1);
    //}
    let flags = MaybeUninit::zeroed();
    let mut flags: vk::native::StdVideoH265PpsFlags = unsafe { flags.assume_init() };
    //https://registry.khronos.org/vulkan/specs/1.3-extensions/html/vkspec.html#decode-h264-pps
    let pps = vec![vk::native::StdVideoH265PictureParameterSet {
        flags,
        pps_pic_parameter_set_id: todo!(),
        pps_seq_parameter_set_id: todo!(),
        sps_video_parameter_set_id: todo!(),
        num_extra_slice_header_bits: todo!(),
        num_ref_idx_l0_default_active_minus1: todo!(),
        num_ref_idx_l1_default_active_minus1: todo!(),
        init_qp_minus26: todo!(),
        diff_cu_qp_delta_depth: todo!(),
        pps_cb_qp_offset: todo!(),
        pps_cr_qp_offset: todo!(),
        pps_beta_offset_div2: todo!(),
        pps_tc_offset_div2: todo!(),
        log2_parallel_merge_level_minus2: todo!(),
        log2_max_transform_skip_block_size_minus2: todo!(),
        diff_cu_chroma_qp_offset_depth: todo!(),
        chroma_qp_offset_list_len_minus1: todo!(),
        cb_qp_offset_list: todo!(),
        cr_qp_offset_list: todo!(),
        log2_sao_offset_scale_luma: todo!(),
        log2_sao_offset_scale_chroma: todo!(),
        pps_act_y_qp_offset_plus5: todo!(),
        pps_act_cb_qp_offset_plus5: todo!(),
        pps_act_cr_qp_offset_plus3: todo!(),
        pps_num_palette_predictor_initializers: todo!(),
        luma_bit_depth_entry_minus8: todo!(),
        chroma_bit_depth_entry_minus8: todo!(),
        num_tile_columns_minus1: todo!(),
        num_tile_rows_minus1: todo!(),
        reserved1: todo!(),
        reserved2: todo!(),
        column_width_minus1: todo!(),
        row_height_minus1: todo!(),
        reserved3: todo!(),
        pScalingLists: todo!(),
        pPredictorPaletteEntries: todo!(),
    }];
    let add_info = vk::VideoEncodeH265SessionParametersAddInfoEXT::default()
        .std_vp_ss(&vps)
        .std_sp_ss(&sps)
        .std_pp_ss(&pps);
    let mut codec_info = vk::VideoEncodeH265SessionParametersCreateInfoEXT::default()
        .max_std_vps_count(vps.len() as u32)
        .max_std_sps_count(sps.len() as u32)
        .max_std_pps_count(pps.len() as u32)
        .parameters_add_info(&add_info);

    let video_session_parameters = unsafe {
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
            error!("Failed to create H265 session parameters: {res}");
        }
        res.result_with_success(parameters.assume_init())
    };
    if let (Some(mut output_file), Ok(video_session_parameters)) =
        (output_file, video_session_parameters)
    {
        let mut h265_info = vk::VideoEncodeH265SessionParametersGetInfoEXT::default()
            .write_std_vps(true)
            .write_std_sps(true)
            .write_std_pps(true)
            .std_vps_id(0)
            .std_sps_id(0)
            .std_pps_id(0);
        let mut info = vk::VideoEncodeSessionParametersGetInfoKHR::default()
            .video_session_parameters(video_session_parameters);
        info = info.push_next(&mut h265_info);
        let mut h265_feedback = vk::VideoEncodeH264SessionParametersFeedbackInfoEXT::default();
        let feedback = vk::VideoEncodeSessionParametersFeedbackInfoKHR::default();
        let mut feedback = feedback.push_next(&mut h265_feedback);
        let mut size = 0usize;
        let mut data = Vec::new();
        let mut res = unsafe {
            (encode_queue_fn.get_encoded_video_session_parameters_khr)(
                device.handle(),
                &info,
                &mut feedback,
                &mut size,
                null_mut(),
            )
        };
        if res == vk::Result::SUCCESS {
            info!("Resizing array for feedback: {size} bytes");
            data.resize(size, 0);
            res = unsafe {
                (encode_queue_fn.get_encoded_video_session_parameters_khr)(
                    device.handle(),
                    &info,
                    &mut feedback,
                    &mut size,
                    data.as_mut_ptr() as *mut c_void,
                )
            };
        }
        let h264_feedback = unsafe {
            (feedback.p_next as *const vk::VideoEncodeSessionParametersFeedbackInfoKHR).as_ref()
        };
        if res == vk::Result::SUCCESS {
            info!("Received driver feedback: {size} bytes, {feedback:?} {h264_feedback:?}");
            output_file.write(&data).map_err(|e| {
                error!("Failed to write to file: {e}");
                unsafe {
                    (video_queue_fn.destroy_video_session_parameters_khr)(
                        device.handle(),
                        video_session_parameters,
                        allocator
                            .map(|e| e as *const vk::AllocationCallbacks)
                            .unwrap_or(null()),
                    )
                };
                vk::Result::ERROR_INITIALIZATION_FAILED
            })?;
        } else {
            unsafe {
                (video_queue_fn.destroy_video_session_parameters_khr)(
                    device.handle(),
                    video_session_parameters,
                    allocator
                        .map(|e| e as *const vk::AllocationCallbacks)
                        .unwrap_or(null()),
                )
            };
            warn!("Failed to retrieve encode video session parameters: {res}. Falling back to own bitstream writer logic. Might not use driver applied overwrites");
            return Err(vk::Result::ERROR_INITIALIZATION_FAILED);
        }
        output_file.flush().map_err(|e| {
            error!("Failed flushing output file: {e}!");
            unsafe {
                (video_queue_fn.destroy_video_session_parameters_khr)(
                    device.handle(),
                    video_session_parameters,
                    allocator
                        .map(|e| e as *const vk::AllocationCallbacks)
                        .unwrap_or(null()),
                )
            };
            vk::Result::ERROR_INITIALIZATION_FAILED
        })?;
    }

    video_session_parameters
}
