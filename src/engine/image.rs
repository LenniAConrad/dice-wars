//! RGBA images and a PNG writer using uncompressed DEFLATE blocks.
use std::io::Write;

#[derive(Clone, Debug)]
pub struct Image {
    pub width: u16,
    pub height: u16,
    pub bytes: Vec<u8>,
}
impl Image {
    pub fn export_png(&self, path: &str) -> std::io::Result<()> {
        if self.width == 0
            || self.height == 0
            || self.bytes.len() != self.width as usize * self.height as usize * 4
        {
            return Err(std::io::Error::other("invalid image dimensions"));
        }
        let mut out = std::fs::File::create(path)?;
        out.write_all(b"\x89PNG\r\n\x1a\n")?;
        let mut header = Vec::new();
        header.extend_from_slice(&(self.width as u32).to_be_bytes());
        header.extend_from_slice(&(self.height as u32).to_be_bytes());
        header.extend_from_slice(&[8, 6, 0, 0, 0]);
        chunk(&mut out, b"IHDR", &header)?;
        let mut filtered = Vec::with_capacity(self.bytes.len() + self.height as usize);
        for row in self.bytes.chunks_exact(self.width as usize * 4) {
            filtered.push(0);
            filtered.extend_from_slice(row);
        }
        let mut zlib = vec![0x78, 0x01];
        let blocks = filtered.chunks(65_535);
        let count = blocks.len();
        for (i, block) in blocks.enumerate() {
            zlib.push(u8::from(i + 1 == count));
            let len = block.len() as u16;
            zlib.extend_from_slice(&len.to_le_bytes());
            zlib.extend_from_slice(&(!len).to_le_bytes());
            zlib.extend_from_slice(block);
        }
        let (mut a, mut b) = (1u32, 0u32);
        for &byte in &filtered {
            a = (a + byte as u32) % 65_521;
            b = (b + a) % 65_521;
        }
        zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());
        chunk(&mut out, b"IDAT", &zlib)?;
        chunk(&mut out, b"IEND", &[])
    }
}
fn chunk(out: &mut impl Write, kind: &[u8; 4], data: &[u8]) -> std::io::Result<()> {
    out.write_all(&(data.len() as u32).to_be_bytes())?;
    out.write_all(kind)?;
    out.write_all(data)?;
    let mut crc = !0u32;
    for &v in kind.iter().chain(data) {
        crc ^= v as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (0u32.wrapping_sub(crc & 1)));
        }
    }
    out.write_all(&(!crc).to_be_bytes())
}
