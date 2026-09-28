//! The TUI's screens, drawn off-screen.

mod workbench_fixtures;

use workbench_fixtures::*;

#[test]
fn a_value_is_changed_after_its_preview() {
    let scratch = Scratch::new("edit");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "\n");
    assert!(matches!(workbench.screen, Screen::Sample { .. }));
    // Its name and its tags first, then as the file holds them: the attribute.
    assert_eq!(workbench.entries()[0].field, "name");
    assert_eq!(workbench.entries()[1].field, "tags");
    assert_eq!(workbench.entries()[2].field, "beer");
    let screen_text = screen(&mut workbench);
    assert!(screen_text.contains("malt"), "{screen_text}");
    assert!(screen_text.contains("A note on C1"), "{screen_text}");
    // On malt: e, a new number, Enter previews, y writes.
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e");
    for _ in 0..8 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "12.5\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines[0].contains("12  →  12.5") || lines[0].contains("→  12.5"),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    assert!(
        workbench.message.starts_with("written"),
        "{}",
        workbench.message
    );
    assert!(
        fs::read_to_string(scratch.0.join("C1.md"))
            .unwrap()
            .contains("v: 12.5")
    );
}

#[test]
fn a_samples_files_open_and_help_names_every_key() {
    let scratch = Scratch::new("files");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "o");
    let effects = keys(&mut workbench, "\n");
    assert!(
        matches!(&effects[0], Effect::Open(path) if path.ends_with("images/C1_sem.png")),
        "{effects:?}"
    );
    let effects = keys(&mut workbench, "n");
    assert!(matches!(&effects[0], Effect::Navigate(_)), "{effects:?}");
    keys(&mut workbench, "\u{1b}?");
    assert!(matches!(workbench.mode, Mode::Help { .. }));
    // By section, from the bindings: every action of the screen once.
    let sections = workbench.help_sections();
    let titles: Vec<&str> = sections.iter().map(|(title, _)| *title).collect();
    assert!(titles.contains(&"finding and showing"), "{titles:?}");
    assert!(
        sections
            .iter()
            .flat_map(|(_, rows)| rows)
            .any(|(keys, said)| keys == "/" && said.contains("filter")),
        "{sections:?}"
    );
    // Drawn in two columns where the width allows.
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| samplekit::tui::view::draw(frame, &mut workbench))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let rows: Vec<String> = (0..40)
        .map(|y| (0..120).map(|x| buffer[(x, y)].symbol()).collect())
        .collect();
    // A second section's title beside the first.
    let moving = rows.iter().position(|row| row.contains("moving")).unwrap();
    assert!(
        titles
            .iter()
            .any(|title| *title != "moving" && rows[moving].contains(&format!(" {title} "))),
        "{}",
        rows.join("\n")
    );
    // Any other key closes it.
    keys(&mut workbench, "x");
    assert!(matches!(workbench.mode, Mode::Normal));
}

#[test]
fn the_hints_fit_the_terminal_and_always_name_help() {
    let scratch = Scratch::new("hints");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    for width in [40u16, 60, 100, 160] {
        let mut terminal = Terminal::new(TestBackend::new(width, 12)).unwrap();
        terminal
            .draw(|frame| samplekit::tui::view::draw(frame, &mut workbench))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let last: String = (0..width)
            .map(|x| buffer[(x, 11)].symbol().to_string())
            .collect::<String>()
            .trim_end()
            .to_string();
        assert!(last.ends_with("? help"), "{width}: {last}");
        assert!(last.chars().count() <= width as usize, "{width}: {last}");
    }
}

#[test]
fn a_modal_is_titled_above_and_names_its_keys_below() {
    let scratch = Scratch::new("modal");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    for (width, typed, title, actions) in [
        (100u16, "?", "keys", "any other key closes"),
        (60, "/", "filter", "Esc cancel"),
        (100, "o", "its files", "Enter open"),
    ] {
        keys(&mut workbench, typed);
        let mut terminal = Terminal::new(TestBackend::new(width, 20)).unwrap();
        terminal
            .draw(|frame| samplekit::tui::view::draw(frame, &mut workbench))
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let rows: Vec<String> = (0..20)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect();
        let top = rows
            .iter()
            .position(|row| row.contains(&format!(" {title} ")));
        let bottom = rows.iter().position(|row| row.contains(actions));
        assert!(
            matches!((top, bottom), (Some(top), Some(bottom)) if top < bottom),
            "{typed}: {top:?} {bottom:?}\n{}",
            rows.join("\n")
        );
        assert!(!rows[top.unwrap()].contains(actions), "{}", rows.join("\n"));
        keys(&mut workbench, "\u{1b}");
    }
}

#[test]
fn fs_window_says_it_applies() {
    // `f` applies what is saved to what is shown; nothing is gone to.
    let scratch = rich("apply");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "f");
    let drawn = screen(&mut workbench);
    assert!(
        drawn.contains("apply a saved query or a profile"),
        "{drawn}"
    );
    assert!(drawn.contains("Enter apply"), "{drawn}");
    assert!(!drawn.contains("go to"), "{drawn}");
    assert!(!drawn.contains("Enter go"), "{drawn}");
}

#[test]
fn f_goes_to_a_query() {
    // `g`, which went there, groups.
    let scratch = rich("goto");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    assert_eq!(workbench.view.len(), 4);
    keys(&mut workbench, "f/schwarz\n");
    assert_eq!(workbench.view.len(), 2, "{}", workbench.message);
    // A filter in force says first how it is cleared.
    let line = samplekit::tui::view::hint_line(workbench.place(), true, 100);
    assert!(line.starts_with(" Esc clear filter"), "{line}");
    keys(&mut workbench, "\u{1b}");
    assert_eq!(workbench.view.len(), 4);
}

#[test]
fn a_table_unfolds_and_folds_back() {
    let scratch = rich("table");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "runs");
    keys(&mut workbench, "l");
    assert!(matches!(workbench.screen, Screen::Table { .. }));
    keys(&mut workbench, "j");
    let drawn = screen(&mut workbench);
    assert!(drawn.contains("4.25"), "{drawn}");
    assert!(drawn.contains("runs · 2 rows"), "{drawn}");
    keys(&mut workbench, "h");
    assert!(matches!(workbench.screen, Screen::Sample { .. }));
}

#[test]
fn a_modal_holds_its_size_while_it_is_typed_in() {
    let scratch = rich("steady");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "/");
    let empty = modal_frame(&mut workbench, "filter");
    // Completions, an error, then a filter that reads: one frame throughout.
    for typed in ["m", "ass >", " 3", " &&"] {
        keys(&mut workbench, typed);
        assert_eq!(
            modal_frame(&mut workbench, "filter"),
            empty,
            "after {typed:?}"
        );
    }
    keys(&mut workbench, "\u{1b}c");
    let columns = modal_frame(&mut workbench, "columns, left to right");
    keys(&mut workbench, "/zzz");
    assert_eq!(
        modal_frame(&mut workbench, "columns, left to right"),
        columns
    );
    // Every modal is one width.
    assert_eq!((columns.2, columns.3), (empty.2, empty.3));
    // The help alone is wider: its keys go in two columns.
    keys(&mut workbench, "\u{1b}\u{1b}?");
    let help = modal_frame(&mut workbench, "keys");
    assert!(help.2 <= empty.2 && help.3 >= empty.3, "{help:?} {empty:?}");
}

#[test]
fn a_click_places_the_cursor_in_the_note() {
    let scratch = rich("click");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    keys(&mut workbench, "N");
    screen(&mut workbench);
    let view = workbench.note_view.expect("drawn while edited");
    workbench.key(Key::Click(view.x + 3, view.y + 1));
    let Mode::Note { text, .. } = &workbench.mode else {
        panic!("still editing");
    };
    assert_eq!(text.cursor(), (1, 3));
}

