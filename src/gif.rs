//! Animated GIF export of replays.

use std::collections::HashMap;

use dicegame::engine::prelude::*;

use crate::net::secure_random;

// box-downsample an RGBA image by an integer factor (poor man's AA for
// render targets, which get no MSAA)
pub(crate) fn downsample(img: &Image, f: usize) -> Image {
    let w = img.width as usize / f;
    let h = img.height as usize / f;
    let mut bytes = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 4];
            for dy in 0..f {
                for dx in 0..f {
                    let i = ((y * f + dy) * img.width as usize + x * f + dx) * 4;
                    for (c, a) in acc.iter_mut().enumerate() {
                        *a += img.bytes[i + c] as u32;
                    }
                }
            }
            let o = (y * w + x) * 4;
            for (c, a) in acc.iter().enumerate() {
                bytes[o + c] = (a / (f * f) as u32) as u8;
            }
        }
    }
    Image {
        width: w as u16,
        height: h as u16,
        bytes,
    }
}

// map a render-target image to the 216-color web cube
pub(crate) fn quantize_image(img: &Image) -> Vec<u8> {
    let iw = img.width as usize;
    let ih = img.height as usize;
    let mut out = Vec::with_capacity(iw * ih);
    for y in 0..ih {
        for x in 0..iw {
            let i = (y * iw + x) * 4;
            let r = img.bytes[i] as usize;
            let g = img.bytes[i + 1] as usize;
            let b = img.bytes[i + 2] as usize;
            out.push(
                ((r * 5 + 127) / 255 * 36 + (g * 5 + 127) / 255 * 6 + (b * 5 + 127) / 255) as u8,
            );
        }
    }
    out
}

// the gif renders at 1.5x the virtual canvas for crisp output
pub(crate) const GIF_W: u32 = 1395;

pub(crate) const GIF_H: u32 = 1230;

// Evenly sample long matches, always including the final event, within the export budget.
pub(crate) fn gif_capture_count(index: usize, total: usize) -> usize {
    ((index.min(total) as u128 * total.min(600) as u128) / total.max(1) as u128) as usize
}

struct BitWriter {
    pub(crate) out: Vec<u8>,
    pub(crate) cur: u32,
    pub(crate) nbits: u32,
}

impl BitWriter {
    pub(crate) fn put(&mut self, code: u16, size: u32) {
        self.cur |= (code as u32) << self.nbits;
        self.nbits += size;
        while self.nbits >= 8 {
            self.out.push((self.cur & 0xFF) as u8);
            self.cur >>= 8;
            self.nbits -= 8;
        }
    }
}

// standard GIF LZW with an 8-bit minimum code size
fn gif_lzw(data: &[u8]) -> Vec<u8> {
    let mut bw = BitWriter {
        out: Vec::new(),
        cur: 0,
        nbits: 0,
    };
    let clear: u16 = 256;
    let end: u16 = 257;
    let mut dict: HashMap<(u16, u8), u16> = HashMap::with_capacity(4096);
    let mut code_size: u32 = 9;
    let mut next: u16 = 258;
    bw.put(clear, code_size);
    let mut cur = data[0] as u16;
    for &px in &data[1..] {
        if let Some(&c) = dict.get(&(cur, px)) {
            cur = c;
        } else {
            bw.put(cur, code_size);
            // the decoder's table lags one entry behind, so the width grows
            // BEFORE this insertion, not after it
            if u32::from(next) == (1 << code_size) && code_size < 12 {
                code_size += 1;
            }
            if next == 4096 {
                bw.put(clear, code_size);
                dict.clear();
                next = 258;
                code_size = 9;
            } else {
                dict.insert((cur, px), next);
                next += 1;
            }
            cur = px as u16;
        }
    }
    bw.put(cur, code_size);
    if u32::from(next) == (1 << code_size) && code_size < 12 {
        code_size += 1;
    }
    bw.put(end, code_size);
    if bw.nbits > 0 {
        bw.out.push((bw.cur & 0xFF) as u8);
    }
    bw.out
}

// Write replay frames as they are captured instead of retaining up to 600
// full-size indexed frames in memory. A dropped, unfinished writer removes its
// partial file so cancelled exports do not leave corrupt GIFs behind.
pub(crate) struct GifWriter {
    pub(crate) out: Option<std::io::BufWriter<std::fs::File>>,
    pub(crate) path: String,
    pub(crate) partial_path: String,
    pub(crate) w: u16,
    pub(crate) h: u16,
    pub(crate) delay_cs: u16,
    pub(crate) finished: bool,
}

