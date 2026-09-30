use super::*;
use crate::protocol::delta::{
    ClientShellSurfaceDelta, PaneSurfacePaneDelta, SurfaceFieldUpdate, SurfaceGraphicsDelta,
};
use crate::protocol::{
    CellData, PaneSurfacePatchRow, SurfaceGraphicsAsset, SurfaceGraphicsAssetKey,
    SurfaceGraphicsFormat, SurfaceGraphicsPlacement, SurfaceGraphicsSource, SurfaceGraphicsTarget,
};

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

fn _cell_with_link(symbol: &'static str, link_idx: u32) -> CellData {
    CellData {
        symbol: symbol.into(),
        fg: 0,
        bg: 0,
        modifier: 0,
        skip: false,
        hyperlink: Some(link_idx),
    }
}

#[test]
fn p3_metadata_overtakes_p2_empty_rebind_flushes_accumulated_damage_and_facts() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let mut initial_snapshot = snapshot();
    initial_snapshot.revision = 2;
    state.set_snapshot(Box::new(initial_snapshot));

    let mut initial_surface = surface();
    initial_surface.projection_revision = 2;
    initial_surface.surface_revision = 10;
    initial_surface.panes[0].content_revision = 100;
    state.set_pane_surface(initial_surface);
    let _ = state.compose(100, 30).expect("initial compose");

    // Initial presented content_revision
    assert_eq!(state.hits.panes[0].content_revision, 100);

    // 1. Metadata advances to P3 before S11 arrives
    let mut p3_snapshot = snapshot();
    p3_snapshot.revision = 3;
    state.set_snapshot(Box::new(p3_snapshot));

    // 2. Surface delta arrives with P2: S10 -> S11 with spans on row 0 and pane content_revision: 101
    let delta_s11 = ClientShellSurfaceDelta {
        boot_id: "boot-1".into(),
        projection_revision: 2,
        base_surface_revision: 10,
        surface_revision: 11,
        spans: vec![PaneSurfacePatchRow {
            x: 0,
            y: 0,
            cells: vec![cell_with_symbol("A"), cell_with_symbol("B")],
        }],
        row_moves: Vec::new(),
        panes: vec![PaneSurfacePaneDelta {
            pane_id: "pane_1".into(),
            content_revision: SurfaceFieldUpdate::Set(101),
            scroll: SurfaceFieldUpdate::Unchanged,
            focused: SurfaceFieldUpdate::Unchanged,
            mouse_reporting: SurfaceFieldUpdate::Set(true),
            sgr_pixel_mouse: SurfaceFieldUpdate::Unchanged,
            alternate_screen_active: SurfaceFieldUpdate::Unchanged,
            scrollbar_rect: SurfaceFieldUpdate::Unchanged,
            rect: SurfaceFieldUpdate::Unchanged,
            inner_rect: SurfaceFieldUpdate::Unchanged,
            pixel_width: SurfaceFieldUpdate::Unchanged,
            pixel_height: SurfaceFieldUpdate::Unchanged,
        }],
        splits: None,
        cursor: SurfaceFieldUpdate::Unchanged,
        appended_hyperlinks: Vec::new(),
        graphics: None,
        popup: None,
    };

    // Delta is applied to received state, but not presented because projection (2) != snapshot (3)
    let outcome1 = state.apply_surface_delta(delta_s11);
    assert!(matches!(
        outcome1,
        crate::client::shell::surface_patch::ClientPaneSurfacePatchOutcome::Applied(None)
    ));
    assert_eq!(state.unpresented_damage.get(&0), Some(&(0, 2)));
    // 3. Empty rebind arrives: P3: S11 -> S12 (no spans, no pane field updates, projection_revision: 3)
    let delta_s12 = ClientShellSurfaceDelta {
        boot_id: "boot-1".into(),
        projection_revision: 3,
        base_surface_revision: 11,
        surface_revision: 12,
        spans: Vec::new(),
        row_moves: Vec::new(),
        panes: Vec::new(),
        splits: None,
        cursor: SurfaceFieldUpdate::Unchanged,
        appended_hyperlinks: Vec::new(),
        graphics: None,
        popup: None,
    };

    let outcome2 = state.apply_surface_delta(delta_s12);
    let crate::client::shell::surface_patch::ClientPaneSurfacePatchOutcome::Applied(Some(patch)) =
        outcome2
    else {
        panic!("expected composed patch on matching rebind");
    };

    // All accumulated intervals from S11 are included in the patch
    assert_eq!(patch.rows.len(), 1);
    assert_eq!(patch.rows[0].y, state.layout(100, 30).pane_surface.y);
    assert_eq!(patch.rows[0].cells[0].symbol, "A");
    assert_eq!(patch.rows[0].cells[1].symbol, "B");

    // Pending metadata facts from S11 (content_revision: 101, mouse_reporting: true) are staged until presentation success
    state.commit_presentation_success();
    assert_eq!(state.hits.panes[0].content_revision, 101);
    assert!(state.hits.panes[0].mouse_reporting);
    assert!(state.unpresented_damage.is_empty());
}