#[test]
fn what_an_action_says_is_said_inside_its_window() {
    let scratch = rich("said");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "x");
    let drawn = screen(&mut workbench);
    let lines: Vec<&str> = drawn.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.contains("the basket is empty"))
        .expect(&drawn);
    // Inside the frame: its border on either side, its foot below.
    assert!(lines[at].trim_start().starts_with('│'), "{drawn}");
    assert!(lines[at + 1].trim_start().starts_with('└'), "{drawn}");
}

#[test]
fn a_state_too_long_for_the_pane_is_its_mark() {
    // At 80 columns the state was cut off the line: a stale value read as
    // current. (Read without a model, this value's record says edited.) Where the words do not fit, the table's mark stands for them.
    let scratch = Scratch::new("state-mark");
    fs::write(
        scratch.0.join("S.md"),
        "---\nschema_version: 1\nname: S\nproperties:\n  \
         malt: {v: 13.0, fingerprint: aaaaaaaaaaaa}\n  a_rather_long_quantity_name:\n    \
         v: 3.0\n    computed: {malt: bbbbbbbbbbbb}\n    fingerprint: cccccccccccc\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "S");
    let narrow = drawn_at(&mut workbench, 80);
    assert!(
        narrow.contains('✎') && !narrow.contains("edite"),
        "{narrow}"
    );
    let wide = drawn_at(&mut workbench, 200);
    assert!(wide.contains("edited"), "{wide}");
}

#[test]
fn a_confirmation_says_what_y_does() {
    // Every confirmation said `y write`, over a removal and an undo too.
    let scratch = Scratch::new("confirm-action");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "D");
    let removing = screen(&mut workbench);
    assert!(removing.contains("y delete"), "{removing}");
    assert!(removing.contains("to delete:"), "{removing}");
    keys(&mut workbench, "y");
    keys(&mut workbench, "u");
    let undoing = screen(&mut workbench);
    assert!(undoing.contains("y restore"), "{undoing}");
    // From the folder the workbench is on, so that a cut title keeps the name.
    assert!(undoing.contains("restore C1.md"), "{undoing}");
}

#[test]
fn a_long_list_scrolls_to_its_cursor() {
    let scratch = rich("scroll");
    // Labels long enough to have wrapped, which is what hid the cursor.
    let mut rc = String::from("schema_version = 1\n");
    for at in 0..40 {
        rc.push_str(&format!(
            "[query.q{at:02}-with-a-name-long-enough-to-fill-the-whole-window-width]\n\
             filter = 'malt > 0'\n"
        ));
    }
    fs::write(scratch.0.join(".samplekitrc"), rc).unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "f");
    for _ in 0..35 {
        workbench.key(Key::Down);
    }
    let drawn = screen(&mut workbench);
    assert!(drawn.contains("q35"), "{drawn}");
}

#[test]
fn the_rows_move_only_when_the_cursor_leaves_them() {
    // Each frame scrolled from the top again: past the first screen, the
    // selected row stuck to the bottom, and going up dragged the rows with it.
    let scratch = Scratch::new("offset-kept");
    fs::write(scratch.0.join(".samplekitrc"), "schema_version = 1\n").unwrap();
    for at in 0..40 {
        fs::write(
            scratch.0.join(format!("s{at:02}.md")),
            format!("---\nschema_version: 1\nname: s{at:02}\nproperties:\n  malt: {at}\n---\n"),
        )
        .unwrap();
    }
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let mut before = String::new();
    for _ in 0..30 {
        workbench.key(Key::Down);
        before = screen(&mut workbench);
    }
    for _ in 0..5 {
        workbench.key(Key::Up);
        screen(&mut workbench);
    }
    // Five rows up, still inside what was shown: the rows have not moved.
    let names = |drawn: &str| -> Vec<String> {
        drawn
            .split_whitespace()
            .filter(|word| word.len() == 3 && word.starts_with('s'))
            .map(str::to_string)
            .collect()
    };
    let after = screen(&mut workbench);
    assert_eq!(names(&before), names(&after), "{before}\n{after}");
}

#[test]
fn the_whole_choice_shows_at_the_foot_of_the_picker() {
    let scratch = rich("choice");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "s/malt r\u{1b}/beer");
    let drawn = screen(&mut workbench);
    // Narrowed to beer, and malt still said, with its way.
    assert!(drawn.contains("sorted by  malt ↓"), "{drawn}");
}

#[test]
fn the_tags_are_a_value_edited_whole() {
    let scratch = rich("tags");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    assert_eq!(workbench.entries()[1].field, "tags");
    // Not in the note any more.
    let drawn = screen(&mut workbench);
    assert!(!drawn.contains("tags:"), "{drawn}");
    move_to(&mut workbench, "tags");
    keys(&mut workbench, "e");
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("the tags' prompt");
    };
    assert!(text.is_empty(), "{text}");
    keys(&mut workbench, "reference, checked\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert_eq!(lines, &["+ reference", "+ checked"]);
    keys(&mut workbench, "y");
    keys(&mut workbench, "e");
    for _ in 0.."reference, checked".len() {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "checked\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert_eq!(lines, &["- reference"]);
    keys(&mut workbench, "y");
    let written = fs::read_to_string(scratch.0.join("more/D1.md")).unwrap();
    assert!(
        written.contains("checked") && !written.contains("reference"),
        "{written}"
    );
}

#[test]
fn a_narrow_table_cuts_its_headers_names_first_then_units() {
    use samplekit::tui::view::fitted;
    let headers = [
        "Hopping [g/100L]".to_string(),
        "Expected darkness [EBC]".to_string(),
    ];
    let values = [4, 5];
    // Room enough: nothing is cut.
    assert_eq!(fitted(&headers, &values, 60), headers);
    // Names give way from their end, the widest first, the units kept.
    let cut = fitted(&headers, &values, 32);
    assert!(cut.iter().all(|header| header.contains('[')), "{cut:?}");
    assert!(cut[1].starts_with("Exp") && cut[1].contains('…'), "{cut:?}");
    // Every name at its few characters before any unit goes.
    let cut = fitted(&headers, &values, 24);
    assert_eq!(cut, ["Hop… [g/100L]", "Exp… [EBC]"]);
    // Then the units, and never narrower than the values.
    let cut = fitted(&headers, &values, 4);
    assert_eq!(cut, ["Hop…", "Exp…"]);
}

#[test]
fn a_table_cells_header_cuts_its_table_first() {
    use samplekit::tui::view::fitted;
    let headers = ["tasting.foaminess[50] [%]".to_string()];
    let cut = fitted(&headers, &[6], 22);
    assert_eq!(cut, ["tas….foaminess[50] [%]"]);
    // Then the column's own name, the unit last.
    let cut = fitted(&headers, &[6], 14);
    assert_eq!(cut, ["tas….foam… [%]"]);
}

#[test]
fn values_line_up_on_their_plus_minus() {
    use samplekit::tui::view::aligned;
    let cells = [
        "4.10 ± 0.01 mg".to_string(),
        "14.21 mg".to_string(),
        "123.4 ± 1.2 mg".to_string(),
    ];
    let lined = aligned(&cells);
    assert_eq!(lined, [" 4.10 ± 0.01 mg", "14.21 mg", "123.4 ± 1.2 mg"]);
    // A column with no ± is left as it is.
    let text = ["WLP001".to_string(), "US05".to_string()];
    assert_eq!(aligned(&text), text);
}

#[test]
fn s_summarises_the_columns_shown() {
    let scratch = rich("summary");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "S");
    assert!(workbench.summarised);
    let drawn = screen(&mut workbench);
    assert!(drawn.contains("summary of 4 samples"), "{drawn}");
    // malt over 12, 10, 11 and D1's 9: a mean of 10.5, four of four.
    let malt = drawn
        .lines()
        .find(|line| line.contains("malt"))
        .unwrap_or_default();
    assert!(malt.contains("4/4") && malt.contains("10.5"), "{drawn}");
    keys(&mut workbench, "S");
    assert!(!workbench.summarised);
}

