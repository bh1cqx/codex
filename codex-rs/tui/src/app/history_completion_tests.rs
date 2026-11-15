//! Pagination boundaries preserve grouped item rendering and attach completion metadata once.

#![cfg(test)]

use super::*;
use codex_app_server_protocol::ThreadItem;
use codex_app_server_protocol::TurnItemsView;
use codex_app_server_protocol::TurnStatus;
use pretty_assertions::assert_eq;

fn payloads(turn: &Turn, items: &[ThreadItem]) -> Vec<(String, ThreadItemPayload)> {
    items
        .iter()
        .map(|entry| (turn.id.clone(), entry.payload.clone()))
        .collect()
}

fn turn(id: &str, status: TurnStatus, item_ids: &[&str]) -> Turn {
    Turn {
        id: id.to_string(),
        root_turn_id: None,
        items: item_ids
            .iter()
            .map(|id| ThreadItemPayload::UserMessage {
                id: id.to_string(),
                client_id: None,
                content: Vec::new(),
            })
            .map(ThreadItem::untimed)
            .collect(),
        items_view: TurnItemsView::Summary,
        status,
        error: None,
        started_at: None,
        completed_at: Some(1_700_000_000),
        duration_ms: Some(125_000),
    }
}

#[test]
fn split_turn_only_completes_on_the_page_with_its_last_item() {
    let turns = vec![turn(
        "turn",
        TurnStatus::Completed,
        &["first", "second", "last"],
    )];
    assert_eq!(
        group_turn_items(&payloads(&turns[0], &turns[0].items[1..]), &turns),
        vec![(0..2, Some(&turns[0]))],
    );
    assert_eq!(
        group_turn_items(&payloads(&turns[0], &turns[0].items[..1]), &turns),
        vec![(0..1, None)],
    );
}

#[test]
fn multiple_turns_keep_item_groups_and_completion_order() {
    let turns = vec![
        turn("first", TurnStatus::Completed, &["a", "shared"]),
        turn("second", TurnStatus::Completed, &["middle"]),
        turn("last", TurnStatus::Completed, &["shared"]),
    ];
    let items = turns
        .iter()
        .flat_map(|turn| payloads(turn, &turn.items))
        .collect::<Vec<_>>();
    assert_eq!(
        group_turn_items(&items, &turns),
        vec![
            (0..2, Some(&turns[0])),
            (2..3, Some(&turns[1])),
            (3..4, Some(&turns[2]))
        ],
    );
}

#[test]
fn interleaved_turns_complete_in_item_order() {
    let turns = [
        turn("parent", TurnStatus::Completed, &["c"]),
        turn("follow-up", TurnStatus::Completed, &["b"]),
    ];
    let items = [
        (turns[1].id.clone(), turns[1].items[0].payload.clone()),
        (turns[0].id.clone(), turns[0].items[0].payload.clone()),
    ];
    assert_eq!(
        group_turn_items(&items, &turns),
        vec![(0..1, Some(&turns[1])), (1..2, Some(&turns[0]))],
    );
}

#[test]
fn interleaved_turns_keep_itemless_interruptions_attached() {
    let turns = [
        turn("parent", TurnStatus::Completed, &["a", "c"]),
        turn("after-parent", TurnStatus::Interrupted, &[]),
        turn("follow-up", TurnStatus::Completed, &["b"]),
        turn("after-follow-up", TurnStatus::Interrupted, &[]),
    ];
    let items = [
        (turns[0].id.clone(), turns[0].items[0].payload.clone()),
        (turns[2].id.clone(), turns[2].items[0].payload.clone()),
        (turns[0].id.clone(), turns[0].items[1].payload.clone()),
    ];
    assert_eq!(
        group_turn_items(&items, &turns),
        vec![
            (0..2, Some(&turns[2])),
            (2..2, Some(&turns[3])),
            (2..3, Some(&turns[0])),
            (3..3, Some(&turns[1])),
        ],
    );
}

#[test]
fn itemless_interruptions_keep_their_position_on_the_owning_page() {
    let turns = [
        turn("empty first", TurnStatus::Interrupted, &[]),
        turn("failed", TurnStatus::Failed, &["a"]),
        turn("empty between", TurnStatus::Interrupted, &[]),
        turn("running", TurnStatus::InProgress, &["b"]),
    ];
    assert_eq!(
        group_turn_items(&payloads(&turns[1], &turns[1].items), &turns),
        vec![
            (0..0, Some(&turns[0])),
            (0..1, Some(&turns[1])),
            (1..1, Some(&turns[2]))
        ],
    );
    assert_eq!(
        group_turn_items(&payloads(&turns[3], &turns[3].items), &turns),
        vec![(0..1, Some(&turns[3]))],
    );
}
