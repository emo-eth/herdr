use super::*;

#[test]
fn selection_repaint_cadence_keeps_one_deadline_and_flushes_when_input_stops() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let now = std::time::Instant::now();
    let ms = std::time::Duration::from_millis;
    state.last_composed_at = Some(now);
    for elapsed in [1, 4, 8, 12, 15] {
        assert!(!state.request_selection_drag_repaint(now + ms(elapsed)));
        assert_eq!(state.selection_repaint_deadline, Some(now + ms(16)));
    }
    assert_eq!(state.timer_delay(now + ms(8)), ms(8));
    assert!(!state.tick_selection_autoscroll(now + ms(15)).repaint);
    assert!(state.tick_selection_autoscroll(now + ms(16)).repaint);
    assert!(state.selection_repaint_deadline.is_none());
    assert!(!state.tick_selection_autoscroll(now + ms(17)).repaint);
}

#[test]
fn selection_repaint_cadence_allows_immediate_paint_when_due() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let now = std::time::Instant::now();
    assert!(state.request_selection_drag_repaint(now));
    state.last_composed_at = Some(now);
    assert!(state.request_selection_drag_repaint(now + std::time::Duration::from_millis(16)));
    assert!(state.selection_repaint_deadline.is_none());
}

#[test]
fn selection_repaint_cadence_does_not_leave_work_after_another_composition() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let now = std::time::Instant::now();
    state.selection_repaint_deadline = Some(now);
    state.compose(106, 20).expect("frame");
    assert!(state.selection_repaint_deadline.is_none());
    assert!(!state.tick_selection_autoscroll(now).repaint);
}

#[test]
fn selection_release_copies_latest_position_before_deferred_paint() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("pane frame");
    let pane = state.hits.panes[0].clone();
    let mut mouse = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: pane.inner_rect.x,
        row: pane.inner_rect.y,
        modifiers: KeyModifiers::empty(),
    };
    state.handle_raw_events(vec![RawInputEvent::Mouse(mouse)]);
    mouse.kind = MouseEventKind::Drag(MouseButton::Left);
    mouse.column += 2;
    state.handle_raw_events(vec![RawInputEvent::Mouse(mouse)]);
    state.selection_repaint_deadline = Some(std::time::Instant::now());
    mouse.kind = MouseEventKind::Up(MouseButton::Left);
    let release = state.handle_raw_events(vec![RawInputEvent::Mouse(mouse)]);
    assert!(release.repaint);
    assert!(matches!(
        &release.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(&request.method,
                crate::api::schema::Method::PaneSelectionRead(params)
                    if params.cursor == crate::api::schema::PaneTextPoint { row: 0, col: 2 })
    ));
    state.compose(106, 20).expect("release frame");
    assert!(state.selection_repaint_deadline.is_none());
}

#[test]
fn ctrl_click_routes_link_activation_through_endpoint_then_client_host() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("pane frame");
    let pane = state.hits.panes[0].clone();
    let down = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: pane.inner_rect.x + 2,
        row: pane.inner_rect.y + 1,
        modifiers: KeyModifiers::CONTROL,
    };
    let activate = state.handle_raw_events(vec![RawInputEvent::Mouse(down)]);
    let [ClientShellAction::Endpoint { request, .. }] = &activate.actions[..] else {
        panic!("expected link activation request");
    };
    let request_id = request.id.clone();
    assert!(matches!(
        &request.method,
        crate::api::schema::Method::PaneLinkActivate(params)
            if params.pane_id == "pane_1" && params.viewport_row == 1 && params.col == 2
    ));

    let up = MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        ..down
    };
    let held = state.handle_raw_events(vec![RawInputEvent::Mouse(up)]);
    assert!(held.requests.is_empty() && held.actions.is_empty());
    let (_, actions) = state.handle_endpoint_result(
        "boot-1",
        &request_id,
        Ok(crate::api::schema::ResponseResult::PaneLinkActivated {
            url: Some("https://example.test".to_owned()),
            handled: false,
        }),
    );
    assert!(matches!(
        &actions[..],
        [ClientShellAction::OpenSafeWebUrl(url)] if url == "https://example.test"
    ));
    assert!(!state.url_click_consumes_until_up);
}

#[test]
fn ctrl_click_without_a_link_replays_the_original_gesture() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("pane frame");
    let pane = state.hits.panes[0].clone();
    let down = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: pane.inner_rect.x + 2,
        row: pane.inner_rect.y + 1,
        modifiers: KeyModifiers::CONTROL,
    };
    let activate = state.handle_raw_events(vec![RawInputEvent::Mouse(down)]);
    let request_id = match &activate.actions[..] {
        [ClientShellAction::Endpoint { request, .. }] => request.id.clone(),
        _ => panic!("expected link activation request"),
    };
    let (_, actions) = state.handle_endpoint_result(
        "boot-1",
        &request_id,
        Ok(crate::api::schema::ResponseResult::PaneLinkActivated {
            url: None,
            handled: false,
        }),
    );
    assert!(matches!(
        &actions[..],
        [ClientShellAction::ReplayMouse(events)] if events == &vec![down]
    ));
    let replay = match actions.into_iter().next().expect("replay action") {
        ClientShellAction::ReplayMouse(events) => state.replay_mouse_events(events),
        _ => unreachable!(),
    };
    assert!(matches!(
        &replay.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(request.method, crate::api::schema::Method::PaneFocus(_))
    ));
    assert!(state.selection.is_some());
}

#[test]
fn pane_split_drag_uses_projected_handle_and_stable_tab_path() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    let mut pane_surface = surface();
    pane_surface.splits.push(PaneSurfaceSplit {
        direction: PaneSurfaceSplitDirection::Horizontal,
        pos: 40,
        area: SurfaceRect {
            x: 0,
            y: 0,
            width: 80,
            height: 19,
        },
        hit_rect: SurfaceRect {
            x: 40,
            y: 0,
            width: 1,
            height: 19,
        },
        path: vec![false, true],
    });
    state.set_pane_surface(pane_surface);
    state.compose(106, 20).expect("split pane surface");
    let split = state.hits.pane_splits[0].clone();

    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: split.hit_rect.x,
        row: split.hit_rect.y + 2,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(matches!(
        state.chrome_drag,
        Some(ClientChromeDrag::PaneSplit { .. })
    ));
    let mut replacement = snapshot();
    replacement.revision = 2;
    replacement
        .tab_bar_right
        .push(crate::protocol::ClientShellTabStatusSegment {
            text: "updated".into(),
            accent: false,
        });
    let mut replacement_surface = surface();
    replacement_surface.projection_revision = 2;
    replacement_surface.splits.push(PaneSurfaceSplit {
        direction: PaneSurfaceSplitDirection::Horizontal,
        pos: 40,
        area: SurfaceRect {
            x: 0,
            y: 0,
            width: 80,
            height: 19,
        },
        hit_rect: SurfaceRect {
            x: 40,
            y: 0,
            width: 1,
            height: 19,
        },
        path: vec![false, true],
    });
    state.set_snapshot(Box::new(replacement));
    state.set_pane_surface(replacement_surface);
    let drag = state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: split.area.x + 48,
        row: split.hit_rect.y + 2,
        modifiers: KeyModifiers::empty(),
    })]);
    let [ClientShellAction::Endpoint { request, .. }] = &drag.actions[..] else {
        panic!("pane split drag should use endpoint API");
    };
    assert!(matches!(
        &request.method,
        crate::api::schema::Method::LayoutSetSplitRatio(params)
            if params.tab_id.as_deref() == Some("tab_1")
                && params.path == vec![false, true]
                && (params.ratio - 0.6).abs() < f32::EPSILON
    ));
    let release =
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: split.area.x + 48,
            row: split.hit_rect.y + 2,
            modifiers: KeyModifiers::empty(),
        })]);
    assert!(release.actions.is_empty());
    assert!(state.chrome_drag.is_none());
}