#[test]
fn columns_past_the_edge_wait_for_less_and_greater_than() {
    let scratch = rich("hscroll");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "c");
    // Every field shown: more columns than a narrow screen holds.
    let Mode::Picker { items, .. } = &mut workbench.mode else {
        panic!("columns");
    };
    for item in items.iter_mut() {
        item.1 = true;
    }
    keys(&mut workbench, "\n");
    let narrow = drawn_at(&mut workbench, 40);
    assert!(narrow.contains("> ▸"), "{narrow}");
    assert!(!narrow.contains("◂"), "{narrow}");
    keys(&mut workbench, ">");
    let moved = drawn_at(&mut workbench, 40);
    assert!(moved.contains("◂ <"), "{moved}");
    // The names stay in place.
    assert!(moved.contains("C1"), "{moved}");
    keys(&mut workbench, "<");
    assert_eq!(workbench.hscroll, 0);
    // Where every column fits, > moves nothing; and it stops once the rest
    // fit, rather than scrolling down to one column.
    keys(&mut workbench, ">>>>>>>>");
    drawn_at(&mut workbench, 250);
    assert_eq!(workbench.hscroll, 0);
    keys(&mut workbench, ">>>>>>>>>>>>");
    let far = drawn_at(&mut workbench, 40);
    assert!(!far.contains("> ▸"), "{far}");
    let furthest = workbench.hscroll;
    keys(&mut workbench, ">");
    drawn_at(&mut workbench, 40);
    assert_eq!(workbench.hscroll, furthest);
    keys(&mut workbench, "<");
    assert_eq!(workbench.hscroll, furthest - 1);
}

#[test]
fn a_table_of_its_index_alone_unfolds_without_a_crash() {
    let scratch = rich("indexonly");
    fs::write(
        scratch.0.join("E1.md"),
        "---\nschema_version: 1\nname: E1\ntables:\n  runs:\n    index: run\n    columns:\n      \
         run: {}\n    rows:\n      - run: 1\n      - run: 2\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "E1");
    move_to(&mut workbench, "runs");
    workbench.key(Key::Right);
    let drawn = screen(&mut workbench);
    assert!(drawn.contains("runs · 2 rows"), "{drawn}");
}

#[test]
fn a_click_on_wide_characters_lands_on_the_one_clicked() {
    let scratch = rich("wide");
    fs::write(
        scratch.0.join("more/D1.md"),
        "---\nschema_version: 1\nname: D1\n---\n日本語テキスト\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    keys(&mut workbench, "N");
    screen(&mut workbench);
    let view = workbench.note_view.expect("drawn while edited");
    // テ begins at the seventh column of the screen, the fourth character.
    workbench.key(Key::Click(view.x + 6, view.y));
    let Mode::Note { text, .. } = &workbench.mode else {
        panic!("still editing");
    };
    assert_eq!(text.cursor().1, 3);
}

#[test]
fn a_short_confirmation_does_not_scroll_and_d_keeps_the_cursor_on_a_row() {
    let scratch = rich("bounds");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e");
    workbench.key(Key::Backspace);
    keys(&mut workbench, "9\n");
    assert!(matches!(workbench.mode, Mode::Confirm { .. }));
    keys(&mut workbench, "jjjjj");
    screen(&mut workbench);
    assert_eq!(workbench.confirm_scroll, 0);
    keys(&mut workbench, "n\u{1b}");
    // The last row of a section removed: the cursor on the one before.
    keys(&mut workbench, "P");
    workbench.key(Key::Right);
    let rows = workbench.workspace_rows().len();
    workbench.workspace.as_mut().unwrap().row = rows - 1;
    keys(&mut workbench, "d");
    let row = workbench.workspace.as_ref().unwrap().row;
    assert!(row < workbench.workspace_rows().len().max(1), "{row}");
}

#[test]
fn the_note_is_shown_as_markdown_and_edited_as_its_text() {
    let scratch = Scratch::new("markdown");
    let path = scratch.0.join("C1.md");
    let text = fs::read_to_string(&path).unwrap().replace(
        "A note on C1.",
        "# Batch 7\n\nMashed **twice**, then:\n\n- boiled\n- hopped\n",
    );
    fs::write(&path, text).unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    let shown = screen(&mut workbench);
    // Rendered: the emphasis is a style, not two stars.
    assert!(shown.contains("Batch 7"), "{shown}");
    assert!(shown.contains("Mashed twice"), "{shown}");
    assert!(!shown.contains("**"), "{shown}");
    assert!(shown.contains("boiled"), "{shown}");
    // Edited as typed: every character there.
    keys(&mut workbench, "N");
    let editing = screen(&mut workbench);
    assert!(editing.contains("Mashed **twice**"), "{editing}");
    assert!(editing.contains("# Batch 7"), "{editing}");
}

#[test]
fn a_long_note_scrolls_by_keys_and_by_the_wheel_over_it() {
    let scratch = Scratch::new("notescroll");
    let path = scratch.0.join("C1.md");
    let long: String = (1..=40).map(|at| format!("Line {at}.\n\n")).collect();
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("A note on C1.", &long);
    fs::write(&path, text).unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    let first = screen(&mut workbench);
    assert!(
        first.contains("Line 1.") && !first.contains("Line 20."),
        "{first}"
    );
    assert!(first.contains("J K scroll"), "{first}");
    keys(&mut workbench, "JJJ");
    let moved = screen(&mut workbench);
    assert!(!moved.contains("Line 1."), "{moved}");
    // The wheel over the note scrolls it, down past its end no further.
    let (x, y, _, _) = workbench.note_area.unwrap();
    for _ in 0..60 {
        workbench.key(Key::Wheel {
            down: true,
            x: x + 2,
            y: y + 2,
        });
    }
    let end = screen(&mut workbench);
    assert!(end.contains("Line 40."), "{end}");
    let at_end = workbench.note_scroll.1;
    workbench.key(Key::Wheel {
        down: false,
        x: x + 2,
        y: y + 2,
    });
    screen(&mut workbench);
    assert_eq!(workbench.note_scroll.1, at_end - 3);
    // Elsewhere, it moves the cursor as an arrow does.
    workbench.key(Key::Wheel {
        down: true,
        x: 3,
        y: 3,
    });
    let Screen::Sample { cursor, .. } = workbench.screen else {
        panic!("the sample");
    };
    assert_eq!(cursor, 1);
}

#[test]
fn editing_the_note_keeps_the_screen_as_it_was() {
    // The note's frame does not move as editing begins.
    let scratch = rich("notesteady");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    let before = screen(&mut workbench);
    let at = |drawn: &str| {
        drawn
            .lines()
            .nth(1)
            .and_then(|line| line.find("┌ note"))
            .unwrap()
    };
    let shown = at(&before);
    keys(&mut workbench, "N");
    let editing = screen(&mut workbench);
    let edited = editing
        .lines()
        .nth(1)
        .and_then(|line| line.find("┌ note"))
        .unwrap();
    assert_eq!(shown, edited, "{before}\n{editing}");
}

#[test]
fn a_computation_shows_its_progress_then_its_outcome_in_the_title_bar() {
    use samplekit::tui::model::RunStatus;
    let scratch = Scratch::new("runbar");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.run = Some(RunStatus {
        done: 2,
        total: 4,
        now: "C3 · malt".to_string(),
        outcome: None,
    });
    // On the title bar, the first line, whatever the screen: steps are values.
    let drawn = screen(&mut workbench);
    let title = drawn.lines().next().unwrap();
    assert!(
        title.contains("⟳ [██████░░░░░░] 2/4 · C3 · malt"),
        "{drawn}"
    );
    open_named(&mut workbench, "C1");
    workbench.run.as_mut().unwrap().outcome = Some("3 computed".to_string());
    let drawn = screen(&mut workbench);
    assert!(
        drawn.lines().next().unwrap().contains("✓ 3 computed"),
        "{drawn}"
    );
}

#[test]
fn a_short_terminal_still_shows_its_samples() {
    // At six rows the collection drew its header and no sample.
    let scratch = Scratch::new("shortterm");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(60, 6)).unwrap();
    terminal
        .draw(|frame| samplekit::tui::view::draw(frame, &mut workbench))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let drawn: String = (0..6)
        .map(|y| (0..60).map(|x| buffer[(x, y)].symbol()).collect::<String>() + "\n")
        .collect();
    assert!(drawn.contains("C1"), "{drawn}");
}

#[test]
fn the_title_bar_is_on_the_terminals_background_its_name_in_the_accent() {
    // A reversed bar hid the computation drawn on it.
    use ratatui::style::{Color, Modifier};
    use samplekit::tui::model::RunStatus;
    let scratch = Scratch::new("titlebar");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.run = Some(RunStatus {
        done: 1,
        total: 2,
        now: String::new(),
        outcome: None,
    });
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal
        .draw(|frame| samplekit::tui::view::draw(frame, &mut workbench))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let name = &buffer[(2, 0)];
    assert_eq!(name.symbol(), "S");
    assert_eq!(name.fg, Color::Cyan);
    assert_eq!(name.bg, Color::Reset);
    assert!(!name.modifier.contains(Modifier::REVERSED));
    let bar = (0..100)
        .map(|x| &buffer[(x, 0)])
        .find(|cell| cell.symbol() == "⟳")
        .expect("the computation on the title bar");
    assert_eq!(bar.fg, Color::Cyan);
    assert_eq!(bar.bg, Color::Reset);
}

#[test]
fn the_setup_hint_says_its_keys_as_its_frame_does() {
    // The foot said `Space select · Enter open` under a frame saying tick and
    // preview; the setup asks questions now, Enter answering them.
    let hint = samplekit::tui::view::hint_line(samplekit::tui::model::Place::Setup, false, 200);
    assert!(hint.contains("Enter answer"), "{hint}");
    assert!(!hint.contains("tick"), "{hint}");
}

#[test]
fn the_history_shows_what_a_snapshot_changed_beside_it() {
    let scratch = Scratch::new("history-view");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "\n");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e");
    for _ in 0..8 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "12.5\ny");
    workbench.key(Key::Esc);
    keys(&mut workbench, "H");
    let drawn = screen(&mut workbench);
    assert!(drawn.contains("its history"), "{drawn}");
    assert!(drawn.contains("what it changed"), "{drawn}");
    assert!(drawn.contains("C1.md"), "{drawn}");
    assert!(drawn.contains("→"), "{drawn}");
    // `J K` scroll what it changed: said under that pane, on the right, not
    // under the list `j k` move in.
    let hinted = drawn
        .lines()
        .find_map(|line| line.find("J K").map(|at| line[..at].chars().count()))
        .expect(&drawn);
    assert!(hinted > 50, "{drawn}");
    // Held down past the end, it stops there: one `K` then moves at once.
    for _ in 0..50 {
        keys(&mut workbench, "J");
        screen(&mut workbench);
    }
    let Screen::History { scroll, .. } = workbench.screen else {
        panic!("the history");
    };
    assert!(scroll < 20, "{scroll}");
}

