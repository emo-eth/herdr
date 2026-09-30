use super::*;
use crate::protocol::{CellData, PaneSurfacePatch, PaneSurfacePatchRow};

fn cell_with_symbol(symbol: &'static str) -> CellData {
    CellData {
        symbol: symbol.into(),
        fg: 0,
        bg: 0,
        modifier: 0,
        skip: false,
        hyperlink: None,
    }
}

#[test]
fn newer_pending_surface_does_not_block_current_compose() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    let _ = state.compose(100, 30).expect("initial compose");

    let mut pending = surface();
    pending.surface_revision = 2;
    pending.frame.cells[0].symbol = "Z".into();
    state.pending_pane_surface = Some(pending);

    let frame = state
        .compose(100, 30)
        .expect("compose must still render current surface");
    assert_ne!(frame.cells[0].symbol, "Z");
}

#[test]
fn rejected_surface_patch_requests_a_single_resync() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    let _ = state.compose(100, 30).expect("initial compose");

    let skipped = PaneSurfacePatch {
        boot_id: "boot-1".into(),
        projection_revision: 1,
        base_surface_revision: 99,
        surface_revision: 100,
        rows: Vec::new(),
        panes: Vec::new(),
        cursor: None,
    };
    assert!(matches!(
        state.apply_pane_surface_patch(skipped),
        crate::client::shell::surface_patch::ClientPaneSurfacePatchOutcome::Rejected
    ));
    assert!(state.begin_surface_resync());
    assert!(!state.begin_surface_resync());

    let mut seed = surface();
    seed.surface_revision = 2;
    state.set_pane_surface(seed);
    assert!(state.begin_surface_resync());
}

#[test]
fn sparse_pane_metadata_still_accepts_rows_for_other_panes() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    let mut current = surface();
    current.frame.width = 8;
    current.frame.height = 2;
    current.frame.cells = vec![cell_with_symbol(" "); 16];
    current.panes[0].rect.width = 4;
    current.panes[0].inner_rect.width = 4;
    let mut pane_b = current.panes[0].clone();
    pane_b.pane_id = "pane_2".into();
    pane_b.rect.x = 4;
    pane_b.inner_rect.x = 4;
    current.panes.push(pane_b);
    state.set_pane_surface(current);
    let _ = state.compose(100, 30).expect("compose two panes");

    let mut pane_a = surface().panes.remove(0);
    pane_a.rect.width = 4;
    pane_a.inner_rect.width = 4;
    pane_a.mouse_reporting = true;
    let patch = PaneSurfacePatch {
        boot_id: "boot-1".into(),
        projection_revision: 1,
        base_surface_revision: 1,
        surface_revision: 2,
        rows: vec![
            PaneSurfacePatchRow {
                x: 0,
                y: 0,
                cells: vec![cell_with_symbol("A"), cell_with_symbol("A")],
            },
            PaneSurfacePatchRow {
                x: 4,
                y: 0,
                cells: vec![cell_with_symbol("B"), cell_with_symbol("B")],
            },
        ],
        panes: vec![pane_a],
        cursor: None,
    };
    assert!(matches!(
        state.apply_pane_surface_patch(patch),
        crate::client::shell::surface_patch::ClientPaneSurfacePatchOutcome::Applied(_)
    ));
    let surface = state.pane_surface.as_ref().expect("patched surface");
    assert_eq!(surface.frame.cells[0].symbol, "A");
    assert_eq!(surface.frame.cells[4].symbol, "B");
    assert!(surface.panes[0].mouse_reporting);
}
