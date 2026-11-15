use super::*;
use crate::history_cell::markdown_render_cache::MarkdownRenderCacheKey;
use assert_matches::assert_matches;
use pretty_assertions::assert_eq;

#[test]
fn message_presentation_preserves_source_links_and_markdown() {
    let markdown = "Plain **bold** [link](https://example.test).\n\nAnother paragraph.";
    for (fg, bg) in [
        ((230, 230, 230), (20, 20, 20)),
        ((20, 20, 20), (250, 250, 250)),
    ] {
        crate::terminal_palette::with_test_default_colors(
            crate::terminal_probe::DefaultColors { fg, bg },
            || {
                for (phase, prefix) in [
                    (None, "• "),
                    (Some(MessagePhase::Commentary), "· "),
                    (Some(MessagePhase::PartialAnswer), "▌ "),
                    (Some(MessagePhase::FinalAnswer), "▌ "),
                ] {
                    for width in [80, 24] {
                        let cell = AgentMarkdownCell::new(markdown.into(), Path::new("/tmp"))
                            .with_phase(phase.clone());
                        let lines = cell.display_hyperlink_lines(width);
                        assert!(lines[0].line.to_string().starts_with(prefix));
                        assert_eq!(cell.copy_source(), Some(markdown));
                        assert_eq!(cell.raw_lines(), raw_lines_from_source(markdown));
                        assert_eq!(cell.transcript_hyperlink_lines(width), lines);
                        assert!(lines.iter().any(|line| {
                            line.hyperlinks
                                .iter()
                                .any(|link| link.destination == "https://example.test")
                        }));
                        let bold = lines
                            .iter()
                            .flat_map(|line| &line.line.spans)
                            .find(|span| span.content == "bold")
                            .expect("bold span");
                        assert!(bold.style.add_modifier.contains(Modifier::BOLD));
                        if phase == Some(MessagePhase::Commentary) {
                            let plain = lines[0]
                                .line
                                .spans
                                .iter()
                                .find(|span| span.content.starts_with("Plain"))
                                .expect("plain prose");
                            assert_eq!(plain.style.fg, secondary_text_style().fg);
                            let source = lines[0].source.as_ref().expect("copy source");
                            assert_eq!(
                                source.styled_range(source.range.clone()).spans[0].style.fg,
                                plain.style.fg,
                            );
                        } else if phase.is_some() {
                            assert!(lines.iter().all(|line| {
                                line.line.spans[0].content == "▌ "
                                    && line.line.spans[0].style == answer_gutter_style()
                            }));
                        }
                    }
                }
            },
        );
    }
}

#[test]
fn message_presentation_matches_streaming_cells_and_invalidates_phase_cache() {
    let markdown = "First **line**.\n\nSecond line.";
    for phase in [
        None,
        Some(MessagePhase::Commentary),
        Some(MessagePhase::FinalAnswer),
    ] {
        let body = crate::markdown::render_markdown_agent_with_list_spacing(
            markdown,
            Some(38),
            Some(Path::new("/tmp")),
            None,
            crate::markdown_render::ListSpacing::AfterMultiline,
        );
        let streamed =
            AgentMessageCell::new_hyperlink_lines(body.clone(), true).with_phase(phase.clone());
        let raw_tail = StreamingAgentTailCell::new(body.clone(), true).raw_lines();
        let tail = StreamingAgentTailCell::new(body, true).with_phase(phase.clone());
        assert_eq!(tail.raw_lines(), raw_tail);
        let cell =
            AgentMarkdownCell::new(markdown.into(), Path::new("/tmp")).with_phase(phase.clone());
        let expected = cell.display_lines(40);
        assert_eq!(
            visible_lines(normalize_whitespace_only_hyperlink_lines(
                streamed.display_hyperlink_lines(40)
            )),
            expected,
        );
        assert_eq!(tail.display_lines(40), expected);
        let changed = cell.with_phase(Some(MessagePhase::Commentary));
        assert!(changed.display_lines(40)[0].to_string().starts_with("· "));
    }
}

