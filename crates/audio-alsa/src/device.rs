use alsa::pcm::{Access, Format, HwParams};
use alsa::{Ctl, Direction, PCM};
use format_dsd::DsdLayout;
use format_pcm::SampleEncoding;
use unda::Encoding;

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
    /// Kernel driver, e.g. `USB-Audio` or `HDA-Intel`.
    pub driver: String,
    pub device: u32,
    /// PCM name, e.g. `USB Audio`.
    pub pcm_name: String,
    /// Short name for people, e.g. `iFi USB Audio SE`, `HDMI 2`,
    /// `Built-in audio`. Unique among the listed devices.
    pub label: String,
}

impl Device {
    /// A USB Audio class device: the usual external DAC.
    pub fn is_usb(&self) -> bool {
        self.driver == "USB-Audio"
    }

    /// Listing order, most likely wanted first: USB DACs, built-in analog
    /// output, other outputs, then HDMI (usually a screen).
    fn preference(&self) -> u8 {
        if self.is_usb() {
            0
        } else if self.label == "Built-in audio" {
            1
        } else if is_hdmi(&self.pcm_name) {
            3
        } else {
            2
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
    pub dsd: Vec<(DsdLayout, Vec<u32>)>,
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

pub(crate) fn alsa_format(encoding: Encoding) -> Format {
    match encoding {
        Encoding::Pcm(layout) | Encoding::Dop(layout) | Encoding::Converted(layout) => {
            pcm_format(layout)
        }
        Encoding::Dsd(layout) => dsd_format(layout),
    }
}

fn pcm_format(layout: SampleEncoding) -> Format {
    match layout {
        SampleEncoding::S16Le => Format::S16LE,
        SampleEncoding::S24In3Le => Format::S243LE,
        SampleEncoding::S24In4Le => Format::S24LE,
        SampleEncoding::S32Le => Format::S32LE,
    }
}

fn dsd_format(layout: DsdLayout) -> Format {
    match layout {
        DsdLayout::U8 => Format::DSDU8,
        DsdLayout::U16Le => Format::DSDU16LE,
        DsdLayout::U16Be => Format::DSDU16BE,
        DsdLayout::U32Le => Format::DSDU32LE,
        DsdLayout::U32Be => Format::DSDU32BE,
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
        let short_name = info.get_name()?.to_owned();
        let driver = info.get_driver()?.to_owned();
        let first = devices.len();
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
                driver: driver.clone(),
                device,
                pcm_name: pcm_info.get_name()?.to_owned(),
                label: String::new(),
            });
        }
        name_card_devices(&mut devices[first..], &short_name);
    }
    make_labels_unique(&mut devices);
    devices.sort_by_key(Device::preference);
    Ok(devices)
}

/// Labels for one card's playback devices, from what ALSA reports:
/// - a USB DAC by its product name (plus the PCM name if it has several);
/// - HDMI and `DisplayPort` outputs as `HDMI`, numbered when there are several;
/// - a built-in card's analog output as `Built-in audio`, its digital
///   output as `Built-in digital output`;
/// - anything else by card and PCM name.
fn name_card_devices(devices: &mut [Device], short_name: &str) {
    let several = devices.len() > 1;
    let hdmi_count = devices.iter().filter(|d| is_hdmi(&d.pcm_name)).count();
    let mut hdmi_seen = 0;
    for device in devices.iter_mut() {
        device.label = if device.is_usb() {
            if several {
                format!("{short_name} {}", device.pcm_name)
            } else {
                short_name.to_owned()
            }
        } else if is_hdmi(&device.pcm_name) {
            hdmi_seen += 1;
            if hdmi_count > 1 {
                format!("HDMI {hdmi_seen}")
            } else {
                "HDMI".to_owned()
            }
        } else if device.driver == "HDA-Intel" && device.pcm_name.ends_with("Analog") {
            "Built-in audio".to_owned()
        } else if device.driver == "HDA-Intel" && device.pcm_name.ends_with("Digital") {
            "Built-in digital output".to_owned()
        } else if several {
            format!("{short_name} {}", device.pcm_name)
        } else {
            short_name.to_owned()
        };
    }
}

fn is_hdmi(pcm_name: &str) -> bool {
    pcm_name.contains("HDMI") || pcm_name.contains("DisplayPort")
}