#[test]
fn the_help_lies_within_the_main_frame() {
    // Laid on the whole screen two cells in, its borders ran one cell inside
    // the frame's, each edge drawn twice (`┌┌`, `│┘`).
    let scratch = Scratch::new("help-frame");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "?");
    let mut terminal = Terminal::new(TestBackend::new(110, 34)).unwrap();
    terminal
        .draw(|frame| samplekit::tui::view::draw(frame, &mut workbench))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let drawn: Vec<String> = (0..34)
        .map(|y| {
            (0..110)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect();
    assert!(drawn[1].starts_with(" ┌ keys"), "{}", drawn.join("\n"));
    assert!(
        drawn
            .iter()
            .all(|line| !line.contains("┌┌") && !line.contains("││")),
        "{}",
        drawn.join("\n")
    );
}

#[test]
fn the_quantitys_window_names_its_unit() {
    let scratch = Scratch::new("quantity-unit");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e");
    let drawn = screen(&mut workbench);
    assert!(drawn.contains("value (g)"), "{drawn}");
    assert!(drawn.contains("the number alone"), "{drawn}");
}

#[test]
fn q_says_the_start_page_when_it_leads_there() {
    use samplekit::tui::model::Place;
    use samplekit::tui::view::hints_of;
    assert!(hints_of(Place::Collection, false, false, 400).contains("q quit"));
    let back = hints_of(Place::Collection, false, true, 400);
    assert!(back.contains("q start page"), "{back}");
    assert!(!back.contains("q quit"), "{back}");
}

#[test]
fn the_control_screen_titles_its_marks() {
    let scratch = Scratch::new("control-marks");
    fs::write(
        scratch.0.join("C3.md"),
        "---\nschema_version: 1\nname: C3\nbeer: schwarz\nproperties:\n  \
         malt: {v: 11.0, u: 0.1, unit: g, fingerprint: 000000000000}\n  \
         twice: {v: 22.0, computed: {malt: 111111111111}, fingerprint: 222222222222}\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "v");
    let drawn = screen(&mut workbench);
    // Each mark its own, as the help's legend says it, beside the count of
    // those there are, a noun in the singular for one.
    for said in ["✎ 1 edited", "¶ 1 note ", "✎ edited   C3", "¶ note     C3"] {
        assert!(drawn.contains(said), "{said}: {drawn}");
    }
    for unsaid in ["✗ 0", "· 1", "notes"] {
        assert!(!drawn.contains(unsaid), "{unsaid}: {drawn}");
    }
    let marks: Vec<&str> = samplekit::tui::model::MARKS
        .iter()
        .map(|(mark, _)| *mark)
        .collect();
    for mark in ["✎", "¶", "⊘", "✗", "∅"] {
        assert!(marks.contains(&mark), "{marks:?}");
    }
}

#[test]
fn at_80_columns_the_collection_hints_new_and_undo() {
    let line = samplekit::tui::view::hint_line(samplekit::tui::model::Place::Collection, false, 78);
    assert!(line.contains("N new"), "{line}");
    assert!(line.contains("u undo"), "{line}");
    assert!(line.ends_with("? help"), "{line}");
}

/// A screen drawn at a size, its lines.
fn drawn_sized(workbench: &mut Workbench, width: u16, height: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| samplekit::tui::view::draw(frame, workbench))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect()
}

#[test]
fn a_stale_mark_stays_in_sight_at_80_columns() {
    // The values' pane is capped, and a long value pushed the `⚠` one column
    // past it: a stale value read as current.
    let scratch = Scratch::new("mark-in-sight");
    fs::write(
        scratch.0.join("S.md"),
        "---\nschema_version: 1\nname: S\nproperties:\n  \
         a_long_measured_quantity: {v: 1.0, u: 0.1, unit: g, fingerprint: 000000000000}\n  \
         another_rather_long_derived_quantity:\n    v: 123456.789\n    u: 1234.5\n    \
         unit: kg/hL\n    computed: {a_long_measured_quantity: 111111111111}\n    \
         fingerprint: 222222222222\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "S");
    let lines = drawn_sized(&mut workbench, 80, 24);
    let line = lines
        .iter()
        .find(|line| line.contains("another_rather"))
        .expect("the value's line");
    let pane = line.split('│').nth(1).unwrap_or_default();
    assert!(
        pane.contains('⚠') || pane.contains('✎'),
        "{}",
        lines.join("\n")
    );
}

#[test]
fn the_note_editor_names_its_own_keys() {
    let scratch = Scratch::new("note-keys");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "N");
    let lines = drawn_sized(&mut workbench, 100, 20);
    let foot = lines.last().unwrap();
    assert!(foot.contains("Esc done"), "{foot}");
    assert!(!foot.contains("? help"), "{foot}");
    // The value's own keys are not said while the note takes them.
    assert!(
        !lines.iter().any(|line| line.contains("e edit")),
        "{}",
        lines.join("\n")
    );
}

#[test]
fn the_history_wraps_and_its_hints_read_from_their_start() {
    let scratch = Scratch::new("history-wrap");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e");
    for _ in 0..8 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "12.5\ny");
    workbench.key(Key::Esc);
    keys(&mut workbench, "H");
    let narrow = drawn_sized(&mut workbench, 40, 12).join("\n");
    assert!(!narrow.contains("└ll"), "{narrow}");
    assert!(!narrow.contains("└ what"), "{narrow}");
    let wide = drawn_sized(&mut workbench, 80, 24).join("\n");
    // The list's message under its date, cut with an ellipsis.
    assert!(wide.contains("tui · change"), "{wide}");
    // What it changed, wrapped rather than cut at the pane's edge: the
    // whole message is there.
    let right: String = wide
        .lines()
        .filter_map(|line| line.split('│').nth(3))
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" ");
    assert!(right.contains("written C1.md"), "{wide}");
}