#[test]
fn message_presentation_keeps_reasoning_in_the_expanded_transcript() {
    let reasoning = ReasoningSummaryCell::new(
        String::new(),
        "Thinking through the options.".into(),
        Path::new("/tmp"),
        true,
    );
    assert!(reasoning.display_lines(60).is_empty());
    assert!(reasoning.raw_lines().is_empty());
    let lines = reasoning.transcript_lines(60);
    assert!(lines[0].to_string().starts_with("∴ "));
    assert!(
        lines[0]
            .spans
            .iter()
            .filter(|span| !span.content.trim().is_empty())
            .all(|span| span.style.add_modifier.contains(Modifier::ITALIC))
    );
}

#[test]
fn typed_prompt_markdown_keeps_the_prompt_gutter_and_links() {
    let message = "**testing** _hello_ [example documentation](https://example.test/docs)\n\n- Try the command\n\n```sh\necho '*literal*'\n```";
    let cell = new_user_prompt(message.into(), Vec::new(), Vec::new(), Vec::new());

    let mut snapshots = Vec::new();
    for width in [64, 20] {
        let lines = cell.display_hyperlink_lines(width);
        assert_eq!(lines, cell.transcript_hyperlink_lines(width));
        assert!(lines.iter().all(|line| line.width() < usize::from(width)));
        for (text, modifier) in [("testing", Modifier::BOLD), ("hello", Modifier::ITALIC)] {
            assert!(lines.iter().flat_map(|line| &line.line.spans).any(|span| {
                span.content == text && span.style.add_modifier.contains(modifier)
            }));
        }
        let linked_text = lines
            .iter()
            .flat_map(|line| {
                line.hyperlinks
                    .iter()
                    .filter(|link| link.destination == "https://example.test/docs")
                    .flat_map(|link| {
                        line.line
                            .to_string()
                            .chars()
                            .skip(link.columns.start)
                            .take(link.columns.len())
                            .collect::<Vec<_>>()
                    })
            })
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>();
        assert_eq!(linked_text, "exampledocumentationhttps://example.test/docs");
        let rendered = ratatui::text::Text::from(visible_lines(lines)).to_string();
        let rendered = rendered
            .lines()
            .map(str::trim_end)
            .collect::<Vec<_>>()
            .join("\n");
        snapshots.push(format!("width {width}\n{rendered}"));
    }
    insta::assert_snapshot!(snapshots.join("\n\n"));
}

#[test]
fn user_prompt_urls_keep_destinations_across_widths() {
    let url = "https://example.test/terminal-link-wrapping/solid-octants";
    let placeholder = "[Image #1]";
    let message = format!(
        "from this site, write out the ascii for solid octants 18 columns {url}\n{placeholder} See (https://example.test/a)."
    );
    let image_start = message.find(placeholder).unwrap();
    let cell = new_user_prompt(
        message,
        vec![TextElement::new(
            (image_start..image_start + placeholder.len()).into(),
            Some(placeholder.to_string()),
        )],
        Vec::new(),
        Vec::new(),
    );
    let mut snapshots = Vec::new();
    for width in [120, 60, 24] {
        let lines = cell.display_hyperlink_lines(width);
        let transcript = cell.transcript_hyperlink_lines(width);
        assert_eq!(
            transcript
                .into_iter()
                .map(HyperlinkLine::resolve_span_styles)
                .collect::<Vec<_>>(),
            lines
        );
        assert!(lines.iter().all(|line| line.width() <= usize::from(width)));
        for destination in [url, "https://example.test/a"] {
            let linked_text = lines
                .iter()
                .flat_map(|line| {
                    line.hyperlinks
                        .iter()
                        .filter(|link| link.destination == destination)
                        .flat_map(|link| {
                            line.line
                                .to_string()
                                .chars()
                                .skip(link.columns.start)
                                .take(link.columns.len())
                                .collect::<Vec<_>>()
                        })
                })
                .collect::<String>();
            assert_eq!(linked_text, destination, "width {width}");
        }
        snapshots.push(format!(
            "width {width}\n{}",
            ratatui::text::Text::from(visible_lines(lines))
        ));
    }
    insta::assert_snapshot!(snapshots.join("\n\n"));
}

#[test]
fn local_image_only_user_message_remains_visible() {
    let cell = new_user_prompt(
        String::new(),
        Vec::new(),
        vec![PathBuf::from("fixture.png")],
        Vec::new(),
    );
    let display = cell.display_lines(/*width*/ 40);

    assert_eq!(cell.transcript_lines(/*width*/ 40), display);
    insta::assert_snapshot!(ratatui::text::Text::from(display));
}

