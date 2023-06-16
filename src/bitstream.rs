use ash::vk;
use bitstream_io::{BigEndian, BitWrite, BitWriter};
use std::io::Write;

const START_CODE: u32 = 0x00000001;
const CONSTRAINED_SET_FLAGS3: u32 = 0x00000000;
const FORBIDDEN_ZERO_BIT: u32 = 0;
const NAL_REF_IDC_SPS: u32 = 3;
const NAL_REF_IDC_PPS: u32 = 3;
const NAL_NAL_UNIT_TYPE_SPS: u32 = 7;
const NAL_NAL_UNIT_TYPE_PPS: u32 = 8;

pub fn write_h264_sps(
    writer: &mut impl Write,
    sps: &vk::native::StdVideoH264SequenceParameterSet,
) -> std::io::Result<()> {
    let mut writer = BitWriter::<_, BigEndian>::new(writer);

    u(32, &mut writer, START_CODE)?;
    u(1, &mut writer, FORBIDDEN_ZERO_BIT)?;
    u(2, &mut writer, NAL_REF_IDC_SPS)?;
    u(5, &mut writer, NAL_NAL_UNIT_TYPE_SPS)?;

    u(8, &mut writer, sps.profile_idc)?;
    u(8, &mut writer, CONSTRAINED_SET_FLAGS3)?;
    u(8, &mut writer, sps.level_idc)?;
    ue(&mut writer, sps.seq_parameter_set_id.into())?;
    ue(&mut writer, sps.log2_max_frame_num_minus4.into())?;
    ue(&mut writer, sps.pic_order_cnt_type.into())?;
    if sps.pic_order_cnt_type == 0 {
        ue(&mut writer, sps.log2_max_pic_order_cnt_lsb_minus4.into())?;
    } else {
        todo!();
    }
    ue(&mut writer, sps.max_num_ref_frames.into())?;
    u(
        1,
        &mut writer,
        sps.flags.gaps_in_frame_num_value_allowed_flag(),
    )?;
    ue(&mut writer, sps.pic_width_in_mbs_minus1)?;
    ue(&mut writer, sps.pic_height_in_map_units_minus1)?;
    u(1, &mut writer, sps.flags.frame_mbs_only_flag())?;
    if sps.flags.frame_mbs_only_flag() != 1 {
        writer.write(1, sps.flags.mb_adaptive_frame_field_flag())?;
    }
    u(1, &mut writer, sps.flags.direct_8x8_inference_flag())?;
    u(1, &mut writer, sps.flags.frame_cropping_flag())?;
    if sps.flags.frame_cropping_flag() == 1 {
        ue(&mut writer, sps.frame_crop_left_offset)?;
        ue(&mut writer, sps.frame_crop_right_offset)?;
        ue(&mut writer, sps.frame_crop_top_offset)?;
        ue(&mut writer, sps.frame_crop_bottom_offset)?;
    }
    ue(&mut writer, sps.frame_crop_left_offset)?;

    u(1, &mut writer, sps.flags.vui_parameters_present_flag())?;
    if sps.flags.vui_parameters_present_flag() == 1 {
        todo!();
    }
    Ok(())
}

pub fn write_h264_pps(
    writer: &mut impl Write,
    sps: &vk::native::StdVideoH264SequenceParameterSet,
    pps: &vk::native::StdVideoH264PictureParameterSet,
) -> std::io::Result<()> {
    let mut writer = BitWriter::<_, BigEndian>::new(writer);

    u(32, &mut writer, START_CODE)?;
    u(1, &mut writer, FORBIDDEN_ZERO_BIT)?;
    u(2, &mut writer, NAL_REF_IDC_PPS)?;
    u(5, &mut writer, NAL_NAL_UNIT_TYPE_PPS)?;

    ue(&mut writer, pps.pic_parameter_set_id.into())?;
    ue(&mut writer, sps.seq_parameter_set_id.into())?;
    u(1, &mut writer, pps.flags.entropy_coding_mode_flag())?;
    u(
        1,
        &mut writer,
        0, /* pic_order_present_flag, expressed via pps.pic_order_cnt_type??*/
    )?;
    ue(&mut writer, 0)?; /*num_slice_groups_minus1*/
    ue(&mut writer, pps.num_ref_idx_l0_default_active_minus1.into())?;
    ue(&mut writer, pps.num_ref_idx_l1_default_active_minus1.into())?;
    u(1, &mut writer, pps.flags.weighted_pred_flag())?;
    u(2, &mut writer, pps.weighted_bipred_idc)?;
    se(&mut writer, pps.pic_init_qp_minus26.into())?;
    se(&mut writer, pps.pic_init_qs_minus26.into())?;
    se(&mut writer, pps.chroma_qp_index_offset.into())?;
    u(
        1,
        &mut writer,
        pps.flags.deblocking_filter_control_present_flag(),
    )?;
    u(1, &mut writer, pps.flags.constrained_intra_pred_flag())?;
    u(1, &mut writer, pps.flags.redundant_pic_cnt_present_flag())?;

    Ok(())
}

#[allow(dead_code)]
fn se<W: std::io::Write, E: bitstream_io::Endianness>(
    writer: &mut BitWriter<W, E>,
    data: i32,
) -> std::io::Result<()> {
    let mut k = data.abs() as u32 * 2;
    if data > 0 {
        k -= 1;
    }
    ue(writer, k)
}

fn ue<W: std::io::Write, E: bitstream_io::Endianness>(
    writer: &mut BitWriter<W, E>,
    data: u32,
) -> std::io::Result<()> {
    let xp1 = data.wrapping_add(1);

    let lz = xp1.leading_zeros();

    let num_zeros = 32 - lz - 1;
    writer.write(0, num_zeros)?;
    writer.write(xp1, num_zeros + 1)?;
    Ok(())
}

fn u<W: std::io::Write, E: bitstream_io::Endianness>(
    bits: u32,
    writer: &mut BitWriter<W, E>,
    data: u32,
) -> std::io::Result<()> {
    writer.write(bits, data)
}