#[test]
fn windows_keep_enter_and_esc_and_lie_within_the_main_frame() {
    let scratch = Scratch::new("modal-keys");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    for (width, typed, wanted) in [
        (40u16, "/", ["Enter", "Esc cancel"]),
        (80, "s", ["Enter sort", "Esc cancel"]),
        (60, "p", ["Enter draw", "Esc cancel"]),
    ] {
        keys(&mut workbench, typed);
        let lines = drawn_sized(&mut workbench, width, 12);
        let drawn = lines.join("\n");
        for wanted in wanted {
            assert!(drawn.contains(wanted), "{width} {typed}: {wanted}\n{drawn}");
        }
        // The status line and the hints stay in sight under the window, the
        // hints on one line or two.
        assert!(
            lines[9].contains("of 3 samples") || lines[10].contains("of 3 samples"),
            "{drawn}"
        );
        assert!(lines[11].trim_end().ends_with("? help"), "{drawn}");
        keys(&mut workbench, "\u{1b}\u{1b}");
    }
}

#[test]
fn a_narrow_sample_shows_its_values() {
    // At 40 columns the values' pane was 18 wide, and drew the names alone.
    let scratch = Scratch::new("narrow-sample");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    // One line taller than it was drawn, for the name's line.
    let drawn = drawn_sized(&mut workbench, 40, 13).join("\n");
    assert!(drawn.contains("12.0"), "{drawn}");
    assert!(drawn.contains("schwarz"), "{drawn}");
}

#[test]
fn a_list_fills_its_view_when_the_terminal_grows() {
    // Drawn small with the cursor low, then large: samples hidden above,
    // rows empty below.
    let scratch = Scratch::new("grown");
    for at in 0..20 {
        fs::write(
            scratch.0.join(format!("S{at:02}.md")),
            format!(
                "---\nschema_version: 1\nname: S{at:02}\nproperties:\n  malt: {{v: 1.0, unit: g}}\n---\n"
            ),
        )
        .unwrap();
    }
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.key(Key::End);
    let small = drawn_sized(&mut workbench, 40, 12).join("\n");
    assert!(!small.contains("C1 "), "{small}");
    let large = drawn_sized(&mut workbench, 80, 40).join("\n");
    assert!(large.contains("C1 "), "{large}");
}

#[test]
fn the_title_cuts_its_path_before_the_computation() {
    use samplekit::tui::model::RunStatus;
    let scratch = Scratch::new("title-cut-with-a-rather-long-folder-name-to-cut");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.run = Some(RunStatus {
        done: 1,
        total: 4,
        now: "C2 · malt".to_string(),
        outcome: None,
    });
    let lines = drawn_sized(&mut workbench, 80, 12);
    let top = &lines[0];
    assert!(top.contains('…'), "{top}");
    assert!(top.contains("to-cut"), "{top}");
    assert!(top.contains(" · ⟳") || top.contains(" ⟳"), "{top}");
    assert!(!top.contains("cut⟳"), "{top}");
}

#[test]
fn the_names_are_headed_as_the_summary_heads_them() {
    let scratch = Scratch::new("name-header");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"name\", label = \"Brew\"}, \
         {field = \"malt\"}]\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let drawn = drawn_sized(&mut workbench, 80, 12).join("\n");
    assert!(drawn.contains("Brew"), "{drawn}");
    assert!(!drawn.contains("│ name"), "{drawn}");
}

