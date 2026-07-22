//! Annex-B NALU-parser + FPS-tracker.
//!
//! Leest continu uit de `Ring`, splitst op Annex-B startcodes en levert per
//! complete NAL-unit de *emulation-prevention-stripped* payload via een callback.
//! De EPB-strip was afwezig in de vorige parser — foutieve SPS/PPS leverden
//! een `nil` formatDescription op in VideoToolbox, wat leidde tot de layer
//! failed-state crash.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::ring::Ring;
use crate::stats::{State, Stats};

// H.264 NAL unit types (subset).
pub const NAL_NON_IDR: u8 = 1;
pub const NAL_IDR: u8 = 5;
pub const NAL_SEI: u8 = 6;
pub const NAL_SPS: u8 = 7;
pub const NAL_PPS: u8 = 8;

pub struct NaluParser {
    handle: Option<JoinHandle<()>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

impl NaluParser {
    /// Start de parser-thread. Leest uit `ring`, roept `on_nalu` aan per
    /// complete NAL-unit. Stats (fps, dimensions) worden bijgewerkt via `stats`.
    pub fn start<F>(ring: Arc<Ring>, stats: Arc<Stats>, on_nalu: F) -> Self
    where
        F: FnMut(*const u8, u32, u8) + Send + 'static,
    {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_clone = stop.clone();

        let handle = thread::Builder::new()
            .name("hyprpad-parser".into())
            .spawn(move || {
                let mut on_nalu = on_nalu;
                let mut buffer: Vec<u8> = Vec::with_capacity(READ_CHUNK);
                let mut last_read = 0usize;

                // Acumulerende buffer voor bytes die nog niet tot een complete
                // NAL-unit hebben geleid.
                let mut pending: Vec<u8> = Vec::with_capacity(READ_CHUNK);

                let mut frame_count: u32 = 0;
                let mut last_stats = Instant::now();

                while !stop_clone.load(Ordering::Relaxed) {
                    buffer.clear();
                    last_read = ring.read_since(last_read, &mut buffer);

                    if buffer.is_empty() {
                        thread::sleep(Duration::from_millis(2));
                        continue;
                    }

                    pending.extend_from_slice(&buffer);

                    // Vind alle startcodes en splits in units.
                    let units = split_annexb(&pending);
                    if units.is_empty() {
                        continue;
                    }

                    let last_idx = units.len() - 1;
                    let last_complete = units[last_idx].complete;

                    // Bepaal hoeveel bytes we veilig kunnen "consumen" uit pending.
                    let consumed_upto = if last_complete {
                        units[last_idx].end
                    } else {
                        // Laatste unit is incompleet — bewaren voor volgende iteratie.
                        if units.len() > 1 {
                            units[units.len() - 2].end
                        } else {
                            0
                        }
                    };

                    // Emit alle complete units (en optioneel de laatste als die compleet is).
                    let emit_count = if last_complete {
                        units.len()
                    } else {
                        units.len().saturating_sub(1)
                    };

                    for u in &units[..emit_count] {
                        emit(&pending, u, &mut on_nalu, &stats, &mut frame_count);
                    }

                    // Verwijder verwerkte bytes uit pending.
                    if consumed_upto > 0 {
                        if consumed_upto >= pending.len() {
                            pending.clear();
                        } else {
                            pending.drain(..consumed_upto);
                        }
                    }

                    // FPS ~1x/seconde.
                    let elapsed = last_stats.elapsed();
                    if elapsed >= Duration::from_secs(1) {
                        let fps = (frame_count as f64 / elapsed.as_secs_f64()).round() as u32;
                        stats.set_fps(fps);
                        frame_count = 0;
                        last_stats = Instant::now();
                    }
                }

                stats.set_state(State::Idle);
            })
            .expect("parser-thread spawn");

        Self {
            handle: Some(handle),
            stop,
        }
    }
}

impl Drop for NaluParser {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

const READ_CHUNK: usize = 256 * 1024;

struct UnitRange {
    start: usize, // index van eerste byte ná startcode
    end: usize,   // exclusief
    complete: bool,
}

fn emit<F>(
    data: &[u8],
    unit: &UnitRange,
    on_nalu: &mut F,
    stats: &Arc<Stats>,
    frame_count: &mut u32,
) where
    F: FnMut(*const u8, u32, u8),
{
    if unit.end <= unit.start {
        return;
    }
    let raw = &data[unit.start..unit.end];
    let nal_type = raw.first().copied().unwrap_or(0) & 0x1F;

    // Strip emulation prevention bytes (00 00 03 → 00 00). Dit is essentieel
    // voor correcte SPS/PPS-parsing door VideoToolbox.
    let stripped = strip_epb(raw);

    if nal_type == NAL_IDR || nal_type == NAL_NON_IDR {
        *frame_count += 1;
    } else if nal_type == NAL_SPS {
        // Probeer width/height uit de SPS te halen voor de HUD-stats.
        if let Some((w, h)) = parse_sps_dimensions(&stripped) {
            stats.set_dimensions(w, h);
        }
    }

    on_nalu(stripped.as_ptr(), stripped.len() as u32, nal_type);
}

/// Verwijder emulation prevention bytes: `00 00 03` → `00 00`.
/// De trailing byte (`xx` in `00 00 03 xx`) blijft behouden.
fn strip_epb(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    let n = input.len();
    while i < n {
        if i + 2 < n
            && input[i] == 0
            && input[i + 1] == 0
            && input[i + 2] == 3
        {
            out.push(0);
            out.push(0);
            i += 3; // skip de `03`
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
    out
}

/// Minimale SPS-dimension parse — levert (width, height) in pixels.
/// Volgt de H.264 spec (rough): pic_width_in_mbs_minus1, pic_height_in_map_units_minus1,
/// eventueel frame_mbs_only_flag en crop. Goed genoeg voor HUD-display; de
/// daadwerkelijke decodering krijgt de ruwe (gestripte) SPS ook en doet het zelf
/// nogmaals nauwkeurig via `CMVideoFormatDescriptionCreateFromH264ParameterSets`.
fn parse_sps_dimensions(sps: &[u8]) -> Option<(u32, u32)> {
    if sps.len() < 4 {
        return None;
    }
    // SPS = forbidden_zero_bit(1) | nal_ref_idc(2) | nal_unit_type(5)
    // Daarna: profile_idc(8), constraint flags(8), level_idc(8), seq_parameter_set_id(ue).
    let mut br = BitReader::new(&sps[1..]); // skip NAL header byte
    let _profile_idc: u32 = br.read_bits(8)?;
    let _constraint: u32 = br.read_bits(8)?;
    let _level_idc: u32 = br.read_bits(8)?;
    let _sps_id = br.read_ue()?; // seq_parameter_set_id

    // Voor profile_idc 100/110/122/244/44/83/86/118/128/138 volgen nog chroma-
    // format bits; we skippen ze grof.
    if matches!(_profile_idc, 100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128 | 138) {
        let chroma_format_idc = br.read_ue()?;
        if chroma_format_idc == 3 {
            let _separate_colour_plane_flag: u32 = br.read_bits(1)?;
        }
        let _bit_depth_luma = br.read_ue()?;
        let _bit_depth_chroma = br.read_ue()?;
        let _qpprime_y_zero_transform_bypass: u32 = br.read_bits(1)?;
        let seq_scaling_matrix_present: u32 = br.read_bits(1)?;
        if seq_scaling_matrix_present != 0 {
            let count = if chroma_format_idc != 3 { 8 } else { 12 };
            for _ in 0..count {
                let seq_scaling_list_present: u32 = br.read_bits(1)?;
                if seq_scaling_list_present != 0 {
                    // skip de scaling list — we hoeven hem niet te decoderen.
                    return None;
                }
            }
        }
    }

    let pic_width_in_mbs_minus1 = br.read_ue()?;
    let pic_height_in_map_units_minus1 = br.read_ue()?;
    let frame_mbs_only_flag: u32 = br.read_bits(1)?;

    if frame_mbs_only_flag == 0 {
        let _mb_adaptive_frame_field_flag: u32 = br.read_bits(1)?;
    }

    let _direct_8x8_inference_flag: u32 = br.read_bits(1)?;

    // Vervolgens crop (overslaan als aanwezig) — voor ruwe dimensions is dit
    // voldoende.
    let width = (pic_width_in_mbs_minus1 + 1) * 16;
    let height = (2 - frame_mbs_only_flag) * (pic_height_in_map_units_minus1 + 1) * 16;

    Some((width, height))
}

/// Zeer kleine golomb-bit-reader, voldoende voor de paar velden die we uitlezen.
struct BitReader<'a> {
    data: &'a [u8],
    byte_pos: usize,
    bit_pos: u8, // 0..8, MSB-first
}

impl<'a> BitReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    fn read_bit(&mut self) -> Option<u32> {
        if self.byte_pos >= self.data.len() {
            return None;
        }
        let bit = (self.data[self.byte_pos] >> (7 - self.bit_pos)) & 1;
        self.bit_pos += 1;
        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.byte_pos += 1;
        }
        Some(bit as u32)
    }

