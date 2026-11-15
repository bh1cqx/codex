use super::*;
use crate::history_cell::AgentMarkdownCell;
use crate::history_cell::AgentMessageCell;
use crate::history_cell::ReasoningSummaryCell;
use crate::history_cell::StreamingAgentTailCell;
use crate::history_cell::new_user_prompt;
use crate::transcript_view::tests::cell;
use crate::transcript_view::tests::render;
use crate::transcript_view::tests::text;
use codex_protocol::models::MessagePhase;
use crossterm::event::KeyCode;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use std::path::Path;

fn message(text: &str, phase: Option<MessagePhase>) -> Arc<dyn HistoryCell> {
    Arc::new(AgentMarkdownCell::new(text.into(), Path::new(".")).with_phase(phase))
}

fn answer(text: &str) -> Arc<dyn HistoryCell> {
    message(text, Some(MessagePhase::FinalAnswer))
}

fn press(view: &mut TranscriptView, cells: &[Arc<dyn HistoryCell>], key: char) {
    assert!(matches!(
        view.handle_key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::ALT), cells),
        Some(ViewAction::Changed)
    ));
}

fn assert_at(view: &TranscriptView, cells: &[Arc<dyn HistoryCell>], index: usize) {
    let Position::Reading(anchor) = view.position else {
        panic!("expected an answer anchor");
    };
    assert_eq!(anchor.key, view.entry_key(cells, index));
    assert_eq!(anchor.offset, 0);
}

