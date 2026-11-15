use super::*;
use crate::app_event::ConsolidationScrollbackReflow;

fn item(id: &str, text: &str, phase: Option<MessagePhase>) -> AppServerThreadItemPayload {
    AppServerThreadItemPayload::AgentMessage {
        moderation: None,
        id: id.into(),
        text: text.into(),
        phase,
        memory_citation: None,
        delivery: None,
        questions: None,
    }
}

fn start(chat: &mut ChatWidget, id: &str, text: &str, phase: Option<MessagePhase>) {
    chat.handle_server_notification(
        ServerNotification::ItemStarted(ItemStartedNotification::new(
            chat.thread_id.map(|id| id.to_string()).unwrap_or_default(),
            "turn-1".into(),
            item(id, text, phase),
            0,
        )),
        None,
    );
}

#[tokio::test]
async fn message_presentation_carries_phase_from_empty_start_through_replay() {
    for (phase, prefix) in [
        (None, "• "),
        (Some(MessagePhase::Commentary), "· "),
        (Some(MessagePhase::PartialAnswer), "▌ "),
        (Some(MessagePhase::FinalAnswer), "▌ "),
    ] {
        let (mut chat, mut rx, _ops) = make_chatwidget_manual(None).await;
        handle_turn_started(&mut chat, "turn-1");
        start(&mut chat, "msg-1", "", phase.clone());
        handle_agent_message_delta(&mut chat, "A partial reply");
        assert!(
            chat.transcript
                .active_cell
                .as_ref()
                .expect("live tail")
                .display_lines(60)[0]
                .to_string()
                .starts_with(prefix)
        );
        handle_agent_message_delta(&mut chat, ".\n");
        chat.run_commit_tick();
        let committed = drain_insert_history(&mut rx);
        assert!(
            committed
                .iter()
                .flatten()
                .any(|line| line.to_string().starts_with(prefix))
        );
        complete_assistant_message(&mut chat, "msg-1", "A partial reply.", phase.clone());
        let completed_phase = std::iter::from_fn(|| rx.try_recv().ok())
            .find_map(|event| match event {
                AppEvent::ConsolidateAgentMessage { phase, .. } => Some(phase),
                _ => None,
            })
            .expect("consolidation");
        assert_eq!(completed_phase, phase);

        let replayed = crate::thread_transcript::thread_items_to_transcript_cells(
            chat.thread_id,
            &chat.config.cwd,
            [item("msg-1", "A partial reply.", phase)],
            crate::thread_transcript::RawReasoningVisibility::Hidden,
            None,
        );
        assert!(
            replayed[0].display_lines(60)[0]
                .to_string()
                .starts_with(prefix)
        );
    }
}

#[tokio::test]
async fn message_presentation_repairs_late_phase_and_preserves_interrupted_phase() {
    let (mut chat, mut rx, _ops) = make_chatwidget_manual(None).await;
    handle_turn_started(&mut chat, "turn-1");
    handle_agent_message_delta(&mut chat, "Answer.");
    complete_assistant_message(
        &mut chat,
        "msg-1",
        "Answer.",
        Some(MessagePhase::FinalAnswer),
    );
    let events = std::iter::from_fn(|| rx.try_recv().ok()).collect::<Vec<_>>();
    assert!(events.iter().any(|event| match event {
        AppEvent::InsertHistoryCell(cell) => cell.display_lines(60)[0].to_string() == "▌ Answer.",
        _ => false,
    }));
    let reflow = events
        .into_iter()
        .find_map(|event| match event {
            AppEvent::ConsolidateAgentMessage {
                phase,
                scrollback_reflow,
                ..
            } => Some((phase, scrollback_reflow)),
            _ => None,
        })
        .expect("consolidation");
    assert_eq!(reflow.0, Some(MessagePhase::FinalAnswer));
    assert_eq!(reflow.1, ConsolidationScrollbackReflow::Required);

    start(&mut chat, "msg-1", "", Some(MessagePhase::Commentary));
    handle_agent_message_delta(&mut chat, "Checking.\n");
    chat.flush_answer_stream_with_separator();
    let phase = std::iter::from_fn(|| rx.try_recv().ok())
        .find_map(|event| match event {
            AppEvent::ConsolidateAgentMessage { phase, .. } => Some(phase),
            _ => None,
        })
        .expect("interrupted consolidation");
    assert_eq!(phase, Some(MessagePhase::Commentary));
    handle_agent_message_delta(&mut chat, "Still checking");
    assert!(
        chat.transcript
            .active_cell
            .as_ref()
            .expect("resumed tail")
            .display_lines(60)[0]
            .to_string()
            .starts_with("· ")
    );
}

#[tokio::test]
async fn message_presentation_does_not_reuse_another_items_phase_or_drop_start_text() {
    let (mut chat, _rx, _ops) = make_chatwidget_manual(None).await;
    handle_turn_started(&mut chat, "turn-1");
    start(&mut chat, "other-item", "", Some(MessagePhase::Commentary));
    handle_agent_message_delta(&mut chat, "Unknown phase");
    assert!(
        chat.transcript
            .active_cell
            .as_ref()
            .expect("unclassified tail")
            .display_lines(60)[0]
            .to_string()
            .starts_with("• ")
    );
    chat.flush_answer_stream_with_separator();

    start(&mut chat, "next-item", "", Some(MessagePhase::FinalAnswer));
    start(
        &mut chat,
        "next-item",
        "First text",
        Some(MessagePhase::FinalAnswer),
    );
    assert!(
        chat.transcript
            .active_cell
            .as_ref()
            .expect("start text")
            .display_lines(60)[0]
            .to_string()
            .starts_with("▌ First text")
    );
}
