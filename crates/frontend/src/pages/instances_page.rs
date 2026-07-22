use bridge::{handle::BackendHandle, message::MessageToBackend};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme as _, IndexPath, Sizable, StyledExt, button::{Button, ButtonVariants}, h_flex, select::{Select, SelectDelegate, SelectEvent, SelectItem, SelectState}, table::{DataTable, TableDelegate, TableState}, v_flex
};
use strum::IntoEnumIterator;

use crate::{
    component::{instance_list::InstanceList, named_dropdown::{NamedDropdown, NamedDropdownItem}, responsive_grid::ResponsiveGrid}, entity::{DataEntities, instance::InstanceEntries, metadata::FrontendMetadata}, icon::PandoraIcon, interface_config::{InstancesViewMode, InterfaceConfig, QuickPlayEntry, QuickPlayKind}, pages::page::Page, png_render_cache, recent_plays,
};

pub struct InstancesPage {
    instance_table: Entity<TableState<InstanceList>>,
    view_dropdown: Entity<SelectState<NamedDropdown<InstancesViewMode>>>,

    metadata: Entity<FrontendMetadata>,
    instances: Entity<InstanceEntries>,

    backend_handle: BackendHandle,
    last_world_refresh: Option<std::time::Instant>,
}

impl InstancesPage {
    pub fn new(data: &DataEntities, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let instance_table = InstanceList::create_table(data, window, cx);
        let view_dropdown = cx.new(|cx| {
            let items = InstancesViewMode::iter().map(|view| {
                NamedDropdownItem { name: view.name(), item: view }
            }).collect::<Vec<_>>();
            let current_view = InterfaceConfig::get(cx).instances_view_mode;
            let row = items.iter().position(|v| v.item == current_view).unwrap_or(0);
            let delegate = NamedDropdown::new(items);
            SelectState::new(delegate, Some(IndexPath::new(row)), window, cx)
        });
        cx.subscribe(&view_dropdown, |_, _, event: &SelectEvent<NamedDropdown<InstancesViewMode>>, cx| {
            let SelectEvent::Confirm(Some(value)) = event else {
                return;
            };
            let view = value.item;

            InterfaceConfig::get_mut(cx).instances_view_mode = view;
        }).detach();

        // Load world lists for every instance (not just the one currently being
        // viewed) so the "Recently Played" bar can use the real last-played
        // timestamp from each world's save data, catching worlds launched any
        // way (not just via the Quickplay tab).
        let backend_handle = data.backend_handle.clone();
        let instance_worlds: Vec<_> = data.instances.read(cx).entries.values().map(|entry| {
            let entry = entry.read(cx);
            (entry.id, entry.worlds_state.clone(), entry.worlds.clone())
        }).collect();

        for (id, worlds_state, worlds) in instance_worlds {
            worlds_state.set_observed();
            if worlds_state.should_load() {
                backend_handle.send(MessageToBackend::RequestLoadWorlds { id });
            }

            cx.observe(&worlds, |_, _, cx| cx.notify()).detach();
        }

        Self {
            instance_table,
            view_dropdown,
            metadata: data.metadata.clone(),
            instances: data.instances.clone(),
            backend_handle: data.backend_handle.clone(),
            last_world_refresh: None,
        }
    }
}

