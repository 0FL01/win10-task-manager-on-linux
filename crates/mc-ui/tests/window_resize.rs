/* Copyright 2026 Mission Center Developers
 * SPDX-License-Identifier: GPL-3.0-or-later
 */

//! Native resize handoff: the compositor may consume the mouse release.
use egui::{
    Context, CursorIcon, Event, FullOutput, PointerButton, RawInput, Rect, ResizeDirection,
    ViewportCommand, pos2, vec2,
};
use mc_ui::win10::chrome;

fn frame(ctx: &Context, events: Vec<Event>) -> FullOutput {
    frame_with_modal(ctx, events, false)
}

fn frame_with_modal(ctx: &Context, events: Vec<Event>, modal: bool) -> FullOutput {
    let mut output = ctx.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0., 0.), vec2(1030., 705.))),
            events,
            ..Default::default()
        },
        |ui| {
            let window = ui.max_rect();
            chrome::title_bar(ui, window, "Task Manager", false);
            chrome::resize_edges(ui, window);
            if modal {
                egui::Modal::new(egui::Id::new("resize-test-modal")).show(ui.ctx(), |ui| {
                    ui.label("Modal blocks window resize");
                });
            }
        },
    );
    output.textures_delta.clear();
    output
}

fn press(pos: egui::Pos2, button: PointerButton) -> Event {
    Event::PointerButton {
        pos,
        button,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    }
}

