use std::path::{Path, PathBuf};

use tracing::instrument;
use zbus::{Connection, zvariant::OwnedObjectPath};

use crate::{error::Error, proxy::device::DeviceProxy};

pub(super) struct DeviceController;

/// Returns the `charge_types` sysfs path for a device given its UPower
/// `NativePath` (e.g. `BAT0` or `/sys/devices/.../power_supply/BAT0`), or `None` if the
/// file does not exist (meaning the kernel uses numeric thresholds instead).
pub(crate) fn charge_types_path(native_path: &str) -> Option<PathBuf> {
    if !native_path.is_empty() {
        let base = Path::new(native_path);
        let path = if base.is_absolute() {
            base.join("charge_types")
        } else {
            Path::new("/sys/class/power_supply").join(base).join("charge_types")
        };
        if path.exists() {
            return Some(path);
        }
    }

    // Fallback: look for any power_supply device that exposes `charge_types`
    if let Ok(entries) = std::fs::read_dir("/sys/class/power_supply") {
        for entry in entries.flatten() {
            let candidate = entry.path().join("charge_types");
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

/// Reads the currently active mode from a `charge_types` file (e.g. `Fast Standard [Long_Life]`).
/// Returns `Some(true)` if `[Long_Life]` is active, `Some(false)` if `[Standard]` is active,
/// or `None` if unrecognized.
pub(crate) fn read_charge_types_enabled(path: &Path) -> Option<bool> {
    let content = std::fs::read_to_string(path).ok()?;
    if content.contains("[Long_Life]") {
        Some(true)
    } else if content.contains("[Standard]") {
        Some(false)
    } else {
        None
    }
}

impl DeviceController {
    #[instrument(skip(connection), err)]
    pub(super) async fn refresh(
        connection: &Connection,
        device_path: &OwnedObjectPath,
    ) -> Result<(), Error> {
        let proxy = DeviceProxy::builder(connection)
            .path(device_path)?
            .build()
            .await?;

        proxy.refresh().await?;
        Ok(())
    }

    #[instrument(skip(connection), err)]
    pub(super) async fn get_history(
        connection: &Connection,
        device_path: &OwnedObjectPath,
        history_type: &str,
        timespan: u32,
        resolution: u32,
    ) -> Result<Vec<(u32, f64, u32)>, Error> {
        let proxy = DeviceProxy::builder(connection)
            .path(device_path)?
            .build()
            .await?;

        Ok(proxy
            .get_history(history_type, timespan, resolution)
            .await?)
    }

    #[instrument(skip(connection), err)]
    pub(super) async fn get_statistics(
        connection: &Connection,
        device_path: &OwnedObjectPath,
        stat_type: &str,
    ) -> Result<Vec<(f64, f64)>, Error> {
        let proxy = DeviceProxy::builder(connection)
            .path(device_path)?
            .build()
            .await?;

        Ok(proxy.get_statistics(stat_type).await?)
    }

    /// Enables or disables the battery charge threshold.
    ///
    /// Tries the standard UPower D-Bus method first. If the device does not
    /// support that interface (e.g. Lenovo/HP/Dell laptops that expose a
    /// `charge_types` sysfs file instead of numeric thresholds), falls back to
    /// writing `Long_Life` or `Standard` directly to the sysfs attribute.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Dbus`] if UPower reports the device is unsupported and
    /// no `charge_types` sysfs file exists. Returns [`Error::Sysfs`] if the
    /// file exists but the write fails (e.g. insufficient permissions).
    #[instrument(skip(connection), err)]
    pub(super) async fn enable_charge_threshold(
        connection: &Connection,
        device_path: &OwnedObjectPath,
        enabled: bool,
    ) -> Result<(), Error> {
        let proxy = DeviceProxy::builder(connection)
            .path(device_path)?
            .build()
            .await?;

        match proxy.enable_charge_threshold(enabled).await {
            Ok(()) => return Ok(()),
            Err(e) => tracing::debug!(
                error = %e,
                "UPower enable_charge_threshold failed, attempting sysfs fallback"
            ),
        }

        // Obtain the kernel sysfs path for this device from UPower's
        // NativePath property (e.g. BAT0 or /sys/devices/.../power_supply/BAT0).
        let native_path = proxy.native_path().await.unwrap_or_default();

        if let Some(path) = charge_types_path(&native_path) {
            let mode = if enabled { "Long_Life" } else { "Standard" };
            tracing::debug!(path = %path.display(), mode, "writing charge_types via sysfs");
            std::fs::write(&path, mode).map_err(Error::Sysfs)?;
            return Ok(());
        }

        Err(Error::Dbus(zbus::Error::Failure(
            "charge threshold is not supported on this device".into(),
        )))
    }
}