impl InstancesPage {
    fn make_quickplay_card(
        &self,
        id_prefix: &'static str,
        index: usize,
        entry: QuickPlayEntry,
        icon_bytes: Option<schema::unique_bytes::UniqueBytes>,
        is_pinned: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let icon = if let Some(bytes) = icon_bytes {
            png_render_cache::render(bytes, cx)
        } else {
            gpui::img(ImageSource::Resource(gpui::Resource::Embedded("images/default_world.png".into())))
        };

        let instances = self.instances.clone();
        let backend_handle = self.backend_handle.clone();
        let play_entry = entry.clone();
        let toggle_entry = entry.clone();

        v_flex()
            .w_40()
            .gap_1()
            .p_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .justify_between()
                    .items_start()
                    .child(icon.size_10().min_w_10().min_h_10().rounded(cx.theme().radius))
                    .child(
                        Button::new((SharedString::from(format!("{id_prefix}-pin")), index))
                            .ghost()
                            .small()
                            .icon(if is_pinned { PandoraIcon::StarOff } else { PandoraIcon::Star })
                            .on_click(cx.listener(move |_, _, _, cx| {
                                if is_pinned {
                                    recent_plays::unpin(&toggle_entry.instance_name, toggle_entry.kind, toggle_entry.target.as_ref(), cx);
                                } else {
                                    recent_plays::toggle_pin(toggle_entry.clone(), cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(div().text_sm().font_medium().truncate().child(entry.title.clone()))
            .child(div().text_xs().text_color(cx.theme().muted_foreground).truncate().child(entry.instance_name.clone()))
            .child(
                Button::new((SharedString::from(format!("{id_prefix}-play")), index))
                    .success()
                    .small()
                    .w_full()
                    .icon(PandoraIcon::Play)
                    .label("Play")
                    .on_click(move |_, window, cx| {
                        recent_plays::launch_entry(&play_entry, &instances, &backend_handle, window, cx);
                    }),
            )
            .into_any_element()
    }

    fn render_quickplay_row(
        &self,
        id_prefix: &'static str,
        label: impl Into<SharedString>,
        entries: Vec<(QuickPlayEntry, Option<schema::unique_bytes::UniqueBytes>)>,
        is_pinned: bool,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if entries.is_empty() {
            return None;
        }

        let cards: Vec<AnyElement> = entries
            .into_iter()
            .enumerate()
            .map(|(index, (entry, icon))| self.make_quickplay_card(id_prefix, index, entry, icon, is_pinned, cx))
            .collect();

        Some(
            v_flex()
                .gap_2()
                .child(div().text_sm().font_medium().text_color(cx.theme().muted_foreground).child(label.into()))
                .child(h_flex().id(id_prefix).gap_3().flex_wrap().children(cards))
                .into_any_element(),
        )
    }

    /// Re-requests world data for every instance that isn't currently loading.
    /// Called once from `new()` and again (throttled) from every `render()`,
    /// since pages are cached/reused rather than reconstructed on each visit —
    /// without this, a world played after the home page was first shown would
    /// never refresh here until the app restarted.
    fn refresh_world_data(&self, cx: &App) {
        for entry in self.instances.read(cx).entries.values() {
            let entry = entry.read(cx);
            let id = entry.id;
            entry.worlds_state.set_observed();
            if entry.worlds_state.should_load() {
                self.backend_handle.send(MessageToBackend::RequestLoadWorlds { id });
            }
        }
    }

    fn render_quickplay_bar(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let now = std::time::Instant::now();
        let should_refresh = self.last_world_refresh.map_or(true, |t| now.duration_since(t).as_secs() >= 3);
        if should_refresh {
            self.refresh_world_data(cx);
            self.last_world_refresh = Some(now);
        }

        let cfg = InterfaceConfig::get(cx);
        let pinned = cfg.pinned_plays.clone();

        // Servers have no backend-tracked "last connected" time, so they can
        // only come from what we've recorded ourselves (Quickplay tab or this
        // bar's own Play button).
        let recorded_servers = cfg.recent_plays.clone();

        // Worlds, on the other hand, have a real last-played timestamp coming
        // from the save file itself, independent of how they were launched —
        // use that instead of relying purely on frontend tracking, and keep
        // the real icon bytes for display since these aren't persisted.
        let mut recent: Vec<(QuickPlayEntry, Option<schema::unique_bytes::UniqueBytes>)> = Vec::new();
        for entry in self.instances.read(cx).entries.values() {
            let entry = entry.read(cx);
            let instance_name = entry.name.clone();
            for world in entry.worlds.read(cx).iter() {
                if world.last_played <= 0 {
                    continue;
                }
                let Some(target) = world.level_path.file_name() else { continue };
                recent.push((
                    QuickPlayEntry {
                        instance_name: instance_name.clone(),
                        kind: QuickPlayKind::World,
                        title: world.title.to_string().into(),
                        subtitle: world.subtitle.to_string().into(),
                        target: target.to_string_lossy().to_string().into(),
                        last_played: world.last_played,
                    },
                    world.png_icon.clone(),
                ));
            }
        }
        recent.extend(recorded_servers.into_iter().filter(|e| e.kind == QuickPlayKind::Server).map(|e| (e, None)));

        recent.sort_by_key(|(e, _)| std::cmp::Reverse(e.last_played));
        recent.retain(|(r, _)| {
            !pinned.iter().any(|p| p.instance_name == r.instance_name && p.kind == r.kind && p.target == r.target)
        });
        // A world/server could show up more than once if it was both backend-observed
        // and frontend-recorded; keep the first (most recent) occurrence only.
        let mut seen: Vec<(SharedString, QuickPlayKind, SharedString)> = Vec::new();
        recent.retain(|(e, _)| {
            let key = (e.instance_name.clone(), e.kind, e.target.clone());
            if seen.contains(&key) {
                false
            } else {
                seen.push(key);
                true
            }
        });
        recent.truncate(3);

        let pinned_with_icons: Vec<_> = pinned.into_iter().map(|e| (e, None)).collect();

        let pinned_row = self.render_quickplay_row("pinned-row", "Pinned", pinned_with_icons, true, cx);
        let recent_row = self.render_quickplay_row("recent-row", "Recently Played", recent, false, cx);

        if pinned_row.is_none() && recent_row.is_none() {
            return None;
        }

        Some(
            v_flex()
                .gap_4()
                .p_4()
                .border_t_1()
                .border_color(cx.theme().border)
                .when_some(recent_row, |this, row| this.child(row))
                .when_some(pinned_row, |this, row| this.child(row))
                .into_any_element(),
        )
    }
}

impl Page for InstancesPage {
    fn controls(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let create_instance = Button::new("create_instance")
            .success()
            .icon(PandoraIcon::Plus)
            .label(t::instance::create())
            .on_click(cx.listener(|this, _, window, cx| {
                crate::modals::create_instance::open_create_instance(this.metadata.clone(), this.instances.clone(),
                    this.backend_handle.clone(), window, cx);
            }));
        // wrapping in div makes it not take up the full space of the titlebar
        let select_view = div()
            .child(Select::new(&self.view_dropdown).title_prefix(format!("{}: ", t::instance::view_mode())));

        h_flex().gap_3().child(create_instance).child(select_view)
    }

    fn scrollable(&self, cx: &App) -> bool {
        match InterfaceConfig::get(cx).instances_view_mode {
            InstancesViewMode::Cards => true,
            InstancesViewMode::List => false,
        }
    }
}

impl Render for InstancesPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let bar = self.render_quickplay_bar(cx);

        let body = match InterfaceConfig::get(cx).instances_view_mode {
            InstancesViewMode::Cards => {
                let cards = self.instance_table.update(cx, |table, cx| {
                    let rows = table.delegate().rows_count(cx);
                    (0..rows).map(|i| table.delegate().render_card(i, cx)).collect::<Vec<_>>()
                });

                let size = Size::new(
                    gpui::AvailableSpace::MinContent,
                    gpui::AvailableSpace::MinContent
                );

                div().p_4().child(ResponsiveGrid::new(size).size_full().gap_4().children(cards)).into_any_element()
            },
            InstancesViewMode::List => {
                DataTable::new(&self.instance_table).bordered(false).into_any_element()
            },
        };

        v_flex().size_full().child(div().flex_1().child(body)).when_some(bar, |this, bar| this.child(bar))
    }
}

#[derive(Default)]
pub struct VersionList {
    pub versions: Vec<SharedString>,
    pub matched_versions: Vec<SharedString>,
}

impl SelectDelegate for VersionList {
    type Item = SharedString;

    fn items_count(&self, _section: usize) -> usize {
        self.matched_versions.len()
    }

    fn item(&self, ix: IndexPath) -> Option<&Self::Item> {
        self.matched_versions.get(ix.row)
    }

    fn position<V>(&self, value: &V) -> Option<IndexPath>
    where
        Self::Item: gpui_component::select::SelectItem<Value = V>,
        V: PartialEq,
    {
        for (ix, item) in self.matched_versions.iter().enumerate() {
            if item.value() == value {
                return Some(IndexPath::default().row(ix));
            }
        }

        None
    }

    fn perform_search(&mut self, query: &str, _window: &mut Window, _: &mut Context<SelectState<Self>>) -> Task<()> {
        let lower_query = query.to_lowercase();

        self.matched_versions = self
            .versions
            .iter()
            .filter(|item| item.to_lowercase().starts_with(&lower_query))
            .cloned()
            .collect();

        Task::ready(())
    }
}