#[test]
fn disabled_mouse_chrome_keeps_tab_wheel_but_removes_split_drag_hits() {
    let mut config = Config::default();
    config.ui.mouse_capture = false;
    let mut projected = snapshot();
    let mut second_tab = projected.tabs[0].clone();
    second_tab.tab_id = "tab_2".into();
    second_tab.number = 2;
    second_tab.label = "2".into();
    second_tab.focused = false;
    projected.tabs.push(second_tab);
    let mut pane_surface = surface();
    pane_surface.splits.push(PaneSurfaceSplit {
        direction: PaneSurfaceSplitDirection::Horizontal,
        pos: 40,
        area: SurfaceRect {
            x: 0,
            y: 0,
            width: 80,
            height: 19,
        },
        hit_rect: SurfaceRect {
            x: 40,
            y: 0,
            width: 1,
            height: 19,
        },
        path: Vec::new(),
    });
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(pane_surface);
    state.compose(106, 20).expect("mouse-disabled shell");
    assert!(state.hits.pane_splits.is_empty());
    let first_tab = state.hits.tabs[0].0;
    let wheel = state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: first_tab.x,
        row: first_tab.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(matches!(
        &wheel.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                &request.method,
                crate::api::schema::Method::TabFocus(target) if target.tab_id == "tab_2"
            )
    ));
}

#[test]
fn client_double_click_selects_word_and_copies_only_after_release() {
    for (copy_on_select, release_before_response) in [(false, true), (true, false)] {
        let mut state = word_drag_state(copy_on_select);
        let initial = start_word_drag(&mut state);
        let release = MouseEventKind::Up(MouseButton::Left);
        if release_before_response {
            assert!(word_drag_mouse(&mut state, release, 0, 8)
                .actions
                .is_empty());
        }
        let mut actions = word_row_reply(&mut state, &initial, "alpha bravo charlie");
        if !release_before_response {
            assert!(actions.is_empty(), "holding the second press must not copy");
            assert!(state.selection.as_ref().unwrap().is_in_progress());
            state.tick_copy_feedback(std::time::Instant::now() + std::time::Duration::from_secs(1));
            assert!(state.selection.as_ref().unwrap().is_visible());
            assert!(state.copy_feedback.is_none());
            actions = word_drag_mouse(&mut state, release, 0, 8).actions;
        }
        assert!(state.selection.as_ref().unwrap().is_finalized());
        assert_eq!(
            state.selection.as_ref().unwrap().ordered_cells(),
            ((0, 6), (0, 10))
        );
        assert!(
            word_drag_mouse(&mut state, release, 0, 8)
                .actions
                .is_empty(),
            "copy only once"
        );
        if copy_on_select {
            assert!(
                matches!(&actions[..], [ClientShellAction::Endpoint { request, .. }]
                if matches!(&request.method, crate::api::schema::Method::PaneSelectionRead(params)
                    if params.anchor.col == 6 && params.cursor.col == 10))
            );
            let copied = word_row_reply(&mut state, &word_read_id(&actions), "bravo");
            assert!(
                matches!(&copied[..], [ClientShellAction::ClipboardWrite(bytes)] if bytes == b"bravo")
            );
            assert!(state.tick_copy_feedback(state.selection_highlight_clear_deadline.unwrap()));
            assert!(state.selection.is_none());
        } else {
            assert!(actions.is_empty(), "manual selection must not auto-copy");
            state.tick_copy_feedback(std::time::Instant::now() + std::time::Duration::from_secs(1));
            assert!(
                state.selection.is_some(),
                "manual selection must not expire"
            );
        }
    }
}

fn word_drag_state(copy_on_select: bool) -> ClientShellState {
    let mut config = Config::default();
    config.ui.copy_on_select = copy_on_select;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&config));
    state.set_snapshot(Box::new(snapshot()));
    let mut pane_surface = surface();
    let buffer = Buffer::with_lines([
        "alpha bravo charlie",
        "delta echo foxtrot ",
        "golf hotel india   ",
    ]);
    pane_surface.frame = FrameData::from_ratatui_buffer_with_hyperlinks(&buffer, None, &[]);
    pane_surface.panes[0].rect.width = 19;
    pane_surface.panes[0].rect.height = 3;
    pane_surface.panes[0].inner_rect = pane_surface.panes[0].rect;
    state.set_pane_surface(pane_surface);
    state.compose(106, 20).expect("composed frame");
    state
}

fn word_drag_mouse(
    state: &mut ClientShellState,
    kind: MouseEventKind,
    row: u16,
    col: u16,
) -> ClientShellInput {
    let pane = state.hits.panes[0].clone();
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind,
        column: pane.inner_rect.x + col,
        row: pane.inner_rect.y + row,
        modifiers: KeyModifiers::empty(),
    })])
}

fn word_read_id(actions: &[ClientShellAction]) -> String {
    actions
        .iter()
        .find_map(|action| match action {
            ClientShellAction::Endpoint { request, .. }
                if matches!(
                    request.method,
                    crate::api::schema::Method::PaneSelectionRead(_)
                ) =>
            {
                Some(request.id.clone())
            }
            _ => None,
        })
        .expect("selection read")
}

fn word_row_reply(state: &mut ClientShellState, id: &str, text: &str) -> Vec<ClientShellAction> {
    state
        .handle_endpoint_result(
            "boot-1",
            id,
            Ok(crate::api::schema::ResponseResult::PaneSelection {
                pane_id: "pane_1".into(),
                text: text.into(),
            }),
        )
        .1
}

fn start_word_drag(state: &mut ClientShellState) -> String {
    word_drag_mouse(state, MouseEventKind::Down(MouseButton::Left), 0, 8);
    word_drag_mouse(state, MouseEventKind::Up(MouseButton::Left), 0, 8);
    assert!(state.selection.is_none(), "plain clicks must not select");
    let second = word_drag_mouse(state, MouseEventKind::Down(MouseButton::Left), 0, 8);
    assert!(second.actions.iter().any(|action| matches!(action, ClientShellAction::Endpoint { request, .. }
        if matches!(&request.method, crate::api::schema::Method::PaneSelectionRead(params)
            if params.anchor.col == 0 && params.cursor.col == state.hits.panes[0].inner_rect.width - 1))));
    word_read_id(&second.actions)
}

