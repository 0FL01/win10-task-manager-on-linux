/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */
//! SPD field layouts, not live memory-controller timings. Reject corrupt base blocks.
use crate::i18n::n_;
use mc_magpie::SpdEeprom;

#[derive(Debug, Clone, PartialEq)]
pub struct SpdModule {
    pub source: String,
    pub kind: String,
    pub form_factor: String,
    pub part_number: Option<String>,
    pub serial: Option<String>,
    pub rows: Vec<(String, String)>,
}

fn crc(bytes: &[u8]) -> u16 {
    let mut value = 0_u16;
    for byte in bytes {
        value ^= (*byte as u16) << 8;
        for _ in 0..8 {
            value = if value & 0x8000 != 0 {
                (value << 1) ^ 0x1021
            } else {
                value << 1
            };
        }
    }
    value
}

pub fn decode(eeprom: &SpdEeprom) -> Result<SpdModule, String> {
    let hex = eeprom.hex.as_bytes();
    if hex.len() > 2048 || !hex.len().is_multiple_of(2) {
        return Err("Invalid SPD length".into());
    }
    let bytes: Vec<u8> = hex
        .chunks_exact(2)
        .map(|s| {
            std::str::from_utf8(s)
                .ok()
                .and_then(|s| u8::from_str_radix(s, 16).ok())
                .ok_or("Invalid SPD bytes".to_owned())
        })
        .collect::<Result<_, _>>()?;
    let b = &bytes;
    if b.len() < 128 {
        return Err("Truncated SPD".into());
    }
    let kind = match b[2] {
        8 => "DDR2",
        11 => "DDR3",
        12 => "DDR4",
        18 => "DDR5",
        _ => return Err("Unsupported SPD memory type".into()),
    };
    let coverage = match b[2] {
        8 => 63,
        11 if b[0] & 0x80 != 0 => 117,
        11 | 12 => 126,
        18 => 510,
        _ => unreachable!(),
    };
    if b.len() < coverage + if b[2] == 8 { 1 } else { 2 } {
        return Err("Truncated SPD checksum block".into());
    }
    let valid = if b[2] == 8 {
        b[..63].iter().fold(0_u8, |a, b| a.wrapping_add(*b)) == b[63]
    } else {
        let at = if b[2] == 18 { 510 } else { 126 };
        crc(&b[..coverage]) == u16::from_le_bytes([b[at], b[at + 1]])
    };
    if !valid {
        return Err("SPD checksum mismatch".into());
    }
    let signed = |i: usize| b[i] as i8 as f64;
    let word = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]) as f64;
    let (tck, taa, trcd, trp, tras, trc, trfc, cas, serial_at, part_at, part_len) = match b[2] {
        8 => {
            let fraction = match b[9] & 15 {
                0..=9 => (b[9] & 15) as f64 / 10.,
                10 => 0.25,
                11 => 1. / 3.,
                12 => 2. / 3.,
                13 => 0.75,
                _ => return Err("Invalid DDR2 timebase".into()),
            };
            let cas: Vec<u32> = (2..7).filter(|i| b[18] & (1 << i) != 0).collect();
            (
                (b[9] >> 4) as f64 + fraction,
                None,
                b[29] as f64 / 4.,
                b[27] as f64 / 4.,
                b[30] as f64,
                None,
                None,
                cas,
                95,
                73,
                18,
            )
        }
        11 => {
            if b[11] == 0 || b[9] & 15 == 0 {
                return Err("Invalid DDR3 timebase".into());
            }
            let mtb = b[10] as f64 / b[11] as f64;
            let ftb = (b[9] >> 4) as f64 / (b[9] & 15) as f64 / 1000.;
            let time = |i, f| b[i] as f64 * mtb + signed(f) * ftb;
            let cas = (0..15)
                .filter(|i| (u16::from_le_bytes([b[14], b[15]])) & (1 << i) != 0)
                .map(|i| i + 4)
                .collect();
            (
                time(12, 34),
                Some(time(16, 35)),
                time(18, 36),
                time(20, 37),
                (((b[21] & 15) as u16 * 256) + b[22] as u16) as f64 * mtb,
                Some((((b[21] & 240) as u16 * 16) + b[23] as u16) as f64 * mtb + signed(38) * ftb),
                Some(word(24) * mtb),
                cas,
                122,
                128,
                18,
            )
        }
        12 => {
            if b[17] & 0xc3 != 0 {
                return Err("Unknown DDR4 timebase".into());
            }
            let time = |i, f| b[i] as f64 * 0.125 + signed(f) * 0.001;
            let mask = u32::from_le_bytes([b[20], b[21], b[22], b[23]]);
            let base = if b[23] & 128 != 0 { 23 } else { 7 };
            let cas = (0..30)
                .filter(|i| mask & (1 << i) != 0)
                .map(|i| i + base)
                .collect();
            (
                time(18, 125),
                Some(time(24, 123)),
                time(25, 122),
                time(26, 121),
                (((b[27] & 15) as u16 * 256) + b[28] as u16) as f64 * 0.125,
                Some(
                    (((b[27] & 240) as u16 * 16) + b[29] as u16) as f64 * 0.125
                        + signed(120) * 0.001,
                ),
                Some(word(30) * 0.125),
                cas,
                325,
                329,
                20,
            )
        }
        18 => {
            let cas = (0..40)
                .filter(|i| b[24 + i / 8] & (1 << (i % 8)) != 0)
                .map(|i| (20 + 2 * i) as u32)
                .collect();
            (
                word(20) / 1000.,
                Some(word(30) / 1000.),
                word(32) / 1000.,
                word(34) / 1000.,
                word(36) / 1000.,
                Some(word(38) / 1000.),
                Some(word(42)),
                cas,
                517,
                521,
                30,
            )
        }
        _ => unreachable!(),
    };
    if !(0.1..=20.).contains(&tck)
        || [trcd, trp, tras]
            .iter()
            .any(|v| !v.is_finite() || *v <= 0. || *v > 1000.)
    {
        return Err("Invalid SPD timing values".into());
    }
    let mut rows = vec![
        (n_("SPD memory type").into(), kind.into()),
        (n_("Base checksum").into(), "Valid".into()),
        (
            n_("Maximum JEDEC data rate").into(),
            format!("{:.0} MT/s", 2000. / tck),
        ),
        (n_("tCK minimum").into(), format!("{tck:.3} ns")),
        (
            n_("Supported CAS latencies").into(),
            cas.iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", "),
        ),
    ];
    for (label, value) in [
        ("tAA", taa),
        ("tRCD", Some(trcd)),
        ("tRP", Some(trp)),
        ("tRAS", Some(tras)),
        ("tRC", trc),
        ("tRFC", trfc),
    ] {
        if let Some(value) = value.filter(|v| v.is_finite() && *v > 0.) {
            rows.push((label.into(), format!("{value:.3} ns")));
        }
    }
    let cl = taa
        .and_then(|aa| cas.iter().copied().find(|c| *c as f64 * tck >= aa - 0.001))
        .or_else(|| if b[2] == 8 { cas.last().copied() } else { None });
    if let Some(cl) = cl {
        rows.push((
            n_("JEDEC CL-tRCD-tRP-tRAS").into(),
            format!(
                "{cl}-{:.0}-{:.0}-{:.0}",
                (trcd / tck).ceil(),
                (trp / tck).ceil(),
                (tras / tck).ceil()
            ),
        ));
    }
    let serial = b
        .get(serial_at..serial_at + 4)
        .filter(|s| s.iter().any(|v| *v != 0) && s.iter().any(|v| *v != 255))
        .map(|s| s.iter().map(|b| format!("{b:02X}")).collect::<String>());
    let part_number = b
        .get(part_at..part_at + part_len)
        .and_then(|s| std::str::from_utf8(s).ok())
        .map(|s| s.trim_matches([' ', '\0']).to_owned())
        .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_graphic() || c == ' '));
    let form_factor = if b[2] == 8 {
        if b[20] & 4 != 0 { "SO-DIMM" } else { "DIMM" }
    } else {
        match b[3] & 15 {
            3 => "SO-DIMM",
            8 | 9 | 12 | 13 if b[2] != 18 => "SO-DIMM",
            6 if b[2] == 18 => "SO-DIMM",
            1 | 2 | 4 | 5 | 6 => "DIMM",
            _ => "Unknown",
        }
    };
    Ok(SpdModule {
        source: eeprom.source.clone(),
        kind: kind.into(),
        form_factor: form_factor.into(),
        part_number,
        serial,
        rows,
    })
}