#[test]
fn mixed_image_labels_preserve_existing_placeholders_without_duplicates() {
    let placeholder = "[Image #2]";
    let cell = new_user_prompt(
        format!("{placeholder} **compare** these"),
        vec![TextElement::new(
            (0..placeholder.len()).into(),
            Some(placeholder.to_string()),
        )],
        vec![PathBuf::from("one.png"), PathBuf::from("two.png")],
        vec!["https://example.test/remote.png".to_string()],
    );

    insta::assert_snapshot!(ratatui::text::Text::from(cell.display_lines(/*width*/ 40)));
}

#[test]
fn sanitizer_borrows_clean_text_and_removes_control_sequences() {
    for (text, expected) in [
        ("clean\ttext\n", "clean\ttext\n"),
        ("\x07before", "before"),
        ("before\x07", "before"),
        ("\x1b[31mbefore", "before"),
        ("before\x1b[31m", "before"),
        ("before\x1b[31", "before"),
        ("\x07[31m", "[31m"),
        ("\x07", ""),
    ] {
        assert_matches!(
            sanitize_user_text(text.into()),
            Cow::Borrowed(sanitized) => assert_eq!(sanitized, expected)
        );
    }
    assert_matches!(
        sanitize_user_text("before\x1b[31mafter\x07".into()),
        Cow::Owned(sanitized) => assert_eq!(sanitized, "beforeafter")
    );
    assert_eq!(sanitize_user_text("é\u{85}中".into()), "é中");
    assert_eq!(sanitize_user_text("before\x1bafter".into()), "beforeafter");
}

#[test]
fn sanitizer_preserves_owned_buffer_for_clean_and_edge_trimmed_text() {
    for (text, expected) in [
        ("clean\ttext\n", "clean\ttext\n"),
        ("\x07before", "before"),
        ("before\x07", "before"),
        ("\x07before\x07", "before"),
        ("\x1b[31mbefore", "before"),
        ("before\x1b[31m", "before"),
        ("\x07", ""),
    ] {
        let owned = text.to_string();
        let original_pointer = owned.as_ptr();
        let original_capacity = owned.capacity();

        assert_matches!(sanitize_user_text(owned.into()), Cow::Owned(sanitized) => {
            assert_eq!(sanitized, expected);
            assert_eq!(sanitized.as_ptr(), original_pointer);
            assert_eq!(sanitized.capacity(), original_capacity);
        })
    }
}

#[test]
fn sanitizer_preallocates_owned_multi_fragment_text() {
    let text = "before\x1b[31mafter\x07".to_string();
    let original_length = text.len();

    assert_matches!(sanitize_user_text(text.into()), Cow::Owned(sanitized) => {
        assert_eq!(sanitized, "beforeafter");
        assert!(sanitized.capacity() >= original_length, "{} >= {}", sanitized.capacity(), original_length);
    })
}

#[test]
fn spoken_user_messages_have_a_red_chevron_without_changing_raw_text() {
    let message = "  hello from voice";
    let spoken = new_spoken_user_prompt(message.to_string());
    let typed = new_user_prompt(message.to_string(), Vec::new(), Vec::new(), Vec::new());
    let marker = spoken
        .display_hyperlink_lines(/*width*/ 40)
        .into_iter()
        .flat_map(|line| line.line.spans)
        .find(|span| span.content == "› ")
        .expect("spoken user marker");

    assert!(
        spoken
            .display_lines(/*width*/ 40)
            .iter()
            .any(|line| line.to_string() == "› hello from voice")
    );
    assert_eq!(typed.raw_lines(), vec![Line::from(message)]);
    assert!(
        typed
            .display_lines(/*width*/ 40)
            .iter()
            .any(|line| line.to_string() == "›   hello from voice")
    );
    assert_eq!(marker.style.fg, Some(Color::Red));
    assert!(marker.style.add_modifier.contains(Modifier::BOLD));
    assert_eq!(spoken.raw_lines(), vec![Line::from(message)]);
    assert_eq!(
        spoken.display_lines_for_mode(/*width*/ 40, HistoryRenderMode::Raw),
        vec![Line::from(message)]
    );
    assert!(typed.display_lines(/*width*/ 40).iter().any(|line| {
        line.spans
            .iter()
            .any(|span| span.content == "› " && span.style.fg != Some(Color::Red))
    }));

    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 40, /*height*/ 3,
    );
    let mut buf = Buffer::empty(area);
    let lines = crate::terminal_palette::with_test_default_colors(/*colors*/ None, || {
        spoken.display_lines(area.width)
    });
    Paragraph::new(lines).render(area, &mut buf);
    insta::assert_snapshot!("spoken_user_prompt", format!("{buf:?}"));
}

