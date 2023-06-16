use ash::vk;
use bitstream_io::{BigEndian, BitWrite, BitWriter};
use std::io::Write;

const START_CODE: u32 = 0x00_00_00_01;
const CONSTRAINED_SET_FLAGS3_RESERVED_ZERO_BITS5: u32 = 0x00000000;
const FORBIDDEN_ZERO_BIT: u32 = 0;
const NAL_REF_IDC_SPS: u32 = 3;
const NAL_REF_IDC_PPS: u32 = 3;
const NAL_NAL_UNIT_TYPE_SPS: u32 = 7;
const NAL_NAL_UNIT_TYPE_PPS: u32 = 8;
const RBSP_STOP_ONE_BIT: u32 = 1;

pub fn write_h264_sps(
    writer: &mut impl Write,
    sps: &vk::native::StdVideoH264SequenceParameterSet,
) -> std::io::Result<()> {
    let mut writer = BitWriter::<_, BigEndian>::new(writer);
    dbg!(sps);

    u(32, &mut writer, START_CODE)?;
    u(1, &mut writer, FORBIDDEN_ZERO_BIT)?;
    u(2, &mut writer, NAL_REF_IDC_SPS)?;
    u(5, &mut writer, NAL_NAL_UNIT_TYPE_SPS)?;

    u(8, &mut writer, sps.profile_idc)?;
    u(8, &mut writer, CONSTRAINED_SET_FLAGS3_RESERVED_ZERO_BITS5)?;
    u(
        8,
        &mut writer,
        match sps.level_idc {
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_1_0 => 10,
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_1_1 => 11,
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_1_2 => 12,
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_1_3 => 13,
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_2_0 => 20,
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_2_1 => 21,
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_2_2 => 22,
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_3_0 => 30,
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_3_1 => 31,
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_3_2 => 32,
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_4_0 => 40,
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_4_1 => 41,
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_4_2 => 42,
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_5_0 => 50,
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_5_1 => 51,
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_5_2 => 52,
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_6_0 => 60,
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_6_1 => 61,
            vk::native::StdVideoH264LevelIdc_STD_VIDEO_H264_LEVEL_IDC_6_2 => 62,
            _ => unreachable!(),
        },
    )?;
    ue(&mut writer, sps.seq_parameter_set_id.into())?;
    ue(&mut writer, dbg!(sps.log2_max_frame_num_minus4.into()))?;
    ue(&mut writer, sps.pic_order_cnt_type.into())?;
    if dbg!(sps.pic_order_cnt_type) == 0 {
        ue(
            &mut writer,
            dbg!(sps.log2_max_pic_order_cnt_lsb_minus4).into(),
        )?;
    } else {
        todo!();
    }
    ue(&mut writer, sps.max_num_ref_frames.into())?;
    u(
        1,
        &mut writer,
        sps.flags.gaps_in_frame_num_value_allowed_flag(),
    )?;
    ue(&mut writer, dbg!(sps.pic_width_in_mbs_minus1.into()))?;
    ue(&mut writer, dbg!(sps.pic_height_in_map_units_minus1.into()))?;
    u(1, &mut writer, sps.flags.frame_mbs_only_flag())?;
    if sps.flags.frame_mbs_only_flag() != 1 {
        writer.write(1, sps.flags.mb_adaptive_frame_field_flag())?;
    }
    u(1, &mut writer, sps.flags.direct_8x8_inference_flag())?;
    u(1, &mut writer, sps.flags.frame_cropping_flag())?;
    if sps.flags.frame_cropping_flag() == 1 {
        ue(&mut writer, sps.frame_crop_left_offset.into())?;
        ue(&mut writer, sps.frame_crop_right_offset.into())?;
        ue(&mut writer, sps.frame_crop_top_offset.into())?;
        ue(&mut writer, sps.frame_crop_bottom_offset.into())?;
    }

    u(1, &mut writer, sps.flags.vui_parameters_present_flag())?;
    if sps.flags.vui_parameters_present_flag() == 1 {
        todo!();
    }
    u(1, &mut writer, RBSP_STOP_ONE_BIT)?;
    writer.byte_align()?;

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

    u(1, &mut writer, RBSP_STOP_ONE_BIT)?;
    writer.byte_align()?;

    Ok(())
}

#[allow(dead_code)]
fn se<W: std::io::Write, E: bitstream_io::Endianness>(
    writer: &mut BitWriter<W, E>,
    data: i64,
) -> std::io::Result<()> {
    let mut k = data.abs() as u64 * 2;
    if data > 0 {
        k -= 1;
    }
    ue(writer, k)
}

fn ue<W: std::io::Write, E: bitstream_io::Endianness>(
    writer: &mut BitWriter<W, E>,
    data: u64,
) -> std::io::Result<()> {
    let data_plus_1 = data.wrapping_add(1);

    let lz = data_plus_1.leading_zeros();

    let num_zeros = 64 - lz - 1;
    writer.write(num_zeros, 0)?;
    writer.write(num_zeros + 1, data_plus_1)?;
    Ok(())
}

fn u<W: std::io::Write, E: bitstream_io::Endianness>(
    bits: u32,
    writer: &mut BitWriter<W, E>,
    data: u32,
) -> std::io::Result<()> {
    writer.write(bits, data)
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn ue_test() {
        for (input, expected) in [
            (0, [0b10000000u8; 1]),
            (1, [0b01000000u8; 1]),
            (2, [0b01100000u8; 1]),
            (3, [0b00100000u8; 1]),
            (4, [0b00101000u8; 1]),
            (5, [0b00110000u8; 1]),
        ] {
            let mut buffer = Vec::new();
            let mut writer = BitWriter::new(&mut buffer);
            ue::<_, BigEndian>(&mut writer, input).unwrap();
            writer.byte_align().unwrap();

            println!("{input}: {:b}, {:b}", buffer[0], expected[0]);
            assert_eq!(buffer.as_slice(), expected);
        }
    }

    #[test]
    fn ve_test() {
        for (input, expected) in [
            (0, [0b10000000u8; 1]),
            (1, [0b01000000u8; 1]),
            (-1, [0b01100000u8; 1]),
            (2, [0b00100000u8; 1]),
            (-2, [0b00101000u8; 1]),
            (3, [0b00110000u8; 1]),
        ] {
            let mut buffer = Vec::new();
            let mut writer = BitWriter::new(&mut buffer);
            se::<_, BigEndian>(&mut writer, input).unwrap();
            writer.byte_align().unwrap();

            println!("{input}: {:b}, {:b}", buffer[0], expected[0]);
            assert_eq!(buffer.as_slice(), expected);
        }
    }

    #[test]
    fn start_sps_test() {
        let mut buffer = Vec::new();
        let mut writer = BitWriter::<_, BigEndian>::new(&mut buffer);
        u(32, &mut writer, START_CODE).unwrap();
        u(1, &mut writer, FORBIDDEN_ZERO_BIT).unwrap();
        u(2, &mut writer, NAL_REF_IDC_SPS).unwrap();
        u(5, &mut writer, NAL_NAL_UNIT_TYPE_SPS).unwrap();
        writer.byte_align().unwrap();

        assert_eq!(buffer.as_slice(), [0x00, 0x00, 0x00, 0x01, 0x67]);
    }
}