#[test]
fn matching_rebind_with_new_spans_on_distinct_row_merges_all_damage() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let mut initial_snapshot = snapshot();
    initial_snapshot.revision = 2;
    state.set_snapshot(Box::new(initial_snapshot));

    let mut initial_surface = surface();
    initial_surface.projection_revision = 2;
    initial_surface.surface_revision = 10;
    state.set_pane_surface(initial_surface);
    let _ = state.compose(100, 30).expect("initial compose");

    // Metadata advances to P3
    let mut p3_snapshot = snapshot();
    p3_snapshot.revision = 3;
    state.set_snapshot(Box::new(p3_snapshot));

    // Delta 1: P2: S10 -> S11 with spans on row 0
    let delta_s11 = ClientShellSurfaceDelta {
        boot_id: "boot-1".into(),
        projection_revision: 2,
        base_surface_revision: 10,
        surface_revision: 11,
        spans: vec![PaneSurfacePatchRow {
            x: 0,
            y: 0,
            cells: vec![cell_with_symbol("X")],
        }],
        row_moves: Vec::new(),
        panes: Vec::new(),
        splits: None,
        cursor: SurfaceFieldUpdate::Unchanged,
        appended_hyperlinks: Vec::new(),
        graphics: None,
        popup: None,
    };
    let _ = state.apply_surface_delta(delta_s11);

    // Delta 2: P3: S11 -> S12 has new spans on a distinct row (row 1)
    let delta_s12 = ClientShellSurfaceDelta {
        boot_id: "boot-1".into(),
        projection_revision: 3,
        base_surface_revision: 11,
        surface_revision: 12,
        spans: vec![PaneSurfacePatchRow {
            x: 2,
            y: 1,
            cells: vec![cell_with_symbol("Y")],
        }],
        row_moves: Vec::new(),
        panes: Vec::new(),
        splits: None,
        cursor: SurfaceFieldUpdate::Unchanged,
        appended_hyperlinks: Vec::new(),
        graphics: None,
        popup: None,
    };

    let outcome = state.apply_surface_delta(delta_s12);
    let crate::client::shell::surface_patch::ClientPaneSurfacePatchOutcome::Applied(Some(patch)) =
        outcome
    else {
        panic!("expected composed patch merging old and new damage");
    };

    // Both row 0 (old unpresented) and row 1 (new delta) are present in the patch
    let pane_y = state.layout(100, 30).pane_surface.y;
    assert!(patch.rows.iter().any(|r| r.y == pane_y));
    assert!(patch.rows.iter().any(|r| r.y == pane_y + 1));
}

#[test]
fn presentation_rejection_retains_pending_damage() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    let _ = state.compose(100, 30).expect("initial compose");

    // Delta with damage on row 0
    let delta = ClientShellSurfaceDelta {
        boot_id: "boot-1".into(),
        projection_revision: 1,
        base_surface_revision: 1,
        surface_revision: 2,
        spans: vec![PaneSurfacePatchRow {
            x: 0,
            y: 0,
            cells: vec![cell_with_symbol("Z")],
        }],
        row_moves: Vec::new(),
        panes: Vec::new(),
        splits: None,
        cursor: SurfaceFieldUpdate::Unchanged,
        appended_hyperlinks: Vec::new(),
        graphics: None,
        popup: None,
    };
    let _ = state.apply_surface_delta(delta);

    // Damage on row 0 is recorded
    assert!(state.unpresented_damage.contains_key(&0));

    // If presentation is not confirmed (no clear_unpresented_damage called), damage remains
    assert_eq!(state.unpresented_damage.get(&0), Some(&(0, 1)));
}