#[test]
fn double_click_drag_selects_whole_words_in_both_directions() {
    let mut state = word_drag_state(false);
    let initial = start_word_drag(&mut state);
    word_row_reply(&mut state, &initial, "alpha bravo charlie");
    for (col, expected) in [
        (14, ((0, 6), (0, 18))),
        (2, ((0, 0), (0, 10))),
        (8, ((0, 6), (0, 10))),
        (11, ((0, 6), (0, 11))),
        (16, ((0, 6), (0, 18))),
    ] {
        let motion = word_drag_mouse(&mut state, MouseEventKind::Drag(MouseButton::Left), 0, col);
        assert!(
            motion.actions.is_empty(),
            "reuse the row while dragging within it"
        );
        assert_eq!(state.selection.as_ref().unwrap().ordered_cells(), expected);
    }
    assert!(
        word_drag_mouse(&mut state, MouseEventKind::Up(MouseButton::Left), 0, 16)
            .actions
            .is_empty()
    );
    assert!(state.selection.as_ref().unwrap().is_finalized());
}

#[test]
fn double_click_drag_waits_for_latest_row_before_copying() {
    for release_before_anchor in [false, true] {
        let mut state = word_drag_state(true);
        let initial = start_word_drag(&mut state);
        if !release_before_anchor {
            assert!(word_row_reply(&mut state, &initial, "alpha bravo charlie").is_empty());
        }
        let first_motion =
            word_drag_mouse(&mut state, MouseEventKind::Drag(MouseButton::Left), 1, 8);
        for col in [1, 3, 7] {
            assert!(
                word_drag_mouse(&mut state, MouseEventKind::Drag(MouseButton::Left), 2, col)
                    .actions
                    .is_empty()
            );
        }
        assert!(
            word_drag_mouse(&mut state, MouseEventKind::Up(MouseButton::Left), 2, 7)
                .actions
                .is_empty()
        );
        let final_read = if release_before_anchor {
            word_row_reply(&mut state, &initial, "alpha bravo charlie")
        } else {
            word_row_reply(
                &mut state,
                &word_read_id(&first_motion.actions),
                "delta echo foxtrot",
            )
        };
        assert!(
            matches!(&final_read[..], [ClientShellAction::Endpoint { request, .. }]
            if matches!(&request.method, crate::api::schema::Method::PaneSelectionRead(params)
                if params.anchor.row == 2 && params.cursor.row == 2))
        );
        let copy = word_row_reply(&mut state, &word_read_id(&final_read), "golf hotel india");
        assert!(
            matches!(&copy[..], [ClientShellAction::Endpoint { request, .. }]
            if matches!(&request.method, crate::api::schema::Method::PaneSelectionRead(params)
                if params.anchor == crate::api::schema::PaneTextPoint { row: 0, col: 6 }
                    && params.cursor == crate::api::schema::PaneTextPoint { row: 2, col: 9 }))
        );
        let copied = word_row_reply(
            &mut state,
            &word_read_id(&copy),
            "bravo charlie\ndelta echo foxtrot\ngolf hotel",
        );
        assert!(
            matches!(&copied[..], [ClientShellAction::ClipboardWrite(bytes)]
            if bytes == b"bravo charlie\ndelta echo foxtrot\ngolf hotel")
        );
    }
}

#[test]
fn double_click_drag_ignores_row_reply_after_typing_or_new_click() {
    for typing in [false, true] {
        let mut state = word_drag_state(false);
        let initial = start_word_drag(&mut state);
        word_row_reply(&mut state, &initial, "alpha bravo charlie");
        let drag = word_drag_mouse(&mut state, MouseEventKind::Drag(MouseButton::Left), 1, 8);
        let row_id = word_read_id(&drag.actions);
        if typing {
            state.handle_input_bytes(b"x");
        } else {
            word_drag_mouse(&mut state, MouseEventKind::Down(MouseButton::Left), 0, 0);
            word_drag_mouse(&mut state, MouseEventKind::Up(MouseButton::Left), 0, 0);
        }
        assert!(word_row_reply(&mut state, &row_id, "delta echo foxtrot").is_empty());
        assert!(state.selection.is_none());
    }
}

#[test]
fn double_click_drag_survives_focus_lag_after_anchor_reply() {
    let mut state = word_drag_state(true);
    let initial = start_word_drag(&mut state);
    word_row_reply(&mut state, &initial, "alpha bravo charlie");
    let mut lagging = snapshot();
    lagging.focused_pane_id = None;
    lagging.panes[0].focused = false;
    state.set_snapshot(Box::new(lagging));
    assert!(state.selection.is_some());
    word_drag_mouse(&mut state, MouseEventKind::Drag(MouseButton::Left), 0, 14);
    assert_eq!(
        state.selection.as_ref().unwrap().ordered_cells(),
        ((0, 6), (0, 18))
    );
    let released = word_drag_mouse(&mut state, MouseEventKind::Up(MouseButton::Left), 0, 14);
    assert_eq!(released.actions.len(), 1);
}

#[test]
fn double_click_drag_invalidates_cached_boundaries_outside_selected_cells() {
    for copy_on_select in [false, true] {
        let mut state = word_drag_state(copy_on_select);
        let initial = start_word_drag(&mut state);
        word_row_reply(&mut state, &initial, "alpha bravo charlie");
        let mut changed = state.pane_surface.as_ref().unwrap().clone();
        changed.surface_revision += 1;
        changed.panes[0].content_revision += 2;
        changed.frame.cells[14].symbol = " ".into();
        state.set_pane_surface(changed);
        assert!(
            state.selection.is_none(),
            "unchanged selected cells do not validate cached boundaries outside the selection"
        );
        assert!(
            word_drag_mouse(&mut state, MouseEventKind::Drag(MouseButton::Left), 0, 14)
                .actions
                .is_empty()
        );
        assert!(
            word_drag_mouse(&mut state, MouseEventKind::Up(MouseButton::Left), 0, 14)
                .actions
                .is_empty()
        );
        assert!(state.selection.is_none());
    }
}

#[test]
fn reconnect_word_selection_tracks_content_changes() {
    for content_changed in [false, true] {
        let mut state = word_drag_state(true);
        let initial = start_word_drag(&mut state);
        word_row_reply(&mut state, &initial, "alpha bravo charlie");
        let mut next_surface = state.pane_surface.as_ref().unwrap().clone();
        if content_changed {
            next_surface.panes[0].content_revision += 2;
            next_surface.frame.cells[14].symbol = " ".into();
        }
        let endpoint_id = state.active_endpoint_id.clone();
        let snapshot = state.snapshot.as_ref().unwrap().clone();
        state.mark_endpoint_disconnected(&endpoint_id);
        state.cache_endpoint_snapshot_inactive_for_generation(&endpoint_id, 1, snapshot);
        state.set_endpoint_status(
            &endpoint_id,
            crate::client::endpoint::ClientEndpointStatus::Online,
        );
        assert!(state.activate_endpoint_projection(&endpoint_id));
        state.set_pane_surface(next_surface);

        assert_eq!(state.selection.is_some(), !content_changed);
        assert_eq!(state.word_selection_gesture.is_some(), !content_changed);
    }
}