    fn read_bits(&mut self, n: u8) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            v = (v << 1) | self.read_bit()?;
        }
        Some(v)
    }

    /// Unsigned Exp-Golomb decoder.
    fn read_ue(&mut self) -> Option<u32> {
        let mut leading_zeros = 0u32;
        while self.read_bit()? == 0 {
            leading_zeros += 1;
            if leading_zeros > 32 {
                return None;
            }
        }
        let rest = self.read_bits(leading_zeros as u8)?;
        Some((1u32 << leading_zeros) - 1 + rest)
    }
}

/// Vind alle Annex-B startcodes en retourneer de ranges van de ertussen liggende
/// NAL-units. De laatste unit krijgt `complete: false` als er geen volgende
/// startcode is aangetroffen.
fn split_annexb(data: &[u8]) -> Vec<UnitRange> {
    let mut starts: Vec<(usize, usize)> = Vec::new(); // (startcode_len, payload_start)

    let mut i = 0;
    let n = data.len();
    while i + 2 < n {
        if data[i] == 0 && data[i + 1] == 0 {
            if data[i + 2] == 1 {
                starts.push((3, i + 3));
                i += 3;
                continue;
            } else if i + 3 < n && data[i + 2] == 0 && data[i + 3] == 1 {
                starts.push((4, i + 4));
                i += 4;
                continue;
            }
        }
        i += 1;
    }

    if starts.is_empty() {
        return Vec::new();
    }

    let mut units = Vec::with_capacity(starts.len());
    for idx in 0..starts.len() {
        let (sc_len, payload_start) = starts[idx];
        let payload_end = if idx + 1 < starts.len() {
            // Tot aan volgende startcode (exclusief die startcode zelf).
            starts[idx + 1].1 - starts[idx + 1].0
        } else {
            n
        };
        let _ = sc_len; // sc_len niet meer nodig hier

        units.push(UnitRange {
            start: payload_start,
            end: payload_end,
            complete: idx + 1 < starts.len(),
        });
    }

    units
}