/// A bus address is not a slot number. Link only by unique, non-placeholder serial number.
pub fn match_slot<'a>(
    slot: &mc_magpie::MemoryDevice,
    modules: &'a [SpdModule],
    slots: &[mc_magpie::MemoryDevice],
) -> Option<&'a SpdModule> {
    let serial = slot.serial_number.as_deref()?.trim();
    if serial.is_empty() || serial.chars().all(|c| matches!(c, '0' | 'F' | 'f')) {
        return None;
    }
    if slots
        .iter()
        .filter(|s| {
            s.serial_number
                .as_deref()
                .is_some_and(|s| s.trim().eq_ignore_ascii_case(serial))
        })
        .count()
        != 1
    {
        return None;
    }
    let mut candidates = modules.iter().filter(|m| {
        m.serial
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case(serial))
            && m.kind.eq_ignore_ascii_case(&slot.ram_type)
    });
    let first = candidates.next()?;
    candidates.next().is_none().then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_coreboot_base_blocks_keep_original_crcs_and_timings() {
        for (fixture, kind, tck, rate, timings) in [
            (
                include_str!("../tests/fixtures/spd/coreboot-ddr3-base.hex"),
                "DDR3",
                "1.250 ns",
                "1600 MT/s",
                "11-11-11-28",
            ),
            (
                include_str!("../tests/fixtures/spd/coreboot-ddr4-base.hex"),
                "DDR4",
                "0.938 ns",
                "2132 MT/s",
                "15-15-15-36",
            ),
        ] {
            // Do not use eeprom(): the checksum must remain independent of our CRC code.
            let mut image = SpdEeprom {
                source: format!("public-coreboot-{kind}"),
                driver: "reference".into(),
                hex: fixture.split_whitespace().collect(),
            };
            let module = decode(&image).unwrap();
            assert_eq!(module.kind, kind);
            assert_eq!(module.serial, None);
            assert_eq!(module.part_number, None);
            for (label, value) in [
                ("Base checksum", "Valid"),
                ("tCK minimum", tck),
                ("Maximum JEDEC data rate", rate),
                ("JEDEC CL-tRCD-tRP-tRAS", timings),
            ] {
                assert!(
                    module.rows.iter().any(|(k, v)| k == label && v == value),
                    "{kind}: {label}={value}, got {:?}",
                    module.rows
                );
            }
            image.hex.replace_range(24..26, "ff");
            assert_eq!(decode(&image).unwrap_err(), "SPD checksum mismatch");
        }
    }

    fn eeprom(mut bytes: Vec<u8>) -> SpdEeprom {
        let at = if bytes[2] == 18 { 510 } else { 126 };
        let value = crc(&bytes[..at]).to_le_bytes();
        bytes[at..at + 2].copy_from_slice(&value);
        SpdEeprom {
            source: "0-0050".into(),
            driver: "ee1004".into(),
            hex: bytes.iter().map(|b| format!("{b:02x}")).collect(),
        }
    }
    #[test]
    fn ddr4_crc_fine_timing_and_serial_mapping() {
        let mut b = vec![0; 512];
        b[2] = 12;
        b[3] = 2;
        b[18] = 10;
        b[21] = 1 << 1;
        b[24] = 128;
        b[25] = 128;
        b[26] = 128;
        b[27] = 1;
        b[28] = 24;
        b[325..329].copy_from_slice(&[1, 2, 3, 4]);
        b[329..333].copy_from_slice(b"TEST");
        let image = eeprom(b);
        let module = decode(&image).unwrap();
        assert_eq!(module.serial.as_deref(), Some("01020304"));
        assert!(
            module
                .rows
                .iter()
                .any(|(k, v)| k == "JEDEC CL-tRCD-tRP-tRAS" && v == "16-13-13-28")
        );
        let slot = mc_magpie::MemoryDevice {
            serial_number: Some("01020304".into()),
            ram_type: "DDR4".into(),
            ..Default::default()
        };
        assert!(
            match_slot(
                &slot,
                std::slice::from_ref(&module),
                std::slice::from_ref(&slot)
            )
            .is_some()
        );
        assert!(
            match_slot(
                &slot,
                std::slice::from_ref(&module),
                &[slot.clone(), slot.clone()]
            )
            .is_none()
        );
        let mut corrupt = image;
        corrupt.hex.replace_range(36..38, "ff");
        assert!(decode(&corrupt).is_err());
    }
    #[test]
    fn ddr5_units_and_truncation_are_checked() {
        let mut b = vec![0; 1024];
        b[2] = 18;
        b[3] = 3;
        for (at, value) in [
            (20, 625_u16),
            (30, 14000),
            (32, 14000),
            (34, 14000),
            (36, 32000),
            (42, 295),
        ] {
            b[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
        b[24] = 1 << 2;
        let image = eeprom(b);
        let module = decode(&image).unwrap();
        assert_eq!(module.form_factor, "SO-DIMM");
        assert!(
            module
                .rows
                .iter()
                .any(|(k, v)| k == "tRFC" && v == "295.000 ns")
        );
        let mut truncated = image;
        truncated.hex.truncate(256);
        assert!(decode(&truncated).is_err());
    }

    #[test]
    fn ddr2_checksum_and_ddr3_short_crc_coverage() {
        let mut b = vec![0; 256];
        b[2] = 8;
        b[9] = 0x25;
        b[18] = 1 << 5;
        b[29] = 60;
        b[27] = 60;
        b[30] = 45;
        b[63] = b[..63].iter().fold(0_u8, |a, b| a.wrapping_add(*b));
        let image = SpdEeprom {
            source: "0-0050".into(),
            driver: "eeprom".into(),
            hex: b.iter().map(|v| format!("{v:02x}")).collect(),
        };
        assert_eq!(decode(&image).unwrap().kind, "DDR2");
        b.fill(0);
        b[0] = 0x80;
        b[2] = 11;
        b[3] = 3;
        b[9] = 0x11;
        b[10] = 1;
        b[11] = 8;
        b[12] = 10;
        b[14] = 1 << 7;
        b[16] = 110;
        b[18] = 110;
        b[20] = 110;
        b[21] = 1;
        b[22] = 24;
        let value = crc(&b[..117]).to_le_bytes();
        b[126..128].copy_from_slice(&value);
        let image = SpdEeprom {
            source: "0-0050".into(),
            driver: "eeprom".into(),
            hex: b.iter().map(|v| format!("{v:02x}")).collect(),
        };
        assert_eq!(decode(&image).unwrap().kind, "DDR3");
    }
}
