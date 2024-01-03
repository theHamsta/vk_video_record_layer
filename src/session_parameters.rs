use crate::bitstream::write_h264_pps;
use crate::bitstream::write_h264_sps;
use ash::prelude::VkResult;
use ash::vk;
use log::{error, info, warn};
use std::ffi::c_void;
use std::io::Write;
use std::mem::{transmute, MaybeUninit};
use std::ptr::{null, null_mut};

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
        max_num_ref_frames: 1,
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
    let add_info = vk::VideoEncodeH264SessionParametersAddInfoKHR::default()
        .std_sp_ss(&sps)
        .std_pp_ss(&pps);
    let mut codec_info = vk::VideoEncodeH264SessionParametersCreateInfoKHR::default()
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
        let mut h264_info = vk::VideoEncodeH264SessionParametersGetInfoKHR::default()
            .write_std_sps(true)
            .write_std_pps(true)
            .std_sps_id(0)
            .std_pps_id(0);
        let mut info = vk::VideoEncodeSessionParametersGetInfoKHR::default()
            .video_session_parameters(video_session_parameters);
        info = info.push_next(&mut h264_info);
        let mut h264_feedback = vk::VideoEncodeH264SessionParametersFeedbackInfoKHR::default();
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
            (feedback.p_next as *const vk::VideoEncodeH264SessionParametersFeedbackInfoKHR).as_ref()
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
    coded_extent: vk::Extent2D,
    output_file: Option<impl Write>,
    allocator: Option<&vk::AllocationCallbacks>,
) -> VkResult<vk::VideoSessionParametersKHR> {
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
        vui_num_units_in_tick: 0,
        chroma_sample_loc_type_top_field: 0,
        chroma_sample_loc_type_bottom_field: 0,
        reserved1: 0,
        pHrdParameters: null(),
        reserved2: Default::default(),
        def_disp_win_left_offset: 0,
        def_disp_win_right_offset: 0,
        def_disp_win_top_offset: 0,
        def_disp_win_bottom_offset: 0,
        vui_time_scale: 0,
        vui_num_ticks_poc_diff_one_minus1: 0,
        min_spatial_segmentation_idc: 0,
        reserved3: Default::default(),
        max_bytes_per_pic_denom: 0,
        max_bits_per_min_cu_denom: 0,
        log2_max_mv_length_horizontal: 0,
        log2_max_mv_length_vertical: 0,
    };
    assert_eq!(format, vk::Format::G8_B8R8_2PLANE_420_UNORM);

    let mut flags = unsafe {
        MaybeUninit::<vk::native::StdVideoH265ProfileTierLevelFlags>::zeroed().assume_init()
    };
    flags.set_general_progressive_source_flag(1);
    flags.set_general_frame_only_constraint_flag(1);

    let profile_tier_level = vk::native::StdVideoH265ProfileTierLevel {
        flags,
        general_profile_idc: vk::native::StdVideoH265ProfileIdc_STD_VIDEO_H265_PROFILE_IDC_MAIN,
        general_level_idc: vk::native::StdVideoH265LevelIdc_STD_VIDEO_H265_LEVEL_IDC_6_1,
    };

    let flags = MaybeUninit::zeroed();
    let mut flags: vk::native::StdVideoH265VpsFlags = unsafe { flags.assume_init() };
    flags.set_vps_temporal_id_nesting_flag(1);
    let mut dec_pic_buf_mgr = vk::native::StdVideoH265DecPicBufMgr {
        max_latency_increase_plus1: Default::default(),
        max_dec_pic_buffering_minus1: Default::default(),
        max_num_reorder_pics: Default::default(),
    };
    dec_pic_buf_mgr.max_dec_pic_buffering_minus1[0] = 1;
    let sub_layer_hdr_parameters = vk::native::StdVideoH265SubLayerHrdParameters {
        bit_rate_value_minus1: Default::default(),
        cpb_size_value_minus1: Default::default(),
        cpb_size_du_value_minus1: Default::default(),
        bit_rate_du_value_minus1: Default::default(),
        cbr_flag: 0,
    };
    let sub_layer_hdr_parameters_vcl = sub_layer_hdr_parameters;
    let hdr_parameters = vk::native::StdVideoH265HrdParameters {
        flags: vk::native::StdVideoH265HrdFlags {
            _bitfield_align_1: Default::default(),
            _bitfield_1: Default::default(),
        },
        tick_divisor_minus2: 0,
        du_cpb_removal_delay_increment_length_minus1: 0,
        dpb_output_delay_du_length_minus1: 0,
        bit_rate_scale: 0,
        cpb_size_scale: 0,
        cpb_size_du_scale: 0,
        initial_cpb_removal_delay_length_minus1: 0,
        au_cpb_removal_delay_length_minus1: 0,
        dpb_output_delay_length_minus1: 0,
        cpb_cnt_minus1: Default::default(),
        elemental_duration_in_tc_minus1: Default::default(),
        reserved: Default::default(),
        pSubLayerHrdParametersNal: &sub_layer_hdr_parameters,
        pSubLayerHrdParametersVcl: &sub_layer_hdr_parameters_vcl,
    };
    let vps = vec![vk::native::StdVideoH265VideoParameterSet {
        flags,
        vps_video_parameter_set_id: 0,
        vps_max_sub_layers_minus1: 0,
        reserved1: 0xFF,
        reserved2: 0xFF,
        vps_num_units_in_tick: 0,
        vps_time_scale: 0,
        vps_num_ticks_poc_diff_one_minus1: 4,
        reserved3: 0,
        pDecPicBufMgr: &dec_pic_buf_mgr,
        pHrdParameters: &hdr_parameters,
        pProfileTierLevel: &profile_tier_level,
    }];

    let flags: vk::native::StdVideoH265ShortTermRefPicSetFlags =
        unsafe { MaybeUninit::zeroed().assume_init() };
    let short_term_ref_pics_set = vk::native::StdVideoH265ShortTermRefPicSet {
        flags,
        delta_idx_minus1: 0,
        use_delta_flag: 0,
        abs_delta_rps_minus1: 0,
        used_by_curr_pic_flag: 0,
        used_by_curr_pic_s0_flag: 1,
        used_by_curr_pic_s1_flag: 0,
        reserved1: 0,
        reserved2: 0,
        reserved3: 0,
        num_negative_pics: 1,
        num_positive_pics: 0,
        delta_poc_s0_minus1: Default::default(),
        delta_poc_s1_minus1: Default::default(),
    };
    let long_term_ref_pics_sps = vk::native::StdVideoH265LongTermRefPicsSps {
        used_by_curr_pic_lt_sps_flag: 0,
        lt_ref_pic_poc_lsb_sps: Default::default(),
    };
    let mut flags: vk::native::StdVideoH265SpsFlags =
        unsafe { MaybeUninit::zeroed().assume_init() };
    let scaling_lists = vk::native::StdVideoH265ScalingLists {
        ScalingList4x4: [[0u8; 16]; 6],
        ScalingList8x8: [[0u8; 64]; 6],
        ScalingList16x16: [[0u8; 64]; 6],
        ScalingList32x32: [[0u8; 64]; 2],
        ScalingListDCCoef16x16: [0u8; 6],
        ScalingListDCCoef32x32: [0u8; 2],
    };
    flags.set_amp_enabled_flag(1);
    flags.set_sample_adaptive_offset_enabled_flag(1);
    let sps = vec![vk::native::StdVideoH265SequenceParameterSet {
        flags,
        chroma_format_idc:
            vk::native::StdVideoH265ChromaFormatIdc_STD_VIDEO_H265_CHROMA_FORMAT_IDC_420,
        pic_width_in_luma_samples: coded_extent.width,
        pic_height_in_luma_samples: coded_extent.height,
        sps_video_parameter_set_id: 0,
        sps_max_sub_layers_minus1: 0,
        sps_seq_parameter_set_id: 0,
        bit_depth_luma_minus8: 0,
        bit_depth_chroma_minus8: 0,
        log2_max_pic_order_cnt_lsb_minus4: 8 - 4, // pic order count 0-255
        log2_min_luma_coding_block_size_minus3: 1, // 16
        log2_diff_max_min_luma_coding_block_size: 1, // 32
        log2_min_luma_transform_block_size_minus2: 0,
        log2_diff_max_min_luma_transform_block_size: 3,
        max_transform_hierarchy_depth_inter: 3,
        max_transform_hierarchy_depth_intra: 3,
        num_short_term_ref_pic_sets: 1,
        num_long_term_ref_pics_sps: 0,
        pcm_sample_bit_depth_luma_minus1: 0,
        pcm_sample_bit_depth_chroma_minus1: 0,
        log2_min_pcm_luma_coding_block_size_minus3: 0,
        log2_diff_max_min_pcm_luma_coding_block_size: 0,
        reserved1: 0,
        reserved2: 0,
        palette_max_size: 0,
        delta_palette_max_predictor_size: 0,
        motion_vector_resolution_control_idc: 0,
        sps_num_palette_predictor_initializers_minus1: 0,
        conf_win_left_offset: 0,
        conf_win_right_offset: (32 - coded_extent.width % 32) / 2,
        conf_win_top_offset: 0,
        conf_win_bottom_offset: (32 - coded_extent.height % 32) / 2,
        pProfileTierLevel: &profile_tier_level,
        pDecPicBufMgr: &dec_pic_buf_mgr,
        pScalingLists: &scaling_lists,
        pShortTermRefPicSet: &short_term_ref_pics_set,
        pLongTermRefPicsSps: &long_term_ref_pics_sps,
        pSequenceParameterSetVui: null(),
        pPredictorPaletteEntries: null(),
    }];
    let flags = MaybeUninit::zeroed();
    let mut flags: vk::native::StdVideoH265PpsFlags = unsafe { flags.assume_init() };
    flags.set_transform_skip_enabled_flag(1);
    //flags.set_cu_qp_delta_enabled_flag(1);
    //flags.set_pps_curr_pic_ref_enabled_flag(1);
    flags.set_loop_filter_across_tiles_enabled_flag(1);
    flags.set_deblocking_filter_control_present_flag(1);
    flags.set_pps_scaling_list_data_present_flag(0);
    let pps = vec![vk::native::StdVideoH265PictureParameterSet {
        flags,
        pps_pic_parameter_set_id: 0,
        pps_seq_parameter_set_id: 0,
        sps_video_parameter_set_id: 0,
        num_extra_slice_header_bits: 0,
        num_ref_idx_l0_default_active_minus1: 0,
        num_ref_idx_l1_default_active_minus1: 0,
        init_qp_minus26: 0,
        diff_cu_qp_delta_depth: 0,
        pps_cb_qp_offset: 0,
        pps_cr_qp_offset: 0,
        pps_beta_offset_div2: 0,
        pps_tc_offset_div2: 0,
        log2_parallel_merge_level_minus2: 0,
        log2_max_transform_skip_block_size_minus2: 0,
        diff_cu_chroma_qp_offset_depth: 0,
        chroma_qp_offset_list_len_minus1: 0,
        cb_qp_offset_list: Default::default(),
        cr_qp_offset_list: Default::default(),
        log2_sao_offset_scale_luma: 0,
        log2_sao_offset_scale_chroma: 0,
        pps_act_y_qp_offset_plus5: 0,
        pps_act_cb_qp_offset_plus5: 0,
        pps_act_cr_qp_offset_plus3: 0,
        pps_num_palette_predictor_initializers: 0,
        luma_bit_depth_entry_minus8: 248,
        chroma_bit_depth_entry_minus8: 248,
        num_tile_columns_minus1: 0,
        num_tile_rows_minus1: 0,
        reserved1: 0,
        reserved2: 0,
        column_width_minus1: Default::default(),
        row_height_minus1: Default::default(),
        reserved3: 0,
        pScalingLists: null(),
        pPredictorPaletteEntries: null(),
    }];
    let add_info = vk::VideoEncodeH265SessionParametersAddInfoKHR::default()
        .std_vp_ss(&vps)
        .std_sp_ss(&sps)
        .std_pp_ss(&pps);
    let mut codec_info = vk::VideoEncodeH265SessionParametersCreateInfoKHR::default()
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
        let mut h265_info = vk::VideoEncodeH265SessionParametersGetInfoKHR::default()
            .write_std_vps(true)
            .write_std_sps(true)
            .write_std_pps(true)
            .std_vps_id(0)
            .std_sps_id(0)
            .std_pps_id(0);
        let mut info = vk::VideoEncodeSessionParametersGetInfoKHR::default()
            .video_session_parameters(video_session_parameters);
        info = info.push_next(&mut h265_info);
        let mut h265_feedback = vk::VideoEncodeH265SessionParametersFeedbackInfoKHR::default();
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
        let h265_feedback = unsafe {
            (feedback.p_next as *const vk::VideoEncodeH265SessionParametersFeedbackInfoKHR).as_ref()
        };
        if res == vk::Result::SUCCESS {
            info!("Received driver feedback: {size} bytes, {feedback:?} {h265_feedback:?}");
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
            //unsafe {
            //(video_queue_fn.destroy_video_session_parameters_khr)(
            //device.handle(),
            //video_session_parameters,
            //allocator
            //.map(|e| e as *const vk::AllocationCallbacks)
            //.unwrap_or(null()),
            //)
            //};
            error!("Failed to retrieve encode video session parameters: {res}.");
            //return Err(vk::Result::ERROR_INITIALIZATION_FAILED);
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
