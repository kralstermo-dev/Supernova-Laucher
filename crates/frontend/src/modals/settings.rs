use std::{path::Path, sync::Arc};

use bridge::{handle::BackendHandle, message::{BackendConfigWithPassword, MessageToBackend}};
use gpui::{prelude::FluentBuilder, *};
use gpui_component::{
    IndexPath,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState},
    h_flex,
    input::{Input, InputEvent, InputState, NumberInput},
    select::{SearchableVec, Select, SelectEvent, SelectState},
    sheet::Sheet,
    spinner::Spinner,
    tab::{Tab, TabBar},
    v_flex, ActiveTheme, Colorize, Disableable, Sizable, ThemeRegistry,
};
use schema::backend_config::{BackendConfig, ProxyConfig, ProxyProtocol};

use crate::{
    component::named_dropdown::{NamedDropdown, NamedDropdownItem},
    entity::DataEntities,
    icon::PandoraIcon,
    interface_config::InterfaceConfig,
};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum SettingsTab {
    #[default]
    Interface,
    Network,
}

/// Renders a labeled row containing a color-picker swatch and its paired hex text box.
fn color_slot_row(
    label: &'static str,
    picker: &Entity<ColorPickerState>,
    hex_input: &Entity<InputState>,
) -> impl IntoElement {
    crate::labelled(
        label,
        h_flex()
            .gap_2()
            .items_center()
            .child(ColorPicker::new(picker))
            .child(Input::new(hex_input).w_24()),
    )
}

/// Creates a paired swatch color-picker + typeable hex text box, both starting
/// from `initial_hex` (if it parses; otherwise left at the picker/input's own default).
fn build_color_slot(
    initial_hex: &SharedString,
    window: &mut Window,
    cx: &mut Context<Settings>,
) -> (Entity<ColorPickerState>, Entity<InputState>) {
    let trimmed = initial_hex.trim_ascii().to_string();

    let picker = cx.new(|cx| {
        let mut state = ColorPickerState::new(window, cx);
        if let Ok(color) = gpui::Hsla::parse_hex(&trimmed) {
            state = state.default_value(color);
        }
        state
    });

    let hex_input = cx.new(|cx| {
        let mut input = InputState::new(window, cx)
            .pattern(regex::Regex::new(r"^#[0-9a-fA-F]{0,8}$").unwrap())
            .placeholder("#rrggbb");
        if !trimmed.is_empty() {
            input = input.default_value(trimmed.clone());
        }
        input
    });

    (picker, hex_input)
}

/// Wires a swatch/hex-input pair together (each stays in sync with the other),
/// and on any valid change: saves the hex into `InterfaceConfig` via `setter`,
/// then recomputes the full custom color set.
fn wire_color_slot(
    picker: &Entity<ColorPickerState>,
    hex_input: &Entity<InputState>,
    window: &mut Window,
    cx: &mut Context<Settings>,
    setter: fn(&mut InterfaceConfig, SharedString),
) {
    {
        let hex_input = hex_input.clone();
        cx.subscribe_in(picker, window, move |_, _, event: &ColorPickerEvent, window, cx| {
            let ColorPickerEvent::Change(color) = event;
            let Some(color) = color else { return };
            let hex: SharedString = color.to_hex().into();
            setter(InterfaceConfig::get_mut(cx), hex.clone());
            crate::accent_color::reapply_custom_colors(cx);
            hex_input.update(cx, |input, cx| {
                input.set_value(hex.clone(), window, cx);
            });
        }).detach();
    }

    {
        let picker = picker.clone();
        cx.subscribe_in(hex_input, window, move |_, entity, event: &InputEvent, window, cx| {
            if !matches!(event, InputEvent::Blur) {
                return;
            }
            let value = entity.read(cx).value().trim().to_string();
            let Ok(color) = gpui::Hsla::parse_hex(&value) else { return };
            setter(InterfaceConfig::get_mut(cx), value.clone().into());
            crate::accent_color::reapply_custom_colors(cx);
            picker.update(cx, |state, cx| {
                state.set_value(color, window, cx);
            });
        }).detach();
    }
}

