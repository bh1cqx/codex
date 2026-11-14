#![cfg(test)]

use super::StatusLine;
use super::StatusLineFocus;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::osc8_hyperlink;
use pretty_assertions::assert_eq;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

#[test]
fn status_line_rows_preserve_boundaries_and_clip_to_the_viewport() {
    let status = StatusLine::from(vec![
        HyperlinkLine::from("model"),
        HyperlinkLine::from("usage remaining"),
        HyperlinkLine::from("branch"),
    ]);
    for (width, height, expected) in [
        (20, 3, vec!["model", "usage remaining", "branch"]),
        (6, 3, vec!["model", "usage…", "branch"]),
        (20, 2, vec!["model", "usage remaining…"]),
        (6, 1, vec!["model…"]),
        (0, 3, vec![]),
        (6, 0, vec![]),
    ] {
        let area = Rect::new(/*x*/ 1, /*y*/ 1, width, height);
        let mut buffer =
            Buffer::empty(Rect::new(/*x*/ 0, /*y*/ 0, width + 2, height + 2));
        status.render(area, &mut buffer, StatusLineFocus::Focused);
        let rows = (area.y..area.y + expected.len() as u16)
            .map(|y| {
                (area.x..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>();
        assert_eq!(rows, expected);
        for x in 0..buffer.area.width {
            assert_eq!(buffer[(x, 0)].symbol(), " ");
            assert_eq!(buffer[(x, height + 1)].symbol(), " ");
        }
    }
    assert_eq!(status.explicit_row_count(), Some(3));
    let empty = StatusLine::from(Vec::<HyperlinkLine>::new());
    assert_eq!(empty.explicit_row_count(), Some(0));
    let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 24));
    let before = buffer.clone();
    empty.render(buffer.area, &mut buffer, StatusLineFocus::Focused);
    assert_eq!(buffer, before);
}

#[test]
fn status_line_rows_preserve_link_offsets_and_clear_links_when_unfocused() {
    let url = "https://example.com/pr/42";
    let mut linked = HyperlinkLine::from("ｶﾞ · ");
    linked.push_span("PR #42".into(), Some(url));
    let status = StatusLine::from(vec![HyperlinkLine::from("model"), linked]);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 10, /*height*/ 2,
    );
    for unfocused in [false, true] {
        let mut buffer = Buffer::empty(area);
        if unfocused {
            status.render(area, &mut buffer, StatusLineFocus::Unfocused);
        } else {
            status.render(area, &mut buffer, StatusLineFocus::Focused);
        }
        assert_eq!(buffer[(0, 1)].symbol(), "ｶﾞ");
        for (x, ch) in (5..9).zip("PR #".chars()) {
            assert_eq!(
                buffer[(x, 1)].symbol(),
                if !unfocused {
                    osc8_hyperlink(url, &ch.to_string())
                } else {
                    ch.to_string()
                }
            );
        }
        assert_eq!(buffer[(9, 1)].symbol(), "…");
        if unfocused {
            for y in 0..2 {
                assert!(
                    buffer[(0, y)]
                        .modifier
                        .contains(ratatui::style::Modifier::DIM)
                );
            }
        }
    }
}

#[test]
fn clipped_links_keep_their_cells_after_a_halfwidth_katakana_grapheme() {
    let project_url = "https://example.com/project";
    let pr_url = "https://example.com/pr/42";
    let mut line = HyperlinkLine::default();
    line.push_span("ｶﾞ".into(), Some(project_url));
    line.push_span(" · ".into(), /*destination*/ None);
    line.push_span("PR #42".into(), Some(pr_url));

    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 10, /*height*/ 1,
    );
    let mut buf = Buffer::empty(area);
    StatusLine::from(line).render(area, &mut buf, StatusLineFocus::Focused);
    assert_eq!(buf[(0, 0)].symbol(), osc8_hyperlink(project_url, "ｶﾞ"));
    for (x, ch) in (2..5).zip(" · ".chars()) {
        assert_eq!(buf[(x, 0)].symbol(), ch.to_string());
    }
    for (x, ch) in (5..9).zip("PR #".chars()) {
        assert_eq!(
            buf[(x, 0)].symbol(),
            osc8_hyperlink(pr_url, &ch.to_string())
        );
    }
    assert_eq!(buf[(9, 0)].symbol(), "…");
}

#[test]
fn arabic_prefix_keeps_link_on_the_painted_cells_at_exact_fit_and_overflow() {
    let url = "https://example.com/pr/42";
    let mut line = HyperlinkLine::from("لا · ");
    line.push_span("PR #42".into(), Some(url));
    assert_eq!(line.width(), 11);
    let content = StatusLine::from(line);

    for (width, visible_link) in [(11, "PR #42"), (10, "PR #")] {
        let area = Rect::new(/*x*/ 0, /*y*/ 0, width, /*height*/ 1);
        let mut buf = Buffer::empty(area);
        content.render(area, &mut buf, StatusLineFocus::Focused);
        assert_eq!(buf[(4, 0)].symbol(), " ");
        for (x, ch) in (5..width).zip(visible_link.chars()) {
            assert_eq!(buf[(x, 0)].symbol(), osc8_hyperlink(url, &ch.to_string()));
        }
        if width < 11 {
            assert_eq!(buf[(width - 1, 0)].symbol(), "…");
        }
    }
}

#[test]
fn prepared_tree_paints_only_the_footer_viewport() {
    use crate::terminal_hyperlinks::strip_osc8;
    use crate::ui_tree::UiTree;
    use ratatui::buffer::Cell;

    let url = "https://example.com/build";
    let tree: UiTree = serde_json::json!({
        "type":"link","props":{"href":url},"children":["ok\nlarge build\na\nb\nc\nd"]
    })
    .to_string()
    .parse()
    .unwrap();
    let prepared = StatusLine::from(tree);

    let area = Rect::new(
        /*x*/ 1, /*y*/ 1, /*width*/ 6, /*height*/ 4,
    );
    let mut buffer = Buffer::filled(
        Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 8, /*height*/ 7,
        ),
        Cell::new("."),
    );
    prepared.render(area, &mut buffer, StatusLineFocus::Focused);
    let rows: Vec<String> = buffer
        .content
        .chunks(8)
        .map(|row| row.iter().map(|cell| strip_osc8(cell.symbol())).collect())
        .collect();
    insta::assert_snapshot!(rows.join("\n"));

    assert_eq!(buffer[(1, 4)].symbol(), osc8_hyperlink(url, "a"));
    assert_eq!(buffer[(2, 4)].symbol(), "…");
}
