//! The DoP safety rule, in one place.
//!
//! DoP is DSD dressed as PCM. A device that does not decode it (built-in
//! audio, PCM-only DACs) plays it as loud noise, and any attenuation breaks
//! its markers. So DoP only reaches a device known to decode it
//! ([`OutputFormat::decodes_dop`]: it exposes native DSD, or the user said
//! so) whose volume leaves the samples alone ([`HardwareVolume::passes_dop`]),
//! and that volume must not move while DoP plays. Native DSD has neither
//! risk.

use crate::sink::{Encoding, HardwareVolume, OutputFormat};
use crate::status::{ErrorKind, PlayerError, format_db};

/// Why the volume control is locked while DoP plays.
pub(crate) const VOLUME_LOCKED: &str = "Locked at 0 dB during DoP";

pub(crate) fn is_dop(output: &OutputFormat) -> bool {
    matches!(output.encoding, Encoding::Dop(_))
}

/// Refuses a device just opened for DoP that may not decode it, or whose
/// volume may not pass it intact.
pub(crate) fn check_open(output: &OutputFormat) -> Result<(), PlayerError> {
    if !is_dop(output) {
        return Ok(());
    }
    if !output.decodes_dop {
        return Err(unsupported(
            "Lyra cannot tell whether this DAC decodes DSD over DoP; if it does not, \
             it would play loud noise. If its manual says it supports DoP, turn on \
             “Plays DoP” for it in Settings. Or turn on “Convert DSD to PCM” \
             (not bit-perfect)."
                .to_owned(),
        ));
    }
    if output.hardware_volume.passes_dop() {
        return Ok(());
    }
    Err(unsupported(match output.hardware_volume {
        HardwareVolume::Attenuated { centibels } => format!(
            "DSD over DoP needs the DAC's volume at 0 dB; it is at {} dB. \
             Raise it to 100% with Lyra's volume control under Output and play again.",
            format_db(centibels)
        ),
        _ => "DSD over DoP is only sent to a USB DAC whose volume Lyra can check. \
              This device is not one: if it does not decode DoP (built-in audio cannot), \
              it would play loud noise. To play DSD here, turn on \
              “Convert DSD to PCM” in Settings (not bit-perfect)."
            .to_owned(),
    }))
}

/// Stops DoP when the device's volume moves to `volume` during playback.
pub(crate) fn check_change(
    output: &OutputFormat,
    volume: HardwareVolume,
) -> Result<(), PlayerError> {
    if !is_dop(output) || volume.passes_dop() {
        return Ok(());
    }
    Err(unsupported(
        "Playback stopped: the DAC's volume moved during DoP, which turns \
         the DSD into noise. Set it back to 100% and play again."
            .to_owned(),
    ))
}

fn unsupported(message: String) -> PlayerError {
    PlayerError {
        kind: ErrorKind::Unsupported,
        message,
    }
}