#[test]
fn double_click_release_ignores_reply_after_focus_or_content_changes() {
    for focus_changed in [false, true] {
        let mut state = word_drag_state(true);
        let initial = start_word_drag(&mut state);
        word_drag_mouse(&mut state, MouseEventKind::Up(MouseButton::Left), 0, 8);
        if focus_changed {
            let mut lagging = snapshot();
            lagging.focused_pane_id = None;
            lagging.panes[0].focused = false;
            state.set_snapshot(Box::new(lagging));
            let mut unfocused = snapshot();
            unfocused.focused_pane_id = Some("pane_2".into());
            unfocused.panes[0].focused = false;
            let mut other = unfocused.panes[0].clone();
            other.pane_id = "pane_2".into();
            other.focused = true;
            unfocused.panes.push(other);
            state.set_snapshot(Box::new(unfocused));
        } else {
            let mut changed = state.pane_surface.as_ref().unwrap().clone();
            changed.surface_revision += 1;
            changed.panes[0].content_revision += 2;
            state.set_pane_surface(changed);
        }
        assert!(
            word_row_reply(&mut state, &initial, "alpha bravo charlie").is_empty(),
            "a stale released gesture must not copy"
        );
        assert!(state.selection.is_none());
    }
}

#[test]
fn double_click_drag_resize_cancels_pending_word_lookup() {
    for anchor_ready in [false, true] {
        let mut state = word_drag_state(true);
        let initial = start_word_drag(&mut state);
        let pending = if anchor_ready {
            word_row_reply(&mut state, &initial, "alpha bravo charlie");
            let motion = word_drag_mouse(&mut state, MouseEventKind::Drag(MouseButton::Left), 1, 8);
            word_read_id(&motion.actions)
        } else {
            initial
        };
        word_drag_mouse(&mut state, MouseEventKind::Up(MouseButton::Left), 1, 8);
        let mut resized = state.pane_surface.as_ref().unwrap().clone();
        resized.surface_revision += 1;
        resized.panes[0].rect.width += 5;
        resized.panes[0].inner_rect.width += 5;
        state.set_pane_surface(resized);
        assert!(word_row_reply(&mut state, &pending, "alpha bravo charlie extra").is_empty());
        assert!(
            state.selection.is_none(),
            "a late reply must not restore a resized selection"
        );
        assert!(state.selection_autoscroll.is_none());
    }
}

#[test]
fn double_click_drag_autoscroll_keeps_absolute_word_anchor() {
    let mut state = word_drag_state(false);
    state.hits.panes[0].scroll = Some(crate::pane::ScrollMetrics {
        max_offset_from_bottom: 10,
        offset_from_bottom: 5,
        viewport_rows: 3,
    });
    let initial = start_word_drag(&mut state);
    word_row_reply(&mut state, &initial, "alpha bravo charlie");
    word_drag_mouse(&mut state, MouseEventKind::Drag(MouseButton::Left), 0, 14);
    let tick = state.tick_selection_autoscroll(state.selection_autoscroll_deadline.unwrap());
    word_row_reply(
        &mut state,
        &word_read_id(&tick.actions),
        "delta echo foxtrot",
    );
    assert_eq!(
        state.selection.as_ref().unwrap().ordered_cells(),
        ((4, 11), (5, 10))
    );
    word_drag_mouse(&mut state, MouseEventKind::Up(MouseButton::Left), 0, 14);
    assert!(state.selection.as_ref().unwrap().is_finalized());
    assert!(state.selection_autoscroll.is_none());
}

#[test]
fn pane_content_updates_preserve_live_ranges_until_geometry_or_screen_changes() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    let surface_at = |surface_revision, content_revision, alternate_screen_active| {
        let mut pane_surface = surface();
        pane_surface.surface_revision = surface_revision;
        pane_surface.panes[0].content_revision = content_revision;
        pane_surface.panes[0].scroll = Some(crate::protocol::PaneSurfaceScrollMetrics {
            offset_from_bottom: 0,
            max_offset_from_bottom: 11,
            viewport_rows: 2,
        });
        pane_surface.panes[0].alternate_screen_active = alternate_screen_active;
        pane_surface
    };
    state.set_pane_surface(surface_at(1, 0, true));
    state.compose(106, 20).expect("composed frame");
    let pane = state.hits.panes[0].clone();
    let mouse = |kind, column, row| {
        RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::empty(),
        })
    };

    state.handle_raw_events(vec![mouse(
        MouseEventKind::Down(MouseButton::Left),
        pane.inner_rect.x,
        pane.inner_rect.y + 1,
    )]);
    let mut updated_surface = surface_at(2, 2, true);
    updated_surface.frame.cells[0].symbol = "W".into();
    state.set_pane_surface(updated_surface);
    state.compose(106, 20).expect("updated frame");

    let drag = state.handle_raw_events(vec![mouse(
        MouseEventKind::Drag(MouseButton::Left),
        pane.inner_rect.x + 1,
        pane.inner_rect.y + 1,
    )]);

    assert!(drag.repaint || state.selection_repaint_deadline.is_some());
    let selection = state.selection.as_ref().expect("visible selection");
    assert!(selection.is_visible());
    assert_eq!(selection.ordered_cells(), ((12, 0), (12, 1)));

    let mut replaced_surface = surface_at(3, 4, true);
    replaced_surface.frame.cells[4].symbol = "X".into();
    state.set_pane_surface(replaced_surface);
    assert_eq!(
        state.selection.as_ref().unwrap().ordered_cells(),
        ((12, 0), (12, 1))
    );

    // The selected row can leave the viewport during a drag. A later patch,
    // including an in-flight content revision, must keep that absolute range.
    let mut scrolled = surface_at(4, 5, true);
    scrolled.panes[0]
        .scroll
        .as_mut()
        .unwrap()
        .offset_from_bottom = 2;
    assert!(matches!(
        state.apply_pane_surface_patch(crate::protocol::PaneSurfacePatch {
            boot_id: scrolled.boot_id,
            projection_revision: scrolled.projection_revision,
            base_surface_revision: 3,
            surface_revision: 4,
            panes: scrolled.panes,
            rows: vec![],
            cursor: scrolled.frame.cursor,
        }),
        super::super::surface_patch::ClientPaneSurfacePatchOutcome::Applied(_)
    ));
    assert!(state.selection.as_ref().unwrap().is_in_progress());
    assert_eq!(
        state.selection.as_ref().unwrap().ordered_cells(),
        ((12, 0), (12, 1))
    );

    for (surface_revision, content_revision, width, alternate_screen_active) in
        [(5, 6, 4, false), (6, 8, 3, false)]
    {
        state.selection = Some(crate::selection::Selection::absolute_anchor(
            "pane_1".to_owned(),
            (12, 0),
        ));
        let mut changed_surface =
            surface_at(surface_revision, content_revision, alternate_screen_active);
        changed_surface.panes[0].inner_rect.width = width;
        changed_surface.panes[0].alternate_screen_active = alternate_screen_active;
        state.set_pane_surface(changed_surface);
        assert!(state.selection.is_none());
    }
}