fn replace_cached_lines(
    cell: &AgentMarkdownCell,
    update_key: impl FnOnce(&mut MarkdownRenderCacheKey),
) {
    let rendered_lines = cell
        .rendered_lines
        .as_ref()
        .expect("ordinary markdown should be cacheable");
    let mut rendered_lines = rendered_lines.cached.lock().expect("render cache lock");
    let (key, lines) = rendered_lines
        .as_mut()
        .expect("render cache should be populated");
    *lines = vec![HyperlinkLine::from("cached")];
    update_key(key);
}

#[test]
fn finalized_markdown_reuses_rendered_transcript_lines() {
    let cell = AgentMarkdownCell::new("finalized **markdown**".to_string(), Path::new("/tmp"));
    let width = 48;

    assert_eq!(
        visible_lines(cell.transcript_hyperlink_lines(width)),
        cell.display_lines(width)
    );
    replace_cached_lines(&cell, |_| {});

    assert_eq!(
        visible_lines(cell.transcript_hyperlink_lines(width)),
        vec![Line::from("cached")]
    );
}

#[test]
fn finalized_assistant_file_citation_renders_as_local_path_snapshot() {
    let cwd = std::env::temp_dir();
    let output = cwd.join("Quarterly Report.xlsx").display().to_string();
    let cell = AgentMarkdownCell::new(
        format!(
            r#"Generated :codex-file-citation{{artifact_kind="workbook" path="{output}" purpose="output"}}."#
        ),
        &cwd,
    );

    let rendered = ratatui::text::Text::from(cell.display_lines(/*width*/ 80));

    insta::assert_snapshot!(rendered, @"• Generated Quarterly Report.xlsx.");
}

#[test]
fn finalized_markdown_cache_misses_when_width_or_render_style_changes() {
    let cell = AgentMarkdownCell::new("finalized **markdown**".to_string(), Path::new("/tmp"));
    let width = 48;
    let expected = cell.display_lines(width);

    replace_cached_lines(&cell, |key| key.width = key.width.saturating_sub(1));
    assert_eq!(cell.display_lines(width), expected);

    replace_cached_lines(&cell, |key| {
        key.syntax_theme_revision = key.syntax_theme_revision.wrapping_sub(1);
    });
    assert_eq!(cell.display_lines(width), expected);

    replace_cached_lines(&cell, |key| {
        key.terminal_fg = key
            .terminal_fg
            .map_or(Some((1, 2, 3)), |(r, g, b)| Some((r ^ 1, g, b)));
    });
    assert_eq!(cell.display_lines(width), expected);
}

#[test]
fn raw_markdown_bypasses_the_rich_render_cache() {
    let source = "finalized **markdown**";
    let cell = AgentMarkdownCell::new(source.to_string(), Path::new("/tmp"));
    let width = 48;

    cell.display_lines(width);
    replace_cached_lines(&cell, |_| {});

    assert_eq!(
        cell.display_lines_for_mode(width, HistoryRenderMode::Raw),
        vec![Line::from(source)]
    );
}

#[test]
fn visualization_directives_are_not_cached() {
    for markdown in [
        "::codex-inline-vis{file=\"chart.html\"}",
        "\u{e200}visualize\u{e202}{\"path\":\"/tmp/chart.html\"}\u{e201}",
    ] {
        let cell = AgentMarkdownCell::new(markdown.to_string(), Path::new("/tmp"));

        cell.display_lines(/*width*/ 48);

        assert!(cell.rendered_lines.is_none());
    }
}