fn rgba_asset_and_placement() -> (SurfaceGraphicsAsset, SurfaceGraphicsPlacement) {
    let key = SurfaceGraphicsAssetKey {
        source: SurfaceGraphicsSource::Terminal {
            target: SurfaceGraphicsTarget::Pane {
                pane_id: "pane_1".into(),
            },
            image_id: 419,
        },
        image_width: 1,
        image_height: 1,
        format: SurfaceGraphicsFormat::Rgba,
        data_len: 4,
        data_fingerprint: 419,
    };
    (
        SurfaceGraphicsAsset {
            key: key.clone(),
            data: vec![255, 0, 0, 255],
        },
        SurfaceGraphicsPlacement {
            asset: key,
            logical_placement_id: 1,
            x: 0,
            y: 0,
            cols: 8,
            rows: 3,
            source_x: 0,
            source_y: 0,
            source_width: 1,
            source_height: 1,
            x_offset: 0,
            y_offset: 0,
            z: 0,
            scrollback_offset: 0,
        },
    )
}

#[test]
fn graphics_delta_forces_full_compose_so_kitty_bytes_are_encoded() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    let initial = state.compose(100, 30).expect("initial compose");
    assert!(
        !String::from_utf8_lossy(&initial.graphics.into_inline_bytes()).contains("a=t"),
        "seed frame must not already transmit"
    );

    let (asset, placement) = rgba_asset_and_placement();
    let delta = ClientShellSurfaceDelta {
        boot_id: "boot-1".into(),
        projection_revision: 1,
        base_surface_revision: 1,
        surface_revision: 2,
        spans: vec![PaneSurfacePatchRow {
            x: 0,
            y: 0,
            cells: vec![cell_with_symbol("I")],
        }],
        row_moves: Vec::new(),
        panes: Vec::new(),
        splits: None,
        cursor: SurfaceFieldUpdate::Unchanged,
        appended_hyperlinks: Vec::new(),
        graphics: Some(SurfaceGraphicsDelta {
            added_assets: vec![asset],
            removed_assets: Vec::new(),
            added_placements: vec![placement],
            removed_placements: Vec::new(),
            retained_assets: Vec::new(),
        }),
        popup: None,
    };
    let outcome = state.apply_surface_delta(delta);
    assert!(
        matches!(
            outcome,
            crate::client::shell::surface_patch::ClientPaneSurfacePatchOutcome::Applied(None)
        ),
        "graphics deltas must skip the cell-only blit path"
    );

    let frame = state.compose(100, 30).expect("graphics compose");
    let gfx_bytes = frame.graphics.into_inline_bytes();
    let gfx = String::from_utf8_lossy(&gfx_bytes);
    assert!(
        gfx.contains("a=t") || gfx.contains("a=T"),
        "expected Kitty transmit, got {gfx:?}"
    );
}

#[test]
fn newer_pending_surface_does_not_block_current_compose() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    let _ = state.compose(100, 30).expect("initial compose");

    let mut future = surface();
    future.projection_revision = 2;
    future.surface_revision = 2;
    state.set_pane_surface(future);
    assert!(state.pending_pane_surface.is_none());
    assert_eq!(
        state
            .pane_surface
            .as_ref()
            .map(|surface| surface.projection_revision),
        Some(2)
    );

    let frame = state
        .compose(100, 30)
        .expect("successor surface must still compose before its snapshot");
    assert_eq!(frame.width, 100);
}

#[test]
fn surface_projection_ahead_of_snapshot_still_composes() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let mut initial_snapshot = snapshot();
    initial_snapshot.revision = 1;
    state.set_snapshot(Box::new(initial_snapshot));
    let mut initial_surface = surface();
    initial_surface.projection_revision = 1;
    state.set_pane_surface(initial_surface);
    let _ = state.compose(100, 30).expect("initial compose");

    let ahead = ClientShellSurfaceDelta {
        boot_id: "boot-1".into(),
        projection_revision: 2,
        base_surface_revision: 1,
        surface_revision: 2,
        spans: vec![PaneSurfacePatchRow {
            x: 0,
            y: 0,
            cells: vec![cell_with_symbol("X")],
        }],
        row_moves: Vec::new(),
        panes: Vec::new(),
        splits: None,
        cursor: SurfaceFieldUpdate::Unchanged,
        appended_hyperlinks: Vec::new(),
        graphics: None,
        popup: None,
    };
    assert!(matches!(
        state.apply_surface_delta(ahead),
        crate::client::shell::surface_patch::ClientPaneSurfacePatchOutcome::Applied(None)
    ));
    assert_eq!(
        state
            .pane_surface
            .as_ref()
            .map(|surface| surface.projection_revision),
        Some(2)
    );
    let frame = state
        .compose(100, 30)
        .expect("surface ahead of snapshot must still present");
    assert_eq!(frame.width, 100);
}