impl GifWriter {
    pub(crate) fn create(path: &str, w: u16, h: u16, delay_cs: u16) -> std::io::Result<Self> {
        if w == 0 || h == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "GIF dimensions must be non-zero",
            ));
        }
        // Never truncate an earlier export. Pick a numbered destination when
        // needed, and stream into a private partial file that is renamed only
        // after the GIF trailer has been flushed successfully.
        let requested = std::path::Path::new(path);
        let mut final_path = requested.to_path_buf();
        if final_path.exists() {
            let parent = requested
                .parent()
                .unwrap_or_else(|| std::path::Path::new(""));
            let stem = requested
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("replay");
            let extension = requested
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("gif");
            final_path = (2..=10_000)
                .map(|n| parent.join(format!("{stem}-{n}.{extension}")))
                .find(|candidate| !candidate.exists())
                .ok_or_else(|| std::io::Error::other("too many replay GIFs"))?;
        }
        let final_path = final_path.to_string_lossy().into_owned();
        let (partial_path, file) = loop {
            let suffix: String = secure_random::<8>()?
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            let partial_path = format!("{final_path}.{suffix}.part");
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&partial_path)
            {
                Ok(file) => break (partial_path, file),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        };
        let mut writer = Self {
            out: Some(std::io::BufWriter::new(file)),
            path: final_path,
            partial_path,
            w,
            h,
            delay_cs,
            finished: false,
        };
        writer.write_header()?;
        Ok(writer)
    }

    pub(crate) fn out(&mut self) -> std::io::Result<&mut std::io::BufWriter<std::fs::File>> {
        self.out
            .as_mut()
            .ok_or_else(|| std::io::Error::other("GIF writer is closed"))
    }

    pub(crate) fn write_header(&mut self) -> std::io::Result<()> {
        use std::io::Write;

        let w = self.w;
        let h = self.h;
        let out = self.out()?;
        out.write_all(b"GIF89a")?;
        out.write_all(&w.to_le_bytes())?;
        out.write_all(&h.to_le_bytes())?;
        out.write_all(&[0xF7, 0, 0])?; // global 256-color table
        for i in 0..256usize {
            if i < 216 {
                out.write_all(&[
                    (i / 36 * 51) as u8,
                    (i / 6 % 6 * 51) as u8,
                    (i % 6 * 51) as u8,
                ])?;
            } else {
                out.write_all(&[0, 0, 0])?;
            }
        }
        // loop forever
        out.write_all(&[0x21, 0xFF, 0x0B])?;
        out.write_all(b"NETSCAPE2.0")?;
        out.write_all(&[3, 1, 0, 0, 0])
    }

    pub(crate) fn write_frame(&mut self, frame: &[u8]) -> std::io::Result<()> {
        use std::io::Write;

        if frame.len() != usize::from(self.w) * usize::from(self.h) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "GIF frame dimensions do not match the canvas",
            ));
        }
        let compressed = gif_lzw(frame);
        let w = self.w;
        let h = self.h;
        let delay_cs = self.delay_cs;
        let out = self.out()?;
        out.write_all(&[0x21, 0xF9, 4, 0])?;
        out.write_all(&delay_cs.to_le_bytes())?;
        out.write_all(&[0, 0, 0x2C, 0, 0, 0, 0])?;
        out.write_all(&w.to_le_bytes())?;
        out.write_all(&h.to_le_bytes())?;
        out.write_all(&[0, 8])?; // no local table, 8-bit codes
        for chunk in compressed.chunks(255) {
            out.write_all(&[chunk.len() as u8])?;
            out.write_all(chunk)?;
        }
        out.write_all(&[0])
    }

    pub(crate) fn finish(mut self) -> std::io::Result<String> {
        use std::io::Write;

        let path = self.path.clone();
        let out = self.out()?;
        out.write_all(&[0x3B])?;
        out.flush()?;
        self.out.take();
        std::fs::rename(&self.partial_path, &self.path)?;
        self.finished = true;
        Ok(path)
    }
}

impl Drop for GifWriter {
    fn drop(&mut self) {
        if !self.finished {
            self.out.take();
            let _ = std::fs::remove_file(&self.partial_path);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::net::new_reconnect_token;

    use super::*;

    #[test]
    fn gif_writer_streams_frames_and_removes_cancelled_exports() {
        let unique = new_reconnect_token().unwrap();
        let path = std::env::temp_dir().join(format!("dice-wars-{unique}.gif"));
        let path_text = path.to_string_lossy();
        let mut writer = GifWriter::create(&path_text, 2, 2, 12).unwrap();
        let temporary = writer.partial_path.clone();
        assert!(writer.write_frame(&[0, 1, 2]).is_err());
        writer.write_frame(&[0, 1, 2, 3]).unwrap();
        assert_eq!(writer.finish().unwrap(), path_text);
        assert!(!std::path::Path::new(&temporary).exists());
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"GIF89a"));
        assert_eq!(bytes.last(), Some(&0x3B));
        std::fs::remove_file(&path).unwrap();

        let partial = std::env::temp_dir().join(format!("dice-wars-partial-{unique}.gif"));
        {
            let partial_text = partial.to_string_lossy();
            let mut writer = GifWriter::create(&partial_text, 1, 1, 1).unwrap();
            writer.write_frame(&[0]).unwrap();
            let temporary = writer.partial_path.clone();
            drop(writer);
            assert!(!std::path::Path::new(&temporary).exists());
        }
        assert!(!partial.exists());

        std::fs::write(&path, b"previous export").unwrap();
        let writer = GifWriter::create(&path_text, 1, 1, 1).unwrap();
        let numbered_path = writer.path.clone();
        assert_ne!(numbered_path, path_text);
        drop(writer);
        assert_eq!(std::fs::read(&path).unwrap(), b"previous export");
        assert!(!std::path::Path::new(&numbered_path).exists());
        std::fs::remove_file(&path).unwrap();
    }
}