#[test]
fn the_control_screen_says_what_it_does_not_list() {
    // Without the model read, what it would compute is not listed: said, as
    // `status` says it.
    // No environment: the reason is known before any worker starts.
    let scratch = Scratch::new("control-unread");
    fs::write(
        scratch.0.join("model.py"),
        "import samplekit as sk\nclass M(sk.Sample):\n    pass\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \"missing-interpreter\"\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "v");
    let drawn = drawn_sized(&mut workbench, 100, 16).join("\n");
    assert!(
        drawn.contains("values the model would compute are not listed"),
        "{drawn}"
    );
}

#[test]
fn the_setups_wrapped_lines_keep_their_indent() {
    let root = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-setup-indent-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let mut workbench = Workbench::open(&root).unwrap();
    assert!(matches!(workbench.screen, Screen::Setup { .. }));
    let lines = drawn_sized(&mut workbench, 40, 30);
    for line in &lines[2..lines.len() - 3] {
        // Inside the frame: its border, a space, then the text's own indent.
        let inside = line.trim_start().strip_prefix('│').unwrap_or_default();
        let text = inside.strip_prefix(' ').unwrap_or(inside);
        assert!(
            text.trim().is_empty() || text.starts_with("  ") || text.starts_with('─'),
            "{}",
            lines.join("\n")
        );
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn several_projects_are_headed_each_by_its_title() {
    // Samples of two projects were one list, nothing saying where one ended;
    // each is now headed by a line bearing its title.
    let scratch = rich("projects-headed");
    fs::write(scratch.0.join("more/.samplekitrc"), "schema_version = 1\n").unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let own = scratch.0.file_name().unwrap().to_string_lossy().to_string();
    let drawn = screen(&mut workbench);
    let lines: Vec<&str> = drawn.lines().collect();
    let more = lines
        .iter()
        .position(|line| line.contains("── more ──"))
        .unwrap_or_else(|| panic!("{drawn}"));
    let mine = lines
        .iter()
        .position(|line| line.contains(&format!("── {own} ──")))
        .unwrap_or_else(|| panic!("{drawn}"));
    // Each heading above its own samples.
    assert!(lines[more + 1].contains("D1"), "{drawn}");
    assert!(mine > more + 1 && lines[mine + 1].contains("C1"), "{drawn}");
    // The row marked is the cursor's sample, the headings counted past.
    workbench.cursor = workbench
        .view
        .iter()
        .position(|entry| entry.sample.borrow().name() == Some("C2"))
        .unwrap();
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal
        .draw(|frame| samplekit::tui::view::draw(frame, &mut workbench))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let marked: Vec<String> = (0..20)
        .filter(|y| {
            buffer[(10, *y)]
                .modifier
                .contains(ratatui::style::Modifier::REVERSED)
        })
        .map(|y| (0..100).map(|x| buffer[(x, y)].symbol()).collect())
        .collect();
    assert_eq!(marked.len(), 1, "{marked:?}");
    assert!(marked[0].contains("C2"), "{marked:?}");
    // One project alone draws no heading.
    let alone = Scratch::new("projects-one");
    let mut workbench = Workbench::open(&alone.0).unwrap();
    let own = alone.0.file_name().unwrap().to_string_lossy().to_string();
    let drawn = screen(&mut workbench);
    assert!(!drawn.contains(&format!("── {own}")), "{drawn}");
}

#[test]
fn each_group_is_headed_by_its_values() {
    // A line across the table where each group begins, as a project's.
    let scratch = Scratch::new("group-headings");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "g/beer \n");
    let drawn = screen(&mut workbench);
    let bock = drawn.find("── beer = bock").expect(&drawn);
    let schwarz = drawn.find("── beer = schwarz").expect(&drawn);
    assert!(bock < drawn.find("C2").unwrap(), "{drawn}");
    assert!(drawn.find("C2").unwrap() < schwarz, "{drawn}");
    assert!(schwarz < drawn.find("C1").unwrap(), "{drawn}");
    // `S` summarises each group, its values first.
    keys(&mut workbench, "S");
    let drawn = screen(&mut workbench);
    let bock = drawn.find("bock").expect(&drawn);
    let schwarz = drawn.find("schwarz").expect(&drawn);
    assert!(bock < schwarz, "{drawn}");
    assert!(drawn.contains("beer"), "{drawn}");
    assert!(drawn.contains("1/1") && drawn.contains("2/2"), "{drawn}");
}

#[test]
fn the_keys_take_two_lines_where_one_cannot_hold_them() {
    // Cut to one line, the foot lost its end.
    use samplekit::tui::model::Place;
    use samplekit::tui::view::hints_of;
    let scratch = rich("two-lines");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let every: Vec<String> = hints_of(Place::Collection, false, false, usize::MAX)
        .trim_start()
        .split(" · ")
        .map(str::to_string)
        .collect();
    for (width, height) in [(80u16, 24u16), (100, 30), (40, 12)] {
        let lines = drawn_sized(&mut workbench, width, height);
        let drawn = lines.join("\n");
        let at = height as usize;
        // The frame, the status line, then the keys on the last two rows.
        assert!(lines[at - 4].contains('└'), "{width}\n{drawn}");
        assert!(lines[at - 3].contains("of 4 samples"), "{width}\n{drawn}");
        assert!(
            lines[at - 1].trim_end().ends_with("? help"),
            "{width}\n{drawn}"
        );
        let said: Vec<String> = lines[at - 2..]
            .iter()
            .flat_map(|line| {
                line.trim()
                    .split(" · ")
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .collect();
        // Whole hints, in their order, those given up from the end.
        for hint in &said {
            assert!(every.contains(hint), "{width}: {hint:?} cut\n{drawn}");
        }
        let kept = said.len() - 1;
        assert_eq!(said[..kept], every[..kept], "{width}\n{drawn}");
        assert!(kept >= 6, "{width}\n{drawn}");
    }
    // Where one line holds them, one line: the sample's at 100 columns.
    open_named(&mut workbench, "C1");
    let lines = drawn_sized(&mut workbench, 100, 30);
    let drawn = lines.join("\n");
    assert!(lines[27].contains('└'), "{drawn}");
    assert!(lines[28].contains("of 4 samples"), "{drawn}");
    assert!(lines[29].contains("Esc back"), "{drawn}");
    assert!(lines[29].trim_end().ends_with("? help"), "{drawn}");
}

#[test]
fn a_windows_keys_take_a_second_row_where_its_border_cannot_hold_them() {
    // At 80 columns the sort's window lost `/ narrow`.
    let scratch = rich("window-two-lines");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "s");
    let lines = drawn_sized(&mut workbench, 80, 24);
    let drawn = lines.join("\n");
    // The border filled first, `Enter` and `Esc` together on it.
    let border = lines
        .iter()
        .position(|line| line.contains("⇧↑↓ move · / narrow · Enter sort · Esc cancel ┘"))
        .expect(&drawn);
    assert!(
        lines[border - 1].contains("Space tick · r reverse │"),
        "{drawn}"
    );
    // Its right edge lined up with the keys on the border.
    let end = |line: &str| {
        let window = line.trim_end().trim_end_matches(['│', ' ']);
        window.trim_end_matches(['│', '┘', ' ']).chars().count()
    };
    assert_eq!(end(&lines[border - 1]), end(&lines[border]), "{drawn}");
    // Where the border holds them, on the border alone.
    let lines = drawn_sized(&mut workbench, 120, 30);
    let drawn = lines.join("\n");
    let border = lines
        .iter()
        .position(|line| line.contains("/ narrow · Enter sort · Esc cancel ┘"))
        .expect(&drawn);
    assert!(lines[border].contains("Space tick"), "{drawn}");
    assert!(!lines[border - 1].contains("Space tick"), "{drawn}");
}

/// The width of the frame titled `title` on a drawn screen, its borders
/// included, and the column its left border is in.
fn framed_width(lines: &[String], title: &str) -> (usize, usize) {
    let row = lines
        .iter()
        .find(|line| line.contains(title))
        .unwrap_or_else(|| panic!("{title} is drawn\n{}", lines.join("\n")));
    let characters: Vec<char> = row.chars().collect();
    let wanted: Vec<char> = title.chars().collect();
    let at = characters
        .windows(wanted.len())
        .position(|window| window == wanted.as_slice())
        .unwrap();
    let left = (0..at).rev().find(|x| characters[*x] == '┌').unwrap();
    let right = (at..characters.len())
        .find(|x| characters[*x] == '┐')
        .unwrap();
    (right - left + 1, left)
}

#[test]
fn prose_is_shown_at_a_reading_width() {
    // At 300 columns the note was given two hundred of them.
    let scratch = rich("reading-width");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    let lines = drawn_sized(&mut workbench, 300, 50);
    let (note, at) = framed_width(&lines, " note ");
    assert!(note <= 84, "{note}\n{}", lines.join("\n"));
    // The values take the rest.
    assert!(at > note, "{at}\n{}", lines.join("\n"));
    // Where a value came from.
    workbench.key(Key::Esc);
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "\n");
    let Mode::Panel { title, .. } = &workbench.mode else {
        panic!("where the value came from");
    };
    let title = format!(" {title} ");
    let lines = drawn_sized(&mut workbench, 300, 50);
    let (explained, _) = framed_width(&lines, &title);
    assert!(explained <= 84, "{explained}\n{}", lines.join("\n"));
    // A list keeps its width.
    workbench.key(Key::Esc);
    workbench.key(Key::Esc);
    keys(&mut workbench, "s");
    let lines = drawn_sized(&mut workbench, 300, 50);
    let (sort, _) = framed_width(&lines, " sort by, first to last ");
    assert!(sort > 80, "{sort}\n{}", lines.join("\n"));
    // What a snapshot changed.
    workbench.key(Key::Esc);
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e");
    for _ in 0..8 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "12.5\ny");
    workbench.key(Key::Esc);
    keys(&mut workbench, "H");
    let lines = drawn_sized(&mut workbench, 300, 50);
    let (changed, at) = framed_width(&lines, " what it changed ");
    assert!(changed <= 84, "{changed}\n{}", lines.join("\n"));
    assert!(at > changed, "{at}\n{}", lines.join("\n"));
    // A setup question.
    let root = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-setup-width-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let mut setup = Workbench::open(&root).unwrap();
    assert!(matches!(setup.screen, Screen::Setup { .. }));
    let lines = drawn_sized(&mut setup, 300, 50);
    for line in &lines[2..lines.len() - 3] {
        let text = line.trim_end().trim_end_matches('│').trim_end();
        assert!(text.chars().count() <= 84, "{}", lines.join("\n"));
    }
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_hidden_note_leaves_the_values_the_whole_width() {
    let scratch = rich("note-hides");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    keys(&mut workbench, "n");
    let lines = drawn_sized(&mut workbench, 300, 50);
    let drawn = lines.join("\n");
    assert!(!drawn.contains("┌ note"), "{drawn}");
    // The main frame's, a column in from each edge.
    let (values, _) = framed_width(&lines, " values ");
    assert_eq!(values, 298, "{drawn}");
    assert!(drawn.contains("n show note"), "{drawn}");
    // Edited, it is shown.
    keys(&mut workbench, "N");
    let drawn = drawn_sized(&mut workbench, 300, 50).join("\n");
    assert!(drawn.contains(" note · editing "), "{drawn}");
    workbench.key(Key::Esc);
    if matches!(workbench.mode, Mode::Confirm { .. }) {
        workbench.key(Key::Esc);
    }
    assert!(matches!(workbench.mode, Mode::Normal));
    // And `n` brings it back.
    keys(&mut workbench, "n");
    let lines = drawn_sized(&mut workbench, 300, 50);
    let (note, _) = framed_width(&lines, " note ");
    assert!(note <= 84, "{}", lines.join("\n"));
}

/// A frame drawn off-screen, its cells kept with their styles.
fn buffer_of(workbench: &mut Workbench, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| samplekit::tui::view::draw(frame, workbench))
        .unwrap();
    terminal.backend().buffer().clone()
}

/// Where `needle` is first drawn, row by row: the cell of its first
/// character.
fn found(buffer: &ratatui::buffer::Buffer, needle: &str) -> (u16, u16) {
    let wanted: Vec<String> = needle.chars().map(|c| c.to_string()).collect();
    for y in 0..buffer.area.height {
        let row: Vec<String> = (0..buffer.area.width)
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect();
        if let Some(x) = row
            .windows(wanted.len())
            .position(|window| window == wanted.as_slice())
        {
            return (x as u16, y);
        }
    }
    let drawn: Vec<String> = (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect();
    panic!("{needle} is not drawn:\n{}", drawn.join("\n"));
}

/// Whether the cell `offset` characters into `needle` is drawn as a key: the
/// key colour, bold.
fn drawn_as_key(buffer: &ratatui::buffer::Buffer, needle: &str, offset: u16) -> bool {
    let (x, y) = found(buffer, needle);
    let cell = &buffer[(x + offset, y)];
    let key = samplekit::tui::theme::Theme::default().key;
    cell.fg == key && cell.modifier.contains(ratatui::style::Modifier::BOLD)
}

#[test]
fn the_foots_keys_are_coloured_and_what_they_do_muted() {
    let scratch = Scratch::new("foot-coloured");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let buffer = buffer_of(&mut workbench, 120, 30);
    assert!(drawn_as_key(&buffer, "/ filter", 0));
    assert!(!drawn_as_key(&buffer, "/ filter", 2));
    let (x, y) = found(&buffer, "/ filter");
    assert_eq!(
        buffer[(x + 2, y)].fg,
        samplekit::tui::theme::Theme::default().muted
    );
    assert!(drawn_as_key(&buffer, "N new", 0));
    assert!(drawn_as_key(&buffer, "? help", 0));
    // A frame's keys too: a sample's `e edit`.
    keys(&mut workbench, "\n");
    let buffer = buffer_of(&mut workbench, 120, 30);
    assert!(drawn_as_key(&buffer, "e edit", 0));
    assert!(!drawn_as_key(&buffer, "e edit", 2));
}

#[test]
fn a_windows_keys_and_its_sentences_colour_the_keys_they_name() {
    let scratch = Scratch::new("window-coloured");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "D");
    let buffer = buffer_of(&mut workbench, 110, 30);
    assert!(drawn_as_key(&buffer, "y delete", 0));
    assert!(drawn_as_key(&buffer, "n cancel", 0));
    // Inside it, the key a sentence names, and not the verb after it.
    assert!(drawn_as_key(&buffer, "u gives it back", 0));
    assert!(!drawn_as_key(&buffer, "u gives it back", 2));
    keys(&mut workbench, "n");
    // A picker's note.
    keys(&mut workbench, "s");
    let buffer = buffer_of(&mut workbench, 110, 30);
    assert!(drawn_as_key(&buffer, "Enter sorts by", 0));
    assert!(!drawn_as_key(&buffer, "Enter sorts by", 6));
    keys(&mut workbench, "\u{1b}");
    // The figure's arrows, a key the hints once took for a word.
    keys(&mut workbench, "p\n");
    assert!(
        matches!(workbench.mode, Mode::Figure),
        "{}",
        workbench.message
    );
    let buffer = buffer_of(&mut workbench, 110, 30);
    assert!(drawn_as_key(&buffer, "←→ choose", 0));
    assert!(drawn_as_key(&buffer, "←→ choose", 1));
}

#[test]
fn the_help_colours_its_keys_and_those_its_descriptions_name() {
    let scratch = Scratch::new("help-coloured");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "\n?");
    let buffer = buffer_of(&mut workbench, 124, 60);
    // A key at a line's end, its verb on the next: still a key.
    assert!(
        drawn_as_key(&buffer, "asked first, u", 13),
        "D's description"
    );
    assert!(!drawn_as_key(&buffer, "asked first, u", 0));
    assert!(drawn_as_key(&buffer, "C computes it", 0));
    assert!(!drawn_as_key(&buffer, "C computes it", 2));
    assert!(drawn_as_key(&buffer, "?          this help", 0));
    assert!(!drawn_as_key(&buffer, "?          this help", 11));
}