struct Settings {
    selected_tab: SettingsTab,
    language_select: Entity<SelectState<NamedDropdown<t::Language>>>,
    theme_folder: Arc<Path>,
    theme_select: Entity<SelectState<SearchableVec<SharedString>>>,
    accent_color_picker: Entity<ColorPickerState>,
    accent_hex_input: Entity<InputState>,
    background_color_picker: Entity<ColorPickerState>,
    background_hex_input: Entity<InputState>,
    secondary_color_picker: Entity<ColorPickerState>,
    secondary_hex_input: Entity<InputState>,
    text_color_picker: Entity<ColorPickerState>,
    text_hex_input: Entity<InputState>,
    border_color_picker: Entity<ColorPickerState>,
    border_hex_input: Entity<InputState>,
    danger_color_picker: Entity<ColorPickerState>,
    danger_hex_input: Entity<InputState>,
    success_color_picker: Entity<ColorPickerState>,
    success_hex_input: Entity<InputState>,
    warning_color_picker: Entity<ColorPickerState>,
    warning_hex_input: Entity<InputState>,
    info_color_picker: Entity<ColorPickerState>,
    info_hex_input: Entity<InputState>,
    preset_name_input: Entity<InputState>,
    backend_handle: BackendHandle,
    pending_request: bool,
    backend_config: Option<BackendConfig>,
    get_configuration_task: Option<Task<()>>,
    // Proxy settings state
    proxy_enabled: bool,
    proxy_protocol_select: Entity<SelectState<Vec<&'static str>>>,
    proxy_host_input: Entity<InputState>,
    proxy_port_input: Entity<InputState>,
    proxy_auth_enabled: bool,
    proxy_username_input: Entity<InputState>,
    proxy_password_input: Entity<InputState>,
    proxy_password_changed: bool,
}

