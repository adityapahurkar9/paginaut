//! Multi-page TIFF files (MuPDF writes PNG and JPEG, not TIFF): baseline RGB, PackBits compression (which every
//! TIFF reader understands), a page at a time so long documents don't sit in memory.

use std::fs::File;
use std::io::{Seek, SeekFrom, Write};

pub struct Tiff {
    file: File,
    /// Where the previous page's "next page" pointer is, to link the next one.
    link: u64,
}

impl Tiff {
    pub fn create(path: &std::path::Path) -> Result<Tiff, String> {
        let mut file = File::create(path).map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
        file.write_all(b"II*\0\0\0\0\0").map_err(|e| e.to_string())?;
        Ok(Tiff { file, link: 4 })
    }

    /// Adds a page from a binary PNM picture (P6: RGB, as MuPDF writes it) made at `dpi`.
    pub fn add_pnm(&mut self, pnm: &[u8], dpi: f32) -> Result<(), String> {
        let (w, h, pixels) = parse_pnm(pnm).ok_or("unexpected picture data")?;
        let mut data = Vec::with_capacity(pixels.len() / 2);
        for row in pixels.chunks(w * 3).take(h) {
            packbits(row, &mut data);
        }
        let err = |e: std::io::Error| e.to_string();
        let mut at = self.file.seek(SeekFrom::End(0)).map_err(err)?;
        if at % 2 == 1 {
            self.file.write_all(&[0]).map_err(err)?;
            at += 1;
        }
        let strip = at;
        self.file.write_all(&data).map_err(err)?;
        let mut ifd = strip + data.len() as u64;
        if ifd % 2 == 1 {
            self.file.write_all(&[0]).map_err(err)?;
            ifd += 1;
        }
        // Values that don't fit in an entry follow the entries: bits per sample, then the resolution.
        const ENTRIES: u64 = 13;
        let extra = ifd + 2 + ENTRIES * 12 + 4;
        if extra + 22 > u32::MAX as u64 {
            return Err("The TIFF file would be larger than 4 GB; choose fewer pages or a lower resolution.".into());
        }
        let res = (dpi.round().max(1.0) as u32, 1u32);
        let mut b: Vec<u8> = Vec::with_capacity(200);
        b.extend_from_slice(&(ENTRIES as u16).to_le_bytes());
        let mut entry = |tag: u16, kind: u16, count: u32, value: u32| {
            b.extend_from_slice(&tag.to_le_bytes());
            b.extend_from_slice(&kind.to_le_bytes());
            b.extend_from_slice(&count.to_le_bytes());
            b.extend_from_slice(&value.to_le_bytes());
        };
        const SHORT: u16 = 3;
        const LONG: u16 = 4;
        const RATIONAL: u16 = 5;
        entry(254, LONG, 1, 2); // a page of a multi-page file
        entry(256, LONG, 1, w as u32);
        entry(257, LONG, 1, h as u32);
        entry(258, SHORT, 3, extra as u32); // 8, 8, 8
        entry(259, SHORT, 1, 32773); // PackBits
        entry(262, SHORT, 1, 2); // RGB
        entry(273, LONG, 1, strip as u32);
        entry(277, SHORT, 1, 3);
        entry(278, LONG, 1, h as u32);
        entry(279, LONG, 1, data.len() as u32);
        entry(282, RATIONAL, 1, extra as u32 + 6);
        entry(283, RATIONAL, 1, extra as u32 + 14);
        entry(296, SHORT, 1, 2); // inches
        b.extend_from_slice(&0u32.to_le_bytes()); // no next page yet
        for v in [8u16, 8, 8] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for _ in 0..2 {
            b.extend_from_slice(&res.0.to_le_bytes());
            b.extend_from_slice(&res.1.to_le_bytes());
        }
        self.file.write_all(&b).map_err(err)?;
        // Link this page from the previous one (or the header).
        self.file.seek(SeekFrom::Start(self.link)).map_err(err)?;
        self.file.write_all(&(ifd as u32).to_le_bytes()).map_err(err)?;
        self.link = ifd + 2 + ENTRIES * 12;
        Ok(())
    }

    pub fn finish(mut self) -> Result<(), String> {
        self.file.flush().map_err(|e| e.to_string())
    }
}

/// Width, height and RGB pixels of a binary PNM (P6, 8 bits).
fn parse_pnm(d: &[u8]) -> Option<(usize, usize, &[u8])> {
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while i < d.len() && d[i].is_ascii_whitespace() {
            i += 1;
        }
        if d.get(i) == Some(&b'#') {
            while i < d.len() && d[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
        while i < d.len() && !d[i].is_ascii_whitespace() {
            i += 1;
        }
        fields.push(std::str::from_utf8(&d[start..i]).ok()?);
    }
    if fields[0] != "P6" || fields[3] != "255" {
        return None;
    }
    let (w, h): (usize, usize) = (fields[1].parse().ok()?, fields[2].parse().ok()?);
    let pixels = d.get(i + 1..)?;
    (pixels.len() >= w * h * 3).then_some((w, h, pixels))
}

/// PackBits: runs of a repeated byte, or literal stretches, 128 bytes at most each.
fn packbits(row: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < row.len() {
        let mut run = 1;
        while i + run < row.len() && run < 128 && row[i + run] == row[i] {
            run += 1;
        }
        if run >= 3 {
            out.push((1i16 - run as i16) as i8 as u8);
            out.push(row[i]);
            i += run;
            continue;
        }
        // A literal stretch, up to where a run of three starts.
        let start = i;
        while i < row.len() && i - start < 128 {
            if i + 2 < row.len() && row[i] == row[i + 1] && row[i] == row[i + 2] {
                break;
            }
            i += 1;
        }
        out.push((i - start - 1) as u8);
        out.extend_from_slice(&row[start..i]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unpack(mut d: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        while let Some((&n, rest)) = d.split_first() {
            let n = n as i8;
            if n >= 0 {
                out.extend_from_slice(&rest[..n as usize + 1]);
                d = &rest[n as usize + 1..];
            } else if n != -128 {
                out.extend(std::iter::repeat_n(rest[0], (1 - n as i16) as usize));
                d = &rest[1..];
            } else {
                d = rest;
            }
        }
        out
    }

    #[test]
    fn packbits_round_trips() {
        let rows: Vec<Vec<u8>> = vec![vec![255; 1000], (0..=255).collect(), vec![1, 2, 2, 3, 3, 3, 4, 4, 4, 4, 5], vec![7], (0..700).map(|i| (i / 5) as u8).collect()];
        for r in rows {
            let mut out = Vec::new();
            packbits(&r, &mut out);
            assert_eq!(unpack(&out), r);
        }
    }
}
