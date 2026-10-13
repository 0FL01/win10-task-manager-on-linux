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
