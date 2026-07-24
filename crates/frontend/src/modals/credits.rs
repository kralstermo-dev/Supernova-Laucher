use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme, StyledExt, sheet::Sheet, v_flex};

use crate::icon::PandoraIcon;

struct CreditEntry {
    name: &'static str,
    description: &'static str,
}

const CREDITS: &[CreditEntry] = &[
    CreditEntry { name: "Moulberry", description: "Original creator of Pandora, which this project is based on." },
    CreditEntry { name: "My Friends", description: "Thanks for helping me throughout this journey!" },
    CreditEntry { name: "Myself", description: "For putting in the work and building this project." },
    CreditEntry { name: "Claude", description: "(It hurts a bit to admit, but) Thanks for helping me debug my code." },
];

pub fn build_credits_sheet(_window: &mut Window, _cx: &mut App) -> impl Fn(Sheet, &mut Window, &mut App) -> Sheet + 'static {
    move |sheet, _, cx| {
        sheet
            .title("Credits")
            .size(px(420.))
            .child(
                v_flex()
                    .size_full()
                    .p_4()
                    .gap_4()
                    .child(
                        v_flex().gap_3().children(CREDITS.iter().map(|entry| {
                            v_flex()
                                .gap_0p5()
                                .child(div().text_base().font_medium().child(entry.name))
                                .child(div().text_sm().text_color(cx.theme().muted_foreground).child(entry.description))
                        })),
                    )
                    .child(
                        div()
                            .mt_4()
                            .p_3()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().border)
                            .flex()
                            .gap_2()
                            .items_start()
                            .child(PandoraIcon::TriangleAlert)
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Note: Don't use AI for art, it's bad!"),
                            ),
                    ),
            )
    }
}