#[test]
fn mouse_drag_selection_survives_snapshot_focus_lag() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let mut multi_snapshot = snapshot();
    let mut pane_2 = multi_snapshot.panes[0].clone();
    pane_2.pane_id = "pane_2".into();
    pane_2.focused = false;
    multi_snapshot.panes.push(pane_2);
    multi_snapshot.focused_pane_id = Some("pane_1".into());
    state.set_snapshot(Box::new(multi_snapshot.clone()));

    let mut pane_surface = surface();
    let mut surface_pane_2 = pane_surface.panes[0].clone();
    surface_pane_2.pane_id = "pane_2".into();
    surface_pane_2.rect.x = 53;
    surface_pane_2.inner_rect.x = 54;
    surface_pane_2.focused = false;
    pane_surface.panes.push(surface_pane_2);
    state.set_pane_surface(pane_surface);
    state.compose(106, 20).expect("composed frame");

    let pane_2_hit = state
        .hits
        .panes
        .iter()
        .find(|h| h.pane_id == "pane_2")
        .cloned()
        .expect("pane_2 hit");

    // User clicks down in pane_2 (unfocused pane) and drags
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: pane_2_hit.inner_rect.x,
        row: pane_2_hit.inner_rect.y,
        modifiers: KeyModifiers::empty(),
    })]);
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: pane_2_hit.inner_rect.x + 2,
        row: pane_2_hit.inner_rect.y,
        modifiers: KeyModifiers::empty(),
    })]);

    let sel = state.selection.as_ref().expect("selection active");
    assert_eq!(sel.pane_id, "pane_2");
    assert!(sel.is_in_progress());

    // An intermediate snapshot arrives where focused_pane_id is still pane_1 (focus in transit)
    let mut lagging = multi_snapshot.clone();
    lagging.revision += 1;
    lagging.focused_pane_id = Some("pane_1".into());
    state.set_snapshot(Box::new(lagging));

    assert!(
        state.selection.is_some(),
        "in-progress selection must survive lagging focus snapshot"
    );
    let sel = state.selection.as_ref().unwrap();
    assert_eq!(sel.pane_id, "pane_2");
    assert!(sel.is_in_progress());
    // User continues dragging in pane_2
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: pane_2_hit.inner_rect.x + 3,
        row: pane_2_hit.inner_rect.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert_eq!(
        state.selection.as_ref().unwrap().ordered_cells(),
        ((0, 0), (0, 3))
    );
}

#[test]
fn mouse_drag_selection_survives_snapshot_focus_lag_after_release() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.config.copy_on_select = false;
    let mut multi_snapshot = snapshot();
    let mut pane_2 = multi_snapshot.panes[0].clone();
    pane_2.pane_id = "pane_2".into();
    pane_2.focused = false;
    multi_snapshot.panes.push(pane_2);
    multi_snapshot.focused_pane_id = Some("pane_1".into());
    state.set_snapshot(Box::new(multi_snapshot.clone()));

    let mut pane_surface = surface();
    let mut surface_pane_2 = pane_surface.panes[0].clone();
    surface_pane_2.pane_id = "pane_2".into();
    surface_pane_2.rect.x = 53;
    surface_pane_2.inner_rect.x = 54;
    surface_pane_2.focused = false;
    pane_surface.panes.push(surface_pane_2);
    state.set_pane_surface(pane_surface);
    state.compose(106, 20).expect("composed frame");

    let pane_2_hit = state
        .hits
        .panes
        .iter()
        .find(|h| h.pane_id == "pane_2")
        .cloned()
        .expect("pane_2 hit");

    // User clicks down in pane_2 (unfocused pane), drags, and releases
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: pane_2_hit.inner_rect.x,
        row: pane_2_hit.inner_rect.y,
        modifiers: KeyModifiers::empty(),
    })]);
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: pane_2_hit.inner_rect.x + 2,
        row: pane_2_hit.inner_rect.y,
        modifiers: KeyModifiers::empty(),
    })]);
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: pane_2_hit.inner_rect.x + 2,
        row: pane_2_hit.inner_rect.y,
        modifiers: KeyModifiers::empty(),
    })]);

    let sel = state
        .selection
        .as_ref()
        .expect("selection still active after mouse up");
    assert_eq!(sel.pane_id, "pane_2");
    assert!(sel.is_finalized());

    // An intermediate snapshot arrives where focused_pane_id is still pane_1 (focus in transit)
    let mut lagging = multi_snapshot.clone();
    lagging.revision += 1;
    lagging.focused_pane_id = Some("pane_1".into());
    state.set_snapshot(Box::new(lagging));

    assert!(
        state.selection.is_some(),
        "finalized selection must survive lagging focus snapshot"
    );
}

#[test]
fn mouse_drag_selection_survives_snapshot_revision_advance_clearing_hits() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("initial frame");
    let pane = state.hits.panes[0].clone();

    // Start drag
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: pane.inner_rect.x,
        row: pane.inner_rect.y,
        modifiers: KeyModifiers::empty(),
    })]);
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: pane.inner_rect.x + 2,
        row: pane.inner_rect.y,
        modifiers: KeyModifiers::empty(),
    })]);

    // Snapshot revision advances ahead of surface, clearing state.hits
    let mut advanced_snapshot = snapshot();
    advanced_snapshot.revision = 2;
    state.set_snapshot(Box::new(advanced_snapshot));
    assert!(
        state.hits.panes.is_empty(),
        "hits should be cleared while awaiting matching surface"
    );

    // User continues dragging while hits are empty
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: pane.inner_rect.x + 3,
        row: pane.inner_rect.y,
        modifiers: KeyModifiers::empty(),
    })]);

    let sel = state.selection.as_ref().expect("selection still active");
    assert!(sel.is_in_progress());
    assert_eq!(sel.ordered_cells(), ((0, 0), (0, 3)));
}

#[test]
fn pane_mouse_input_keeps_stable_target_and_endpoint_encoding() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    let mut pane_surface = surface();
    pane_surface.panes[0].mouse_reporting = true;
    state.set_pane_surface(pane_surface);
    state.compose(106, 20).expect("composed frame");
    let pane = state.hits.panes[0].clone();

    let click = state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: pane.inner_rect.x + 2,
        row: pane.inner_rect.y + 1,
        modifiers: KeyModifiers::ALT,
    })]);
    let [ClientMessage::ClientShellPaneInput { pane_id, events }] = &click.requests[..] else {
        panic!("pane application click should use targeted canonical input");
    };
    assert_eq!(pane_id, "pane_1");
    assert!(matches!(
        &events[..],
        [ClientPaneInputEvent::Mouse {
            kind: crate::protocol::ClientMouseKind::Down(
                crate::protocol::ClientMouseButton::Left
            ),
            position: ClientMousePosition::Cell { column: 2, row: 1 },
            modifiers,
            ..
        }] if *modifiers == KeyModifiers::ALT.bits()
    ));
    let moved = state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Moved,
        column: 0,
        row: 0,
        modifiers: KeyModifiers::ALT,
    })]);
    assert!(moved.requests.is_empty());
    assert!(state.pane_mouse_gesture.is_some());
    state.hits.panes.clear();
    let release =
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: 0,
            row: 0,
            modifiers: KeyModifiers::ALT,
        })]);
    assert!(matches!(
        &release.requests[..],
        [ClientMessage::ClientShellPaneInput { pane_id, events }]
            if pane_id == "pane_1"
                && matches!(
                    &events[..],
                    [ClientPaneInputEvent::Mouse {
                        kind: crate::protocol::ClientMouseKind::Up(
                            crate::protocol::ClientMouseButton::Left
                        ),
                        ..
                    }]
                )
    ));
    assert!(state.pane_mouse_gesture.is_none());
}

