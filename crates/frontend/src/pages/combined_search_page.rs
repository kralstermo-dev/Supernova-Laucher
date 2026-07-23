use std::sync::Arc;

use bridge::{instance::InstanceID, meta::MetadataRequest};
use gpui::{prelude::*, *};
use gpui_component::{
    ActiveTheme, Disableable, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    skeleton::Skeleton,
    v_flex,
};
use schema::curseforge::{CurseforgeClassId, CurseforgeHit, CurseforgeSearchRequest, CurseforgeSearchResult, CurseforgeSortField};
use schema::modrinth::{ModrinthHit, ModrinthSearchIndex, ModrinthSearchRequest, ModrinthSearchResult};

use crate::{
    entity::{DataEntities, metadata::{AsMetadataResult, FrontendMetadata, FrontendMetadataResult}},
    format_downloads,
    icon::PandoraIcon,
    modals::{curseforge_install, modrinth_install},
    pages::page::Page,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ContentKind {
    Mod,
    ResourcePack,
    Shader,
    Modpack,
    Plugin,
}

impl ContentKind {
    const ALL: [ContentKind; 5] = [Self::Mod, Self::ResourcePack, Self::Shader, Self::Modpack, Self::Plugin];

    fn label(self) -> &'static str {
        match self {
            Self::Mod => "Mods",
            Self::ResourcePack => "Resource Packs",
            Self::Shader => "Shaders",
            Self::Modpack => "Modpacks",
            Self::Plugin => "Plugins",
        }
    }

    /// `None` means this content kind isn't queried against Modrinth at all
    /// (Modrinth doesn't expose Bukkit/Spigot plugins as a distinct project
    /// type the way CurseForge does).
    fn modrinth_project_type(self) -> Option<&'static str> {
        match self {
            Self::Mod => Some("mod"),
            Self::ResourcePack => Some("resourcepack"),
            Self::Shader => Some("shader"),
            Self::Modpack => Some("modpack"),
            Self::Plugin => None,
        }
    }

    fn curseforge_class_id(self) -> u32 {
        match self {
            Self::Mod => CurseforgeClassId::Mod as u32,
            Self::ResourcePack => CurseforgeClassId::Resourcepack as u32,
            Self::Shader => CurseforgeClassId::Shader as u32,
            Self::Modpack => CurseforgeClassId::Modpack as u32,
            Self::Plugin => CurseforgeClassId::BukkitPlugin as u32,
        }
    }
}

#[derive(Clone)]
enum CombinedHit {
    Modrinth(ModrinthHit),
    Curseforge(CurseforgeHit),
}

impl CombinedHit {
    fn title(&self) -> SharedString {
        match self {
            Self::Modrinth(hit) => hit.title.as_ref().map(Arc::clone).map(SharedString::new).unwrap_or_else(|| "Unnamed".into()),
            Self::Curseforge(hit) => SharedString::new(hit.name.clone()),
        }
    }

    fn subtitle(&self) -> SharedString {
        match self {
            Self::Modrinth(hit) => hit.description.clone().map(SharedString::new).unwrap_or_default(),
            Self::Curseforge(hit) => SharedString::new(hit.summary.clone()),
        }
    }

    fn icon_url(&self) -> Option<Arc<str>> {
        match self {
            Self::Modrinth(hit) => hit.icon_url.clone(),
            Self::Curseforge(hit) => hit.logo.as_ref().map(|logo| logo.thumbnail_url.clone()),
        }
    }

    fn downloads(&self) -> u64 {
        match self {
            Self::Modrinth(hit) => hit.downloads,
            Self::Curseforge(hit) => hit.download_count,
        }
    }

    fn source_label(&self) -> &'static str {
        match self {
            Self::Modrinth(_) => "Modrinth",
            Self::Curseforge(_) => "CurseForge",
        }
    }
}

