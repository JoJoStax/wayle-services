use zbus::{proxy, zvariant::OwnedObjectPath};

#[proxy(
    interface = "org.freedesktop.UPower",
    default_service = "org.freedesktop.UPower",
    default_path = "/org/freedesktop/UPower"
)]
pub(crate) trait UPower {
    async fn enumerate_devices(&self) -> zbus::Result<Vec<OwnedObjectPath>>;
    async fn get_display_device(&self) -> zbus::Result<OwnedObjectPath>;
}