#[test]
fn pane_pixel_mouse_preserves_pane_relative_pixel_coordinates() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    let mut pane_surface = surface();
    pane_surface.panes[0].mouse_reporting = true;
    pane_surface.panes[0].sgr_pixel_mouse = true;
    pane_surface.panes[0].pixel_width = 39;
    pane_surface.panes[0].pixel_height = 38;
    state.set_pane_surface(pane_surface);
    state.compose(106, 20).expect("composed frame");
    let pane = state.hits.panes[0].clone();
    let geometry =
        crate::input::mouse::HostGeometry::new(106, 20, 1060, 400).expect("host geometry");
    let x = u32::from(pane.inner_rect.x) * 10 + 21;
    let y = u32::from(pane.inner_rect.y) * 20 + 21;
    let report = format!("\x1b[<0;{x};{y}M");

    let outcome = state.handle_pixel_mouse(report.as_bytes(), geometry);
    assert!(matches!(
        &outcome.requests[..],
        [ClientMessage::ClientShellPaneInput { pane_id, events }]
            if pane_id == "pane_1"
                && matches!(
                    &events[..],
                    [ClientPaneInputEvent::Mouse {
                        kind: crate::protocol::ClientMouseKind::Down(
                            crate::protocol::ClientMouseButton::Left
                        ),
                        position: ClientMousePosition::Pixels { x: 20, y: 20, .. },
                        ..
                    }]
                )
    ));

    let lost = state.handle_raw_events(vec![RawInputEvent::OuterFocusLost]);
    assert!(matches!(
        &lost.requests[..],
        [
            ClientMessage::ClientShellPaneInput { pane_id, events },
            ClientMessage::ClientShellFocus { focused: false }
        ] if pane_id == "pane_1" && matches!(
            &events[..],
            [ClientPaneInputEvent::Mouse {
                kind: crate::protocol::ClientMouseKind::Up(
                    crate::protocol::ClientMouseButton::Left
                ),
                position: ClientMousePosition::Pixels { x: 20, y: 20, .. },
                ..
            }]
        )
    ));
}

#[test]
fn pane_owned_right_click_forwards_the_complete_gesture() {
    let mut snapshot = snapshot();
    snapshot.panes[0].right_click_passthrough = true;
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot));
    let mut pane_surface = surface();
    pane_surface.panes[0].mouse_reporting = true;
    state.set_pane_surface(pane_surface);
    state.compose(106, 20).expect("composed frame");
    let pane = state.hits.panes[0].clone();

    let down = state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column: pane.inner_rect.x + 1,
        row: pane.inner_rect.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(matches!(
        &down.requests[..],
        [ClientMessage::ClientShellPaneInput { pane_id, .. }] if pane_id == "pane_1"
    ));
    assert!(state.overlay.is_none());
    assert!(state.pane_mouse_gesture.is_some());

    let up = state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Right),
        column: 0,
        row: 0,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(matches!(
        &up.requests[..],
        [ClientMessage::ClientShellPaneInput { pane_id, events }]
            if pane_id == "pane_1"
                && matches!(
                    &events[..],
                    [ClientPaneInputEvent::Mouse {
                        kind: crate::protocol::ClientMouseKind::Up(
                            crate::protocol::ClientMouseButton::Right
                        ),
                        ..
                    }]
                )
    ));
    assert!(state.pane_mouse_gesture.is_none());
}

#[test]
fn tab_click_waits_for_release_and_drag_reorders_by_stable_id() {
    let mut projected = snapshot();
    for index in 2..=3 {
        let mut tab = projected.tabs[0].clone();
        tab.tab_id = format!("tab_{index}");
        tab.number = index;
        tab.label = index.to_string();
        tab.focused = false;
        projected.tabs.push(tab);
    }
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("three tabs");
    let first = state.hits.tabs[0].0;
    let third = state.hits.tabs[2].0;

    let down = state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: first.x + 1,
        row: first.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(down.actions.is_empty());
    let drag = state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: third.right().saturating_sub(1),
        row: third.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(drag.repaint);
    assert!(matches!(
        state.chrome_drag,
        Some(ClientChromeDrag::Tab {
            ref tab_id,
            insert_index: Some(3),
            ..
        }) if tab_id == "tab_1"
    ));
    let frame = state.compose(106, 20).expect("tab drop indicator");
    assert!(frame
        .cells
        .iter()
        .take(frame.width as usize)
        .any(|cell| cell.symbol == "│"));

    let release =
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: third.right().saturating_sub(1),
            row: third.y,
            modifiers: KeyModifiers::empty(),
        })]);
    let [ClientShellAction::Endpoint { request, .. }] = &release.actions[..] else {
        panic!("tab drag should use endpoint API");
    };
    assert!(matches!(
        &request.method,
        crate::api::schema::Method::TabMove(params)
            if params.tab_id == "tab_1" && params.insert_index == 3
    ));

    state.compose(106, 20).expect("tabs after drag");
    let second = state.hits.tabs[1].0;
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: second.x + 1,
        row: second.y,
        modifiers: KeyModifiers::empty(),
    })]);
    let click = state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: second.x + 1,
        row: second.y,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(matches!(
        &click.actions[0],
        ClientShellAction::Endpoint { request, .. }
            if matches!(&request.method, crate::api::schema::Method::TabFocus(target) if target.tab_id == "tab_2")
    ));
}

#[test]
fn tab_drag_clears_its_drop_target_after_leaving_the_tab_row() {
    let mut projected = snapshot();
    for index in 2..=3 {
        let mut tab = projected.tabs[0].clone();
        tab.tab_id = format!("tab_{index}");
        tab.number = index;
        tab.label = index.to_string();
        tab.focused = false;
        projected.tabs.push(tab);
    }
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(projected));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("three tabs");
    let first = state.hits.tabs[0].0;
    let third = state.hits.tabs[2].0;
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: first.x + 1,
        row: first.y,
        modifiers: KeyModifiers::empty(),
    })]);
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: third.x,
        row: third.y,
        modifiers: KeyModifiers::empty(),
    })]);
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: third.x,
        row: third.y + 1,
        modifiers: KeyModifiers::empty(),
    })]);
    assert!(matches!(
        state.chrome_drag,
        Some(ClientChromeDrag::Tab {
            insert_index: None,
            ..
        })
    ));
    let release =
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Up(MouseButton::Left),
            column: third.x,
            row: third.y + 1,
            modifiers: KeyModifiers::empty(),
        })]);
    assert!(release.actions.is_empty());
}

#[test]
fn tab_wheel_switches_tabs_without_changing_overflow_scroll() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("tab bar");
    let tab = state.hits.tabs[0].0;

    let outcome =
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: tab.x,
            row: tab.y,
            modifiers: KeyModifiers::empty(),
        })]);
    assert!(matches!(
        &outcome.actions[..],
        [ClientShellAction::Endpoint { request, .. }]
            if matches!(
                &request.method,
                crate::api::schema::Method::TabFocus(target) if target.tab_id == "tab_1"
            )
    ));
    assert_eq!(state.tab_scroll, 0);
    state.compose(106, 20).expect("tab bar after wheel");
    assert!(state.hits.tabs.iter().any(|(_, tab_id)| tab_id == "tab_1"));
}