#[test]
fn snapshot_skip_installs_pending_successor_so_receive_chain_continues() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let mut initial_snapshot = snapshot();
    initial_snapshot.revision = 1;
    state.set_snapshot(Box::new(initial_snapshot));
    let mut initial_surface = surface();
    initial_surface.projection_revision = 1;
    initial_surface.surface_revision = 1;
    state.set_pane_surface(initial_surface);
    let _ = state.compose(100, 30).expect("initial compose");

    let mut successor = surface();
    successor.projection_revision = 2;
    successor.surface_revision = 2;
    successor.frame.cells[0].symbol = "Q".into();
    state.set_pane_surface(successor);
    assert!(state.pending_pane_surface.is_none());
    assert_eq!(
        state
            .pane_surface
            .as_ref()
            .map(|surface| surface.surface_revision),
        Some(2)
    );

    let mut skipped = snapshot();
    skipped.revision = 3;
    state.set_snapshot(Box::new(skipped));
    assert_eq!(
        state
            .pane_surface
            .as_ref()
            .map(|surface| surface.surface_revision),
        Some(2)
    );

    let follow = ClientShellSurfaceDelta {
        boot_id: "boot-1".into(),
        projection_revision: 3,
        base_surface_revision: 2,
        surface_revision: 3,
        spans: vec![PaneSurfacePatchRow {
            x: 0,
            y: 0,
            cells: vec![cell_with_symbol("Z")],
        }],
        row_moves: Vec::new(),
        panes: Vec::new(),
        splits: None,
        cursor: SurfaceFieldUpdate::Unchanged,
        appended_hyperlinks: Vec::new(),
        graphics: None,
        popup: None,
    };
    assert!(matches!(
        state.apply_surface_delta(follow),
        crate::client::shell::surface_patch::ClientPaneSurfacePatchOutcome::Applied(_)
    ));
    assert_eq!(
        state
            .pane_surface
            .as_ref()
            .map(|surface| surface.surface_revision),
        Some(3)
    );
    let _ = state
        .compose(100, 30)
        .expect("installed successor must remain presentable");
}

