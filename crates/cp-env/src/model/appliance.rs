//! Appliance gates: every tool the orchestrator may drive on the box.

use std::path::PathBuf;

use crate::resolve::Raw;

/// Appliance gates. Each `Option` is a gate: `None` means the matching
/// integration stays inert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Appliance {
    /// Caddyfile the orchestrator regenerates.
    pub caddyfile: Option<PathBuf>,
    /// Caddy binary for reloads.
    pub caddy_bin: Option<PathBuf>,
    /// Root certificate of the private CA.
    pub ca_root: Option<PathBuf>,
    /// `nmcli` - master gate of the network applier.
    pub nmcli_bin: Option<PathBuf>,
    /// `mmcli` - modem status.
    pub mmcli_bin: Option<PathBuf>,
    /// `iw` - radio capabilities and regulatory domain.
    pub iw_bin: Option<PathBuf>,
    /// `ip` - interface addresses.
    pub ip_bin: Option<PathBuf>,
    /// `networkctl` - networkd reload.
    pub networkctl_bin: Option<PathBuf>,
    /// `systemctl` - supervisor restart.
    pub systemctl_bin: Option<PathBuf>,
    /// `nft` - NAT rules.
    pub nft_bin: Option<PathBuf>,
    /// `cp-regdom` - Wi-Fi country script.
    pub regdom_bin: Option<PathBuf>,
    /// systemd-networkd units directory.
    pub networkd_dir: Option<PathBuf>,
    /// Environment file of the uplink supervisor.
    pub uplink_env: Option<PathBuf>,
    /// State file of the uplink supervisor.
    pub uplink_state: PathBuf,
    /// Applied-network marker.
    pub network_applied: PathBuf,
    /// Ethernet uplink port.
    pub wan_iface: String,
    /// Access-point radio.
    pub ap_iface: String,
    /// Modem control device.
    pub wwan_dev: String,
    /// Forced modem presence, when overridden.
    pub wwan_present: Option<bool>,
}

impl Appliance {
    /// From the validated values.
    pub(crate) fn from_raw(raw: &Raw) -> Self {
        Self {
            caddyfile: raw.path("CP_CADDYFILE"),
            caddy_bin: raw.path("CP_CADDY_BIN"),
            ca_root: raw.path("CP_CA_ROOT"),
            nmcli_bin: raw.path("CP_NMCLI_BIN"),
            mmcli_bin: raw.path("CP_MMCLI_BIN"),
            iw_bin: raw.path("CP_IW_BIN"),
            ip_bin: raw.path("CP_IP_BIN"),
            networkctl_bin: raw.path("CP_NETWORKCTL_BIN"),
            systemctl_bin: raw.path("CP_SYSTEMCTL_BIN"),
            nft_bin: raw.path("CP_NFT_BIN"),
            regdom_bin: raw.path("CP_REGDOM_BIN"),
            networkd_dir: raw.path("CP_NETWORKD_DIR"),
            uplink_env: raw.path("CP_UPLINK_ENV"),
            uplink_state: raw.path("CP_UPLINK_STATE").unwrap_or_default(),
            network_applied: raw.path("CP_NETWORK_APPLIED").unwrap_or_default(),
            wan_iface: raw.text("CP_WAN_IFACE").unwrap_or_default().to_owned(),
            ap_iface: raw.text("CP_AP_IFACE").unwrap_or_default().to_owned(),
            wwan_dev: raw.text("CP_WWAN_DEV").unwrap_or_default().to_owned(),
            wwan_present: raw.flag("CP_WWAN_PRESENT"),
        }
    }
}
