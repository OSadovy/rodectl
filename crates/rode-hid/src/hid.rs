//! Finding devices and opening them through hidapi.

use std::ffi::CString;
use std::time::Duration;

use hidapi::{HidApi, HidDevice};

use crate::link::{Link, Transport};
use crate::protocol::{USAGE_PAGE, VENDOR_ID};
use crate::{DeviceKind, Error, Power, Result};

/// A RØDE vendor HID interface found on USB.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// The model.
    pub kind: DeviceKind,
    /// USB product id.
    pub product_id: u16,
    /// USB serial number.
    pub serial: Option<String>,
    /// USB device release number.
    pub release: u16,
    /// For a Wireless ME receiver, whether it is switched on.
    pub power: Option<Power>,
    /// The platform's path to the interface.
    pub path: CString,
}

/// Access to the system's HID devices.
pub struct Hid {
    api: HidApi,
}

impl Hid {
    /// Enumerates HID devices.
    ///
    /// # Errors
    /// When the platform's HID layer cannot be initialised.
    pub fn new() -> Result<Self> {
        let api = HidApi::new().map_err(Error::transport)?;
        Ok(Self { api })
    }

    /// Enumerates again, to notice devices plugged in or out.
    ///
    /// # Errors
    /// When enumeration fails.
    pub fn refresh(&mut self) -> Result<()> {
        self.api.refresh_devices().map_err(Error::transport)
    }

    /// RØDE's vendor interfaces, one per device.
    #[must_use]
    pub fn devices(&self) -> Vec<DeviceInfo> {
        self.api
            .device_list()
            .filter(|d| d.vendor_id() == VENDOR_ID && d.usage_page() == USAGE_PAGE)
            .map(|d| {
                let kind = DeviceKind::from_product_id(d.product_id());
                DeviceInfo {
                    kind,
                    product_id: d.product_id(),
                    serial: d
                        .serial_number()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned),
                    release: d.release_number(),
                    power: (kind == DeviceKind::WirelessMeRx).then(|| power(d.path())),
                    path: d.path().to_owned(),
                }
            })
            .collect()
    }

    /// Opens `device`, shared with any other program that has it open.
    ///
    /// # Errors
    /// When the device is gone or cannot be opened.
    pub fn open(&self, device: &DeviceInfo) -> Result<HidTransport> {
        let device = self.api.open_path(&device.path).map_err(Error::transport)?;
        Ok(HidTransport(device))
    }

    /// Opens `device` and wraps it in a register link.
    ///
    /// # Errors
    /// When the device is unsupported, gone or cannot be opened.
    pub fn link(&self, device: &DeviceInfo, timeout: Duration) -> Result<Link<HidTransport>> {
        let family = device
            .kind
            .family()
            .ok_or_else(|| Error::OutOfRange(format!("no register map for {}", device.kind)))?;
        Ok(Link::new(self.open(device)?, family, timeout))
    }
}

/// A switched-off receiver on Windows is a plain HID device; switched on, it
/// is a composite device whose HID interface is `MI_00`.
#[cfg(windows)]
fn power(path: &std::ffi::CStr) -> Power {
    if path.to_string_lossy().to_ascii_lowercase().contains("&mi_") {
        Power::On
    } else {
        Power::Off
    }
}

#[cfg(not(windows))]
fn power(_path: &std::ffi::CStr) -> Power {
    Power::Unknown
}

/// An open HID interface.
pub struct HidTransport(HidDevice);

impl Transport for HidTransport {
    fn send(&mut self, report: &[u8]) -> Result<()> {
        self.0.write(report).map(drop).map_err(Error::transport)
    }

    fn receive(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        let mut buf = [0; 2048];
        let millis = i32::try_from(timeout.as_micros().div_ceil(1000)).unwrap_or(i32::MAX);
        let len = self
            .0
            .read_timeout(&mut buf, millis)
            .map_err(Error::transport)?;
        Ok((len > 0).then(|| buf[..len].to_vec()))
    }
}
