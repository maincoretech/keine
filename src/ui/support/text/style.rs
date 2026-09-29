use bevy::camera::visibility::InheritedVisibility;
use bevy::math::{Affine2, Rect};
use bevy::prelude::*;
use bevy::render::{Extract, ExtractSchedule, RenderApp, sync_world::TemporaryRenderEntity};
use bevy::text::TextLayoutInfo;
use bevy::ui::widget::TextScroll;
use bevy::ui::{CalculatedClip, ComputedStackIndex, ComputedUiTargetCamera};
use bevy::ui_render::{
    ExtractedGlyph, ExtractedUiItem, ExtractedUiNode, ExtractedUiNodes, RenderUiSystems,
    UiCameraMap, stack_z_offsets,
};

#[derive(Component)]
pub struct TextBackdrop;

#[derive(Component)]
pub(crate) struct NoTextShadow;

/// Four bilinear-filtered diagonal samples form a compact diffuse shadow while
/// keeping global text geometry and extracted draw items bounded.
#[derive(Component, Clone, Copy)]
pub(crate) struct SoftTextShadow {
    radius: f32,
    color: Color,
}

type TextShadowTarget = (With<Text>, Without<TextBackdrop>, Without<NoTextShadow>);
type SoftShadowNodeQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static ComputedNode,
        &'static ComputedStackIndex,
        &'static UiGlobalTransform,
        &'static ComputedUiTargetCamera,
        &'static InheritedVisibility,
        Option<&'static CalculatedClip>,
        &'static TextLayoutInfo,
        &'static TextColor,
        &'static SoftTextShadow,
        Option<&'static TextScroll>,
    ),
>;

pub(crate) fn install_renderer(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render_app.add_systems(
        ExtractSchedule,
        extract_soft_text_shadows.in_set(RenderUiSystems::ExtractTextShadows),
    );
}

/// Applies the fixed MainCore text treatment to every UI text entity, including
/// text spawned later by dialogs, choices, previews, and the title screen.
pub fn apply_text_shadows(texts: Query<Entity, TextShadowTarget>, mut commands: Commands) {
    for entity in &texts {
        commands.entity(entity).insert((
            TextBackdrop,
            SoftTextShadow {
                radius: 2.4,
                color: Color::srgba(0.0, 0.0, 0.0, 0.38),
            },
        ));
    }
}

fn extract_soft_text_shadows(
    mut commands: Commands,
    mut extracted: ResMut<ExtractedUiNodes>,
    nodes: Extract<SoftShadowNodeQuery>,
    cameras: Extract<UiCameraMap>,
) {
    let mut camera_mapper = cameras.get_mapper();
    for (
        entity,
        node,
        stack_index,
        global_transform,
        target,
        inherited_visibility,
        maybe_clip,
        layout,
        text_color,
        shadow,
        text_scroll,
    ) in &nodes
    {
        if !inherited_visibility.get() || node.is_empty() || text_color.0.alpha() <= 0.0 {
            continue;
        }
        let Some(camera) = camera_mapper.map(target) else {
            continue;
        };
        let color = shadow
            .color
            .with_alpha(shadow.color.alpha() * text_color.0.alpha());
        let radius = shadow.radius / node.inverse_scale_factor();
        let diagonal = radius * 0.8;
        let offsets = [
            Vec2::new(-diagonal, -diagonal),
            Vec2::new(diagonal, -diagonal),
            Vec2::new(-diagonal, diagonal),
            Vec2::new(diagonal, diagonal),
        ];
        let clip = text_clip(node, global_transform, maybe_clip, text_scroll, radius);

        for offset in offsets {
            let transform = Affine2::from(*global_transform)
                * Affine2::from_translation(
                    node.content_box().min + offset
                        - text_scroll.map_or(Vec2::ZERO, |scroll| scroll.0),
                );
            extract_glyph_run(
                &mut commands,
                &mut extracted,
                layout,
                color,
                transform,
                clip,
                camera,
                entity,
                stack_index.0 as f32 + stack_z_offsets::TEXT,
            );
        }
    }
}

fn text_clip(
    node: &ComputedNode,
    transform: &UiGlobalTransform,
    maybe_clip: Option<&CalculatedClip>,
    scroll: Option<&TextScroll>,
    shadow_radius: f32,
) -> Option<Rect> {
    if scroll.is_some() {
        let content = node.content_box();
        let text_clip = Rect::from_center_size(
            transform.affine().translation + content.center(),
            content.size(),
        );
        Some(maybe_clip.map_or(text_clip, |clip| clip.clip.intersect(text_clip)))
    } else {
        maybe_clip.map(|clip| Rect {
            min: clip.clip.min - Vec2::splat(shadow_radius),
            max: clip.clip.max + Vec2::splat(shadow_radius),
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn extract_glyph_run(
    commands: &mut Commands,
    extracted: &mut ExtractedUiNodes,
    layout: &TextLayoutInfo,
    color: Color,
    transform: Affine2,
    clip: Option<Rect>,
    camera: Entity,
    entity: Entity,
    z_order: f32,
) {
    let mut start = extracted.glyphs.len();
    for (index, glyph) in layout.glyphs.iter().enumerate() {
        let end = extracted.glyphs.len() + 1;
        extracted.glyphs.push(ExtractedGlyph {
            color: color.into(),
            translation: glyph.position,
            rect: glyph.atlas_info.rect,
        });
        if layout.glyphs.get(index + 1).is_none_or(|next| {
            next.section_index != glyph.section_index
                || next.atlas_info.texture != glyph.atlas_info.texture
        }) {
            extracted.uinodes.push(ExtractedUiNode {
                transform,
                z_order,
                render_entity: commands.spawn(TemporaryRenderEntity).id(),
                image: glyph.atlas_info.texture,
                clip,
                extracted_camera_entity: camera,
                item: ExtractedUiItem::Glyphs { range: start..end },
                main_entity: entity.into(),
            });
            start = end;
        }
    }
}