#[test]
fn a_message_and_the_setup_colour_the_keys_they_name() {
    let scratch = Scratch::new("message-coloured");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.message = "changes not written: w writes them, r lets them go".to_string();
    let buffer = buffer_of(&mut workbench, 110, 30);
    assert!(drawn_as_key(&buffer, "w writes", 0));
    assert!(drawn_as_key(&buffer, "r lets", 0));
    let (x, y) = found(&buffer, "changes not written");
    assert_eq!(
        buffer[(x, y)].fg,
        samplekit::tui::theme::Theme::default().message
    );
    // The setup's lines of keys.
    let root = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-setup-keys-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let mut setup = Workbench::open(&root).unwrap();
    assert!(matches!(setup.screen, Screen::Setup { .. }));
    let buffer = buffer_of(&mut setup, 110, 40);
    assert!(drawn_as_key(&buffer, "j k choose · Enter answer", 0));
    assert!(drawn_as_key(&buffer, "j k choose · Enter answer", 2));
    assert!(!drawn_as_key(&buffer, "j k choose · Enter answer", 4));
    assert!(drawn_as_key(&buffer, "j k choose · Enter answer", 13));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_sentence_colours_the_keys_it_names_and_no_number() {
    use samplekit::tui::theme::{Theme, is_key, key_style, keyed};
    let base = ratatui::style::Style::new();
    let key = key_style(&Theme::default());
    // The words drawn as keys, in order.
    let keys_of = |text: &str| -> Vec<String> {
        keyed(text, base)
            .iter()
            .filter(|span| span.style == key)
            .map(|span| span.content.to_string())
            .collect()
    };
    assert_eq!(keys_of("removed C1 · u gives it back"), ["u"]);
    assert_eq!(
        keys_of("y leaves without them · n stays — P, then w, writes them"),
        ["y", "n", "P", "w"]
    );
    assert_eq!(keys_of("nothing declared here: a adds a setting"), ["a"]);
    assert_eq!(keys_of("Ctrl+C again quits at once"), ["Ctrl+C"]);
    assert_eq!(
        keys_of("the step marked; Enter on a project opens it"),
        ["Enter"]
    );
    assert!(keys_of("installing samplekit, a minute or so").is_empty());
    assert!(keys_of("3 samples computed, 1 remains").is_empty());
    // The sentence itself is kept, character for character.
    let text = "outdated: an input changed — C computes it";
    let said: String = keyed(text, base)
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    assert_eq!(said, text);
    for word in ["←→", "⇧↑↓", "1…9", "Ctrl+C", "Shift+Tab", "Backspace", "q"] {
        assert!(is_key(word), "{word}");
    }
    for word in ["filter", "apply", "Ctrl+"] {
        assert!(!is_key(word), "{word}");
    }
}

/// The rows of the window titled `title` and of the main frame around it:
/// (window top, window bottom, frame top, frame bottom).
fn window_in_frame(buffer: &ratatui::buffer::Buffer, title: &str) -> (u16, u16, u16, u16) {
    // `title` begins at the window's corner.
    let (left, top) = found(buffer, title);
    let bottom = (top + 1..buffer.area.height)
        .find(|y| buffer[(left, *y)].symbol() == "└")
        .expect("the window's foot");
    // The main frame's corners, a column in from the screen's edge.
    let frame_top = (0..buffer.area.height)
        .find(|y| buffer[(1, *y)].symbol() == "┌")
        .expect("the frame's top");
    let frame_bottom = (0..buffer.area.height)
        .rev()
        .find(|y| buffer[(1, *y)].symbol() == "└")
        .expect("the frame's foot");
    (top, bottom, frame_top, frame_bottom)
}

#[test]
fn a_window_stays_inside_the_main_frame() {
    // As tall as the frame, a window's corners fell on its border.
    let scratch = rich("window-inside");
    for (width, height) in [(40u16, 12u16), (60, 14), (100, 30)] {
        for (typed, title) in [
            ("s", "┌ sort by"),
            ("c", "┌ columns,"),
            ("g", "┌ group by"),
            ("D", "┌ remove "),
        ] {
            let mut workbench = Workbench::open(&scratch.0).unwrap();
            keys(&mut workbench, typed);
            let buffer = buffer_of(&mut workbench, width, height);
            let (top, bottom, frame_top, frame_bottom) = window_in_frame(&buffer, title);
            assert!(
                top > frame_top && bottom < frame_bottom,
                "{typed} at {width}×{height}: {top}..{bottom} in {frame_top}..{frame_bottom}"
            );
        }
        // Where a value came from, and a quantity's window, over a sample.
        for (typed, title) in [("\n\n", "┌ name — where"), ("\njjje", "┌ malt ")] {
            let mut workbench = Workbench::open(&scratch.0).unwrap();
            open_named(&mut workbench, "C1");
            keys(&mut workbench, &typed[1..]);
            let buffer = buffer_of(&mut workbench, width, height);
            let (top, bottom, frame_top, frame_bottom) = window_in_frame(&buffer, title);
            assert!(
                top > frame_top && bottom < frame_bottom,
                "{typed:?} at {width}×{height}: {top}..{bottom} in {frame_top}..{frame_bottom}"
            );
        }
    }
    // The help, narrower than a wide frame, inside it too.
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "?");
    let buffer = buffer_of(&mut workbench, 160, 20);
    let (top, bottom, frame_top, frame_bottom) = window_in_frame(&buffer, "┌ keys ");
    assert!(top > frame_top && bottom < frame_bottom, "{top}..{bottom}");
}