pub fn build_settings_sheet(data: &DataEntities, window: &mut Window, cx: &mut App) -> impl Fn(Sheet, &mut Window, &mut App) -> Sheet + 'static {
    let theme_folder = data.theme_folder.clone();
    let settings = cx.new(|cx| {
        let language_select = cx.new(|cx| {
            let lang_options = Settings::build_language_options();
            let lang = &InterfaceConfig::get(cx).language;
            let selected_index = lang_options.iter()
                .position(|item| item.item == *lang)
                .map(IndexPath::new);
            SelectState::new(NamedDropdown::new(lang_options), selected_index, window, cx)
        });

        cx.subscribe_in(&language_select, window, Settings::on_language_changed).detach();

        let theme_select_delegate = SearchableVec::new(ThemeRegistry::global(cx).sorted_themes()
            .iter().map(|cfg| cfg.name.clone()).collect::<Vec<_>>());

        let theme_select = cx.new(|cx| {
            let mut state = SelectState::new(theme_select_delegate, Default::default(), window, cx).searchable(true);
            state.set_selected_value(&cx.theme().theme_name().clone(), window, cx);
            state
        });

        cx.subscribe_in(&theme_select, window, |_, entity, _: &SelectEvent<_>, _, cx| {
            let Some(theme_name) = entity.read(cx).selected_value().cloned() else {
                return;
            };

            InterfaceConfig::get_mut(cx).active_theme = theme_name.clone();

            let Some(theme) = gpui_component::ThemeRegistry::global(cx).themes().get(&SharedString::new(theme_name.trim_ascii())).cloned() else {
                return;
            };

            gpui_component::Theme::global_mut(cx).apply_config(&theme);
            // Re-apply the custom accent color on top of the newly selected base theme,
            // since apply_config fully recomputes the resolved colors from scratch.
            crate::accent_color::reapply_custom_colors(cx);
        }).detach();

        let (accent_color_picker, accent_hex_input) =
            build_color_slot(&InterfaceConfig::get(cx).custom_accent_color.clone(), window, cx);
        wire_color_slot(&accent_color_picker, &accent_hex_input, window, cx, |cfg, hex| {
            cfg.custom_accent_color = hex;
        });

        let (background_color_picker, background_hex_input) =
            build_color_slot(&InterfaceConfig::get(cx).custom_background_color.clone(), window, cx);
        wire_color_slot(&background_color_picker, &background_hex_input, window, cx, |cfg, hex| {
            cfg.custom_background_color = hex;
        });

        let (secondary_color_picker, secondary_hex_input) =
            build_color_slot(&InterfaceConfig::get(cx).custom_secondary_color.clone(), window, cx);
        wire_color_slot(&secondary_color_picker, &secondary_hex_input, window, cx, |cfg, hex| {
            cfg.custom_secondary_color = hex;
        });

        let (text_color_picker, text_hex_input) =
            build_color_slot(&InterfaceConfig::get(cx).custom_text_color.clone(), window, cx);
        wire_color_slot(&text_color_picker, &text_hex_input, window, cx, |cfg, hex| {
            cfg.custom_text_color = hex;
        });

        let (border_color_picker, border_hex_input) =
            build_color_slot(&InterfaceConfig::get(cx).custom_border_color.clone(), window, cx);
        wire_color_slot(&border_color_picker, &border_hex_input, window, cx, |cfg, hex| {
            cfg.custom_border_color = hex;
        });

        let (danger_color_picker, danger_hex_input) =
            build_color_slot(&InterfaceConfig::get(cx).custom_danger_color.clone(), window, cx);
        wire_color_slot(&danger_color_picker, &danger_hex_input, window, cx, |cfg, hex| {
            cfg.custom_danger_color = hex;
        });

        let (success_color_picker, success_hex_input) =
            build_color_slot(&InterfaceConfig::get(cx).custom_success_color.clone(), window, cx);
        wire_color_slot(&success_color_picker, &success_hex_input, window, cx, |cfg, hex| {
            cfg.custom_success_color = hex;
        });

        let (warning_color_picker, warning_hex_input) =
            build_color_slot(&InterfaceConfig::get(cx).custom_warning_color.clone(), window, cx);
        wire_color_slot(&warning_color_picker, &warning_hex_input, window, cx, |cfg, hex| {
            cfg.custom_warning_color = hex;
        });

        let (info_color_picker, info_hex_input) =
            build_color_slot(&InterfaceConfig::get(cx).custom_info_color.clone(), window, cx);
        wire_color_slot(&info_color_picker, &info_hex_input, window, cx, |cfg, hex| {
            cfg.custom_info_color = hex;
        });

        let preset_name_input = cx.new(|cx| InputState::new(window, cx).placeholder("Preset name"));

        let proxy_protocol_select = cx.new(|cx| {
            let protocols = vec!["HTTP", "HTTPS", "SOCKS5"];
            let mut state = SelectState::new(protocols, None, window, cx);
            state.set_selected_value(&"HTTP", window, cx);
            state
        });

        let proxy_host_input = cx.new(|cx| InputState::new(window, cx).placeholder("proxy.example.com"));
        let proxy_port_input = cx.new(|cx| InputState::new(window, cx).default_value("8080".to_string()));
        let proxy_username_input = cx.new(|cx| InputState::new(window, cx).placeholder("username"));
        let proxy_password_input = cx.new(|cx| {
            let mut state = InputState::new(window, cx).placeholder("password");
            state.set_masked(true, window, cx);
            state
        });

        let mut settings = Settings {
            selected_tab: SettingsTab::Interface,
            language_select,
            theme_folder,
            theme_select,
            accent_color_picker,
            accent_hex_input,
            background_color_picker,
            background_hex_input,
            secondary_color_picker,
            secondary_hex_input,
            text_color_picker,
            text_hex_input,
            border_color_picker,
            border_hex_input,
            danger_color_picker,
            danger_hex_input,
            success_color_picker,
            success_hex_input,
            warning_color_picker,
            warning_hex_input,
            info_color_picker,
            info_hex_input,
            preset_name_input,
            backend_handle: data.backend_handle.clone(),
            pending_request: false,
            backend_config: None,
            get_configuration_task: None,
            proxy_enabled: false,
            proxy_protocol_select,
            proxy_host_input,
            proxy_port_input,
            proxy_auth_enabled: false,
            proxy_username_input,
            proxy_password_input,
            proxy_password_changed: false,
        };

        cx.subscribe(&settings.proxy_protocol_select, Settings::on_proxy_protocol_changed).detach();
        cx.subscribe(&settings.proxy_host_input, Settings::on_proxy_input_changed).detach();
        cx.subscribe(&settings.proxy_port_input, Settings::on_proxy_input_changed).detach();
        cx.subscribe(&settings.proxy_username_input, Settings::on_proxy_input_changed).detach();
        cx.subscribe(&settings.proxy_password_input, Settings::on_proxy_password_changed).detach();

        settings.update_backend_configuration(window, cx);

        settings
    });

    let version = option_env!("PANDORA_RELEASE_VERSION").unwrap_or("Dev");
    let version_string = if let Some(git_rev) = option_env!("GIT_REVISION") {
        SharedString::new(format!("{} ({})", version, git_rev))
    } else {
        version.into()
    };
    let version_icon = if version == "Dev" {
        PandoraIcon::GitBranch
    } else {
        PandoraIcon::Rocket
    };

    move |sheet, _, cx| {
        sheet
            .title(t::settings::title())
            .size(px(420.))
            .p_0()
            .when(cfg!(target_os = "macos"), |this| this.pt_5())
            .child(v_flex()
                .size_full()
                .border_t_1()
                .border_color(cx.theme().border)
                .child(settings.clone())
            )
            .child(h_flex().p_2().gap_2().child(version_icon.clone()).child(version_string.clone()))
    }
}