#[test]
fn context_menu_keyboard_and_outside_click_are_client_owned() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("composed frame");
    let tab = state.hits.tabs[0].0;
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Right),
        column: tab.x + 1,
        row: tab.y,
        modifiers: KeyModifiers::empty(),
    })]);
    state.compose(106, 20).expect("tab context menu");
    let moved = state.handle_input_bytes(b"\x1b[B");
    assert!(moved.repaint);
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::ContextMenu(ClientContextMenuOverlay {
            highlighted: 1,
            ..
        }))
    ));
    let text = state.handle_raw_events(vec![RawInputEvent::Text(crate::input::TextCommit::new(
        "not pane input",
    ))]);
    assert!(text.requests.is_empty());
    let paste = state.handle_raw_events(vec![RawInputEvent::Paste("not pane input".into())]);
    assert!(paste.requests.is_empty());
    let outside =
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 105,
            row: 19,
            modifiers: KeyModifiers::empty(),
        })]);
    assert!(outside.repaint);
    assert!(state.overlay.is_none());
}

fn cell_with_sym(symbol: &str) -> crate::protocol::CellData {
    crate::protocol::CellData {
        symbol: symbol.into(),
        fg: 0,
        bg: 0,
        modifier: 0,
        skip: false,
        hyperlink: None,
    }
}

#[test]
fn mouse_drag_selection_clamps_to_active_pane_across_boundaries() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    let mut multi_snapshot = snapshot();
    let mut pane_2 = multi_snapshot.panes[0].clone();
    pane_2.pane_id = "pane_2".into();
    pane_2.focused = false;
    multi_snapshot.panes.push(pane_2);
    state.set_snapshot(Box::new(multi_snapshot));

    let mut pane_surface = surface();
    pane_surface.frame.width = 106;
    pane_surface.frame.height = 20;
    pane_surface.frame.cells = vec![cell_with_sym(" "); 106 * 20];
    pane_surface.panes[0].rect.y = 1;
    pane_surface.panes[0].inner_rect.y = 1;
    pane_surface.panes[0].rect.width = 50;
    pane_surface.panes[0].inner_rect.width = 50;
    pane_surface.panes[0].rect.height = 18;
    pane_surface.panes[0].inner_rect.height = 18;
    pane_surface.panes[0].scroll = Some(crate::protocol::PaneSurfaceScrollMetrics {
        offset_from_bottom: 0,
        max_offset_from_bottom: 50,
        viewport_rows: 18,
    });

    let mut surface_pane_2 = pane_surface.panes[0].clone();
    surface_pane_2.pane_id = "pane_2".into();
    surface_pane_2.rect.x = 53;
    surface_pane_2.inner_rect.x = 54;
    surface_pane_2.rect.width = 50;
    surface_pane_2.inner_rect.width = 50;
    surface_pane_2.focused = false;
    pane_surface.panes.push(surface_pane_2);
    state.set_pane_surface(pane_surface);
    state.compose(106, 20).expect("composed frame");

    let p1 = state
        .hits
        .panes
        .iter()
        .find(|h| h.pane_id == "pane_1")
        .cloned()
        .unwrap();
    let p2 = state
        .hits
        .panes
        .iter()
        .find(|h| h.pane_id == "pane_2")
        .cloned()
        .unwrap();

    // 1. MouseDown in pane_1 at (col 5, row 2)
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: p1.inner_rect.x + 5,
        row: p1.inner_rect.y + 2,
        modifiers: KeyModifiers::empty(),
    })]);

    // 2. Drag far into pane_2 area (col = p2.inner_rect.x + 10, row = p1.inner_rect.y + 2)
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: p2.inner_rect.x + 10,
        row: p1.inner_rect.y + 2,
        modifiers: KeyModifiers::empty(),
    })]);

    let sel = state.selection.as_ref().expect("selection active");
    assert_eq!(
        sel.pane_id, "pane_1",
        "drag into another pane must not switch selection ownership"
    );
    assert!(sel.is_in_progress());
    let rightmost_col = p1.inner_rect.width - 1;
    assert_eq!(sel.ordered_cells(), ((52, 5), (52, rightmost_col)));

    // 3. Drag vertically above pane_1 into tab bar (row 0)
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: p1.inner_rect.x + 1,
        row: 0,
        modifiers: KeyModifiers::empty(),
    })]);

    let sel = state.selection.as_ref().expect("selection active");
    assert_eq!(sel.pane_id, "pane_1");
    assert_eq!(sel.ordered_cells(), ((44, 1), (52, 5)));
    assert!(
        state.selection_autoscroll.is_some(),
        "dragging above pane should trigger autoscroll"
    );

    // 4. Drag vertically below bottom of pane_1
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: p1.inner_rect.x + 10,
        row: p1.inner_rect.y + p1.inner_rect.height + 5,
        modifiers: KeyModifiers::empty(),
    })]);

    let sel = state.selection.as_ref().expect("selection active");
    assert_eq!(sel.ordered_cells(), ((52, 5), (67, 10)));

    // 5. Release mouse
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: p1.inner_rect.x + 10,
        row: p1.inner_rect.y + p1.inner_rect.height + 5,
        modifiers: KeyModifiers::empty(),
    })]);

    assert!(
        state.selection_autoscroll.is_none(),
        "releasing mouse stops autoscroll"
    );
}

#[test]
fn mouse_drag_selection_survives_rapid_deltas_and_blocks_direct_blit() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    let mut pane_surface = surface();
    pane_surface.frame.width = 106;
    pane_surface.frame.height = 20;
    pane_surface.frame.cells = vec![cell_with_sym(" "); 106 * 20];
    pane_surface.panes[0].rect.width = 50;
    pane_surface.panes[0].inner_rect.width = 50;
    pane_surface.panes[0].rect.height = 18;
    pane_surface.panes[0].inner_rect.height = 18;
    state.set_pane_surface(pane_surface);
    state.compose(106, 20).expect("initial frame");
    let pane = state.hits.panes[0].clone();

    // Start drag
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: pane.inner_rect.x + 2,
        row: pane.inner_rect.y,
        modifiers: KeyModifiers::empty(),
    })]);
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: pane.inner_rect.x + 6,
        row: pane.inner_rect.y,
        modifiers: KeyModifiers::empty(),
    })]);

    let sel = state.selection.as_ref().expect("drag active");
    assert!(sel.is_in_progress());
    assert_eq!(sel.ordered_cells(), ((0, 2), (0, 6)));

    // Deliver rapid deltas while drag is in progress
    for rev in 1..=5 {
        let delta = crate::protocol::delta::ClientShellSurfaceDelta {
            boot_id: "boot-1".into(),
            projection_revision: 1,
            base_surface_revision: rev,
            surface_revision: rev + 1,
            spans: vec![crate::protocol::PaneSurfacePatchRow {
                x: 0,
                y: 0,
                cells: vec![cell_with_sym(&format!("{rev}"))],
            }],
            row_moves: Vec::new(),
            panes: vec![crate::protocol::delta::PaneSurfacePaneDelta {
                pane_id: "pane_1".into(),
                content_revision: crate::protocol::delta::SurfaceFieldUpdate::Set(100 + rev),
                scroll: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
                focused: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
                mouse_reporting: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
                sgr_pixel_mouse: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
                alternate_screen_active: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
                scrollbar_rect: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
                rect: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
                inner_rect: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
                pixel_width: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
                pixel_height: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
            }],
            splits: None,
            cursor: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
            appended_hyperlinks: Vec::new(),
            graphics: None,
            popup: None,
        };

        let outcome = state.apply_surface_delta(delta);
        // Direct cell blit MUST be blocked by active selection
        assert!(
            matches!(
                outcome,
                crate::client::shell::surface_patch::ClientPaneSurfacePatchOutcome::Applied(None)
            ),
            "active drag selection must block direct cell blit to prevent overwriting highlight"
        );

        // Selection remains intact and in progress despite advancing content_revision
        assert!(
            state.selection.as_ref().is_some_and(|s| s.is_in_progress()),
            "in-progress drag selection must survive content delta"
        );
        assert_eq!(
            state.selection.as_ref().unwrap().ordered_cells(),
            ((0, 2), (0, 6))
        );
    }

    // Drag further after deltas
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: pane.inner_rect.x + 10,
        row: pane.inner_rect.y,
        modifiers: KeyModifiers::empty(),
    })]);

    assert_eq!(
        state.selection.as_ref().unwrap().ordered_cells(),
        ((0, 2), (0, 10))
    );
}