#[test]
fn answers_only_filters_history_in_both_presentations_and_restores_it() {
    let cells = vec![
        Arc::new(new_user_prompt(
            "User prompt".into(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )) as Arc<dyn HistoryCell>,
        message("Progress note", Some(MessagePhase::Commentary)),
        cell("Tool result"),
        Arc::new(ReasoningSummaryCell::new(
            "Thinking".into(),
            "Reasoning detail".into(),
            Path::new("."),
            /*transcript_only*/ false,
        )),
        message("Partial response", Some(MessagePhase::PartialAnswer)),
        answer("Final response"),
        message("Legacy response", None),
    ];
    for mode in [HistoryRenderMode::Rich, HistoryRenderMode::Raw] {
        for detailed in [false, true] {
            let mut view = TranscriptView::default();
            view.set_presentation(detailed, mode);
            let original = text(&render(&mut view, &cells, 60, 30));
            press(&mut view, &cells, 'a');
            for width in [60, 24] {
                let clean = text(&render(&mut view, &cells, width, 30));
                for visible in [
                    "User prompt",
                    "Partial response",
                    "Final response",
                    "Legacy response",
                ] {
                    assert!(clean.contains(visible), "{clean}");
                }
                for hidden in ["Progress note", "Tool result", "Reasoning detail"] {
                    assert!(!clean.contains(hidden), "{clean}");
                }
            }
            press(&mut view, &cells, 'a');
            assert_eq!(text(&render(&mut view, &cells, 60, 30)), original);
        }
    }
}

#[test]
fn answer_navigation_skips_progress_and_stream_continuations() {
    let cells = vec![
        answer("First answer"),
        message("Progress", Some(MessagePhase::Commentary)),
        Arc::new(
            AgentMessageCell::new(vec!["Partial start".into()], true)
                .with_phase(Some(MessagePhase::PartialAnswer)),
        ),
        Arc::new(
            AgentMessageCell::new(vec!["Partial continuation".into()], false)
                .with_phase(Some(MessagePhase::PartialAnswer)),
        ),
        cell("Tool output"),
        answer("Last answer\nSecond line\nThird line"),
    ];
    let mut view = TranscriptView::default();
    render(&mut view, &cells, 40, 4);
    for expected in [5, 2, 0, 0] {
        press(&mut view, &cells, 'k');
        assert_at(&view, &cells, expected);
    }
    for expected in [2, 5, 5] {
        press(&mut view, &cells, 'j');
        assert_at(&view, &cells, expected);
    }
    view.scroll(&cells, 3);
    press(&mut view, &cells, 'k');
    assert_at(&view, &cells, 5);
}

#[test]
fn previous_answer_continues_across_pages_without_answers() {
    let mut cells = vec![answer("Newest")];
    let mut view = TranscriptView::default();
    view.history = TranscriptHistoryState::Partial;
    render(&mut view, &cells, 30, 3);
    press(&mut view, &cells, 'k');
    press(&mut view, &cells, 'k');
    assert!(view.pending_previous_answer);
    assert!(view.needs_history(&cells));

    cells.insert(0, cell("Earlier tool"));
    view.history_loaded(&cells, 0..1);
    assert!(view.pending_previous_answer);
    cells.splice(0..0, [answer("Older answer"), cell("Another tool")]);
    view.history_loaded(&cells, 0..2);
    assert_at(&view, &cells, 0);
    assert!(!view.pending_previous_answer);

    press(&mut view, &cells, 'k');
    assert!(view.pending_previous_answer);
    view.jump_to_latest();
    cells.insert(0, answer("Oldest"));
    view.history_loaded(&cells, 0..1);
    assert!(view.is_following());
}

#[test]
fn live_answer_is_a_target_only_at_the_start_of_a_message() {
    let cells = vec![answer("Committed")];
    let mut view = TranscriptView::default();
    for (phase, first, expected) in [
        (MessagePhase::FinalAnswer, true, 1),
        (MessagePhase::PartialAnswer, true, 1),
        (MessagePhase::FinalAnswer, false, 0),
        (MessagePhase::Commentary, true, 0),
    ] {
        let live =
            StreamingAgentTailCell::new(vec!["Live text".into()], first).with_phase(Some(phase));
        view.set_live_answer(Some(&live));
        view.sync_live_tail(40, None, |width| {
            Some(live.transcript_hyperlink_lines(width))
        });
        view.jump_to_latest();
        press(&mut view, &cells, 'k');
        assert_at(&view, &cells, expected);
    }
}

#[test]
fn answers_only_clears_hidden_selection_and_searches_only_visible_messages() {
    let cells = vec![
        answer("Answer needle"),
        cell("Hidden needle"),
        answer("Newest"),
    ];
    let mut view = TranscriptView::default();
    render(&mut view, &cells, 40, 20);
    view.begin_search();
    view.paste_search("Hidden");
    while view.advance_search(&cells) {}
    assert!(view.search.match_anchor().is_some());

    press(&mut view, &cells, 'a');
    assert!(!view.search.is_active());
    assert!(view.snapshot().is_none());
    view.begin_search();
    view.paste_search("Hidden");
    while view.advance_search(&cells) {}
    assert!(view.search.match_anchor().is_none());
    view.cancel_search();
    view.jump_to_entry(&cells, 0);
    render(&mut view, &cells, 40, 20);
    view.begin_selection(&cells, 0, 0, 1);
    view.extend_selection(39, 19);
    let copied = view
        .selected_text(&cells)
        .expect("selected visible transcript");
    assert!(copied.contains("Answer needle"));
    assert!(copied.contains("Newest"));
    assert!(!copied.contains("Hidden"));
    press(&mut view, &cells, 'a');
    assert!(view.selection.is_none());
    assert!(text(&render(&mut view, &cells, 40, 20)).contains("Hidden"));
}

#[test]
fn filtered_view_has_no_turn_disclosure_and_bookmark_preserves_mode() {
    let cells = vec![
        Arc::new(new_user_prompt(
            "Prompt".into(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )) as Arc<dyn HistoryCell>,
        message("Working", Some(MessagePhase::Commentary)),
        answer("Answer"),
    ];
    let mut view = TranscriptView::default();
    view.sync_turn_details(&cells, true, None);
    press(&mut view, &cells, 'a');
    assert!(view.turn_row(&cells, 0).is_none());
    render(&mut view, &cells, 40, 20);
    view.jump_to_entry(&cells, 2);
    let bookmark = view.bookmark(&cells);
    press(&mut view, &cells, 'a');
    view.restore_bookmark(bookmark);
    assert!(view.is_answers_only());
    assert!(!text(&render(&mut view, &cells, 40, 20)).contains("Working"));
}

#[test]
fn releasing_shortcut_does_not_toggle_twice() {
    let mut view = TranscriptView::default();
    press(&mut view, &[], 'a');
    let mut release = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::ALT);
    release.kind = KeyEventKind::Release;
    assert!(view.handle_key(release, &[]).is_none());
    assert!(view.is_answers_only());
}

#[test]
fn whole_response_copy_ignores_later_commentary_in_filtered_view() {
    let cells = vec![
        answer("Answer **Markdown**"),
        message("Later progress", Some(MessagePhase::Commentary)),
    ];
    let mut view = TranscriptView::default();
    press(&mut view, &cells, 'a');
    render(&mut view, &cells, 40, 10);
    assert!(view.begin_copy_mode(&cells, None));
    let selected = view.selected_text(&cells).expect("answer selected");
    let mut copied = String::new();
    view.copy_selected_text_with(&cells, &selected, false, |text, _| {
        copied = text.into();
        Ok(crate::clipboard_copy::CopyStatus::Confirmed)
    })
    .expect("copy");
    assert_eq!(copied, "Answer **Markdown**");
}

#[test]
fn filtering_a_live_activity_returns_to_conversation_and_labels_narrow_footer() {
    let cells = vec![answer("Previous answer")];
    let mut view = TranscriptView::default();
    view.sync_live_tail(40, None, |_| Some(vec!["Live tool output".into()]));
    render(&mut view, &cells, 40, 10);
    view.jump_to_entry(&cells, cells.len());
    press(&mut view, &cells, 'a');
    assert!(text(&render(&mut view, &cells, 40, 10)).contains("Previous answer"));
    for width in [20, 80] {
        let footer = view
            .footer_with_navigation(width, crate::motion::MotionMode::Reduced, "")
            .expect("mode indicator");
        assert!(footer.text.to_string().contains("Answers only"));
    }
}

#[test]
fn filtered_history_keeps_proposed_plans_and_structured_questions() {
    let question = "Which option?";
    let cells: Vec<Arc<dyn HistoryCell>> = vec![
        Arc::new(crate::history_cell::new_proposed_plan(
            "Proposed plan".into(),
            Path::new("."),
        )),
        Arc::new(
            AgentMarkdownCell::new(question.into(), Path::new("."))
                .with_phase(Some(MessagePhase::Commentary))
                .with_questions(&[codex_protocol::items::AsyncUserInputQuestion {
                    title: question.into(),
                    options: None,
                }]),
        ),
    ];
    let mut view = TranscriptView::default();
    press(&mut view, &cells, 'a');
    let rendered = text(&render(&mut view, &cells, 40, 20));
    assert!(rendered.contains("Proposed plan"));
    assert!(rendered.contains(question));
    assert!(cells.iter().all(|cell| !cell.is_answer()));
}
