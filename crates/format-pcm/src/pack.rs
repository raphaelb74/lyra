/// An integer PCM byte layout a DAC can accept (ALSA naming, little endian).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SampleEncoding {
    /// `S16_LE`: 16 bits in 2 bytes.
    S16Le,
    /// `S24_3LE`: 24 bits in 3 bytes.
    S24In3Le,
    /// `S24_LE`: 24 bits in the low 3 bytes of 4, sign-extended.
    S24In4Le,
    /// `S32_LE`: 32 bits in 4 bytes. Narrower samples are zero-padded below.
    S32Le,
}

impl SampleEncoding {
    /// Significant bits this layout carries.
    pub const fn bits(self) -> u32 {
        match self {
            Self::S16Le => 16,
            Self::S24In3Le | Self::S24In4Le => 24,
            Self::S32Le => 32,
        }
    }

    /// Bytes per sample.
    pub const fn bytes(self) -> usize {
        match self {
            Self::S16Le => 2,
            Self::S24In3Le => 3,
            Self::S24In4Le | Self::S32Le => 4,
        }
    }

    /// Lossless layouts for a source bit depth, best first: the narrowest
    /// container that holds every bit, then wider zero-padded ones.
    pub fn lossless_for(bits_per_sample: u32) -> impl Iterator<Item = Self> {
        [Self::S16Le, Self::S24In3Le, Self::S24In4Le, Self::S32Le]
            .into_iter()
            .filter(move |encoding| encoding.bits() >= bits_per_sample)
    }

    /// Appends left-justified samples to `out` in this layout.
    ///
    /// The caller must only pick a layout from [`Self::lossless_for`] for the
    /// stream's bit depth; otherwise the low bits are dropped.
    pub fn pack(self, samples: &[i32], out: &mut Vec<u8>) {
        out.reserve(samples.len() * self.bytes());
        for &sample in samples {
            let bytes = sample.to_le_bytes();
            match self {
                Self::S16Le => out.extend_from_slice(&bytes[2..]),
                Self::S24In3Le => out.extend_from_slice(&bytes[1..]),
                Self::S24In4Le => out.extend_from_slice(&(sample >> 8).to_le_bytes()),
                Self::S32Le => out.extend_from_slice(&bytes),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SampleEncoding;

    fn packed(encoding: SampleEncoding, samples: &[i32]) -> Vec<u8> {
        let mut out = Vec::new();
        encoding.pack(samples, &mut out);
        out
    }

    #[test]
    fn sixteen_bit_extremes_round_trip() {
        let samples = [
            i32::from(i16::MIN) << 16,
            -1 << 16,
            0,
            1 << 16,
            i32::from(i16::MAX) << 16,
        ];
        assert_eq!(
            packed(SampleEncoding::S16Le, &samples),
            [0x00, 0x80, 0xff, 0xff, 0, 0, 0x01, 0x00, 0xff, 0x7f]
        );
    }

    #[test]
    fn twenty_four_bit_layouts() {
        let sample = -0x0012_3456 << 8; // 24-bit 0xEDCBAA, left-justified
        assert_eq!(
            packed(SampleEncoding::S24In3Le, &[sample]),
            [0xaa, 0xcb, 0xed]
        );
        assert_eq!(
            packed(SampleEncoding::S24In4Le, &[sample]),
            [0xaa, 0xcb, 0xed, 0xff]
        );
        assert_eq!(
            packed(SampleEncoding::S32Le, &[sample]),
            [0x00, 0xaa, 0xcb, 0xed]
        );
    }

    #[test]
    fn lossless_choices_never_narrow() {
        let for_16: Vec<_> = SampleEncoding::lossless_for(16).collect();
        assert_eq!(for_16.first(), Some(&SampleEncoding::S16Le));
        assert_eq!(for_16.len(), 4);
        let for_24: Vec<_> = SampleEncoding::lossless_for(24).collect();
        assert_eq!(
            for_24,
            [
                SampleEncoding::S24In3Le,
                SampleEncoding::S24In4Le,
                SampleEncoding::S32Le
            ]
        );
        assert_eq!(
            SampleEncoding::lossless_for(32).collect::<Vec<_>>(),
            [SampleEncoding::S32Le]
        );
    }
}
