use ash::vk;
use bitstream_io::{BigEndian, BitWrite, BitWriter};
use std::io::Write;

pub trait SerializeH264BitstreamUnit {
    fn serialize_bitstream_unit(
        &mut self,
        sps: vk::native::StdVideoH264SequenceParameterSet,
        pps: vk::native::StdVideoH264PictureParameterSet,
        writer: &mut impl Write,
    ) -> std::io::Result<()>;
}

impl SerializeH264BitstreamUnit for vk::native::StdVideoH264PictureParameterSet {
    fn serialize_bitstream_unit(
        &mut self,
        _sps: vk::native::StdVideoH264SequenceParameterSet,
        _pps: vk::native::StdVideoH264PictureParameterSet,
        _writer: &mut impl Write,
    ) -> std::io::Result<()> {
        todo!()
    }
}

impl SerializeH264BitstreamUnit for vk::native::StdVideoH264SequenceParameterSet {
    fn serialize_bitstream_unit(
        &mut self,
        _sps: vk::native::StdVideoH264SequenceParameterSet,
        _pps: vk::native::StdVideoH264PictureParameterSet,
        _writer: &mut impl Write,
    ) -> std::io::Result<()> {
        todo!()
    }
}

impl SerializeH264BitstreamUnit for vk::VideoEncodeH264VclFrameInfoEXT<'_> {
    fn serialize_bitstream_unit(
        &mut self,
        sps: vk::native::StdVideoH264SequenceParameterSet,
        _pps: vk::native::StdVideoH264PictureParameterSet,
        writer: &mut impl Write,
    ) -> std::io::Result<()> {
        let mut writer = BitWriter::<_, BigEndian>::new(writer);
        for i in 0..self.nalu_slice_entry_count {
            let nalu = unsafe {
                self.p_nalu_slice_entries
                    .offset(i as isize)
                    .as_ref()
                    .unwrap()
            };
            let pic = unsafe { self.p_std_picture_info.offset(i as isize).as_ref().unwrap() };
            let header = unsafe { nalu.p_std_slice_header.as_ref().unwrap() };
            ue(header.first_mb_in_slice.into(), &mut writer)?;
            ue(header.slice_type, &mut writer)?;
            ue(pic.pic_parameter_set_id.into(), &mut writer)?;
            writer.write(
                sps.log2_max_pic_order_cnt_lsb_minus4 as u32 + 4,
                pic.frame_num,
            )?;
            if pic.flags.idr_flag() != 0 {
                ue(header.idr_pic_id.into(), &mut writer)?
            }
            // ref pic list reordering
            assert_eq!(
                pic.pictureType,
                vk::native::StdVideoH264SliceType_STD_VIDEO_H264_SLICE_TYPE_I
            );

            // slice qp delta
            se(1, &mut writer)?; // leave at start (start defined in PPS)
        }
        Ok(())
    }
}

fn se<W: std::io::Write, E: bitstream_io::Endianness>(
    data: i32,
    writer: &mut BitWriter<W, E>,
) -> std::io::Result<()> {
    let mut k = data.abs() as u32 * 2;
    if data > 0 {
        k -= 1;
    }
    ue(k, writer)
}

fn ue<W: std::io::Write, E: bitstream_io::Endianness>(
    data: u32,
    writer: &mut BitWriter<W, E>,
) -> std::io::Result<()> {
    let xp1 = data.wrapping_add(1);

    let lz = xp1.leading_zeros();

    let num_zeros = 32 - lz - 1;
    writer.write(0, num_zeros)?;
    writer.write(xp1, num_zeros + 1)?;
    Ok(())
}
