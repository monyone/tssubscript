use std::fs::File;
use std::io::{BufWriter, Write, Read, stdin, stdout};
use std::collections::{HashSet, VecDeque, HashMap};
use std::env;
use std::process::exit;

extern crate getopts;
use getopts::Options;

mod mpeg2ts;
use mpeg2ts::packetize::packtize_section;
use mpeg2ts::reader::SectionReader;

fn main() {
  let args: Vec<String> = env::args().collect();

  let mut opts = Options::new();
  opts.optopt("i", "input", "input mpegts file", "INPUT_PATH");
  opts.optopt("m", "metadata", "metadata mpegts file", "META_PATH");
  opts.optopt("o", "output", "output mpegts file", "OUTPUT_PATH");

  let Ok(matches) = opts.parse(&args[1..]) else {
    eprintln!("failed to parse command line options");
    exit(1);
  };

  let input_file_path = matches.opt_str("i");
  let meta_file_path = matches.opt_str("m");
  let output_file_path = matches.opt_str("o");

  let si_pids: HashSet<u16> = vec![0x10 /*NIT*/, 0x11 /*SDT*/, 0x12 /*EIT*/, 0x14 /*TOT*/,  0x24 /*BIT*/].into_iter().collect();

  let replace_pmt_offsets = 0;
  let mut pat_transport_stream_id = None;
  let mut pmt_program_number = None;
  let mut replace_pmt_streams: HashMap<u16, (u8, u16, Vec<u8>)> = HashMap::new();

  let mut meta_queue = VecDeque::new();
  if let Some(meta_file_path) = meta_file_path {
    let Ok(meta) = File::open(meta_file_path) else {
      eprintln!("failed to open metadata ts file");
      exit(1);
    };
    let mut reader = mpeg2ts::reader::PacketReader::new(meta);

    let mut pat_reader = SectionReader::new();
    let mut pmt_reader = SectionReader::new();

    let mut pmt_pid = None;
    let mut pcr_pid = None;

    let mut previous_pcr = None;
    let mut elapsed = 0u64;

    while let Ok(packet) = reader.read() {
      let pid =  mpeg2ts::packet::pid(&packet);

      match pid {
        0 => {
          pat_reader.push(&packet);
          while let Some(pat) = pat_reader.pop() {
            pmt_pid = None;
            if pat_transport_stream_id.is_none() {
              pat_transport_stream_id = Some(mpeg2ts::section::table_id_extension(&pat));
            }

            let mut begin = mpeg2ts::section::EXTENDED_HEADER_SIZE;
            while begin < mpeg2ts::section::BASIC_HEADER_SIZE + mpeg2ts::section::section_length(&pat) - mpeg2ts::section::CRC_SIZE {
              let program_number = ((pat[begin + 0] as u16) << 8) | ((pat[begin + 1] as u16) << 0);
              let program_map_pid = (((pat[begin + 2] & 0x1F) as u16) << 8) | ((pat[begin + 3] as u16) << 0);

              if program_map_pid == 0x10 {  // NIT
                begin += 4;
                continue;
              }

              if pmt_pid.is_none() {
                pmt_pid = Some(program_map_pid);
              }
              if pmt_program_number.is_none() {
                pmt_program_number = Some(program_number);
              }

              begin += 4;
            }
          }
        },
        _ if pmt_pid == Some(pid) => {
          pmt_reader.push(&packet);
          while let Some(pmt) = pmt_reader.pop() {
            pcr_pid = Some((((pmt[mpeg2ts::section::EXTENDED_HEADER_SIZE + 0] & 0x1F) as u16) << 8) | ((pmt[mpeg2ts::section::EXTENDED_HEADER_SIZE + 1] as u16) << 0));

            let program_info_length = ((((pmt[mpeg2ts::section::EXTENDED_HEADER_SIZE + 2] & 0x0F) as u16) << 8) | ((pmt[mpeg2ts::section::EXTENDED_HEADER_SIZE + 3] as u16) << 0)) as usize;
            {
              let mut begin = mpeg2ts::section::EXTENDED_HEADER_SIZE + 4 + program_info_length;
              while begin < mpeg2ts::section::BASIC_HEADER_SIZE + mpeg2ts::section::section_length(&pmt) - mpeg2ts::section::CRC_SIZE {
                let stream_type = pmt[begin + 0];
                let elementary_pid = (((pmt[begin + 1] & 0x1F) as u16) << 8) | ((pmt[begin + 2] as u16) << 0);
                let es_info_length = (((pmt[begin + 3] & 0x0F) as usize) << 8) | ((pmt[begin + 4] as usize) << 0);

                match stream_type {
                  0x06 => {
                    let mut offset = begin + 5;
                    while offset < begin + 5 + es_info_length {
                      let tag = pmt[offset + 0];
                      let length = pmt[offset + 1] as usize;

                      if tag == 0x52 && pmt[offset + 2] == 0x38 {
                        replace_pmt_streams.insert(elementary_pid, (stream_type, replace_pmt_offsets + elementary_pid, pmt[begin + 5 .. begin + 5 + es_info_length].to_vec()));
                      }

                      offset += 2 + length;
                    }
                  },
                  0x0D => {
                    replace_pmt_streams.insert(elementary_pid, (stream_type, replace_pmt_offsets + elementary_pid, pmt[begin + 5 .. begin + 5 + es_info_length].to_vec()));
                  },
                  _ => {}
                }

                begin += 5 + es_info_length;
              }
            }
          }
        }
        _ => {}
      };

      if si_pids.contains(&pid) || replace_pmt_streams.contains_key(&pid) {
        meta_queue.push_back((elapsed, pid, packet));
      };

      if pcr_pid == Some(pid) && mpeg2ts::packet::has_pcr(&packet){
        if let Some(pcr) = mpeg2ts::packet::pcr(&packet) {
          elapsed += ((pcr - previous_pcr.unwrap_or(pcr)) + mpeg2ts::packet::MPEGTS_TIMESTAMP_CLOCKS) % mpeg2ts::packet::MPEGTS_TIMESTAMP_CLOCKS;
          previous_pcr = Some(pcr);
        }
      };
    }
  } else {
    eprintln!("Please specify metadata file");
    exit(1);
  };
  let Some(pat_transport_stream_id) = pat_transport_stream_id else {
    eprintln!("failed to detect transport stream id");
    exit(1);
  };
  let Some(pmt_program_number) = pmt_program_number else {
    eprintln!("failed to detect program number");
    exit(1);
  };

  {
    let input_file = match input_file_path {
      Some(path) => File::open(path).map(|f| Box::new(f) as Box<dyn Read>).unwrap_or_else(|_| {
        eprintln!("failed to open input ts file");
        exit(1);
      }),
      None => Box::new(stdin()) as Box<dyn Read>
    };
    let mut reader = mpeg2ts::reader::PacketReader::new(input_file);

    let output_file =  match output_file_path {
      Some(path) => File::create(path).map(|f| Box::new(f) as Box<dyn Write>).unwrap_or_else(|_| {
        eprintln!("failed to open output ts file");
        exit(1);
      }),
      None => Box::new(stdout()) as Box<dyn Write>
    };
    let mut writer: BufWriter<Box<dyn Write>> = BufWriter::new(output_file);

    let mut pat_reader = SectionReader::new();
    let mut pmt_reader = SectionReader::new();

    let mut pmt_pid = None;
    let mut pcr_pid = None;

    let mut pat_cc = 0u8;
    let mut pmt_cc = 0u8;

    let mut previous_pcr = None;
    let mut elapsed = 0u64;

    while let Ok(packet) = reader.read() {
      let pid =  mpeg2ts::packet::pid(&packet);

      match pid {
        0 => {
          pat_reader.push(&packet);
          while let Some(pat) = pat_reader.pop() {
            pmt_pid = None;

            {
              let mut begin = mpeg2ts::section::EXTENDED_HEADER_SIZE;
              while begin < mpeg2ts::section::BASIC_HEADER_SIZE + mpeg2ts::section::section_length(&pat) - mpeg2ts::section::CRC_SIZE {
                let program_map_pid = (((pat[begin + 2] & 0x1F) as u16) << 8) | ((pat[begin + 3] as u16) << 0);

                if program_map_pid == 0x10 /* NIT */ {
                  begin += 4;
                  continue;
                }

                pmt_pid = Some(program_map_pid);
                break;
              }
            }

            let Some(pmt_pid) = pmt_pid else { break; };

            let mut new_pat: Vec<u8> = Vec::new();
            new_pat.extend(pat[0..mpeg2ts::section::EXTENDED_HEADER_SIZE].iter());
            new_pat[3] = ((pat_transport_stream_id & 0xFF00) >> 8) as u8;
            new_pat[4] = ((pat_transport_stream_id & 0x00FF) >> 0) as u8;
            new_pat.push(((0 & 0xFF00) >> 8) as u8);
            new_pat.push(((0 & 0x00FF) >> 0) as u8);
            new_pat.push(((0x10 & 0x1F00) >> 8) as u8);
            new_pat.push(((0x10 & 0x00FF) >> 0) as u8);
            new_pat.push(((pmt_program_number & 0xFF00) >> 8) as u8);
            new_pat.push(((pmt_program_number & 0x00FF) >> 0) as u8);
            new_pat.push(((pmt_pid & 0x1F00) >> 8) as u8);
            new_pat.push(((pmt_pid & 0x00FF) >> 0) as u8);

            // section length
            new_pat[1] = (new_pat[1] & 0xF0) | (((new_pat.len() + mpeg2ts::section::CRC_SIZE - mpeg2ts::section::BASIC_HEADER_SIZE) & 0x0F00) >> 8) as u8;
            new_pat[2] = (((new_pat.len() + mpeg2ts::section::CRC_SIZE - mpeg2ts::section::BASIC_HEADER_SIZE) & 0x00FF) >> 0) as u8;

            // crc32
            let crc32 = mpeg2ts::section::crc32(&new_pat);
            new_pat.push(((crc32 & 0xFF000000u32) >> 24) as u8);
            new_pat.push(((crc32 & 0x00FF0000u32) >> 16) as u8);
            new_pat.push(((crc32 & 0x0000FF00u32) >> 8) as u8);
            new_pat.push(((crc32 & 0x000000FFu32) >> 0) as u8);

            let packets = packtize_section(&new_pat, pid, pat_cc);
            pat_cc = (pat_cc + ((packets.len() % 0x0F) as u8)) & 0x0F;
            for packet in packets {
              let Ok(_) = writer.write(&packet) else {
                eprintln!("failed to write output ts file");
                exit(1);
              };
            }
          }
        },
        _ if pmt_pid == Some(pid) => {
          pmt_reader.push(&packet);
          while let Some(pmt) = pmt_reader.pop() {
            pcr_pid = Some((((pmt[mpeg2ts::section::EXTENDED_HEADER_SIZE + 0] & 0x1F) as u16) << 8) | ((pmt[mpeg2ts::section::EXTENDED_HEADER_SIZE + 1] as u16) << 0));

            let mut new_pmt: Vec<u8> = Vec::new();

            let program_info_length = ((((pmt[mpeg2ts::section::EXTENDED_HEADER_SIZE + 2] & 0x0F) as u16) << 8) | ((pmt[mpeg2ts::section::EXTENDED_HEADER_SIZE + 3] as u16) << 0)) as usize;
            let mut begin = mpeg2ts::section::EXTENDED_HEADER_SIZE + 4 + program_info_length;
            new_pmt.extend(pmt[0..begin].iter());
            new_pmt[3] = ((pmt_program_number & 0xFF00) >> 8) as u8;
            new_pmt[4] = ((pmt_program_number & 0x00FF) >> 0) as u8;

            while begin < mpeg2ts::section::BASIC_HEADER_SIZE + mpeg2ts::section::section_length(&pmt) - mpeg2ts::section::CRC_SIZE {
              let es_info_length = (((pmt[begin + 3] & 0x0F) as usize) << 8) | ((pmt[begin + 4] as usize) << 0);
              new_pmt.extend(pmt[begin..begin + 5 + es_info_length].iter());
              begin += 5 + es_info_length;
            }
            let mut append_streams = Vec::from_iter(replace_pmt_streams.values());
            append_streams.sort_by(|(_, p1, _), (_, p2, _)| { p1.cmp(p2) });
            for (stream_type, elementary_pid, esinfo) in append_streams {
              new_pmt.push(*stream_type);
              new_pmt.push(((elementary_pid & 0x1F00) >> 8) as u8);
              new_pmt.push(((elementary_pid & 0x00FF) >> 0) as u8);
              new_pmt.push(((esinfo.len() & 0x0F00) >> 8) as u8);
              new_pmt.push(((esinfo.len() & 0x00FF) >> 0) as u8);
              new_pmt.extend(esinfo);
            }

            // section length
            new_pmt[1] = (new_pmt[1] & 0xF0) | (((new_pmt.len() + mpeg2ts::section::CRC_SIZE - mpeg2ts::section::BASIC_HEADER_SIZE) & 0x0F00) >> 8) as u8;
            new_pmt[2] = (((new_pmt.len() + mpeg2ts::section::CRC_SIZE - mpeg2ts::section::BASIC_HEADER_SIZE) & 0x00FF) >> 0) as u8;

            // crc32
            let crc32 = mpeg2ts::section::crc32(&new_pmt);
            new_pmt.push(((crc32 & 0xFF000000u32) >> 24) as u8);
            new_pmt.push(((crc32 & 0x00FF0000u32) >> 16) as u8);
            new_pmt.push(((crc32 & 0x0000FF00u32) >> 8) as u8);
            new_pmt.push(((crc32 & 0x000000FFu32) >> 0) as u8);

            let packets = packtize_section(&new_pmt, pid, pmt_cc);
            pmt_cc = (pmt_cc + ((packets.len() % 0x0F) as u8)) & 0x0F;
            for packet in packets {
              let Ok(_) = writer.write(&packet) else {
                eprintln!("failed to write output ts file");
                exit(1);
              };
            }
          }
        }
        _ if si_pids.contains(&pid) || replace_pmt_streams.contains_key(&pid) => {},
        _ => {
          let Ok(_) = writer.write(&packet) else {
            eprintln!("failed to write output ts file");
            exit(1);
          };
        }
      };

      while !meta_queue.is_empty() {
        let Some(mut meta) = meta_queue.pop_front() else { break; };
        if meta.0 >= elapsed {
          meta_queue.push_front(meta);
          break;
        }

        if let Some((_, replaced_pid, _)) = replace_pmt_streams.get(&meta.1){
          meta.2[1] = (meta.2[1] & 0xE0) | (((replaced_pid & 0x1F00) >> 8) as u8);
          meta.2[2] = (replaced_pid & 0x00FF) as u8;
        }

        let Ok(_) = writer.write(&meta.2) else {
          eprintln!("failed to write output ts file");
          exit(1);
        };
      };

      if pcr_pid == Some(pid) && mpeg2ts::packet::has_pcr(&packet){
        if let Some(pcr) = mpeg2ts::packet::pcr(&packet) {
          elapsed += ((pcr - previous_pcr.unwrap_or(pcr)) + mpeg2ts::packet::MPEGTS_TIMESTAMP_CLOCKS) % mpeg2ts::packet::MPEGTS_TIMESTAMP_CLOCKS;
          previous_pcr = Some(pcr);
        }
      };
    }
  }
}
