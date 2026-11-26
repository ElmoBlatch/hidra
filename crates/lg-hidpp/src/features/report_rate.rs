//! Adjustable report rate (0x8060).
use crate::{Device, Result, Transport};

pub const PAGE: u16 = 0x8060;

/// Supported polling rates in Hz, from the bitmask where bit n means an
/// (n+1) ms interval.
pub fn supported_rates_hz<T: Transport>(dev: &mut Device<T>) -> Result<Vec<u16>> {
    let idx = dev.require(PAGE)?;
    let mask = dev.call(idx, 0, &[0, 0, 0])?[0];
    let mut rates: Vec<u16> = (0..8).filter(|b| mask & (1 << b) != 0).map(|b| 1000 / (b + 1)).collect();
    rates.sort_unstable();
    Ok(rates)
}

/// Current polling rate in Hz.
pub fn current_rate_hz<T: Transport>(dev: &mut Device<T>) -> Result<u16> {
    let idx = dev.require(PAGE)?;
    let ms = dev.call(idx, 1, &[0, 0, 0])?[0];
    Ok(if ms == 0 { 0 } else { 1000 / ms as u16 })
}
