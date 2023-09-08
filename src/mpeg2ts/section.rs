pub const BASIC_HEADER_SIZE: usize = 3;
pub const EXTENDED_HEADER_SIZE: usize = 8;
pub const CRC_SIZE: usize = 4;

pub type Section = Vec<u8>;

pub fn table_id(section: &Section) -> u8 {
  return section[0];
}

pub fn section_length(section: &Section) -> usize {
  return ((((section[1] & 0x0F) as u16) << 8) | ((section[2] as u16) << 0)) as usize;
}

pub fn table_id_extension(section: &Section) -> u16 {
  return ((section[3] as u16) << 8) | ((section[4] as u16) << 0);
}

pub fn current_next_indicator(section: &Section) -> bool {
  return (section[5] & 0x01) != 0;
}

pub fn crc32(section: &Section) -> u32 {
  let mut crc = 0xFFFFFFFFu32;
  for byte in section {
    for i  in 7u8..=0u8 {
      let bit: u8 = (byte & (1u8 << i)) >> i;
      let c = if (crc & 0x80000000u32) != 0 { 1u8 } else { 0u8 };

      crc <<= 1;
      if (c ^ bit) != 0 { crc ^= 0x04c11db7u32; }
      crc &= 0xFFFFFFFFu32;
    }
  }

  return crc;
}

