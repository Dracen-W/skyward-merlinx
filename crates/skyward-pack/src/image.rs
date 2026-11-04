use abootimg_oxide::{binrw::io::Cursor, Header, HeaderV0, HeaderV0Versioned, HeaderV3};
use anyhow::{anyhow, Context, Result};
use sha1::{Digest, Sha1};
use std::fs;
use std::path::Path;

/// A boot image, in either of the two layouts Android has used: the older
/// one a phone without slots carries (header versions 0 to 2), where the
/// kernel, ramdisk, second stage, recovery DTBO and DTB sit one after
/// another, and the ramdisk-only one of versions 3 and 4.
pub struct BootImage {
    pub header: Header,
    pub kernel: Vec<u8>,
    pub ramdisk: Vec<u8>,
    /// The second stage bootloader, which only the older layout has.
    pub second: Vec<u8>,
    /// The recovery DTBO, which only header versions 1 and 2 have.
    pub recovery_dtbo: Vec<u8>,
    /// The device tree, which only header version 2 has.
    pub dtb: Vec<u8>,
}

fn section(bytes: &[u8], at: usize, len: u32, what: &str) -> Result<Vec<u8>> {
    let end = at + len as usize;
    bytes.get(at..end).map(<[u8]>::to_vec).ok_or_else(|| anyhow!("{what} runs past the end of the image"))
}

/// How many bytes of padding a section of `len` bytes is followed by.
fn padding(len: usize, page: usize) -> usize {
    (page - (len % page)) % page
}

impl BootImage {
    pub fn read(path: &Path) -> Result<Self> {
        let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
        let header = Header::parse(&mut Cursor::new(&bytes)).with_context(|| format!("{}: header", path.display()))?;
        let kernel = section(&bytes, header.kernel_position(), header.kernel_size(), "kernel")?;
        let ramdisk = section(&bytes, header.ramdisk_position(), header.ramdisk_size(), "ramdisk")?;
        let (second, recovery_dtbo, dtb) = match &header {
            Header::V3(_) => (Vec::new(), Vec::new(), Vec::new()),
            Header::V0(h) => {
                // The crate's own DTB position leaves out the second stage,
                // so the sections are followed here the way they are written.
                let page = h.page_size as usize;
                let second_at = h.ramdisk_position() + h.ramdisk_size as usize + padding(h.ramdisk_size as usize, page);
                let dtbo_at = second_at + h.second_bootloader_size as usize + padding(h.second_bootloader_size as usize, page);
                let second = section(&bytes, second_at, h.second_bootloader_size, "second stage")?;
                let (recovery_dtbo, dtb) = match &h.versioned {
                    HeaderV0Versioned::V0 => (Vec::new(), Vec::new()),
                    HeaderV0Versioned::V1 { recovery_dtbo_size, .. } => {
                        (section(&bytes, dtbo_at, *recovery_dtbo_size, "recovery DTBO")?, Vec::new())
                    }
                    HeaderV0Versioned::V2 { recovery_dtbo_size, dtb_size, .. } => {
                        let dtb_at = dtbo_at + *recovery_dtbo_size as usize + padding(*recovery_dtbo_size as usize, page);
                        let dtbo = section(&bytes, dtbo_at, *recovery_dtbo_size, "recovery DTBO")?;
                        let dtb = section(&bytes, dtb_at, *dtb_size, "DTB")?;
                        (dtbo, dtb)
                    }
                };
                (second, recovery_dtbo, dtb)
            }
        };
        Ok(BootImage { header, kernel, ramdisk, second, recovery_dtbo, dtb })
    }