#[test]
fn the_setup_says_its_keys_one_way() {
    // The frame, the foot and the screen said Enter and Esc four ways.
    let root = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-setup-said-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let mut workbench = Workbench::open(&root).unwrap();
    let drawn = drawn_sized(&mut workbench, 120, 40).join("\n");
    assert_eq!(drawn.matches("Enter answer").count(), 3, "{drawn}");
    assert_eq!(drawn.matches("Esc back").count(), 3, "{drawn}");
    for unsaid in ["this answer", "keeps the answer", "previous question"] {
        assert!(!drawn.contains(unsaid), "{unsaid}: {drawn}");
    }
    // Past the last question, Enter sets the project up, said so thrice.
    keys(&mut workbench, "\n\n\n");
    assert!(matches!(workbench.screen, Screen::Setup { .. }));
    let drawn = drawn_sized(&mut workbench, 120, 40).join("\n");
    assert_eq!(drawn.matches("Enter set up").count(), 3, "{drawn}");
    assert!(!drawn.contains("Enter answer"), "{drawn}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn every_window_says_esc_cancel() {
    // The files' and the figure's windows said `Esc close`.
    let scratch = Scratch::new("esc-cancel");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "o");
    let drawn = screen(&mut workbench);
    assert!(drawn.contains("Esc cancel"), "{drawn}");
    keys(&mut workbench, "\u{1b}p\n");
    assert!(
        matches!(workbench.mode, Mode::Figure),
        "{}",
        workbench.message
    );
    let drawn = drawn_sized(&mut workbench, 120, 30).join("\n");
    assert!(drawn.contains("Esc cancel"), "{drawn}");
    assert!(!drawn.contains("Esc close"), "{drawn}");
}

#[test]
fn nothing_to_report_is_said_once() {
    // Its title and a message said it twice.
    let scratch = Scratch::new("nothing-to-report");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "v");
    assert!(workbench.message.is_empty(), "{}", workbench.message);
    let drawn = screen(&mut workbench);
    assert_eq!(
        drawn
            .matches("everything is current, and nothing is defective")
            .count(),
        1,
        "{drawn}"
    );
    assert!(!drawn.contains("nothing to report"), "{drawn}");
}

#[test]
fn the_notes_key_says_show_or_hide() {
    // `n hide` beside the note, `n note` once it was hidden.
    let scratch = Scratch::new("note-key");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "\n");
    let drawn = drawn_sized(&mut workbench, 120, 30).join("\n");
    assert!(drawn.contains("n hide note"), "{drawn}");
    keys(&mut workbench, "n");
    let drawn = drawn_sized(&mut workbench, 120, 30).join("\n");
    assert!(drawn.contains("n show note"), "{drawn}");
    assert!(!drawn.contains("n hide"), "{drawn}");
}

/// Where `needle` is drawn on a screen of 100×20, and the terminal that
/// drew it.
fn drawn_where(workbench: &mut Workbench, needle: &str) -> (u16, u16, Terminal<TestBackend>) {
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal
        .draw(|frame| samplekit::tui::view::draw(frame, workbench))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    for y in 0..20u16 {
        let row: Vec<String> = (0..100u16)
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect();
        let joined: String = row.concat();
        if let Some(at) = joined.find(needle) {
            let x = joined[..at].chars().count() as u16;
            return (x, y, terminal);
        }
    }
    panic!("{needle} is not drawn");
}

#[test]
fn a_drag_selects_in_a_line_typed_and_a_double_click_a_word() {
    use crossterm::event::{MouseButton, MouseEventKind};
    let scratch = rich("drag");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let typed = |workbench: &Workbench| match &workbench.mode {
        Mode::Filter { text, .. } => text.text().to_string(),
        _ => panic!("the filter"),
    };
    // Pressed on `beer`, dragged to its end, let go: typing replaces it.
    keys(&mut workbench, "/beer == schwarz");
    let (x, y, _) = drawn_where(&mut workbench, "beer == schwarz");
    workbench.key(Key::Click(x, y));
    workbench.key(pointer(MouseEventKind::Drag(MouseButton::Left), x + 4, y));
    workbench.key(pointer(MouseEventKind::Up(MouseButton::Left), x + 4, y));
    keys(&mut workbench, "malt");
    assert_eq!(typed(&workbench), "malt == schwarz");
    // A double click selects the word under it.
    keys(&mut workbench, "\u{1b}/malt == schwarz");
    let (x, y, _) = drawn_where(&mut workbench, "schwarz");
    workbench.key(Key::Click(x + 2, y));
    workbench.key(pointer(MouseEventKind::Up(MouseButton::Left), x + 2, y));
    workbench.key(Key::Click(x + 2, y));
    workbench.key(pointer(MouseEventKind::Up(MouseButton::Left), x + 2, y));
    keys(&mut workbench, "bock");
    assert_eq!(typed(&workbench), "malt == bock");
    // In the quantity's window a click on another field types there.
    keys(&mut workbench, "\u{1b}");
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e");
    let (x, y, _) = drawn_where(&mut workbench, "uncertainty");
    workbench.key(Key::Click(x + 20, y));
    let Mode::Quantity { on, .. } = &workbench.mode else {
        panic!("the quantity's window");
    };
    assert_eq!(*on, 1);
}

#[test]
fn the_cursor_of_a_line_typed_is_the_terminals_and_a_selection_is_reversed() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let scratch = rich("terminal-cursor");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    // The new sample's name: the cursor after what is typed.
    keys(&mut workbench, "Nabc");
    assert!(matches!(workbench.mode, Mode::Prompt { .. }));
    let (x, y, mut terminal) = drawn_where(&mut workbench, "abc");
    let cursor = terminal.get_cursor_position().unwrap();
    assert_eq!((cursor.x, cursor.y), (x + 3, y));
    // Shift+← selects the last character, drawn reversed; the rest not.
    workbench.key(held(KeyCode::Left, KeyModifiers::SHIFT));
    let (x, y, terminal) = drawn_where(&mut workbench, "abc");
    let buffer = terminal.backend().buffer();
    let reversed = |at: u16| {
        buffer[(at, y)]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED)
    };
    assert!(reversed(x + 2) && !reversed(x + 1) && !reversed(x));
    // The note's cursor is the terminal's, where the editor's is.
    keys(&mut workbench, "\u{1b}");
    open_named(&mut workbench, "D1");
    keys(&mut workbench, "N");
    let (_, _, mut terminal) = drawn_where(&mut workbench, "First line.");
    let view = workbench.note_view.expect("drawn while edited");
    let cursor = terminal.get_cursor_position().unwrap();
    assert_eq!((cursor.x, cursor.y), (view.x, view.y));
}