#[test]
fn direct_blit_blocked_when_selection_deadline_or_word_gesture_active() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("initial frame");

    // 1. In terminal mode with no selection or overlay, direct blit is allowed
    assert_eq!(
        crate::client::shell::surface_patch::client_local_overlay_blocks_direct_blit(&state),
        None
    );

    // 2. Selection active blocks direct blit
    state.selection = Some(crate::selection::Selection::anchor(
        "pane_1".into(),
        0,
        0,
        None,
    ));
    assert_eq!(
        crate::client::shell::surface_patch::client_local_overlay_blocks_direct_blit(&state),
        Some("client_surface_patch.fallback.selection")
    );
    state.selection = None;

    // 3. Selection highlight clear deadline blocks direct blit
    state.selection_highlight_clear_deadline =
        Some(std::time::Instant::now() + std::time::Duration::from_millis(500));
    assert_eq!(
        crate::client::shell::surface_patch::client_local_overlay_blocks_direct_blit(&state),
        Some("client_surface_patch.fallback.selection_deadline")
    );
    state.selection_highlight_clear_deadline = None;

    // 4. Word selection gesture active blocks direct blit
    let mut word_state = word_drag_state(false);
    let _ = start_word_drag(&mut word_state);
    assert!(word_state.word_selection_gesture.is_some());
    assert_eq!(
        crate::client::shell::surface_patch::client_local_overlay_blocks_direct_blit(&word_state),
        Some("client_surface_patch.fallback.word_selection")
    );
}

#[test]
fn mouse_drag_selection_rapid_snapshot_and_delta_interleaving() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 20).expect("initial frame");
    let pane = state.hits.panes[0].clone();

    // Start drag
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: pane.inner_rect.x,
        row: pane.inner_rect.y,
        modifiers: KeyModifiers::empty(),
    })]);

    for step in 1u64..=4u64 {
        // Drag step
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: pane.inner_rect.x + (step * 2) as u16,
            row: pane.inner_rect.y,
            modifiers: KeyModifiers::empty(),
        })]);
        assert!(
            state.selection.is_some(),
            "selection must survive drag step {step}"
        );

        // Snapshot revision advances ahead of surface (clearing hits)
        let mut snap = snapshot();
        snap.revision = step + 1;
        state.set_snapshot(Box::new(snap));
        assert!(
            state.hits.panes.is_empty(),
            "hits cleared on revision advance"
        );

        // Drag step while hits are empty
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Drag(MouseButton::Left),
            column: pane.inner_rect.x + (step * 2 + 1) as u16,
            row: pane.inner_rect.y,
            modifiers: KeyModifiers::empty(),
        })]);
        assert!(
            state.selection.is_some(),
            "selection must survive drag with empty hits"
        );

        // Surface delta arrives catching up
        let delta = crate::protocol::delta::ClientShellSurfaceDelta {
            boot_id: "boot-1".into(),
            projection_revision: step + 1,
            base_surface_revision: step,
            surface_revision: step + 1,
            spans: vec![crate::protocol::PaneSurfacePatchRow {
                x: 0,
                y: 0,
                cells: vec![cell_with_sym("Z")],
            }],
            row_moves: Vec::new(),
            panes: vec![crate::protocol::delta::PaneSurfacePaneDelta {
                pane_id: "pane_1".into(),
                content_revision: crate::protocol::delta::SurfaceFieldUpdate::Set(100 + step),
                scroll: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
                focused: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
                mouse_reporting: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
                sgr_pixel_mouse: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
                alternate_screen_active: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
                scrollbar_rect: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
                rect: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
                inner_rect: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
                pixel_width: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
                pixel_height: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
            }],
            splits: None,
            cursor: crate::protocol::delta::SurfaceFieldUpdate::Unchanged,
            appended_hyperlinks: Vec::new(),
            graphics: None,
            popup: None,
        };
        state.apply_surface_delta(delta);
        assert!(
            state.selection.is_some(),
            "selection must survive surface delta"
        );
    }

    // Final release
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: pane.inner_rect.x + 9,
        row: pane.inner_rect.y,
        modifiers: KeyModifiers::empty(),
    })]);
    // Selection was copied (copy_on_select defaults to true)
    assert!(state.selection.is_none());
}

#[test]
fn mouse_drag_selection_in_progress_wheel_scrolling() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    let mut pane_surface = surface();
    pane_surface.frame.width = 106;
    pane_surface.frame.height = 20;
    pane_surface.frame.cells = vec![cell_with_sym(" "); 106 * 20];
    pane_surface.panes[0].rect.width = 50;
    pane_surface.panes[0].inner_rect.width = 50;
    pane_surface.panes[0].rect.height = 18;
    pane_surface.panes[0].inner_rect.height = 18;
    pane_surface.panes[0].scroll = Some(crate::protocol::PaneSurfaceScrollMetrics {
        offset_from_bottom: 5,
        max_offset_from_bottom: 20,
        viewport_rows: 5,
    });
    state.set_pane_surface(pane_surface);
    state.compose(106, 20).expect("initial frame");
    let pane = state.hits.panes[0].clone();

    // Start drag
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: pane.inner_rect.x + 2,
        row: pane.inner_rect.y + 2,
        modifiers: KeyModifiers::empty(),
    })]);
    state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: pane.inner_rect.x + 5,
        row: pane.inner_rect.y + 2,
        modifiers: KeyModifiers::empty(),
    })]);

    assert!(state.selection.as_ref().unwrap().is_in_progress());

    // Scroll wheel up while dragging
    let scroll_up =
        state.handle_raw_events(vec![RawInputEvent::Mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: pane.inner_rect.x + 5,
            row: pane.inner_rect.y + 2,
            modifiers: KeyModifiers::empty(),
        })]);

    assert!(scroll_up.repaint);
    assert!(state.selection.as_ref().unwrap().is_in_progress());
    assert!(scroll_up.actions.iter().any(|a| matches!(
        a,
        ClientShellAction::Endpoint { request, .. }
            if matches!(&request.method, crate::api::schema::Method::PaneScroll(params) if params.pane_id == "pane_1")
    )));
}
