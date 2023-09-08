use std::io::{BufReader, Read};
use std::{collections::VecDeque, cmp::min};

pub struct PacketReader<R> {
  reader: BufReader<R>
}

impl<R: Read> PacketReader<R> {
  pub fn new(inner: R) -> Self {
    return PacketReader { reader: BufReader::new(inner) }
  }

  pub fn read(&mut self) -> std::io::Result<super::packet::Packet> {
    loop {
      let mut syncbyte = [0u8; 1];
      self.reader.read_exact(&mut syncbyte)?;
      if syncbyte[0] != super::packet::SYNC_BYTE { continue }

      let mut payload: [u8; super::packet::PACKET_SIZE - 1] = [0u8; super::packet::PACKET_SIZE - 1];
      self.reader.read_exact(&mut payload)?;

      let mut packet: super::packet::Packet = [0u8; super::packet::PACKET_SIZE];
      packet[..1].copy_from_slice(&syncbyte);
      packet[1..].copy_from_slice(&payload);

      return Ok(packet);
    }
  }
}

pub struct SectionReader {
  section: Option<Vec<u8>>,
  queue: VecDeque<super::section::Section>
}

impl SectionReader {
  pub fn new() -> Self {
    return SectionReader { section: None, queue: VecDeque::new() }
  }

  pub fn push(&mut self, packet: &super::packet::Packet) {
    let mut begin: usize = super::packet::pointer_field_offset(packet);

    if super::packet::payload_unit_start_indicator(packet) {
      if self.section.is_none() {
        begin += packet[super::packet::pointer_field_offset(packet)] as usize;
      }
      begin += 1;
    }

    if !super::packet::payload_unit_start_indicator(packet) {
      if let Some(ref mut section) = self.section {
        let next = min(super::packet::PACKET_SIZE, begin + (super::section::BASIC_HEADER_SIZE + super::section::section_length(&section)) - section.len());
        section.extend(&packet[begin..next]);

        if section.len() == (super::section::BASIC_HEADER_SIZE + super::section::section_length(&section)) {
          self.queue.push_back(section.to_vec());
        }
        if section.len() >= (super::section::BASIC_HEADER_SIZE + super::section::section_length(&section)) {
          self.section = None;
        }
      }
    } else {
      while begin < super::packet::PACKET_SIZE {
        if packet[begin] == super::packet::STUFFING_BYTE { return; }

        let section = self.section.get_or_insert(vec![] as super::section::Section);
        let next = min(super::packet::PACKET_SIZE, if section.len() == 0 {
          begin + ((((packet[begin + 1] as u16) & 0x0F) << 8) | ((packet[begin + 2] as u16) << 0)) as usize
        } else {
          begin + (super::section::BASIC_HEADER_SIZE + super::section::section_length(&section)) - section.len()
        });

        section.extend(&packet[begin..next]);

        if section.len() == (super::section::BASIC_HEADER_SIZE + super::section::section_length(&section)) {
          self.queue.push_back(section.to_vec());
        }
        if section.len() >= (super::section::BASIC_HEADER_SIZE + super::section::section_length(&section)) {
          self.section = None;
        }

        begin = next;
      }
    }
  }

  pub fn pop(&mut self) -> Option<super::section::Section> {
    return self.queue.pop_front();
  }
}
