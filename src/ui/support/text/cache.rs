//! Bound retired font sizes without invalidating any live text layout.
use std::collections::{HashMap, HashSet};

use bevy::asset::AssetId;
use bevy::prelude::*;
use bevy::text::{FontAtlasKey, FontAtlasSet, TextLayoutInfo};

use crate::ui::activity::UiAnimationActivity;

const MAX_ATLAS_BYTES: usize = 32 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct FontAtlasRetention {
    generation: u64,
    last_used: HashMap<FontAtlasKey, u64>,
    retire_frames: u8,
}

impl FontAtlasRetention {
    fn trim(
        &mut self,
        atlases: &mut FontAtlasSet,
        images: &Assets<Image>,
        used: &HashSet<AssetId<Image>>,
        budget: usize,
    ) -> bool {
        self.generation = self.generation.saturating_add(1);
        self.last_used.retain(|key, _| atlases.contains_key(key));
        let mut total = 0usize;
        let mut idle = Vec::new();
        for (key, group) in atlases.iter() {
            let bytes = group
                .iter()
                .filter_map(|atlas| images.get(&atlas.texture))
                .filter_map(|image| image.data.as_ref())
                .map(Vec::len)
                .sum::<usize>();
            total = total.saturating_add(bytes);
            if group.iter().any(|atlas| used.contains(&atlas.texture.id())) {
                self.last_used.insert(*key, self.generation);
            } else {
                idle.push((*key, bytes, self.last_used.get(key).copied().unwrap_or(0)));
            }
        }
        if total <= budget {
            return false;
        }
        idle.sort_by_key(|(_, _, age)| *age);
        let mut removed = false;
        for (key, bytes, _) in idle {
            if total <= budget {
                break;
            }
            atlases.remove(&key);
            self.last_used.remove(&key);
            total = total.saturating_sub(bytes);
            removed = true;
        }
        removed
    }
}