pub struct CombinedSearchPage {
    data: DataEntities,
    install_for: Option<InstanceID>,
    search_state: Entity<InputState>,
    _search_input_subscription: Subscription,
    content_kind: ContentKind,
    last_search: Arc<str>,
    modrinth_hits: Vec<ModrinthHit>,
    curseforge_hits: Vec<CurseforgeHit>,
    loading_modrinth: Option<Subscription>,
    loading_curseforge: Option<Subscription>,
    error_modrinth: Option<SharedString>,
    error_curseforge: Option<SharedString>,
    page: usize,
    modrinth_total: usize,
    curseforge_total: usize,
}

const PAGE_SIZE: usize = 20;

impl CombinedSearchPage {
    pub fn new(install_for: Option<InstanceID>, data: &DataEntities, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search_state = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Search mods, resource packs, shaders, modpacks, plugins...").clean_on_escape()
        });

        let _search_input_subscription = cx.subscribe_in(&search_state, window, Self::on_search_input_event);

        let mut page = Self {
            data: data.clone(),
            install_for,
            search_state,
            _search_input_subscription,
            content_kind: ContentKind::Mod,
            last_search: Arc::from(""),
            modrinth_hits: Vec::new(),
            curseforge_hits: Vec::new(),
            loading_modrinth: None,
            loading_curseforge: None,
            error_modrinth: None,
            error_curseforge: None,
            page: 0,
            modrinth_total: 0,
            curseforge_total: 0,
        };
        page.reload(cx);
        page
    }

    fn on_search_input_event(&mut self, entity: &Entity<InputState>, event: &InputEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let InputEvent::PressEnter { secondary: false } = event else {
            return;
        };

        self.last_search = Arc::from(entity.read(cx).value().trim());
        self.reload(cx);
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        self.page = 0;
        self.modrinth_hits.clear();
        self.curseforge_hits.clear();
        self.error_modrinth = None;
        self.error_curseforge = None;
        self.loading_modrinth = None;
        self.loading_curseforge = None;

        self.load_modrinth(cx);
        self.load_curseforge(cx);
    }

    fn go_to_page(&mut self, page: usize, cx: &mut Context<Self>) {
        if page == self.page {
            return;
        }
        self.page = page;
        self.modrinth_hits.clear();
        self.curseforge_hits.clear();
        self.error_modrinth = None;
        self.error_curseforge = None;
        self.loading_modrinth = None;
        self.loading_curseforge = None;

        self.load_modrinth(cx);
        self.load_curseforge(cx);
    }

    /// Whether there's likely another page, based on the totals each source
    /// last reported. Best-effort — if a source hasn't loaded yet this page,
    /// its previous total is used.
    fn has_next_page(&self) -> bool {
        let next_offset = (self.page + 1) * PAGE_SIZE;
        next_offset < self.modrinth_total || next_offset < self.curseforge_total
    }

    fn load_modrinth(&mut self, cx: &mut Context<Self>) {
        let Some(project_type) = self.content_kind.modrinth_project_type() else {
            return;
        };

        let query = if self.last_search.is_empty() { None } else { Some(self.last_search.clone()) };
        let facets = format!("[[\"project_type={project_type}\"]]");

        let request = ModrinthSearchRequest {
            query,
            facets: Some(facets.into()),
            index: ModrinthSearchIndex::Relevance,
            offset: self.page * PAGE_SIZE,
            limit: PAGE_SIZE,
        };

        let data = FrontendMetadata::request(&self.data.metadata, MetadataRequest::ModrinthSearch(request), cx);
        let result: FrontendMetadataResult<ModrinthSearchResult> = data.read(cx).result();
        match result {
            FrontendMetadataResult::Loading => {
                let subscription = cx.observe(&data, |page, data, cx| {
                    let result: FrontendMetadataResult<ModrinthSearchResult> = data.read(cx).result();
                    match result {
                        FrontendMetadataResult::Loading => {},
                        FrontendMetadataResult::Loaded(result) => {
                            page.modrinth_hits = result.hits.to_vec();
                            page.modrinth_total = result.total_hits;
                            page.loading_modrinth = None;
                            cx.notify();
                        },
                        FrontendMetadataResult::Error(err) => {
                            page.error_modrinth = Some(err);
                            page.loading_modrinth = None;
                            cx.notify();
                        },
                    }
                });
                self.loading_modrinth = Some(subscription);
            },
            FrontendMetadataResult::Loaded(result) => {
                self.modrinth_hits = result.hits.to_vec();
                self.modrinth_total = result.total_hits;
            },
            FrontendMetadataResult::Error(err) => {
                self.error_modrinth = Some(err);
            },
        }
    }

    fn load_curseforge(&mut self, cx: &mut Context<Self>) {
        let search_filter = if self.last_search.is_empty() { None } else { Some(Arc::from(self.last_search.as_ref())) };

        let request = CurseforgeSearchRequest {
            class_id: self.content_kind.curseforge_class_id(),
            category_ids: None,
            game_version: None,
            search_filter,
            mod_loader_types: None,
            sort_field: CurseforgeSortField::Popularity as u32,
            index: (self.page * PAGE_SIZE) as u32,
            page_size: PAGE_SIZE as u32,
        };

        let data = FrontendMetadata::request(&self.data.metadata, MetadataRequest::CurseforgeSearch(request), cx);
        let result: FrontendMetadataResult<CurseforgeSearchResult> = data.read(cx).result();
        match result {
            FrontendMetadataResult::Loading => {
                let subscription = cx.observe(&data, |page, data, cx| {
                    let result: FrontendMetadataResult<CurseforgeSearchResult> = data.read(cx).result();
                    match result {
                        FrontendMetadataResult::Loading => {},
                        FrontendMetadataResult::Loaded(result) => {
                            page.curseforge_hits = result.data.to_vec();
                            page.curseforge_total = result.pagination.total_count as usize;
                            page.loading_curseforge = None;
                            cx.notify();
                        },
                        FrontendMetadataResult::Error(err) => {
                            page.error_curseforge = Some(err);
                            page.loading_curseforge = None;
                            cx.notify();
                        },
                    }
                });
                self.loading_curseforge = Some(subscription);
            },
            FrontendMetadataResult::Loaded(result) => {
                self.curseforge_hits = result.data.to_vec();
                self.curseforge_total = result.pagination.total_count as usize;
            },
            FrontendMetadataResult::Error(err) => {
                self.error_curseforge = Some(err);
            },
        }
    }

    fn set_content_kind(&mut self, kind: ContentKind, cx: &mut Context<Self>) {
        if self.content_kind == kind {
            return;
        }
        self.content_kind = kind;
        self.reload(cx);
    }

    fn render_row(&self, hit: CombinedHit, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let image = if let Some(icon_url) = hit.icon_url().filter(|url| !url.is_empty()) {
            gpui::img(SharedUri::from(icon_url))
                .size_16()
                .min_w_16()
                .min_h_16()
                .rounded(cx.theme().radius)
                .with_fallback(|| Skeleton::new().rounded_lg().size_16().into_any_element())
                .into_any_element()
        } else {
            gpui::img(ImageSource::Resource(Resource::Embedded("images/default_mod.png".into())))
                .size_16()
                .min_w_16()
                .min_h_16()
                .rounded(cx.theme().radius)
                .into_any_element()
        };

        let data = self.data.clone();
        let install_for = self.install_for;
        let install_hit = hit.clone();

        h_flex()
            .w_full()
            .gap_3()
            .p_3()
            .items_center()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(image)
            .child(
                v_flex()
                    .flex_1()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(div().text_base().font_medium().child(hit.title()))
                            .child(
                                div()
                                    .text_xs()
                                    .px_2()
                                    .py_0p5()
                                    .rounded(cx.theme().radius)
                                    .bg(cx.theme().secondary)
                                    .text_color(cx.theme().secondary_foreground)
                                    .child(hit.source_label()),
                            ),
                    )
                    .child(div().text_sm().text_color(cx.theme().muted_foreground).truncate().child(hit.subtitle())),
            )
            .child(
                v_flex()
                    .items_end()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_1()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(PandoraIcon::Download)
                            .child(format_downloads(hit.downloads())),
                    )
                    .child(
                        Button::new(("browse-install", index))
                            .success()
                            .small()
                            .icon(PandoraIcon::Download)
                            .label("Install")
                            .on_click(move |_, window, cx| match &install_hit {
                                CombinedHit::Modrinth(hit) => {
                                    let name = hit.title.as_ref().map(|s| s.to_string()).unwrap_or_else(|| "Unnamed".to_string());
                                    modrinth_install::open(&name, hit.project_id.clone(), hit.project_type, install_for, &data, window, cx);
                                },
                                CombinedHit::Curseforge(hit) => {
                                    curseforge_install::open(hit.clone(), install_for, &data, window, cx);
                                },
                            }),
                    ),
            )
            .into_any_element()
    }
}

