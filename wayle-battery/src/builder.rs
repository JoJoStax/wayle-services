use tokio_util::sync::CancellationToken;
use tracing::instrument;
use wayle_traits::Reactive;
use zbus::{Connection, zvariant::OwnedObjectPath};

use crate::{
    core::device::{Device, types::LiveDeviceParams},
    error::Error,
    service::BatteryService,
};

/// Builder for configuring a BatteryService.
pub struct BatteryServiceBuilder {
    device_path: Option<OwnedObjectPath>,
}

impl BatteryServiceBuilder {
    /// Creates a new builder with default configuration.
    pub fn new() -> Self {
        Self { device_path: None }
    }

    /// Sets a specific UPower device path.
    ///
    /// If not set, defaults to the DisplayDevice which aggregates all batteries.
    ///
    /// # Arguments
    /// * `path` - D-Bus path to the UPower device (e.g., "/org/freedesktop/UPower/devices/battery_BAT0")
    pub fn device_path(mut self, path: impl Into<OwnedObjectPath>) -> Self {
        self.device_path = Some(path.into());
        self
    }

    async fn find_battery_device(connection: &Connection) -> Option<OwnedObjectPath> {
        let upower = crate::proxy::upower::UPowerProxy::builder(connection)
            .build()
            .await
            .ok()?;
        let devices = upower.enumerate_devices().await.ok()?;

        for path in devices {
            let Ok(builder) = crate::proxy::device::DeviceProxy::builder(connection).path(&path)
            else {
                continue;
            };
            let Ok(proxy) = builder.build().await else {
                continue;
            };

            let is_battery = proxy.device_type().await.map(|t| t == 2).unwrap_or(false);
            let is_power_supply = proxy.power_supply().await.unwrap_or(false);
            let is_present = proxy.is_present().await.unwrap_or(false);

            if is_battery && is_power_supply && is_present {
                return Some(path);
            }
        }
        None
    }

    /// Builds the BatteryService.
    ///
    /// Auto-detects the primary battery device from UPower. If none is found,
    /// falls back to the DisplayDevice which aggregates power status.
    ///
    /// # Errors
    ///
    /// Returns `Error::InvalidObjectPath` if the device path is invalid, or
    /// `Error::Dbus` if D-Bus connection fails.
    #[instrument(skip_all)]
    pub async fn build(self) -> Result<BatteryService, Error> {
        let connection = Connection::system().await?;
        let cancellation_token = CancellationToken::new();

        let device_path = if let Some(path) = self.device_path {
            path
        } else if let Some(path) = Self::find_battery_device(&connection).await {
            path
        } else {
            OwnedObjectPath::try_from("/org/freedesktop/UPower/devices/DisplayDevice")?
        };

        let device = Device::get_live(LiveDeviceParams {
            connection: &connection,
            device_path: &device_path,
            cancellation_token: &cancellation_token,
        })
        .await?;

        Ok(BatteryService {
            device,
            cancellation_token,
        })
    }
}

impl Default for BatteryServiceBuilder {
    fn default() -> Self {
        Self::new()
    }
}