#[test]
fn rejected_surface_delta_requests_a_single_resync() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    let _ = state.compose(100, 30).expect("initial compose");

    let skipped = ClientShellSurfaceDelta {
        boot_id: "boot-1".into(),
        projection_revision: 1,
        base_surface_revision: 99,
        surface_revision: 100,
        spans: Vec::new(),
        row_moves: Vec::new(),
        panes: Vec::new(),
        splits: None,
        cursor: SurfaceFieldUpdate::Unchanged,
        appended_hyperlinks: Vec::new(),
        graphics: None,
        popup: None,
    };
    assert!(matches!(
        state.apply_surface_delta(skipped),
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
    let patch = crate::protocol::PaneSurfacePatch {
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

#[test]
fn v2_cell_delta_does_not_blit_through_an_unchanged_popup() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface_with_popup());
    let _ = state.compose(100, 30).expect("compose popup");
    let delta = ClientShellSurfaceDelta {
        boot_id: "boot-1".into(),
        projection_revision: 1,
        base_surface_revision: 1,
        surface_revision: 2,
        spans: vec![PaneSurfacePatchRow {
            x: 0,
            y: 0,
            cells: vec![cell_with_symbol("Z")],
        }],
        row_moves: Vec::new(),
        panes: Vec::new(),
        splits: None,
        cursor: SurfaceFieldUpdate::Unchanged,
        appended_hyperlinks: Vec::new(),
        graphics: None,
        popup: None,
    };
    assert!(matches!(
        state.apply_surface_delta(delta),
        crate::client::shell::surface_patch::ClientPaneSurfacePatchOutcome::Applied(None)
    ));
}
#[test]
fn v2_surface_delta_reconciles_copy_mode_selection_and_scroll_targets() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));

    let mut initial_surface = surface();
    initial_surface.projection_revision = 1;
    initial_surface.surface_revision = 10;
    initial_surface.panes[0].content_revision = 100;
    initial_surface.panes[0].scroll = Some(crate::protocol::PaneSurfaceScrollMetrics {
        offset_from_bottom: 0,
        max_offset_from_bottom: 50,
        viewport_rows: 24,
    });
    state.set_pane_surface(initial_surface.clone());
    let _ = state.compose(100, 30).expect("initial compose");

    // 1. Enter copy mode on pane_1 with content_revision 100 and a search match
    state.mode = ClientShellMode::Copy;
    state.copy_mode = Some(ClientCopyModeState {
        pane_id: "pane_1".into(),
        content_revision: 100,
        geometry: (80, 24),
        alternate_screen_active: false,
        cursor: crate::api::schema::PaneTextPoint { row: 0, col: 0 },
        offset_from_bottom: 0,
        max_offset_from_bottom: 50,
        entry_offset_from_bottom: 0,
        selection: None,
        search_prompt: None,
        search_query: "needle".into(),
        search_direction: Some(crate::api::schema::PaneCopySearchDirection::Forward),
        search_matches: vec![crate::api::schema::PaneTextRange {
            start: crate::api::schema::PaneTextPoint { row: 0, col: 0 },
            end: crate::api::schema::PaneTextPoint { row: 0, col: 6 },
        }],
        search_total: 1,
        search_current: Some(0),
        search_current_global: Some(0),
        search_generation: 1,
        copy_after_search: false,
    });

    // 2. Set an active scroll target for pane_1
    state.pane_scroll_targets.insert("pane_1".into(), 10);

    // 3. Set an active word selection gesture on pane_1
    let mut outcome = ClientShellInput::default();
    let hit = state.hits.panes[0].clone();
    state.request_word_selection(&hit, 0, 2, &mut outcome);
    assert!(state.word_selection_gesture.is_some());
    state.selection = Some(crate::selection::Selection::absolute_range(
        "pane_1".into(),
        (0, 0),
        (0, 4),
    ));

    let mut next_pane = initial_surface.panes[0].clone();
    next_pane.content_revision = 101;
    next_pane.scroll = Some(crate::protocol::PaneSurfaceScrollMetrics {
        offset_from_bottom: 10,
        max_offset_from_bottom: 50,
        viewport_rows: 24,
    });
    let pane_delta = PaneSurfacePaneDelta::diff(&initial_surface.panes[0], &next_pane).unwrap();

    // 4. Apply a v2 surface delta with updated content_revision (101) and scroll offset reaching the target (10)
    let delta = ClientShellSurfaceDelta {
        boot_id: "boot-1".into(),
        projection_revision: 1,
        base_surface_revision: 10,
        surface_revision: 11,
        spans: vec![PaneSurfacePatchRow {
            x: 0,
            y: 0,
            cells: vec![cell_with_symbol("N")],
        }],
        row_moves: Vec::new(),
        panes: vec![pane_delta],
        splits: None,
        cursor: SurfaceFieldUpdate::Unchanged,
        appended_hyperlinks: Vec::new(),
        graphics: None,
        popup: None,
    };

    assert!(matches!(
        state.apply_surface_delta(delta),
        crate::client::shell::surface_patch::ClientPaneSurfacePatchOutcome::Applied(_)
    ));

    // Acknowledged scroll target must be cleared
    assert_eq!(state.pane_scroll_targets.get("pane_1"), None);

    // Obsolete word selection gesture and selection must be cleared on content change
    assert!(state.word_selection_gesture.is_none());
    assert!(state.selection.is_none());

    // Copy mode state must be updated to the new revision, stale search cleared, search_generation bumped
    let copy = state.copy_mode.as_ref().expect("copy mode active");
    assert_eq!(copy.content_revision, 101);
    assert_eq!(copy.search_matches, Vec::new());
    assert_eq!(copy.search_total, 0);
    assert_eq!(copy.search_generation, 2);

    // A stale search result for the old revision 100 or old generation 1 must be rejected
    let mut search_outcome = ClientShellInput::default();
    let stale_result = ClientCopySearchResult {
        content_revision: 100,
        matches: vec![crate::api::schema::PaneTextRange {
            start: crate::api::schema::PaneTextPoint { row: 0, col: 0 },
            end: crate::api::schema::PaneTextPoint { row: 0, col: 6 },
        }],
        total: 1,
        current: Some(0),
        current_global: Some(0),
    };
    assert!(!state.apply_copy_search_result(
        "pane_1",
        crate::api::schema::PaneTextPoint { row: 0, col: 0 },
        "needle".into(),
        crate::api::schema::PaneCopySearchDirection::Forward,
        false,
        1,
        stale_result,
        &mut search_outcome,
    ));

    // A current search result for revision 101 and generation 2 must be accepted
    let fresh_result = ClientCopySearchResult {
        content_revision: 101,
        matches: vec![crate::api::schema::PaneTextRange {
            start: crate::api::schema::PaneTextPoint { row: 0, col: 0 },
            end: crate::api::schema::PaneTextPoint { row: 0, col: 6 },
        }],
        total: 1,
        current: Some(0),
        current_global: Some(0),
    };
    assert!(state.apply_copy_search_result(
        "pane_1",
        crate::api::schema::PaneTextPoint { row: 0, col: 0 },
        "needle".into(),
        crate::api::schema::PaneCopySearchDirection::Forward,
        false,
        2,
        fresh_result,
        &mut search_outcome,
    ));
    assert_eq!(state.copy_mode.as_ref().unwrap().search_total, 1);
}

