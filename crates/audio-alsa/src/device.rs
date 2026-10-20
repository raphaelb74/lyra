use alsa::pcm::{Access, Format, HwParams};
use alsa::{Ctl, Direction, PCM};
use format_pcm::SampleEncoding;

use crate::error::AlsaError;

/// Rates probed on every device: both families up to 768 kHz. They also cover
/// native DSD carrier rates (DSD64 as `DSD_U32_*` is 88.2 kHz).
const RATES: [u32; 10] = [
    44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000, 705_600, 768_000,
];

const ENCODINGS: [SampleEncoding; 4] = [
    SampleEncoding::S16Le,
    SampleEncoding::S24In3Le,
    SampleEncoding::S24In4Le,
    SampleEncoding::S32Le,
];

/// A hardware playback endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// ALSA name to open, `hw:CARD=<id>,DEV=<n>`. Uses the card id, not its
    /// index, so it stays valid when cards are re-ordered or replugged.
    pub name: String,
    /// Card index at discovery time (changes across reconnects).
    pub card_index: i32,
    /// Card id, e.g. `ONE`.
    pub card_id: String,
    /// Long card name, e.g. `iFi (by AMR) ZEN One at usb-0000:00:14.0-2, high speed`.
    pub card_name: String,
    pub device: u32,
    /// PCM name, e.g. `USB Audio`.
    pub pcm_name: String,
}

/// Native DSD sample layouts ALSA can expose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DsdFormat {
    U8,
    U16Le,
    U16Be,
    U32Le,
    U32Be,
}

impl DsdFormat {
    const ALL: [Self; 5] = [Self::U8, Self::U16Le, Self::U16Be, Self::U32Le, Self::U32Be];

    fn alsa(self) -> Format {
        match self {
            Self::U8 => Format::DSDU8,
            Self::U16Le => Format::DSDU16LE,
            Self::U16Be => Format::DSDU16BE,
            Self::U32Le => Format::DSDU32LE,
            Self::U32Be => Format::DSDU32BE,
        }
    }
}

/// Rates the device accepts for one PCM layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PcmSupport {
    pub encoding: SampleEncoding,
    pub rates: Vec<u32>,
}

/// What the hardware accepts, as reported by the driver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Capabilities {
    pub channels_min: u32,
    pub channels_max: u32,
    pub pcm: Vec<PcmSupport>,
    /// Native DSD layouts with their ALSA rates. Descriptor evidence only;
    /// playback must still be verified.
    pub dsd: Vec<(DsdFormat, Vec<u32>)>,
}

impl Capabilities {
    /// Rates accepted with `encoding`, or none.
    pub fn rates_for(&self, encoding: SampleEncoding) -> &[u32] {
        self.pcm
            .iter()
            .find(|support| support.encoding == encoding)
            .map_or(&[], |support| support.rates.as_slice())
    }
}

pub(crate) fn alsa_format(encoding: SampleEncoding) -> Format {
    match encoding {
        SampleEncoding::S16Le => Format::S16LE,
        SampleEncoding::S24In3Le => Format::S243LE,
        SampleEncoding::S24In4Le => Format::S24LE,
        SampleEncoding::S32Le => Format::S32LE,
    }
}

/// Lists every hardware playback device on every card.
pub fn list_playback_devices() -> Result<Vec<Device>, alsa::Error> {
    let mut devices = Vec::new();
    for card in alsa::card::Iter::new() {
        let card = card?;
        let ctl = Ctl::from_card(&card, false)?;
        let info = ctl.card_info()?;
        let card_id = info.get_id()?.to_owned();
        let card_name = info.get_longname()?.to_owned();
        for device in alsa::ctl::DeviceIter::new(&ctl) {
            let Ok(device) = u32::try_from(device) else {
                continue;
            };
            // Capture-only devices have no playback info.
            let Ok(pcm_info) = ctl.pcm_info(device, 0, Direction::Playback) else {
                continue;
            };
            devices.push(Device {
                name: format!("hw:CARD={card_id},DEV={device}"),
                card_index: card.get_index(),
                card_id: card_id.clone(),
                card_name: card_name.clone(),
                device,
                pcm_name: pcm_info.get_name()?.to_owned(),
            });
        }
    }
    Ok(devices)
}

/// Opens the device briefly, asks the driver what it accepts, and closes it.
///
/// Fails with [`AlsaError::Busy`] when another program holds the device.
pub fn probe(device: &Device) -> Result<Capabilities, AlsaError> {
    let fail =
        |error| AlsaError::from_alsa(&device.name, Some(device.card_index), device.device, error);
    let pcm = open(&device.name).map_err(fail)?;

    let any = HwParams::any(&pcm).map_err(fail)?;
    let channels_min = any.get_channels_min().map_err(fail)?;
    let channels_max = any.get_channels_max().map_err(fail)?;

    let mut pcm_support = Vec::new();
    for encoding in ENCODINGS {
        let rates = rates_with_format(&pcm, alsa_format(encoding)).map_err(fail)?;
        if !rates.is_empty() {
            pcm_support.push(PcmSupport { encoding, rates });
        }
    }
    let mut dsd = Vec::new();
    for format in DsdFormat::ALL {
        let rates = rates_with_format(&pcm, format.alsa()).map_err(fail)?;
        if !rates.is_empty() {
            dsd.push((format, rates));
        }
    }
    Ok(Capabilities {
        channels_min,
        channels_max,
        pcm: pcm_support,
        dsd,
    })
}

/// Opens a playback PCM without blocking: a busy device fails at once with
/// `EBUSY` instead of waiting for the holder to close it.
pub(crate) fn open(name: &str) -> Result<PCM, alsa::Error> {
    PCM::new(name, Direction::Playback, true)
}

/// Rates from [`RATES`] the device accepts with `format` and interleaved
/// access; empty when the format is not supported at all.
fn rates_with_format(pcm: &PCM, format: Format) -> Result<Vec<u32>, alsa::Error> {
    let params = HwParams::any(pcm)?;
    if params.set_access(Access::RWInterleaved).is_err() || params.set_format(format).is_err() {
        return Ok(Vec::new());
    }
    Ok(RATES
        .into_iter()
        .filter(|&rate| params.test_rate(rate).is_ok())
        .collect())
}