impl Settings {
    /// Refreshes every color swatch + hex input to reflect whatever the theme
    /// engine actually resolved (post reapply/preset-load/reset), so the UI
    /// never shows a stale value.
    fn sync_all_color_widgets(&self, window: &mut Window, cx: &mut Context<Self>) {
        let colors = cx.theme().colors;
        let widgets: [(&Entity<ColorPickerState>, &Entity<InputState>, Hsla); 9] = [
            (&self.accent_color_picker, &self.accent_hex_input, colors.primary),
            (&self.background_color_picker, &self.background_hex_input, colors.background),
            (&self.secondary_color_picker, &self.secondary_hex_input, colors.secondary),
            (&self.text_color_picker, &self.text_hex_input, colors.foreground),
            (&self.border_color_picker, &self.border_hex_input, colors.border),
            (&self.danger_color_picker, &self.danger_hex_input, colors.danger),
            (&self.success_color_picker, &self.success_hex_input, colors.success),
            (&self.warning_color_picker, &self.warning_hex_input, colors.warning),
            (&self.info_color_picker, &self.info_hex_input, colors.info),
        ];
        for (picker, hex_input, color) in widgets {
            picker.update(cx, |state, cx| {
                state.set_value(color, window, cx);
            });
            hex_input.update(cx, |input, cx| {
                input.set_value(color.to_hex(), window, cx);
            });
        }
    }

    pub fn update_backend_configuration(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.get_configuration_task.is_some() {
            self.pending_request = true;
            return;
        }

        let (send, recv) = tokio::sync::oneshot::channel();
        self.get_configuration_task = Some(cx.spawn_in(window, async move |page, cx| {
            let result: BackendConfigWithPassword = recv.await.unwrap_or_default();
            let _ = page.update_in(cx, move |settings, window, cx| {
                settings.proxy_enabled = result.config.proxy.enabled;
                settings.proxy_auth_enabled = result.config.proxy.auth_enabled;

                settings.proxy_host_input.update(cx, |input, cx| {
                    input.set_value(&result.config.proxy.host, window, cx);
                });
                settings.proxy_port_input.update(cx, |input, cx| {
                    input.set_value(result.config.proxy.port.to_string(), window, cx);
                });
                settings.proxy_username_input.update(cx, |input, cx| {
                    input.set_value(&result.config.proxy.username, window, cx);
                });
                settings.proxy_protocol_select.update(cx, |select, cx| {
                    select.set_selected_value(&result.config.proxy.protocol.name(), window, cx);
                });
                if let Some(ref password) = result.proxy_password {
                    settings.proxy_password_input.update(cx, |input, cx| {
                        input.set_value(password, window, cx);
                    });
                }

                settings.backend_config = Some(result.config);
                settings.get_configuration_task = None;
                cx.notify();

                if settings.pending_request {
                    settings.pending_request = false;
                    settings.update_backend_configuration(window, cx);
                }
            });
        }));

        self.backend_handle.send(MessageToBackend::GetBackendConfiguration {
            channel: send,
        });
    }

