pub const PACKET_SIZE: usize = 188;
pub const HEADER_SIZE: usize = 4;
pub const SYNC_BYTE: u8 = 0x47;
pub const STUFFING_BYTE: u8 = 0xFF;
pub const MPEGTS_TIMESTAMP_CLOCKS: u64 = 8589934592;
pub const MPEGTS_TIMESTAMP_1SEC: u64 = 90000;

pub type Packet = [u8; PACKET_SIZE];

pub fn sync_byte(packet: &Packet) -> u8 {
  return packet[0];
}

pub fn transport_error_indicator(packet: &Packet) -> bool {
  return (packet[1] & 0b10000000) != 0;
}

pub fn payload_unit_start_indicator(packet: &Packet) -> bool {
  return (packet[1] & 0b01000000) != 0;
}

pub fn transport_priority(packet: &Packet) -> bool {
  return (packet[1] & 0b00100000) != 0;
}

pub fn pid(packet: &Packet) -> u16 {
  return (((packet[1] & 0b00011111) as u16) << 8) | (((packet[2] & 0b11111111) as u16) << 0);
}

pub fn transport_scrambling_control(packet: &Packet) -> u8 {
  return (packet[3] & 0b11000000) >> 6;
}

pub fn has_adaptation_field(packet: &Packet) -> bool {
  return (packet[3] & 0b00100000) != 0;
}

pub fn has_payload(packet: &Packet) -> bool {
  return (packet[3] & 0b00010000) != 0;
}

pub fn continuity_counter(packet: &Packet) -> u8 {
  return (packet[3] & 0b00001111) >> 0;
}

pub fn adaptation_field_length(packet: &Packet) -> usize {
  if !has_adaptation_field(packet) {
    return 0;
  } else {
    return packet[HEADER_SIZE] as usize;
  }
}

pub fn pointer_field_offset(packet: &Packet) -> usize {
  return HEADER_SIZE + adaptation_field_length(packet);
}
pub fn has_pcr(packet: &Packet) -> bool {
  return has_adaptation_field(packet) && adaptation_field_length(packet) != 0 && (packet[HEADER_SIZE + 1] & 0x10) != 0;
}

pub fn pcr(packet: &Packet) -> Option<u64> {
  if !has_pcr(packet) { return None; }

  let mut pcr_base = 0u64;
  pcr_base = (pcr_base << 8) | (((packet[HEADER_SIZE + 1 + 1] & 0xFF) >> 0) as u64);
  pcr_base = (pcr_base << 8) | (((packet[HEADER_SIZE + 1 + 2] & 0xFF) >> 0) as u64);
  pcr_base = (pcr_base << 8) | (((packet[HEADER_SIZE + 1 + 3] & 0xFF) >> 0) as u64);
  pcr_base = (pcr_base << 8) | (((packet[HEADER_SIZE + 1 + 4] & 0xFF) >> 0) as u64);
  pcr_base = (pcr_base << 1) | (((packet[HEADER_SIZE + 1 + 5] & 0x80) >> 7) as u64);

  return Some(pcr_base);
}
