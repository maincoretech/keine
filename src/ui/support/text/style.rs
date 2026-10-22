use bevy::camera::visibility::InheritedVisibility;
use bevy::math::Affine2;
use bevy::prelude::*;
use bevy::render::{Extract, ExtractSchedule, RenderApp, sync_world::MainEntity};
use bevy::text::EditableText;
use bevy::text::TextLayoutInfo;
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
        Option<&'static EditableText>,
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
        if !extracted.changed.contains(&MainEntity::from(entity)) {
            continue;
        }
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
                        - text_scroll.map_or(Vec2::ZERO, |scroll| scroll.viewport.offset),
                );
            extract_glyph_run(
                &mut commands,
                &mut extracted,
                layout,
                color,
                transform,
                clip.clone(),
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
    scroll: Option<&EditableText>,
    shadow_radius: f32,
) -> Option<CalculatedClip> {
    if scroll.is_some() {
        Some(
            maybe_clip
                .cloned()
                .unwrap_or_default()
                .with_rect(node.content_box(), transform),
        )
    } else {
        maybe_clip.cloned().map(|mut clip| {
            if let CalculatedClip::Rects(rects) = &mut clip {
                for region in rects {
                    let matrix = region.world_to_clip_local.matrix2;
                    let padding = (matrix.x_axis.abs() + matrix.y_axis.abs()) * shadow_radius;
                    region.rect.min -= padding;
                    region.rect.max += padding;
                }
            }
            clip
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
    clip: Option<CalculatedClip>,
    camera: Entity,
    entity: Entity,
    z_order: f32,
) {
    let mut glyphs = Vec::new();
    for (index, glyph) in layout.glyphs.iter().enumerate() {
        glyphs.push(ExtractedGlyph {
            color: color.into(),
            translation: glyph.position,
            rect: glyph.atlas_info.rect,
        });
        if layout.glyphs.get(index + 1).is_none_or(|next| {
            next.section_index != glyph.section_index
                || next.atlas_info.texture != glyph.atlas_info.texture
        }) {
            extracted
                .uinodes
                .entry(entity.into())
                .or_insert_with(|| (camera, default()))
                .1
                .insert(
                    commands.spawn_empty().id(),
                    ExtractedUiNode {
                        transform,
                        z_order,
                        image: glyph.atlas_info.texture,
                        clip: clip.clone(),
                        item: ExtractedUiItem::Glyphs {
                            glyphs: std::mem::take(&mut glyphs),
                        },
                    },
                );
        }
    }
}
