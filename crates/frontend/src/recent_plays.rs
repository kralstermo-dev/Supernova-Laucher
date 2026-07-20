use std::ffi::OsString;

use bridge::{
    handle::BackendHandle,
    instance::{InstanceServerSummary, InstanceWorldSummary},
    message::QuickPlayLaunch,
};
use gpui::{App, Entity, SharedString, Window};
use gpui_component::WindowExt;

use crate::{
    entity::instance::InstanceEntries,
    interface_config::{InterfaceConfig, QuickPlayEntry, QuickPlayKind},
    root,
};

fn matches(a: &QuickPlayEntry, instance_name: &SharedString, kind: QuickPlayKind, target: &str) -> bool {
    a.instance_name == *instance_name && a.kind == kind && a.target.as_ref() == target
}

/// Records (or bumps) a world in the recent-plays list. Call this right before
/// actually launching, using the same summary data already on hand in the UI.
pub fn record_world_played(instance_name: SharedString, summary: &InstanceWorldSummary, cx: &mut App) {
    let target = summary.level_path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();

    record_played(QuickPlayEntry {
        instance_name,
        kind: QuickPlayKind::World,
        title: summary.title.to_string().into(),
        subtitle: summary.subtitle.to_string().into(),
        target: target.into(),
        last_played: chrono::Utc::now().timestamp_millis(),
    }, cx);
}

/// Records (or bumps) a server in the recent-plays list.
pub fn record_server_played(instance_name: SharedString, summary: &InstanceServerSummary, cx: &mut App) {
    record_played(QuickPlayEntry {
        instance_name,
        kind: QuickPlayKind::Server,
        title: summary.name.to_string().into(),
        subtitle: summary.ip.to_string().into(),
        target: summary.ip.to_string().into(),
        last_played: chrono::Utc::now().timestamp_millis(),
    }, cx);
}

/// Records a pre-built entry as just-played (stamps the current time). Useful
/// when the caller already has a `QuickPlayEntry` snapshot on hand (e.g. built
/// once outside a 'static on_click closure) rather than the raw summary types.
pub fn record_entry_played(mut entry: QuickPlayEntry, cx: &mut App) {
    entry.last_played = chrono::Utc::now().timestamp_millis();
    record_played(entry, cx);
}

fn record_played(entry: QuickPlayEntry, cx: &mut App) {
    let cfg = InterfaceConfig::get_mut(cx);
    cfg.recent_plays.retain(|e| !matches(e, &entry.instance_name, entry.kind, entry.target.as_ref()));
    cfg.recent_plays.insert(0, entry);
    cfg.recent_plays.truncate(12);
}

/// Whether this exact world/server is currently pinned.
pub fn is_pinned(instance_name: &SharedString, kind: QuickPlayKind, target: &str, cx: &App) -> bool {
    InterfaceConfig::get(cx).pinned_plays.iter().any(|e| matches(e, instance_name, kind, target))
}

/// Pins or unpins a world/server, keyed by (instance_name, kind, target).
pub fn toggle_pin(entry: QuickPlayEntry, cx: &mut App) {
    let cfg = InterfaceConfig::get_mut(cx);
    if let Some(pos) = cfg.pinned_plays.iter().position(|e| matches(e, &entry.instance_name, entry.kind, entry.target.as_ref())) {
        cfg.pinned_plays.remove(pos);
    } else {
        cfg.pinned_plays.push(entry);
    }
}

pub fn unpin(instance_name: &SharedString, kind: QuickPlayKind, target: &str, cx: &mut App) {
    let cfg = InterfaceConfig::get_mut(cx);
    cfg.pinned_plays.retain(|e| !matches(e, instance_name, kind, target));
}

/// Resolves a saved entry's instance name back to a live `InstanceID` (instance
/// IDs aren't stable across restarts) and launches it with the right quickplay
/// target, bumping recency in the process. No-ops with an error notification if
/// the instance no longer exists.
pub fn launch_entry(
    entry: &QuickPlayEntry,
    instances: &Entity<InstanceEntries>,
    backend_handle: &BackendHandle,
    window: &mut Window,
    cx: &mut App,
) {
    let found = instances.read(cx).entries.values().find_map(|e| {
        let e = e.read(cx);
        (e.name == entry.instance_name).then(|| (e.id, e.name.clone()))
    });

    let Some((id, name)) = found else {
        let notification: gpui_component::notification::Notification = (
            gpui_component::notification::NotificationType::Error,
            SharedString::from(format!("Instance \"{}\" no longer exists", entry.instance_name)),
        ).into();
        window.push_notification(notification, cx);
        return;
    };

    let target = OsString::from(entry.target.to_string());
    let quick_play = match entry.kind {
        QuickPlayKind::World => QuickPlayLaunch::Singleplayer(target),
        QuickPlayKind::Server => QuickPlayLaunch::Multiplayer(target),
    };

    root::start_instance(id, name, Some(quick_play), backend_handle, window, cx);

    record_played(entry.clone(), cx);
}