/// Two cards can give the same label (two built-in cards, two identical
/// DACs): the later ones get ` 2`, ` 3`, ….
fn make_labels_unique(devices: &mut [Device]) {
    for index in 1..devices.len() {
        let same = devices[..index]
            .iter()
            .filter(|earlier| earlier.label == devices[index].label)
            .count();
        if same > 0 {
            devices[index].label = format!("{} {}", devices[index].label, same + 1);
        }
    }
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
        let rates = rates_with_format(&pcm, pcm_format(encoding)).map_err(fail)?;
        if !rates.is_empty() {
            pcm_support.push(PcmSupport { encoding, rates });
        }
    }
    let mut dsd = Vec::new();
    for layout in DsdLayout::ALL {
        let rates = rates_with_format(&pcm, dsd_format(layout)).map_err(fail)?;
        if !rates.is_empty() {
            dsd.push((layout, rates));
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

#[cfg(test)]
mod tests {
    use super::*;

    fn device(card_id: &str, driver: &str, number: u32, pcm_name: &str) -> Device {
        Device {
            name: format!("hw:CARD={card_id},DEV={number}"),
            card_index: 0,
            card_id: card_id.to_owned(),
            card_name: format!("{card_id} at 0xc1810000 irq 81"),
            driver: driver.to_owned(),
            device: number,
            pcm_name: pcm_name.to_owned(),
            label: String::new(),
        }
    }

    fn labels(devices: &[Device]) -> Vec<&str> {
        devices.iter().map(|d| d.label.as_str()).collect()
    }

    #[test]
    fn a_usb_dac_is_named_after_the_product() {
        let mut devices = vec![device("SE", "USB-Audio", 0, "USB Audio")];
        name_card_devices(&mut devices, "iFi USB Audio SE");
        assert_eq!(labels(&devices), ["iFi USB Audio SE"]);
    }

    #[test]
    fn hdmi_outputs_are_numbered_from_one() {
        let mut devices = vec![
            device("HDMI", "HDA-Intel", 3, "HDMI 0"),
            device("HDMI", "HDA-Intel", 7, "HDMI 1"),
            device("HDMI", "HDA-Intel", 8, "HDMI 2"),
        ];
        name_card_devices(&mut devices, "HDA Intel HDMI");
        assert_eq!(labels(&devices), ["HDMI 1", "HDMI 2", "HDMI 3"]);
    }

    #[test]
    fn built_in_outputs_say_what_they_are() {
        let mut devices = vec![
            device("PCH", "HDA-Intel", 0, "CS4208 Analog"),
            device("PCH", "HDA-Intel", 1, "CS4208 Digital"),
        ];
        name_card_devices(&mut devices, "HDA Intel PCH");
        assert_eq!(
            labels(&devices),
            ["Built-in audio", "Built-in digital output"]
        );
    }

    #[test]
    fn other_cards_fall_back_to_card_and_pcm_names() {
        let mut devices = vec![
            device("X", "Other", 0, "Out A"),
            device("X", "Other", 1, "Out B"),
        ];
        name_card_devices(&mut devices, "Pro Card");
        assert_eq!(labels(&devices), ["Pro Card Out A", "Pro Card Out B"]);
    }

    #[test]
    fn dacs_come_first_and_hdmi_last() {
        let mut devices = vec![
            device("HDMI", "HDA-Intel", 3, "HDMI 0"),
            device("PCH", "HDA-Intel", 1, "CS4208 Digital"),
            device("PCH", "HDA-Intel", 0, "CS4208 Analog"),
            device("SE", "USB-Audio", 0, "USB Audio"),
        ];
        name_card_devices(&mut devices[..1], "HDA Intel HDMI");
        name_card_devices(&mut devices[1..3], "HDA Intel PCH");
        name_card_devices(&mut devices[3..], "iFi USB Audio SE");
        devices.sort_by_key(Device::preference);
        assert_eq!(
            labels(&devices),
            [
                "iFi USB Audio SE",
                "Built-in audio",
                "Built-in digital output",
                "HDMI"
            ]
        );
    }

    #[test]
    fn identical_labels_are_told_apart() {
        let mut devices = vec![
            device("A", "USB-Audio", 0, "USB Audio"),
            device("B", "USB-Audio", 0, "USB Audio"),
        ];
        name_card_devices(&mut devices[..1], "ZEN DAC");
        name_card_devices(&mut devices[1..], "ZEN DAC");
        make_labels_unique(&mut devices);
        assert_eq!(labels(&devices), ["ZEN DAC", "ZEN DAC 2"]);
    }
}