    fn on_proxy_protocol_changed(
        &mut self,
        _state: Entity<SelectState<Vec<&'static str>>>,
        event: &SelectEvent<Vec<&'static str>>,
        _cx: &mut Context<Self>,
    ) {
        let SelectEvent::Confirm(_) = event;
        self.save_proxy_config(_cx);
    }

    fn on_proxy_input_changed(
        &mut self,
        _state: Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<Self>,
    ) {
        if let InputEvent::Blur = event {
            self.save_proxy_config(cx);
        }
    }

    fn on_proxy_password_changed(
        &mut self,
        _state: Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                self.proxy_password_changed = true;
            }
            InputEvent::Blur => {
                if self.proxy_password_changed {
                    self.save_proxy_config(cx);
                }
            }
            _ => {}
        }
    }

    fn get_proxy_config(&self, cx: &App) -> ProxyConfig {
        let protocol_name = self.proxy_protocol_select.read(cx).selected_value()
            .map(|s| *s)
            .unwrap_or("HTTP");

        ProxyConfig {
            enabled: self.proxy_enabled,
            protocol: ProxyProtocol::from_name(protocol_name),
            host: self.proxy_host_input.read(cx).value().to_string(),
            port: self.proxy_port_input.read(cx).value().parse().unwrap_or(8080),
            auth_enabled: self.proxy_auth_enabled,
            username: self.proxy_username_input.read(cx).value().to_string(),
        }
    }

    fn save_proxy_config(&mut self, cx: &mut Context<Self>) {
        let config = self.get_proxy_config(cx);

        if let Some(backend_config) = &mut self.backend_config {
            if !self.proxy_password_changed && backend_config.proxy == config {
                return;
            }
            backend_config.proxy = config.clone();
        }

        let password = if self.proxy_password_changed {
            Some(self.proxy_password_input.read(cx).value().to_string())
        } else {
            None
        };

        self.backend_handle.send(MessageToBackend::SetProxyConfiguration {
            config,
            password,
        });

        self.proxy_password_changed = false;
    }

    fn build_language_options() -> Vec<NamedDropdownItem<t::Language>> {
        std::iter::once(NamedDropdownItem {
            name: t::settings::language::system().into(),
            item: t::Language::System,
        }).chain(t::languages().iter().map(|&(code, name)| NamedDropdownItem {
            name: name.into(),
            item: t::Language::Code(code.to_string()),
        }))
        .collect()
    }

    fn on_language_changed(
        &mut self,
        _state: &Entity<SelectState<NamedDropdown<t::Language>>>,
        event: &SelectEvent<NamedDropdown<t::Language>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let SelectEvent::Confirm(_) = event;
        let Some(lang_item) = self.language_select.read(cx).selected_value().cloned() else {
            return;
        };
        let lang = lang_item.item;
        t::set_lang(&lang);

        let lang_options = Self::build_language_options();
        let selected_index = lang_options.iter()
            .position(|option| option.item == lang)
            .map(IndexPath::new);

        InterfaceConfig::get_mut(cx).language = lang;

        self.language_select.update(cx, |select, cx| {
            select.set_items(NamedDropdown::new(lang_options), window, cx);
            select.set_selected_index(selected_index, window, cx);
        });

        cx.notify();
    }

    fn render_interface_tab(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let interface_config = InterfaceConfig::get(cx);

        let mut div = v_flex()
            .px_4()
            .py_3()
            .gap_3()
            .child(crate::labelled(
                t::settings::language::title(),
                Select::new(&self.language_select)
            ))
            .child(crate::labelled(
                t::settings::theme::title(),
                Select::new(&self.theme_select).search_placeholder(t::common::search())
            ))
            .child(Button::new("open-theme-folder").info().icon(PandoraIcon::FolderOpen).label(t::settings::theme::open_folder()).on_click({
                let theme_folder = self.theme_folder.clone();
                move |_, window, cx| {
                    crate::open_folder(&theme_folder, window, cx);
                }
            }))
            .child(Button::new("open-theme-repo").info().icon(PandoraIcon::Globe).label(t::settings::theme::open_repo()).on_click({
                move |_, _, cx| {
                    cx.open_url("https://github.com/longbridge/gpui-component/tree/main/themes");
                }
            }))
            .child(crate::labelled(
                "Custom Colors",
                v_flex()
                    .gap_3()
                    .child(
                        h_flex()
                            .gap_3()
                            .flex_wrap()
                            .child(color_slot_row("Accent", &self.accent_color_picker, &self.accent_hex_input))
                            .child(color_slot_row("Background", &self.background_color_picker, &self.background_hex_input))
                            .child(color_slot_row("Secondary", &self.secondary_color_picker, &self.secondary_hex_input))
                            .child(color_slot_row("Text", &self.text_color_picker, &self.text_hex_input))
                            .child(color_slot_row("Border", &self.border_color_picker, &self.border_hex_input)),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .flex_wrap()
                            .child(color_slot_row("Danger", &self.danger_color_picker, &self.danger_hex_input))
                            .child(color_slot_row("Success", &self.success_color_picker, &self.success_hex_input))
                            .child(color_slot_row("Warning", &self.warning_color_picker, &self.warning_hex_input))
                            .child(color_slot_row("Info", &self.info_color_picker, &self.info_hex_input)),
                    )
                    .child(Button::new("reset-custom-colors").info().icon(PandoraIcon::CircleX).label("Reset All").on_click(
                        cx.listener(|settings, _, window, cx| {
                            let cfg = InterfaceConfig::get_mut(cx);
                            cfg.custom_accent_color = SharedString::default();
                            cfg.custom_background_color = SharedString::default();
                            cfg.custom_secondary_color = SharedString::default();
                            cfg.custom_text_color = SharedString::default();
                            cfg.custom_border_color = SharedString::default();
                            cfg.custom_danger_color = SharedString::default();
                            cfg.custom_success_color = SharedString::default();
                            cfg.custom_warning_color = SharedString::default();
                            cfg.custom_info_color = SharedString::default();

                            let theme_name = InterfaceConfig::get(cx).active_theme.clone();
                            if let Some(theme) = gpui_component::ThemeRegistry::global(cx)
                                .themes()
                                .get(&SharedString::new(theme_name.trim_ascii()))
                                .cloned()
                            {
                                gpui_component::Theme::global_mut(cx).apply_config(&theme);
                            }

                            settings.sync_all_color_widgets(window, cx);
                        }),
                    )),
            ))
            .child(crate::labelled(
                "Color Presets",
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(Input::new(&self.preset_name_input).w_48())
                            .child(Button::new("save-preset").info().icon(PandoraIcon::Plus).label("Save Preset").on_click(
                                cx.listener(|settings, _, window, cx| {
                                    let name = settings.preset_name_input.read(cx).value().trim().to_string();
                                    if name.is_empty() {
                                        return;
                                    }
                                    let preset = crate::accent_color::capture_preset(name.into(), cx);
                                    InterfaceConfig::get_mut(cx).color_presets.push(preset);
                                    settings.preset_name_input.update(cx, |input, cx| {
                                        input.set_value("", window, cx);
                                    });
                                    cx.notify();
                                }),
                            )),
                    )
                    .children({
                        let presets: Vec<(usize, SharedString)> = InterfaceConfig::get(cx)
                            .color_presets
                            .iter()
                            .enumerate()
                            .map(|(index, preset)| (index, preset.name.clone()))
                            .collect();

                        presets.into_iter().map(|(index, name)| {
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(div().text_sm().flex_1().child(name))
                                .child(Button::new(("load-preset", index)).info().small().label("Load").on_click(
                                    cx.listener(move |settings, _, window, cx| {
                                        let Some(preset) = InterfaceConfig::get(cx).color_presets.get(index).cloned() else {
                                            return;
                                        };
                                        crate::accent_color::apply_preset(&preset, cx);
                                        settings.sync_all_color_widgets(window, cx);
                                    }),
                                ))
                                .child(Button::new(("delete-preset", index)).danger().small().icon(PandoraIcon::Trash2).on_click(
                                    cx.listener(move |_, _, _, cx| {
                                        let presets = &mut InterfaceConfig::get_mut(cx).color_presets;
                                        if index < presets.len() {
                                            presets.remove(index);
                                        }
                                        cx.notify();
                                    }),
                                ))
                        }).collect::<Vec<_>>()
                    }),
            ))
            .child(crate::labelled(t::settings::delete::title(),
                v_flex().gap_2()
                    .child(Checkbox::new("confirm-delete-mods")
                        .label(t::settings::delete::skip_mod_delete_confirmation())
                        .checked(interface_config.quick_delete_mods)
                        .on_click(|value, _, cx| {
                            InterfaceConfig::get_mut(cx).quick_delete_mods = *value;
                        }))
                    .child(Checkbox::new("confirm-delete-instance")
                        .label(t::settings::delete::skip_instance_delete_confirmation())
                        .checked(interface_config.quick_delete_instance).on_click(|value, _, cx| {
                            InterfaceConfig::get_mut(cx).quick_delete_instance = *value;
                        }))
                    )
            );

        if let Some(backend_config) = &self.backend_config {
            div = div
                .child(crate::labelled(
                    t::settings::windows::title(),
                    v_flex().gap_2()
                        .child(Checkbox::new("hide-on-launch")
                            .label(t::settings::windows::hide_main_window())
                            .checked(interface_config.hide_main_window_on_launch)
                            .on_click(|value, _, cx| {
                                InterfaceConfig::get_mut(cx).hide_main_window_on_launch = *value;
                            }))
                        .child(Checkbox::new("open-game-output")
                            .label(t::settings::windows::open_game_output())
                            .checked(!backend_config.dont_open_game_output_when_launching)
                            .on_click(cx.listener({
                                let backend_handle = self.backend_handle.clone();
                                move |settings, value, window, cx| {
                                    backend_handle.send(MessageToBackend::SetOpenGameOutputAfterLaunching {
                                        value: *value
                                    });
                                    settings.update_backend_configuration(window, cx);
                                }
                            })))
                        .child(Checkbox::new("quit-on-main-close")
                            .label(t::settings::windows::close_all_when_main_closed())
                            .checked(interface_config.quit_on_main_closed)
                            .on_click(|value, _, cx| {
                                InterfaceConfig::get_mut(cx).quit_on_main_closed = *value;
                            }))
                        .child(Checkbox::new("use-os-titlebar")
                            .label(t::settings::windows::use_os_titlebar())
                            .checked(interface_config.use_os_titlebar)
                            .on_click(|value, _, cx| {
                                InterfaceConfig::get_mut(cx).use_os_titlebar = *value;
                            }))
                        .child(Checkbox::new("auto-upload-mclogs")
                            .label("Automatically upload log to mclo.gs when a game session ends")
                            .checked(interface_config.auto_upload_mclogs_on_exit)
                            .on_click(|value, _, cx| {
                                InterfaceConfig::get_mut(cx).auto_upload_mclogs_on_exit = *value;
                            }))
                ))
        } else {
            div = div.child(Spinner::new().large());
        }

        div = div.child(crate::labelled(t::settings::privacy::title(),
            v_flex().gap_2()
                .child(Checkbox::new("hide-usernames")
                    .label(t::settings::privacy::hide_usernames())
                    .checked(interface_config.hide_usernames)
                    .on_click(|value, _, cx| {
                        InterfaceConfig::get_mut(cx).hide_usernames = *value;
                    }))
                .child(Checkbox::new("hide-skins")
                    .label(t::settings::privacy::hide_skins())
                    .checked(interface_config.hide_skins)
                    .on_click(|value, _, cx| {
                        InterfaceConfig::get_mut(cx).hide_skins = *value;
                    }))
                .child(Checkbox::new("hide-server-addresses")
                    .label(t::settings::privacy::hide_server_addresses())
                    .checked(interface_config.hide_server_addresses)
                    .on_click(|value, _, cx| {
                        InterfaceConfig::get_mut(cx).hide_server_addresses = *value;
                    }))
        ));

        div
    }

    fn render_network_tab(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let proxy_enabled = self.proxy_enabled;
        let proxy_auth_enabled = self.proxy_auth_enabled;

        v_flex()
            .px_4()
            .py_3()
            .gap_3()
            .child(crate::labelled(
                t::settings::proxy::title(),
                v_flex().gap_2()
                    .child(Checkbox::new("proxy-enabled")
                        .label(t::settings::proxy::enabled())
                        .checked(proxy_enabled)
                        .on_click(cx.listener(|settings, value, _, cx| {
                            settings.proxy_enabled = *value;
                            settings.save_proxy_config(cx);
                            cx.notify();
                        })))
                    .child(h_flex().gap_2()
                        .child(v_flex().gap_1().w_32()
                            .child(t::settings::proxy::protocol())
                            .child(Select::new(&self.proxy_protocol_select)
                                .disabled(!proxy_enabled)
                                .w_full()))
                        .child(v_flex().gap_1().flex_1()
                            .child(t::settings::proxy::host())
                            .child(Input::new(&self.proxy_host_input)
                                .disabled(!proxy_enabled)))
                        .child(v_flex().gap_1().w_32()
                            .child(t::settings::proxy::port())
                            .child(NumberInput::new(&self.proxy_port_input)
                                .disabled(!proxy_enabled))))
            ))
            .child(crate::labelled(
                t::settings::proxy::auth(),
                v_flex().gap_2()
                    .child(Checkbox::new("proxy-auth-enabled")
                        .label(t::settings::proxy::use_auth())
                        .checked(proxy_auth_enabled)
                        .disabled(!proxy_enabled)
                        .on_click(cx.listener(|settings, value, _, cx| {
                            settings.proxy_auth_enabled = *value;
                            settings.save_proxy_config(cx);
                            cx.notify();
                        })))
                    .child(h_flex().gap_2()
                        .child(v_flex().gap_1().flex_1()
                            .child(t::settings::proxy::username())
                            .child(Input::new(&self.proxy_username_input)
                                .disabled(!proxy_enabled || !proxy_auth_enabled)))
                        .child(v_flex().gap_1().flex_1()
                            .child(t::settings::proxy::password())
                            .child(Input::new(&self.proxy_password_input)
                                .disabled(!proxy_enabled || !proxy_auth_enabled))))
            ))
            .child(div()
                .pt_2()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(t::settings::proxy::launcher_only_note()))
    }
}
impl Render for Settings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let selected_tab = self.selected_tab;

        let tab_bar = TabBar::new("settings-tabs")
            .prefix(div().w_4())
            .selected_index(match selected_tab {
                SettingsTab::Interface => 0,
                SettingsTab::Network => 1,
            })
            .underline()
            .child(Tab::new().label(t::settings::interface()))
            .child(Tab::new().label(t::settings::network()))
            .on_click(cx.listener(|settings, index, _window, cx| {
                settings.selected_tab = match index {
                    0 => SettingsTab::Interface,
                    1 => SettingsTab::Network,
                    _ => SettingsTab::Interface,
                };
                cx.notify();
            }));

        let content = match selected_tab {
            SettingsTab::Interface => self.render_interface_tab(window, cx).into_any_element(),
            SettingsTab::Network => self.render_network_tab(window, cx).into_any_element(),
        };

        v_flex()
            .child(tab_bar)
            .child(content)
    }
}