impl Page for CombinedSearchPage {
    fn controls(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }

    fn scrollable(&self, _cx: &App) -> bool {
        true
    }
}

impl Render for CombinedSearchPage {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut hits: Vec<CombinedHit> = Vec::new();
        hits.extend(self.modrinth_hits.iter().cloned().map(CombinedHit::Modrinth));
        hits.extend(self.curseforge_hits.iter().cloned().map(CombinedHit::Curseforge));
        hits.sort_by_key(|h| std::cmp::Reverse(h.downloads()));

        let is_loading = self.loading_modrinth.is_some() || self.loading_curseforge.is_some();

        let rows: Vec<AnyElement> = hits.into_iter().enumerate().map(|(index, hit)| self.render_row(hit, index, cx)).collect();

        let tabs = h_flex().gap_2().children(ContentKind::ALL.into_iter().map(|kind| {
            let active = kind == self.content_kind;
            Button::new(("content-kind", kind as usize))
                .with_variant(if active { gpui_component::button::ButtonVariant::Primary } else { gpui_component::button::ButtonVariant::Ghost })
                .label(kind.label())
                .on_click(cx.listener(move |page, _, _, cx| {
                    page.set_content_kind(kind, cx);
                }))
        }));

        v_flex()
            .size_full()
            .p_4()
            .gap_4()
            .child(Input::new(&self.search_state))
            .child(div().text_xs().text_color(cx.theme().muted_foreground).child("Press Enter to search"))
            .child(tabs)
            .when(self.error_modrinth.is_some() || self.error_curseforge.is_some(), |this| {
                this.child(
                    v_flex()
                        .gap_1()
                        .text_sm()
                        .text_color(cx.theme().danger)
                        .when_some(self.error_modrinth.clone(), |this, err| this.child(SharedString::from(format!("Modrinth: {err}"))))
                        .when_some(self.error_curseforge.clone(), |this, err| this.child(SharedString::from(format!("CurseForge: {err}")))),
                )
            })
            .when(is_loading && rows.is_empty(), |this| this.child(div().text_sm().text_color(cx.theme().muted_foreground).child("Loading...")))
            .child(v_flex().w_full().children(rows))
            .child(
                h_flex()
                    .w_full()
                    .justify_center()
                    .items_center()
                    .gap_3()
                    .py_2()
                    .child(
                        Button::new("browse-prev-page")
                            .ghost()
                            .small()
                            .icon(PandoraIcon::ChevronLeft)
                            .label("Previous")
                            .disabled(self.page == 0)
                            .on_click(cx.listener(|page, _, _, cx| {
                                if page.page > 0 {
                                    page.go_to_page(page.page - 1, cx);
                                }
                            })),
                    )
                    .child(div().text_sm().text_color(cx.theme().muted_foreground).child(format!("Page {}", self.page + 1)))
                    .child(
                        Button::new("browse-next-page")
                            .ghost()
                            .small()
                            .icon(PandoraIcon::ChevronRight)
                            .label("Next")
                            .disabled(!self.has_next_page())
                            .on_click(cx.listener(|page, _, _, cx| {
                                page.go_to_page(page.page + 1, cx);
                            })),
                    ),
            )
    }
}