    /// The image with sizes taken from its parts. A version 4 header keeps its
    /// version but carries no boot signature; unlocked bootloaders do not ask
    /// for one.
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        match &self.header {
            Header::V3(h) => self.modern_bytes(h),
            Header::V0(h) => self.legacy_bytes(h),
        }
    }

    fn modern_bytes(&self, h: &HeaderV3) -> Result<Vec<u8>> {
        // The ramdisk-only layout always lays its sections out in 4 KiB pages.
        let page = 4096;
        let mut header = h.clone();
        header.kernel_size = u32::try_from(self.kernel.len()).context("kernel size")?;
        header.ramdisk_size = u32::try_from(self.ramdisk.len()).context("ramdisk size")?;
        header.v4_signature_size = header.v4_signature_size.map(|_| 0);
        let mut out = Vec::new();
        Header::V3(header).write(&mut out).context("write header")?;
        pad(&mut out, page);
        out.extend_from_slice(&self.kernel);
        pad(&mut out, page);
        out.extend_from_slice(&self.ramdisk);
        pad(&mut out, page);
        Ok(out)
    }

    /// Write the older layout, where every section after the ramdisk is
    /// placed at the offset its size leaves it at. The hash digest is over
    /// all five sections, each followed by its size, as `mkbootimg` makes it.
    fn legacy_bytes(&self, h: &HeaderV0) -> Result<Vec<u8>> {
        let page = h.page_size as usize;
        let mut header = h.clone();
        header.kernel_size = u32::try_from(self.kernel.len()).context("kernel size")?;
        header.ramdisk_size = u32::try_from(self.ramdisk.len()).context("ramdisk size")?;
        header.second_bootloader_size = u32::try_from(self.second.len()).context("second stage size")?;
        let ramdisk_at = header.kernel_position() + self.kernel.len() + padding(self.kernel.len(), page);
        let second_at = ramdisk_at + self.ramdisk.len() + padding(self.ramdisk.len(), page);
        let dtbo_at = second_at + self.second.len() + padding(self.second.len(), page);
        match &mut header.versioned {
            HeaderV0Versioned::V0 => {}
            HeaderV0Versioned::V1 { recovery_dtbo_size, recovery_dtbo_addr, .. } => {
                *recovery_dtbo_size = u32::try_from(self.recovery_dtbo.len()).context("recovery DTBO size")?;
                *recovery_dtbo_addr = u64::try_from(dtbo_at).context("recovery DTBO offset")?;
            }
            HeaderV0Versioned::V2 { recovery_dtbo_size, recovery_dtbo_addr, dtb_size, .. } => {
                *recovery_dtbo_size = u32::try_from(self.recovery_dtbo.len()).context("recovery DTBO size")?;
                *recovery_dtbo_addr = u64::try_from(dtbo_at).context("recovery DTBO offset")?;
                *dtb_size = u32::try_from(self.dtb.len()).context("DTB size")?;
            }
        }
        header.hash_digest = self.hash_digest(header.header_version());
        let mut out = Vec::new();
        Header::V0(header).write(&mut out).context("write header")?;
        pad(&mut out, page);
        for section in [&self.kernel, &self.ramdisk, &self.second, &self.recovery_dtbo, &self.dtb] {
            out.extend_from_slice(section);
            pad(&mut out, page);
        }
        Ok(out)
    }

    /// The SHA-1 over the sections of an older-layout image, each followed by
    /// its size as a 32-bit little-endian number, in the header's 32-byte
    /// digest field, zero-filled after the 20 bytes of the hash. Header
    /// version 0 hashes the first three sections, version 1 adds the recovery
    /// DTBO and version 2 the DTB, as `mkbootimg` does.
    fn hash_digest(&self, version: u32) -> [u8; 32] {
        let mut sections = vec![&self.kernel, &self.ramdisk, &self.second];
        if version >= 1 {
            sections.push(&self.recovery_dtbo);
        }
        if version >= 2 {
            sections.push(&self.dtb);
        }
        let mut digest = Sha1::new();
        for section in sections {
            digest.update(section);
            digest.update((section.len() as u32).to_le_bytes());
        }
        let mut out = [0u8; 32];
        out[..20].copy_from_slice(&digest.finalize());
        out
    }
}

fn pad(out: &mut Vec<u8>, page: usize) {
    out.resize(out.len().div_ceil(page) * page, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A header version 2 as `mkbootimg` lays it out, parsed rather than
    /// built by hand because the crate keeps some of its fields private.
    fn legacy_header() -> Header {
        let mut b = vec![0u8; 2048];
        b[..8].copy_from_slice(b"ANDROID!");
        b[12..16].copy_from_slice(&0x4008_0000u32.to_le_bytes());
        b[20..24].copy_from_slice(&0x47c8_0000u32.to_le_bytes());
        b[36..40].copy_from_slice(&2048u32.to_le_bytes());
        b[40..44].copy_from_slice(&2u32.to_le_bytes());
        b[1644..1648].copy_from_slice(&1660u32.to_le_bytes());
        b[1652..1660].copy_from_slice(&0x4bc8_0000u64.to_le_bytes());
        Header::parse(&mut Cursor::new(&b)).unwrap()
    }

    fn legacy() -> BootImage {
        BootImage {
            header: legacy_header(),
            kernel: vec![1; 5000],
            ramdisk: vec![2; 7000],
            second: vec![3; 100],
            recovery_dtbo: vec![4; 3000],
            dtb: vec![5; 2000],
        }
    }

    #[test]
    fn a_legacy_image_round_trips_every_section() {
        let img = legacy();
        let bytes = img.to_bytes().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("recovery.img");
        std::fs::write(&path, &bytes).unwrap();
        let back = BootImage::read(&path).unwrap();
        assert_eq!(back.kernel, img.kernel);
        assert_eq!(back.ramdisk, img.ramdisk);
        assert_eq!(back.second, img.second);
        assert_eq!(back.recovery_dtbo, img.recovery_dtbo);
        assert_eq!(back.dtb, img.dtb);
        assert_eq!(back.header.header_version(), 2);
        assert_eq!(back.header.page_size(), 2048);
    }

    #[test]
    fn a_legacy_image_keeps_the_offset_of_its_recovery_dtbo() {
        let img = legacy();
        let bytes = img.to_bytes().unwrap();
        let header = match Header::parse(&mut Cursor::new(&bytes)).unwrap() {
            Header::V0(h) => h,
            Header::V3(_) => panic!("the older layout stays the older layout"),
        };
        let HeaderV0Versioned::V2 { recovery_dtbo_addr, dtb_size, .. } = header.versioned else {
            panic!("version 2 stays version 2");
        };
        assert_eq!(recovery_dtbo_addr as usize, header.recovery_dtbo_position());
        assert_eq!(dtb_size as usize, img.dtb.len());
    }
}
