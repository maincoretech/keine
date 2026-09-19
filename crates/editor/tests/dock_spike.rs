use gpui_kit::base::Placement;
use gpui_kit::component::dock::{
    BasePanel, DockArea, DockLayout, DockPlacement, DockSkin, InsertTarget, PaneRef, Panel,
    PanelEvent, PanelId, panel_handle,
};
use gpui_kit::{
    App, AppContext, Context, Entity, EventEmitter, FocusHandle, Focusable, IntoElement, Render,
    TestAppContext, Window,
};

struct Probe {
    name: &'static str,
    focus: FocusHandle,
}

impl Probe {
    fn new(name: &'static str, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| Self {
            name,
            focus: cx.focus_handle(),
        })
    }
}

impl BasePanel for Probe {
    fn panel_name(&self) -> &'static str {
        self.name
    }
}

impl Panel for Probe {}
impl EventEmitter<PanelEvent> for Probe {}

impl Focusable for Probe {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.name
    }
}

#[gpui_kit::test]
fn dock_reorders_splits_and_rejects_a_stale_drop(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);
    let (area, cx) = cx.add_window_view(|window, cx| {
        DockArea::new("phase0-test", Some(1), window, cx).with_renderer(DockSkin::new(cx))
    });

    let (alpha, beta) = cx.update(|window, cx| {
        let alpha = Probe::new("alpha", cx);
        let beta = Probe::new("beta", cx);
        let ids = (
            PanelId::from(alpha.entity_id()),
            PanelId::from(beta.entity_id()),
        );
        let layout = DockLayout::tabs()
            .panel_view(panel_handle(alpha), cx)
            .panel_view(panel_handle(beta), cx);
        area.update(cx, |area, cx| area.set_center(layout, window, cx));
        ids
    });

    let original_group = area.read_with(cx, |area, _| {
        area.layout(DockPlacement::Center)
            .unwrap()
            .find_panel_node(alpha)
            .unwrap()
    });

    cx.update(|window, cx| {
        area.update(cx, |area, cx| {
            area.move_panel(
                beta,
                InsertTarget::Tabs {
                    node: original_group,
                    ix: Some(0),
                    activate: true,
                },
                window,
                cx,
            );
        });
    });
    area.read_with(cx, |area, _| {
        let group = area
            .layout(DockPlacement::Center)
            .unwrap()
            .find_node(original_group)
            .unwrap();
        let PaneRef::Tabs { panels, .. } = group.kind() else {
            panic!("expected a tab group")
        };
        assert_eq!(panels, [beta, alpha]);
    });

    cx.update(|window, cx| {
        area.update(cx, |area, cx| {
            area.move_panel(
                beta,
                InsertTarget::Split {
                    node: original_group,
                    placement: Placement::Right,
                    size: None,
                },
                window,
                cx,
            );
        });
    });
    let stale_group = area.read_with(cx, |area, _| {
        let tree = area.layout(DockPlacement::Center).unwrap();
        let PaneRef::Split { axis, children, .. } = tree.root().kind() else {
            panic!("expected split root")
        };
        assert_eq!(axis, gpui_kit::Axis::Horizontal);
        assert_eq!(children.len(), 2);
        tree.find_panel_node(beta).unwrap()
    });

    cx.update(|window, cx| {
        area.update(cx, |area, cx| {
            area.move_panel(
                beta,
                InsertTarget::Tabs {
                    node: original_group,
                    ix: None,
                    activate: true,
                },
                window,
                cx,
            );
        });
    });
    let before_invalid_drop = area.read_with(cx, |area, _| {
        area.layout(DockPlacement::Center).unwrap().clone()
    });
    cx.update(|window, cx| {
        area.update(cx, |area, cx| {
            area.move_panel(
                beta,
                InsertTarget::Tabs {
                    node: stale_group,
                    ix: None,
                    activate: true,
                },
                window,
                cx,
            );
        });
    });
    area.read_with(cx, |area, _| {
        assert_eq!(
            area.layout(DockPlacement::Center).unwrap(),
            &before_invalid_drop,
            "a stale/cancelled drop must leave the layout unchanged"
        );
    });
}