fn resize_commands(output: &FullOutput) -> Vec<ResizeDirection> {
    output
        .viewport_output
        .values()
        .flat_map(|viewport| &viewport.commands)
        .filter_map(|command| {
            if let ViewportCommand::BeginResize(direction) = command {
                Some(*direction)
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn compositor_consumed_release_does_not_lock_the_next_edge() {
    let ctx = Context::default();
    frame(&ctx, vec![]);
    let corner = pos2(2., 2.);
    frame(&ctx, vec![Event::PointerMoved(corner)]);
    let started = frame(&ctx, vec![press(corner, PointerButton::Primary)]);
    assert_eq!(resize_commands(&started), [ResizeDirection::NorthWest]);
    assert!(
        ctx.dragged_id().is_none(),
        "native resize must not retain an egui drag"
    );
    assert!(
        !ctx.input(|input| input.pointer.primary_down()),
        "release may never reach the client"
    );
    // Wayland leaves the surface when KWin takes the pointer grab. The release belongs to
    // KWin, so the client can see only leave + re-enter at a different edge afterwards.
    frame(&ctx, vec![Event::PointerGone]);
    let edge = pos2(1028., 350.);
    let returned = frame(&ctx, vec![Event::PointerMoved(edge)]);
    assert_eq!(
        returned.platform_output.cursor_icon,
        CursorIcon::ResizeHorizontal
    );
    assert!(resize_commands(&returned).is_empty());
    let next = frame(&ctx, vec![press(edge, PointerButton::Primary)]);
    assert_eq!(resize_commands(&next), [ResizeDirection::East]);
}

#[test]
fn resize_handoff_without_leave_still_allows_another_press() {
    let ctx = Context::default();
    frame(&ctx, vec![]);
    let bottom = pos2(500., 703.);
    frame(&ctx, vec![Event::PointerMoved(bottom)]);
    assert_eq!(
        resize_commands(&frame(&ctx, vec![press(bottom, PointerButton::Primary)])),
        [ResizeDirection::South]
    );
    let top = pos2(500., 2.);
    let moved = frame(&ctx, vec![Event::PointerMoved(top)]);
    assert_eq!(
        moved.platform_output.cursor_icon,
        CursorIcon::ResizeVertical
    );
    assert!(resize_commands(&moved).is_empty());
    assert_eq!(
        resize_commands(&frame(&ctx, vec![press(top, PointerButton::Primary)])),
        [ResizeDirection::North]
    );
}

#[test]
fn resize_does_not_capture_secondary_press_or_caption_interior() {
    for (pos, button) in [
        (pos2(2., 2.), PointerButton::Secondary),
        (pos2(1020., 10.), PointerButton::Primary),
        (pos2(1020., 695.), PointerButton::Primary),
    ] {
        let ctx = Context::default();
        frame(&ctx, vec![]);
        let hover = frame(&ctx, vec![Event::PointerMoved(pos)]);
        if pos.x == 1020. {
            assert_eq!(hover.platform_output.cursor_icon, CursorIcon::Default);
        }
        assert!(resize_commands(&frame(&ctx, vec![press(pos, button)])).is_empty());
    }
}

#[test]
fn corners_are_reachable_along_both_adjacent_borders() {
    for (pos, direction) in [
        (pos2(12., 2.), ResizeDirection::NorthWest),
        (pos2(2., 12.), ResizeDirection::NorthWest),
        (pos2(1018., 2.), ResizeDirection::NorthEast),
        (pos2(1028., 12.), ResizeDirection::NorthEast),
        (pos2(12., 703.), ResizeDirection::SouthWest),
        (pos2(2., 693.), ResizeDirection::SouthWest),
        (pos2(1018., 703.), ResizeDirection::SouthEast),
        (pos2(1028., 693.), ResizeDirection::SouthEast),
    ] {
        let ctx = Context::default();
        frame(&ctx, vec![]);
        frame(&ctx, vec![Event::PointerMoved(pos)]);
        let output = frame(&ctx, vec![press(pos, PointerButton::Primary)]);
        assert_eq!(resize_commands(&output), [direction], "corner at {pos:?}");
    }
}

#[test]
fn press_and_leave_in_one_frame_starts_resize() {
    let ctx = Context::default();
    let corner = pos2(2., 2.);
    frame(&ctx, vec![]);
    frame(&ctx, vec![Event::PointerMoved(corner)]);
    let output = frame(
        &ctx,
        vec![press(corner, PointerButton::Primary), Event::PointerGone],
    );
    assert_eq!(resize_commands(&output), [ResizeDirection::NorthWest]);
    assert!(ctx.dragged_id().is_none());
    assert!(!ctx.input(|input| input.pointer.primary_down()));
}

#[test]
fn press_position_selects_direction_before_same_frame_move() {
    let ctx = Context::default();
    let corner = pos2(2., 2.);
    frame(&ctx, vec![]);
    frame(&ctx, vec![Event::PointerMoved(corner)]);
    let output = frame(
        &ctx,
        vec![
            press(corner, PointerButton::Primary),
            Event::PointerMoved(pos2(1028., 350.)),
        ],
    );
    assert_eq!(resize_commands(&output), [ResizeDirection::NorthWest]);
}

#[test]
fn caption_press_followed_by_move_to_border_does_not_resize() {
    let ctx = Context::default();
    let caption = pos2(1007., 16.);
    frame(&ctx, vec![]);
    frame(&ctx, vec![Event::PointerMoved(caption)]);
    let output = frame(
        &ctx,
        vec![
            press(caption, PointerButton::Primary),
            Event::PointerMoved(pos2(2., 2.)),
        ],
    );
    assert!(resize_commands(&output).is_empty());
    assert!(ctx.input(|input| input.pointer.primary_down()));
}

#[test]
fn last_primary_press_in_frame_selects_resize_direction() {
    let ctx = Context::default();
    let corner = pos2(2., 2.);
    let edge = pos2(1028., 350.);
    frame(&ctx, vec![]);
    frame(&ctx, vec![Event::PointerMoved(corner)]);
    let output = frame(
        &ctx,
        vec![
            press(corner, PointerButton::Primary),
            Event::PointerButton {
                pos: corner,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
            press(edge, PointerButton::Primary),
        ],
    );
    assert_eq!(resize_commands(&output), [ResizeDirection::East]);
}

#[test]
fn secondary_press_with_primary_held_does_not_resize_or_reset_pointer() {
    let ctx = Context::default();
    let caption = pos2(1007., 16.);
    let corner = pos2(2., 2.);
    frame(&ctx, vec![]);
    frame(&ctx, vec![Event::PointerMoved(caption)]);
    let held = frame(&ctx, vec![press(caption, PointerButton::Primary)]);
    assert!(resize_commands(&held).is_empty());
    let output = frame(
        &ctx,
        vec![
            Event::PointerMoved(corner),
            press(corner, PointerButton::Secondary),
        ],
    );
    assert!(resize_commands(&output).is_empty());
    assert!(ctx.input(|input| input.pointer.primary_down()));
    assert!(ctx.input(|input| input.pointer.secondary_down()));
    assert!(ctx.dragged_id().is_some());
}

#[test]
fn press_and_release_in_one_frame_does_not_handoff() {
    let ctx = Context::default();
    let corner = pos2(2., 2.);
    frame(&ctx, vec![]);
    frame(&ctx, vec![Event::PointerMoved(corner)]);
    let output = frame(
        &ctx,
        vec![
            press(corner, PointerButton::Primary),
            Event::PointerButton {
                pos: corner,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(resize_commands(&output).is_empty());
    assert_eq!(ctx.input(|input| input.pointer.hover_pos()), Some(corner));
}

#[test]
fn shared_corner_edge_seam_uses_later_zone() {
    let ctx = Context::default();
    let seam = pos2(16., 2.);
    frame(&ctx, vec![]);
    frame(&ctx, vec![Event::PointerMoved(seam)]);
    let output = frame(&ctx, vec![press(seam, PointerButton::Primary)]);
    assert_eq!(resize_commands(&output), [ResizeDirection::North]);
}

#[test]
fn modal_blocks_native_resize() {
    let ctx = Context::default();
    let corner = pos2(2., 2.);
    frame_with_modal(&ctx, vec![], true);
    frame_with_modal(&ctx, vec![Event::PointerMoved(corner)], true);
    let output = frame_with_modal(&ctx, vec![press(corner, PointerButton::Primary)], true);
    assert!(resize_commands(&output).is_empty());
    assert!(ctx.input(|input| input.pointer.primary_down()));
}