/// Run after text layout, before extraction. Glyphs contain asset IDs rather
/// than strong handles, so hidden layouts must also protect their atlases.
/// Keep whole active font/size groups: later characters can reuse their glyphs.
pub(crate) fn trim_font_atlases(
    mut atlases: ResMut<FontAtlasSet>,
    images: Res<Assets<Image>>,
    layouts: Query<Ref<TextLayoutInfo>>,
    mut removed: RemovedComponents<TextLayoutInfo>,
    mut retention: Local<FontAtlasRetention>,
    mut activity: ResMut<UiAnimationActivity>,
) {
    let layout_removed = removed.read().count() != 0;
    if atlases.is_changed() || layouts.iter().any(|layout| layout.is_changed()) || layout_removed {
        let mut used = HashSet::new();
        for layout in &layouts {
            used.extend(layout.glyphs.iter().map(|glyph| glyph.atlas_info.texture));
        }
        if retention.trim(
            atlases.bypass_change_detection(),
            &images,
            &used,
            MAX_ATLAS_BYTES,
        ) {
            // Dropping atlas handles needs asset tracking and render extraction
            // to run before the reactive window sleeps again.
            retention.retire_frames = 3;
        }
    }
    if retention.retire_frames != 0 {
        retention.retire_frames -= 1;
        activity.0 = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::asset::{AssetApp, AssetPlugin};
    use bevy::ecs::system::RunSystemOnce;
    use bevy::text::{
        ComputedTextBlock, FontCx, FontHinting, LayoutCx, LineHeight, ScaleCx, TextBounds,
        TextPipeline, load_font_assets_into_font_collection,
    };

    struct Rasterizer {
        app: App,
        fonts: Assets<Font>,
        font: TextFont,
        font_cx: FontCx,
        layout_cx: LayoutCx,
        scale_cx: ScaleCx,
        pipeline: TextPipeline,
        computed: ComputedTextBlock,
        layout: TextLayoutInfo,
        atlases: FontAtlasSet,
        retention: FontAtlasRetention,
    }

    impl Rasterizer {
        fn new() -> Self {
            let mut app = App::new();
            app.add_plugins((TaskPoolPlugin::default(), AssetPlugin::default()))
                .init_asset::<Image>()
                .init_resource::<Assets<Font>>()
                .init_resource::<FontCx>();
            let font = app
                .world_mut()
                .resource_mut::<Assets<Font>>()
                .add(Font::from_bytes(
                    include_bytes!("../../../assets/fonts/MavenPro-CJK.ttf").to_vec(),
                ));
            app.world_mut()
                .run_system_once(load_font_assets_into_font_collection)
                .unwrap();
            let fonts = app.world_mut().remove_resource::<Assets<Font>>().unwrap();
            let font_cx = app.world_mut().remove_resource::<FontCx>().unwrap();
            Self {
                app,
                fonts,
                font_cx,
                font: TextFont {
                    font: font.into(),
                    font_size: FontSize::Px(34.2),
                    ..default()
                },
                layout_cx: LayoutCx::default(),
                scale_cx: ScaleCx::default(),
                pipeline: TextPipeline::default(),
                computed: ComputedTextBlock::default(),
                layout: TextLayoutInfo::default(),
                atlases: FontAtlasSet::default(),
                retention: FontAtlasRetention::default(),
            }
        }

        fn rasterize(&mut self, scale: f32, text: &str) {
            self.pipeline
                .update_buffer(
                    &self.fonts,
                    std::iter::once((
                        Entity::PLACEHOLDER,
                        0,
                        bevy::text::TextElement::Text {
                            text,
                            font: &self.font,
                            color: Color::WHITE,
                            line_height: LineHeight::default(),
                            letter_spacing: bevy::text::LetterSpacing::default(),
                        },
                    )),
                    LineBreak::AnyCharacter,
                    Justify::Left,
                    TextBounds::UNBOUNDED,
                    scale,
                    &mut self.computed,
                    &mut self.font_cx,
                    &mut self.layout_cx,
                    Vec2::new(1920.0, 1080.0),
                    bevy::text::RemSize(16.0),
                )
                .unwrap();
            self.pipeline
                .update_text_layout_info(
                    &mut self.layout,
                    &mut self.atlases,
                    &mut self.app.world_mut().resource_mut::<Assets<Image>>(),
                    &mut self.computed,
                    &mut self.scale_cx,
                    TextBounds::UNBOUNDED,
                    Justify::Left,
                    FontHinting::default(),
                )
                .unwrap();
            assert!(!self.layout.glyphs.is_empty());
        }

        fn trim(&mut self, used: &HashSet<AssetId<Image>>, budget: usize) {
            self.retention.trim(
                &mut self.atlases,
                self.app.world().resource::<Assets<Image>>(),
                used,
                budget,
            );
            self.app.update();
            self.app.update();
        }

        fn bytes(&self) -> u64 {
            self.atlases
                .total_bytes(self.app.world().resource::<Assets<Image>>())
        }

        fn used(&self) -> HashSet<AssetId<Image>> {
            self.layout
                .glyphs
                .iter()
                .map(|glyph| glyph.atlas_info.texture)
                .collect()
        }
    }

    #[test]
    fn embedded_font_renders_titles_and_common_symbols_without_fallback() {
        let mut text = Rasterizer::new();
        let samples = concat!(
            "■エピローグ　：　－　７　Ｆ　－\n■终章：　－　７Ｆ　－\n",
            "■プロローグ　―　姫　子　―\n■─　尼洛和阿洛伊斯　─\n",
            include_str!("../../../assets/fonts/KeneText-symbols.txt"),
        );
        let embedded = include_bytes!("../../../assets/fonts/MavenPro-CJK.ttf");
        for sample in samples.lines() {
            text.rasterize(1.0, sample);
            for line in text.computed.buffer().lines() {
                for run in line.runs() {
                    assert!(run.font().data.as_ref() == embedded, "fallback: {sample}");
                    for cluster in run.clusters() {
                        for glyph in cluster.glyphs() {
                            assert_ne!(glyph.id, 0, "missing glyph: {sample}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn repeated_scales_reclaim_idle_rasters_and_keep_current_glyphs() {
        let mut text = Rasterizer::new();
        let mut unbounded = Rasterizer::new();
        for step in 0..128 {
            let scale = 0.6 + step as f32 * 0.007;
            text.rasterize(scale, "水仙 Memory 文字");
            unbounded.rasterize(scale, "水仙 Memory 文字");
            let used = text.used();
            text.trim(&used, MAX_ATLAS_BYTES);
            assert!(
                used.iter()
                    .all(|id| text.app.world().resource::<Assets<Image>>().contains(*id))
            );
            assert!(text.bytes() <= MAX_ATLAS_BYTES as u64);
        }
        println!(
            "128 actual font scales: unbounded {} bytes / {} keys; bounded {} bytes / {} keys",
            unbounded.bytes(),
            unbounded.atlases.len(),
            text.bytes(),
            text.atlases.len()
        );
        assert!(unbounded.bytes() >= 128 * 1024 * 1024);
        assert!(text.atlases.len() <= 32);
        let images = text.app.world().resource::<Assets<Image>>();
        assert!(images.len() <= 32, "dropped atlas images must retire too");
    }

    #[test]
    fn hidden_layouts_and_oversized_live_working_sets_are_protected() {
        let mut text = Rasterizer::new();
        text.rasterize(1.0, "隐藏 Hidden");
        let hidden = text.used();
        text.rasterize(1.25, "当前 Current");
        let current = text.used();
        let used = hidden.union(&current).copied().collect();
        text.trim(&used, 0);
        assert_eq!(text.atlases.len(), 2);
        let images = text.app.world().resource::<Assets<Image>>();
        assert!(hidden.union(&current).all(|id| images.contains(*id)));
        // Once the hidden layout is gone its IDs no longer pin old rasters.
        text.trim(&current, 0);
        assert_eq!(text.atlases.len(), 1);
        assert!(
            hidden
                .iter()
                .all(|id| !text.app.world().resource::<Assets<Image>>().contains(*id))
        );
        text.rasterize(1.0, "隐藏 Hidden");
        assert!(
            text.used()
                .iter()
                .all(|id| text.app.world().resource::<Assets<Image>>().contains(*id))
        );
    }

    #[test]
    fn removing_layouts_releases_pinned_sizes_and_finishes_retirement_before_sleep() {
        let mut text = Rasterizer::new();
        let mut entities = Vec::new();
        for step in 0..40 {
            text.rasterize(0.6 + step as f32 * 0.007, "保留 Hidden");
            entities.push(
                text.app
                    .world_mut()
                    .spawn((text.layout.clone(), Visibility::Hidden))
                    .id(),
            );
        }
        let live = text.used();
        text.app
            .insert_resource(text.atlases)
            .init_resource::<UiAnimationActivity>()
            .add_systems(Last, trim_font_atlases);
        text.app.update();
        text.app.update();
        assert_eq!(text.app.world().resource::<FontAtlasSet>().len(), 40);
        for entity in entities.into_iter().take(39) {
            text.app.world_mut().despawn(entity);
        }
        text.app.update();
        assert!(text.app.world().resource::<UiAnimationActivity>().0);
        assert_eq!(text.app.world().resource::<FontAtlasSet>().len(), 32);
        for _ in 0..3 {
            text.app.world_mut().resource_mut::<UiAnimationActivity>().0 = false;
            text.app.update();
        }
        assert!(!text.app.world().resource::<UiAnimationActivity>().0);
        let images = text.app.world().resource::<Assets<Image>>();
        assert_eq!(images.len(), 32);
        assert!(live.iter().all(|id| images.contains(*id)));
    }
}
