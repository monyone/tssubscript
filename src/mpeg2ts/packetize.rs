use std::cmp::min;

pub fn packtize_section(section: &super::section::Section, pid: u16, continuity_counter: u8) -> Vec<[u8; super::packet::PACKET_SIZE]> {
  let mut packets = Vec::new();
  let mut cc = continuity_counter;

  let mut begin = 0;
  while begin < section.len() {
    let mut packet = [0xFFu8; super::packet::PACKET_SIZE];
    packet[0..4].copy_from_slice(&[0, 0, 0, 0]);
    packet[0] = super::packet::SYNC_BYTE;
    packet[1] = (if begin == 0 { 1 << 6 } else { 0 }) | (((pid & 0x1F00) >> 8) as u8);
    packet[2] = ((pid & 0x00FF) >> 0) as u8;
    packet[3] = (1 << 4) | (cc & 0x0F);
    cc = (cc + 1) & 0x0F;

    let next = min(section.len(), begin + (super::packet::PACKET_SIZE - super::packet::HEADER_SIZE) - (if begin == 0 { 1 } else { 0 }));
    let length = next - begin;
    if begin == 0 {
      packet[4] = 0;
      packet[5..5+length].copy_from_slice(&section[begin..next]);
    } else {
      packet[4..4+length].copy_from_slice(&section[begin..next])
    }

    packets.push(packet);
    begin = next;
  }

  return packets;
}
