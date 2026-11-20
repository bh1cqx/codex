#![cfg(test)]

use super::*;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

#[test]
fn buffered_answer_requires_a_paint_before_wheel_input() {
    let cwd = AbsolutePathBuf::try_from(std::env::current_dir().unwrap()).unwrap();
    let mut pane = SideCompanion::empty(cwd);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 20, /*height*/ 4,
    );
    pane.view
        .render(area, &mut Buffer::empty(area), &pane.cells);
    assert!(pane.can_scroll_without_refresh());

    let mut answer = SideAnswer::new(
        ("turn".into(), "item".into()),
        /*index*/ 0,
        /*phase*/ None,
    );
    answer.pending = "new reply".into();
    pane.answer = Some(answer);
    assert!(!pane.can_scroll_without_refresh());

    pane.flush_answer();
    pane.view
        .render(area, &mut Buffer::empty(area), &pane.cells);
    assert!(pane.can_scroll_without_refresh());
}

use codex_app_server_protocol::AgentMessageDeltaNotification;
use codex_app_server_protocol::ItemStartedNotification;
use pretty_assertions::assert_eq;

#[test]
fn message_presentation_survives_side_pane_streaming_and_completion() {
    let cwd = AbsolutePathBuf::from_absolute_path(std::env::temp_dir()).unwrap();
    let store = ThreadEventStore::new(32);
    for (phase, prefix) in [
        (None, "• "),
        (Some(MessagePhase::Commentary), "· "),
        (Some(MessagePhase::FinalAnswer), "▌ "),
    ] {
        let mut side = SideCompanion::empty(cwd.clone());
        let mut item = ThreadItemPayload::AgentMessage {
            moderation: None,
            id: "msg-1".into(),
            text: String::new(),
            phase: phase.clone(),
            memory_citation: None,
            delivery: None,
            questions: None,
        };
        side.apply_notification(
            &ServerNotification::ItemStarted(ItemStartedNotification::new(
                "thread".into(),
                "turn".into(),
                item.clone(),
                0,
            )),
            &store,
            || panic!("no new turn status"),
        );
        side.apply_notification(
            &ServerNotification::AgentMessageDelta(AgentMessageDeltaNotification {
                thread_id: "thread".into(),
                turn_id: "turn".into(),
                item_id: "msg-1".into(),
                delta: "Partial reply".into(),
            }),
            &store,
            || panic!("no new turn status"),
        );
        side.flush_answer();
        assert!(
            side.cells[0].display_lines(60)[0]
                .to_string()
                .starts_with(prefix)
        );
        if let ThreadItemPayload::AgentMessage { text, .. } = &mut item {
            *text = "Complete reply".into();
        }
        side.finish_item("turn", &item);
        assert_eq!(side.cells.len(), 1);
        assert_eq!(side.cells[0].copy_source(), Some("Complete reply"));
        assert!(
            side.cells[0].display_lines(60)[0]
                .to_string()
                .starts_with(prefix)
        );
    }
}