#[test]
fn v2_surface_delta_in_terminal_mode_clears_word_selection_and_removes_highlighting() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));

    let mut initial_surface = surface();
    initial_surface.projection_revision = 1;
    initial_surface.surface_revision = 10;
    initial_surface.panes[0].content_revision = 100;
    state.set_pane_surface(initial_surface.clone());
    let initial_frame = state.compose(100, 30).expect("initial compose");

    // In Terminal mode, start a word selection on pane_1 covering cells 0..4
    assert_eq!(state.mode, ClientShellMode::Terminal);
    let mut outcome = ClientShellInput::default();
    let hit = state.hits.panes[0].clone();
    state.request_word_selection(&hit, 0, 2, &mut outcome);
    assert!(state.word_selection_gesture.is_some());
    state.selection = Some(crate::selection::Selection::absolute_range(
        "pane_1".into(),
        (0, 0),
        (0, 4),
    ));

    // Compose with selection active: cells 0..4 should have selection highlighting
    let frame_with_selection = state.compose(100, 30).expect("compose with selection");
    let cell_idx = |col: u16, row: u16| {
        usize::from(hit.inner_rect.y + row) * usize::from(frame_with_selection.width)
            + usize::from(hit.inner_rect.x + col)
    };
    // Cell (2, 0) should have selection highlighting distinct from the unselected initial frame
    assert_ne!(
        frame_with_selection.cells[cell_idx(2, 0)].bg,
        initial_frame.cells[cell_idx(2, 0)].bg
    );

    // Apply a v2 surface delta with updated content revision (101), but only a 1-cell span update at x=0
    let mut next_pane = initial_surface.panes[0].clone();
    next_pane.content_revision = 101;
    let pane_delta = PaneSurfacePaneDelta::diff(&initial_surface.panes[0], &next_pane).unwrap();
    let delta = ClientShellSurfaceDelta {
        boot_id: "boot-1".into(),
        projection_revision: 1,
        base_surface_revision: 10,
        surface_revision: 11,
        spans: vec![PaneSurfacePatchRow {
            x: 0,
            y: 0,
            cells: vec![cell_with_symbol("N")],
        }],
        row_moves: Vec::new(),
        panes: vec![pane_delta],
        splits: None,
        cursor: SurfaceFieldUpdate::Unchanged,
        appended_hyperlinks: Vec::new(),
        graphics: None,
        popup: None,
    };
    let patch_outcome = state.apply_surface_delta(delta);
    assert!(
        matches!(
            patch_outcome,
            crate::client::shell::surface_patch::ClientPaneSurfacePatchOutcome::Applied(None)
        ),
        "cleared selection decoration must force composition rather than direct-blitting a partial span"
    );
    // Both word selection gesture and selection must be cleared
    assert!(state.word_selection_gesture.is_none());
    assert!(state.selection.is_none());
    let composed_frame = state.compose(100, 30).expect("compose after delta");
    assert_eq!(
        composed_frame.cells[cell_idx(2, 0)].bg,
        initial_frame.cells[cell_idx(2, 0)].bg,
        "selection styling must be completely removed from previously highlighted cells"
    );
    assert_eq!(
        composed_frame.cells[cell_idx(2, 0)].fg,
        initial_frame.cells[cell_idx(2, 0)].fg,
    );
}
