use gpui::{prelude::*, *};
use gpui_component::{ActiveTheme, sheet::Sheet, v_flex};
use rand::Rng;

#[derive(Clone)]
enum CatSource {
    /// A locally bundled image, under `assets/images/cats/`.
    Local(&'static str),
    /// A remote URL, fetched at display time.
    Remote(&'static str),
}

struct CatEntry {
    source: CatSource,
    /// Relative weight — higher shows up more often. Not a percentage.
    weight: u32,
    caption: Option<&'static str>,
}

/// The cat pool.
fn pool() -> Vec<CatEntry> {
    vec![
        CatEntry { source: CatSource::Local("cats/grey_cat.png"), weight: 5, caption: None },
        CatEntry { source: CatSource::Local("cats/white_tabby.png"), weight: 5, caption: None },
        CatEntry { source: CatSource::Local("cats/orange_kitten.png"), weight: 5, caption: None },
        CatEntry { source: CatSource::Local("cats/black_cat_grass.png"), weight: 5, caption: None },
        CatEntry { source: CatSource::Local("cats/sleeping_tabby.png"), weight: 5, caption: None },
        CatEntry { source: CatSource::Local("cats/peeking_black_cat.png"), weight: 5, caption: None },
        CatEntry { source: CatSource::Local("cats/microwave_cat.png"), weight: 5, caption: None },
        CatEntry { source: CatSource::Local("cats/minecraft_cats.png"), weight: 5, caption: Some("even Minecraft cats showed up") },
        CatEntry { source: CatSource::Local("cats/orange_and_black.jpg"), weight: 5, caption: Some("these are my cats") },
        // Very low weight relative to the rest (1 vs 10 each) — a rare pull.
        CatEntry { source: CatSource::Local("cats/drdonut.png"), weight: 3, caption: None },
        // A random cat from a public cat-image API, thrown in for variety —
        // weighted lower than your own photos so it shows up less often.
        CatEntry { source: CatSource::Remote("https://cataas.com/cat?width=500"), weight: 52, caption: None },
    ]
}

enum PickedSource {
    Local(&'static str),
    Remote(String),
}

fn pick(entries: &[CatEntry]) -> Option<(PickedSource, Option<&'static str>)> {
    if entries.is_empty() {
        return None;
    }

    let total: u32 = entries.iter().map(|e| e.weight.max(1)).sum();
    let mut roll = rand::thread_rng().gen_range(0..total);

    for entry in entries {
        let weight = entry.weight.max(1);
        if roll < weight {
            return Some((materialize(&entry.source), entry.caption));
        }
        roll -= weight;
    }

    entries.last().map(|entry| (materialize(&entry.source), entry.caption))
}

fn materialize(source: &CatSource) -> PickedSource {
    match source {
        CatSource::Local(path) => PickedSource::Local(*path),
        // Append a cache-busting value so a fresh image is actually fetched
        // each time instead of gpui's image cache serving the first result
        // it ever saw for this URL.
        CatSource::Remote(url) => {
            let separator = if url.contains('?') { '&' } else { '?' };
            PickedSource::Remote(format!("{url}{separator}_cb={}", rand::thread_rng().gen_range(0..u64::MAX)))
        },
    }
}

pub fn build_cats_sheet(_window: &mut Window, _cx: &mut App) -> impl Fn(Sheet, &mut Window, &mut App) -> Sheet + 'static {
    // Roll once here so the same cat sticks around even if the sheet re-renders,
    // rather than re-rolling on every frame.
    let picked = pick(&pool());

    move |sheet, _, cx| {
        let content: AnyElement = match &picked {
            Some((source, _)) => match source {
                PickedSource::Local(path) => {
                    gpui::img(ImageSource::Resource(Resource::Embedded(format!("images/{path}").into())))
                        .max_w(px(400.))
                        .rounded_lg()
                        .into_any_element()
                },
                PickedSource::Remote(url) => gpui::img(SharedUri::from(url.clone())).max_w(px(400.)).rounded_lg().into_any_element(),
            },
            None => div().child("No cats in the pool yet!").into_any_element(),
        };

        let caption = picked.as_ref().and_then(|(_, caption)| *caption);

        sheet
            .title("???")
            .size(px(460.))
            .child(
                v_flex()
                    .size_full()
                    .p_4()
                    .gap_3()
                    .items_center()
                    .child(content)
                    .when_some(caption, |this, caption| {
                        this.child(div().text_sm().text_color(cx.theme().muted_foreground).child(caption))
                    }),
            )
    }
}