#[test]
fn spoken_artifacts_link_only_real_workspace_files_and_preserve_existing_urls() {
    let workspace = tempfile::tempdir().expect("workspace");
    let source_directory = workspace.path().join("src");
    std::fs::create_dir(&source_directory).expect("source directory");
    let source_file = source_directory.join("lib.rs");
    std::fs::write(&source_file, "fn example() {}\n").expect("source file");
    let markdown = "中 src/lib.rs:42 and https://example.com";
    let spoken = AgentMarkdownCell::new_spoken(markdown.to_string(), workspace.path());
    let lines = spoken.display_hyperlink_lines(/*width*/ 90);
    let line = &lines[0];

    let expected_destination =
        url::Url::from_file_path(source_file.canonicalize().expect("canonical source file"))
            .expect("workspace file URL")
            .to_string();
    assert_eq!(
        line.hyperlinks
            .iter()
            .map(|link| (link.columns.clone(), link.destination.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (5..18, expected_destination.as_str()),
            (23..42, "https://example.com"),
        ]
    );
    assert!(line.line.spans.iter().any(|span| {
        span.content == "src/lib.rs:42" && span.style.add_modifier.contains(Modifier::UNDERLINED)
    }));
    assert_eq!(spoken.raw_lines(), vec![Line::from(markdown)]);
    for width in [90, 24] {
        for line in spoken.display_hyperlink_lines(width) {
            let source = line.source.expect("spoken source");
            assert!(
                source
                    .styled_range(0..source.text.len())
                    .spans
                    .iter()
                    .any(|span| {
                        span.content == "src/lib.rs:42"
                            && span.style.add_modifier.contains(Modifier::UNDERLINED)
                    })
            );
        }
    }
    insta::assert_snapshot!(
        format!(
            "{}\n{:?} -> <workspace>/src/lib.rs\n{:?} -> https://example.com",
            line.line, line.hyperlinks[0].columns, line.hyperlinks[1].columns
        ),
        @r"
    • 中 src/lib.rs:42 and https://example.com
    5..18 -> <workspace>/src/lib.rs
    23..42 -> https://example.com
    "
    );

    let ordinary = AgentMarkdownCell::new(markdown.to_string(), workspace.path());
    assert_eq!(
        ordinary.display_hyperlink_lines(/*width*/ 90)[0].hyperlinks[0].destination,
        "https://example.com"
    );
    let report = workspace.path().join("report.md");
    std::fs::write(&report, "# Report").expect("report file");
    let linked = AgentMarkdownCell::new_spoken("[Report](report.md)".into(), workspace.path());
    let expected = url::Url::from_file_path(report.canonicalize().expect("canonical report"))
        .expect("workspace file URL")
        .to_string();
    assert_eq!(
        linked.display_hyperlink_lines(/*width*/ 90)[0]
            .hyperlinks
            .iter()
            .filter_map(crate::terminal_hyperlinks::TerminalHyperlink::terminal_destination)
            .collect::<Vec<_>>(),
        vec![expected],
    );
    let bare = AgentMarkdownCell::new_spoken("src/lib.rs".to_string(), workspace.path());
    assert_eq!(
        bare.display_hyperlink_lines(/*width*/ 40)[0]
            .hyperlinks
            .len(),
        1
    );
}

#[test]
fn spoken_windows_relative_paths_keep_original_text_and_link_the_workspace_file() {
    let workspace = tempfile::tempdir().expect("workspace");
    let source_directory = workspace.path().join("src");
    std::fs::create_dir(&source_directory).expect("source directory");
    let source_file = source_directory.join("lib.rs");
    std::fs::write(&source_file, "fn example() {}\n").expect("source file");
    let markdown = "Updated src\\lib.rs:42";
    let spoken = AgentMarkdownCell::new_spoken(markdown.to_string(), workspace.path());
    let lines = spoken.display_hyperlink_lines(/*width*/ 80);
    let line = &lines[0];

    assert_eq!(spoken.raw_lines(), vec![Line::from(markdown)]);
    assert_eq!(line.hyperlinks.len(), 1);
    assert_eq!(line.hyperlinks[0].columns, 10..23);
    assert_eq!(
        line.hyperlinks[0].destination,
        url::Url::from_file_path(source_file.canonicalize().expect("canonical source file"))
            .expect("workspace file URL")
            .to_string()
    );
    assert!(line.line.spans.iter().any(|span| {
        span.content == "src\\lib.rs:42" && span.style.add_modifier.contains(Modifier::UNDERLINED)
    }));
    insta::assert_snapshot!(line.line.to_string(), @"• Updated src\\lib.rs:42");
}
