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
    assert_eq!(workbench.entries()[2].field, "alloy");
    let screen_text = screen(&mut workbench);
    assert!(screen_text.contains("diameter"), "{screen_text}");
    assert!(screen_text.contains("A note on C1"), "{screen_text}");
    // On diameter: e, a new number typed over its cell, Enter keeps it
    // waiting; w previews what waits, y writes it.
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, "e12.5\n\u{1b}");
    assert!(!workbench.typing, "{}", workbench.message);
    keys(&mut workbench, "w");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines.iter().any(|line| line.contains("→  12.5")),
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
        assert!(last.ends_with("? all keys"), "{width}: {last}");
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
    keys(&mut workbench, "f/cobalt\n");
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
    // current. (Read without a model, this value's record says edited.) The
    // mark stands for it, at every width: its words are the card's.
    let scratch = Scratch::new("state-mark");
    fs::write(
        scratch.0.join("S.md"),
        "---\nschema_version: 1\nname: S\nproperties:\n  \
         diameter: {v: 13.0, fingerprint: aaaaaaaaaaaa}\n  a_rather_long_quantity_name:\n    \
         v: 3.0\n    computed: {diameter: bbbbbbbbbbbb}\n    fingerprint: cccccccccccc\n---\n",
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
    assert!(wide.contains('✎') && !wide.contains("edite"), "{wide}");
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
    write_waiting(&mut workbench);
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
             filter = 'diameter > 0'\n"
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
            format!("---\nschema_version: 1\nname: s{at:02}\nproperties:\n  diameter: {at}\n---\n"),
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
    keys(&mut workbench, "s/diameter r\u{1b}/alloy");
    let drawn = screen(&mut workbench);
    // Narrowed to alloy, and diameter still said, with its way.
    assert!(drawn.contains("sorted by  diameter ↓"), "{drawn}");
}

#[test]
fn a_narrow_table_cuts_its_headers_names_first_then_units() {
    use samplekit::tui::view::fitted;
    let headers = [
        "Heating [g/100L]".to_string(),
        "Expected darkness [HV]".to_string(),
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
    assert_eq!(cut, ["Hea… [g/100L]", "Expe… [HV]"]);
    // Then the units, and never narrower than the values.
    let cut = fitted(&headers, &values, 4);
    assert_eq!(cut, ["Hea…", "Exp…"]);
}

#[test]
fn a_table_cells_header_cuts_its_table_first() {
    use samplekit::tui::view::fitted;
    let headers = ["inspection.waviness[50] [%]".to_string()];
    let cut = fitted(&headers, &[6], 21);
    assert_eq!(cut, ["ins….waviness[50] [%]"]);
    // Then the column's own name, the unit last.
    let cut = fitted(&headers, &[6], 14);
    assert_eq!(cut, ["ins….wavi… [%]"]);
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
    let text = ["WQ-001".to_string(), "UT05".to_string()];
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
    // diameter over 12, 10, 11 and D1's 9: a mean of 10.5, four of four.
    let diameter = drawn
        .lines()
        .find(|line| line.contains("diameter"))
        .unwrap_or_default();
    assert!(
        diameter.contains("4/4") && diameter.contains("10.5"),
        "{drawn}"
    );
    keys(&mut workbench, "S");
    assert!(!workbench.summarised);
}

#[test]
fn s_summarises_each_project_apart() {
    // Over two projects, `S` pooled their samples, where `--summary` writes
    // one table per project.
    let scratch = rich("summary-projects");
    fs::write(scratch.0.join("more/.samplekitrc"), "schema_version = 1\n").unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "S");
    let drawn = drawn_sized(&mut workbench, 120, 30).join("\n");
    assert!(drawn.contains("project"), "{drawn}");
    let diameters: Vec<&str> = drawn
        .lines()
        .filter(|line| line.contains("diameter"))
        .collect();
    // 12, 10 and 11 in one, D1's 9 in the other.
    assert!(
        diameters
            .iter()
            .any(|line| line.contains("3/3") && line.contains("11")),
        "{drawn}"
    );
    assert!(
        diameters
            .iter()
            .any(|line| line.contains("1/1") && line.contains('9')),
        "{drawn}"
    );
}

#[test]
fn the_names_go_when_the_picker_takes_them_away() {
    // `name` is a column like the others: shown first while it is chosen, and
    // gone when the picker takes it away.
    let scratch = rich("names-away");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    assert!(screen(&mut workbench).contains("D1"));
    keys(&mut workbench, "c");
    let Mode::Picker { items, .. } = &mut workbench.mode else {
        panic!("columns");
    };
    for item in items.iter_mut() {
        if item.0 == "name" {
            item.1 = false;
        }
    }
    keys(&mut workbench, "\n");
    assert!(
        !workbench
            .profile
            .columns()
            .iter()
            .any(|column| column.field == "name")
    );
    let drawn = screen(&mut workbench);
    assert!(!drawn.contains("D1"), "{drawn}");
}

#[test]
fn a_narrow_summary_leaves_columns_out_whole() {
    // At 80 columns and less, a count and a mean were cut from their left,
    // `2` for `11/12` and `053` for `1.053`.
    let scratch = rich("summary-narrow");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "S");
    let drawn = drawn_sized(&mut workbench, 44, 20).join("\n");
    assert!(drawn.contains("4/4"), "{drawn}");
    assert!(drawn.contains("median, min, max"), "{drawn}");
}

#[test]
fn what_a_small_screen_cuts_is_marked() {
    // At 40×16 a value's window lost its advice, and the configuration its
    // values and its keys, all without a sign.
    let scratch = rich("small-cut");
    let rc = fs::read_to_string(scratch.0.join(".samplekitrc")).unwrap();
    fs::write(
        scratch.0.join(".samplekitrc"),
        rc.replace(
            "recursive = true",
            "recursive = true\nexclude = [\"README.md\", \"drafts/**\", \"old/**\"]",
        ),
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "diameter");
    // A value's card: the state it cannot hold, `no statistic`, cut.
    keys(&mut workbench, "i");
    let drawn = drawn_sized(&mut workbench, 40, 16).join("\n");
    let state = drawn
        .lines()
        .find(|line| line.contains("diameter  9.0"))
        .unwrap_or_default();
    assert!(state.contains("· no s") && state.contains('…'), "{drawn}");
    keys(&mut workbench, "\u{1b}\u{1b}P\n");
    let drawn = drawn_sized(&mut workbench, 40, 16).join("\n");
    let exclude = drawn
        .lines()
        .find(|line| line.contains("exclude"))
        .unwrap_or_default();
    assert!(exclude.contains('…'), "{drawn}");
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
    // The sample's removal, asked first: a short question.
    keys(&mut workbench, "D");
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
        "# Batch 7\n\nPolished **twice**, then:\n\n- soaked\n- treated\n",
    );
    fs::write(&path, text).unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    let shown = screen(&mut workbench);
    // Rendered: the emphasis is a style, not two stars.
    assert!(shown.contains("Batch 7"), "{shown}");
    assert!(shown.contains("Polished twice"), "{shown}");
    assert!(!shown.contains("**"), "{shown}");
    assert!(shown.contains("soaked"), "{shown}");
    // Edited as typed: every character there.
    keys(&mut workbench, "N");
    let editing = screen(&mut workbench);
    assert!(editing.contains("Polished **twice**"), "{editing}");
    assert!(editing.contains("# Batch 7"), "{editing}");
}

#[test]
fn a_long_line_of_the_note_wraps_at_words_while_edited() {
    // Edited, the note scrolled sideways: each line's beginning went out of
    // sight, `londe steel`, `ighter steel for summer`.
    let scratch = Scratch::new("note-wraps");
    let path = scratch.0.join("C1.md");
    let text = fs::read_to_string(&path).unwrap().replace(
        "A note on C1.",
        "A blonde steel poured a little lighter than the last, for the long days of summer, \
         with a second dose of carbon at the end.",
    );
    fs::write(&path, text).unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    keys(&mut workbench, "N");
    // The cursor at the end of the text, where a line scrolled furthest.
    workbench.key(Key::End);
    let editing = drawn_sized(&mut workbench, 60, 30).join("\n");
    assert!(editing.contains("A blonde steel"), "{editing}");
    assert!(editing.contains("carbon at the end."), "{editing}");
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
        now: "C3 · diameter".to_string(),
        outcome: None,
    });
    // On the title bar, the first line, whatever the screen: steps are values.
    let drawn = screen(&mut workbench);
    let title = drawn.lines().next().unwrap();
    assert!(
        title.contains("⟳ [██████░░░░░░] 2/4 · C3 · diameter"),
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
    // The terminal's own colours, which `terminal` leaves it.
    workbench.theme = samplekit::tui::theme::Themes::read(None)
        .theme("terminal", None)
        .unwrap();
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
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, "e12.5\n\u{1b}");
    write_waiting(&mut workbench);
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
        "---\nschema_version: 1\nname: C3\nalloy: cobalt\nproperties:\n  \
         diameter: {v: 11.0, u: 0.1, unit: g, fingerprint: '000000000000'}\n  \
         twice: {v: 22.0, computed: {diameter: '111111111111'}, fingerprint: '222222222222'}\n---\n",
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
    assert!(line.ends_with("? all keys"), "{line}");
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
         a_long_measured_quantity: {v: 1.0, u: 0.1, unit: g, fingerprint: '000000000000'}\n  \
         another_rather_long_derived_quantity:\n    v: 123456.789\n    u: 1234.5\n    \
         unit: kg/hL\n    computed: {a_long_measured_quantity: '111111111111'}\n    \
         fingerprint: '222222222222'\n---\n",
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
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, "N");
    let lines = drawn_sized(&mut workbench, 100, 20);
    let foot = lines.last().unwrap();
    assert!(foot.contains("Esc done"), "{foot}");
    assert!(!foot.contains("? all keys"), "{foot}");
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
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, "e12.5\n\u{1b}");
    write_waiting(&mut workbench);
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
        (60, "p", ["Enter run", "Esc cancel"]),
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
        assert!(lines[11].trim_end().ends_with("? all keys"), "{drawn}");
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
    assert!(drawn.contains("cobalt"), "{drawn}");
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
                "---\nschema_version: 1\nname: S{at:02}\nproperties:\n  diameter: {{v: 1.0, unit: g}}\n---\n"
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
        now: "C2 · diameter".to_string(),
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
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"name\", label = \"Piece\"}, \
         {field = \"diameter\"}]\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let drawn = drawn_sized(&mut workbench, 80, 12).join("\n");
    assert!(drawn.contains("Piece"), "{drawn}");
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
        .filter(|y| buffer[(10, *y)].bg == workbench.theme.line)
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
    keys(&mut workbench, "g/alloy \n");
    let drawn = screen(&mut workbench);
    let chrome = drawn.find("── alloy = chrome").expect(&drawn);
    let cobalt = drawn.find("── alloy = cobalt").expect(&drawn);
    assert!(chrome < drawn.find("C2").unwrap(), "{drawn}");
    assert!(drawn.find("C2").unwrap() < cobalt, "{drawn}");
    assert!(cobalt < drawn.find("C1").unwrap(), "{drawn}");
    // `S` summarises each group, its values first.
    keys(&mut workbench, "S");
    let drawn = screen(&mut workbench);
    let chrome = drawn.find("chrome").expect(&drawn);
    let cobalt = drawn.find("cobalt").expect(&drawn);
    assert!(chrome < cobalt, "{drawn}");
    assert!(drawn.contains("alloy"), "{drawn}");
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
            lines[at - 1].trim_end().ends_with("? all keys"),
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
        assert!(kept >= 5, "{width}\n{drawn}");
    }
    // Where one line holds them, one line: the sample's at 100 columns.
    open_named(&mut workbench, "C1");
    let lines = drawn_sized(&mut workbench, 100, 30);
    let drawn = lines.join("\n");
    assert!(lines[27].contains('└'), "{drawn}");
    assert!(lines[28].contains("of 4 samples"), "{drawn}");
    assert!(lines[29].contains("Esc back"), "{drawn}");
    assert!(lines[29].trim_end().ends_with("? all keys"), "{drawn}");
}

#[test]
fn an_80_column_foot_keeps_update_model_and_remove_filter_on_or_not() {
    // At 80 columns, or with a filter on, `C`, `M` and `D` were given up
    // before the basket's keys.
    use samplekit::tui::model::Place;
    use samplekit::tui::view::foot_lines;
    for filtered in [false, true] {
        let foot = foot_lines(Place::Collection, filtered, false, 80).join("\n");
        for key in ["C update", "M model", "D remove"] {
            assert!(foot.contains(key), "{filtered}: {key}\n{foot}");
        }
        assert!(foot.trim_end().ends_with("? all keys"), "{foot}");
    }
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
    // A list keeps its width.
    workbench.key(Key::Esc);
    keys(&mut workbench, "s");
    let lines = drawn_sized(&mut workbench, 300, 50);
    let (sort, _) = framed_width(&lines, " sort by, first to last ");
    assert!(sort > 80, "{sort}\n{}", lines.join("\n"));
    // What a snapshot changed.
    workbench.key(Key::Esc);
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, "e12.5\n\u{1b}");
    write_waiting(&mut workbench);
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
    let key = default_theme().key;
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
    assert_eq!(buffer[(x + 2, y)].fg, default_theme().muted);
    assert!(drawn_as_key(&buffer, "N new", 0));
    assert!(drawn_as_key(&buffer, "? all keys", 0));
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
    assert_eq!(buffer[(x, y)].fg, default_theme().message);
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
        // A value's card, and a list's window, over a sample.
        for (typed, title) in [
            ("\njjji", "┌ diameter · its card"),
            ("\njj\n", "┌ alloy · "),
        ] {
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
    // Pressed on `alloy`, dragged to its end, let go: typing replaces it.
    keys(&mut workbench, "/alloy == cobalt");
    let (x, y, _) = drawn_where(&mut workbench, "alloy == cobalt");
    workbench.key(Key::Click(x, y));
    workbench.key(pointer(MouseEventKind::Drag(MouseButton::Left), x + 5, y));
    workbench.key(pointer(MouseEventKind::Up(MouseButton::Left), x + 5, y));
    keys(&mut workbench, "diameter");
    assert_eq!(typed(&workbench), "diameter == cobalt");
    // A double click selects the word under it.
    keys(&mut workbench, "\u{1b}/diameter == cobalt");
    let (x, y, _) = drawn_where(&mut workbench, "cobalt");
    workbench.key(Key::Click(x + 2, y));
    workbench.key(pointer(MouseEventKind::Up(MouseButton::Left), x + 2, y));
    workbench.key(Key::Click(x + 2, y));
    workbench.key(pointer(MouseEventKind::Up(MouseButton::Left), x + 2, y));
    keys(&mut workbench, "chrome");
    assert_eq!(typed(&workbench), "diameter == chrome");
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

#[test]
fn a_tables_unit_heads_its_column_and_a_list_window_is_headed() {
    // Each cell of a table carried its unit and no header did, where `view`
    // heads its columns; a list's window had no header at all.
    let scratch = rich("units-headed");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "runs");
    workbench.key(Key::Right);
    let drawn = screen(&mut workbench);
    assert!(drawn.contains("load [N]"), "{drawn}");
    assert!(!drawn.contains("3.5 N"), "{drawn}");
    // An attribute's window: a column headed `value`, titled as a table.
    workbench.key(Key::Esc);
    workbench.key(Key::Home);
    move_to(&mut workbench, "alloy");
    keys(&mut workbench, "\n");
    let drawn = screen(&mut workbench);
    assert!(drawn.contains("│ value "), "{drawn}");
    assert!(drawn.contains("alloy · one value"), "{drawn}");
    // A line typed there is titled by its address.
    keys(&mut workbench, "e");
    let drawn = screen(&mut workbench);
    assert!(drawn.contains("alloy[1] ="), "{drawn}");
}

#[test]
fn a_tables_window_is_as_large_as_it_needs() {
    // A table's window took nearly the whole screen: a table of two short
    // rows is no taller than a list's window of one item.
    let scratch = rich("table-size");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "alloy");
    keys(&mut workbench, "\n");
    let buffer = buffer_of(&mut workbench, 120, 40);
    let (top, bottom, frame_top, frame_bottom) = window_in_frame(&buffer, "┌ alloy ");
    let list_height = bottom - top;
    assert!(
        list_height < (frame_bottom - frame_top) / 2,
        "{list_height}"
    );
    keys(&mut workbench, "\u{1b}");
    workbench.key(Key::Home);
    move_to(&mut workbench, "runs");
    workbench.key(Key::Right);
    let buffer = buffer_of(&mut workbench, 120, 40);
    let (top, bottom, _, _) = window_in_frame(&buffer, "┌ runs");
    assert_eq!(bottom - top, list_height);
}

#[test]
fn a_value_waiting_behind_a_failure_is_marked_waiting() {
    // *waits for young_modulus, which failed* held the word failed, and was
    // marked ✗ beside the colour of waiting, where `v` says waiting.
    use samplekit::tui::view::state_mark;
    assert_eq!(state_mark("waits for young_modulus, which failed"), "∅");
    assert_eq!(state_mark("failed — ValueError"), "✗");
    assert_eq!(state_mark("never computed"), "∅");
}

#[test]
fn a_scripts_window_shows_what_it_printed_and_the_line_answering_it() {
    let scratch = Scratch::new("script-window");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let (_sender, events) = std::sync::mpsc::channel();
    workbench.scripting = Some(samplekit::tui::model::ScriptRunning {
        name: "asked".to_string(),
        events,
        input: None,
        printed: "scaling C1\nFactor? ".to_string(),
    });
    workbench.mode = Mode::Script {
        line: samplekit::tui::typing::Typed::new("0.5"),
    };
    let drawn = screen(&mut workbench);
    assert!(drawn.contains(" run asked "), "{drawn}");
    assert!(drawn.contains("scaling C1"), "{drawn}");
    // What is typed goes on where the question stops, as in a terminal.
    assert!(drawn.contains("Factor? 0.5"), "{drawn}");
    assert!(drawn.contains("Enter answers"), "{drawn}");
}

#[test]
fn a_menu_item_too_long_for_its_window_is_cut_with_its_mark() {
    let scratch = Scratch::new("cut-item");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let long = format!(
        "  scale   on each sample   {}",
        "a very long description ".repeat(6)
    );
    workbench.mode = Mode::Picker {
        purpose: samplekit::tui::model::Choosing::Declare(Vec::new()),
        items: vec![(long.clone(), false), ("  short".to_string(), false)],
        cursor: 1,
        query: samplekit::tui::typing::Typed::unfocused(""),
        typing: false,
    };
    let drawn = screen(&mut workbench);
    let row = drawn
        .lines()
        .find(|line| line.contains("scale   on each sample"))
        .expect("the item drawn");
    assert!(row.contains('…'), "{drawn}");
    assert!(!row.contains(&long), "{drawn}");
}

// ------------------------------------------------------------ typing and what waits

/// The screen drawn, and its buffer, at `width` by 24.
fn drawn_buffer(workbench: &mut Workbench, width: u16) -> ratatui::buffer::Buffer {
    let mut terminal = ratatui::Terminal::new(TestBackend::new(width, 24)).unwrap();
    terminal
        .draw(|frame| samplekit::tui::view::draw(frame, workbench))
        .unwrap();
    terminal.backend().buffer().clone()
}

fn row_of(buffer: &ratatui::buffer::Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

fn line_holding(buffer: &ratatui::buffer::Buffer, text: &str) -> u16 {
    (0..buffer.area.height)
        .find(|y| row_of(buffer, *y).contains(text))
        .unwrap_or_else(|| panic!("no line holds {text}"))
}

#[test]
fn a_cell_typed_in_never_widens_its_text_scrolling_inside() {
    let scratch = Scratch::new("view-typing-wide");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "diameter");
    let before = drawn_buffer(&mut workbench, 100);
    let y = line_holding(&before, "diameter");
    let pane_edge = |buffer: &ratatui::buffer::Buffer| {
        (1..buffer.area.width)
            .find(|x| buffer[(*x, y)].symbol() == "│" && buffer[(*x + 1, y)].symbol() == "│")
    };
    let edge = pane_edge(&before);
    keys(&mut workbench, "e");
    keys(&mut workbench, &"9".repeat(80));
    let typing = drawn_buffer(&mut workbench, 100);
    let line = row_of(&typing, y);
    assert_eq!(
        pane_edge(&typing),
        edge,
        "the panes stay where they were: {line}"
    );
    assert!(line.contains("diameter"), "the name stays: {line}");
    assert!(line.matches('9').count() < 80, "it scrolls inside: {line}");
}

#[test]
fn the_status_line_ends_with_the_value_under_the_cursor_unrounded() {
    let scratch = rich("view-raw");
    let c1 = scratch.0.join("C1.md");
    fs::write(
        &c1,
        fs::read_to_string(&c1)
            .unwrap()
            .replace("v: 12.0", "v: 12.345678"),
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "diameter");
    let shown = drawn_buffer(&mut workbench, 120);
    let status = row_of(&shown, 22);
    assert!(
        status.trim_end().ends_with("diameter = 12.345678 ± 0.1 g"),
        "{status}"
    );
    keys(&mut workbench, "\u{1b}");
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "runs");
    workbench.key(Key::Right);
    workbench.key(Key::Right);
    let status = row_of(&drawn_buffer(&mut workbench, 120), 22);
    assert!(status.trim_end().ends_with("= 3.5"), "{status}");
    assert!(status.contains("runs.load"), "{status}");
    keys(&mut workbench, "\u{1b}\u{1b}");
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "alloy");
    let status = row_of(&drawn_buffer(&mut workbench, 120), 22);
    assert!(!status.contains(" = "), "{status}");
}

#[test]
fn the_cursors_line_is_lit_across_the_pane_and_the_typing_mode_said() {
    let scratch = Scratch::new("view-lit");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "tags");
    let shown = drawn_buffer(&mut workbench, 100);
    let y = line_holding(&shown, "tags");
    let edge = (1..shown.area.width)
        .find(|x| shown[(*x, y)].symbol() == "│" && shown[(*x + 1, y)].symbol() == "│")
        .unwrap();
    assert!(
        shown[(edge - 2, y)].bg == workbench.theme.line,
        "the pane's last column is lit"
    );
    workbench.key(Key::Home);
    move_to(&mut workbench, "alloy");
    keys(&mut workbench, "e");
    let typing = drawn_buffer(&mut workbench, 100);
    assert!(row_of(&typing, 22).contains("TYPING") || row_of(&typing, 23).contains("TYPING"));
    let y = line_holding(&typing, "cobalt");
    let x = (0..typing.area.width)
        .find(|x| typing[(*x, y)].symbol() == "c" && typing[(*x + 1, y)].symbol() == "o")
        .unwrap();
    assert!(
        typing[(x, y)]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED),
        "the cell under the cursor stands out: {}",
        row_of(&typing, y)
    );
    // Its line on the `line` colour, not reversed: the cell is what stands out.
    let edge = (1..typing.area.width)
        .find(|x| typing[(*x, y)].symbol() == "│" && typing[(*x + 1, y)].symbol() == "│")
        .unwrap();
    assert_eq!(typing[(edge - 2, y)].bg, workbench.theme.line);
    assert!(
        !typing[(edge - 2, y)]
            .modifier
            .contains(ratatui::style::Modifier::REVERSED)
    );
}

#[test]
fn a_value_without_uncertainty_shows_its_place() {
    let scratch = rich("view-place");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    let shown = drawn_buffer(&mut workbench, 100);
    let line = row_of(&shown, line_holding(&shown, "diameter"));
    assert!(line.contains("9.0 ± — g"), "{line}");
}

#[test]
fn what_waits_is_marked_and_counted() {
    let scratch = Scratch::new("view-waits");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "alloy");
    keys(&mut workbench, "enickel\n\u{1b}");
    let shown = drawn_buffer(&mut workbench, 100);
    let y = line_holding(&shown, "nickel");
    let x = (0..shown.area.width)
        .find(|x| shown[(*x, y)].symbol() == "n" && shown[(*x + 1, y)].symbol() == "i")
        .unwrap();
    assert!(
        shown[(x, y)]
            .modifier
            .contains(ratatui::style::Modifier::ITALIC)
    );
    assert!(
        row_of(&shown, 22).contains("1 file not written"),
        "{}",
        row_of(&shown, 22)
    );
    keys(&mut workbench, "\u{1b}");
    let listed = drawn_buffer(&mut workbench, 100);
    let y = line_holding(&listed, "C1");
    let x = (0..listed.area.width)
        .find(|x| listed[(*x, y)].symbol() == "C" && listed[(*x + 1, y)].symbol() == "1")
        .unwrap();
    assert!(
        listed[(x, y)]
            .modifier
            .contains(ratatui::style::Modifier::ITALIC),
        "named as changed"
    );
}

/// Every cell of a frame drawn under `theme`: its colours.
fn cells_under(
    workbench: &mut Workbench,
    theme: &str,
) -> Vec<(String, ratatui::style::Color, ratatui::style::Color)> {
    workbench.theme_chosen = Some(theme.to_string());
    assert!(workbench.themed().is_empty());
    let buffer = buffer_of(workbench, 100, 30);
    buffer
        .content
        .iter()
        .map(|cell| (cell.symbol().to_string(), cell.fg, cell.bg))
        .collect()
}

#[test]
fn a_theme_paints_the_background_and_the_text_of_every_cell() {
    use ratatui::style::Color;
    let scratch = Scratch::new("painted");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let nord = samplekit::tui::theme::Themes::read(None)
        .theme("nord", None)
        .unwrap();
    let left = |cells: &[(String, Color, Color)]| {
        cells
            .iter()
            .filter(|(_, fg, bg)| *fg == Color::Reset || *bg == Color::Reset)
            .count()
    };
    // The collection, a sample and its note, a window over it, the typing
    // mode's badge.
    let collection = cells_under(&mut workbench, "nord");
    assert_eq!(left(&collection), 0);
    assert!(
        collection.iter().any(|(symbol, fg, bg)| symbol == "C"
            && *fg == nord.foreground
            && *bg == nord.background)
    );
    assert!(left(&cells_under(&mut workbench, "terminal")) > 0);
    keys(&mut workbench, "\n");
    let sample = cells_under(&mut workbench, "nord");
    assert_eq!(left(&sample), 0);
    let note = buffer_of(&mut workbench, 100, 30);
    let (x, y) = found(&note, "A note on C1.");
    assert_eq!(
        (note[(x, y)].fg, note[(x, y)].bg),
        (nord.foreground, nord.background)
    );
    keys(&mut workbench, "D");
    assert!(matches!(workbench.mode, Mode::Confirm { .. }));
    assert_eq!(left(&cells_under(&mut workbench, "nord")), 0);
    assert!(left(&cells_under(&mut workbench, "terminal")) > 0);
    keys(&mut workbench, "n");
    move_to(&mut workbench, "alloy");
    keys(&mut workbench, "e");
    let typing = cells_under(&mut workbench, "nord");
    assert_eq!(left(&typing), 0);
    let badge = buffer_of(&mut workbench, 100, 30);
    let (x, y) = found(&badge, " TYPING ");
    assert_eq!(
        (badge[(x + 1, y)].fg, badge[(x + 1, y)].bg),
        (nord.background, nord.accent)
    );
}

#[test]
fn the_built_in_themes_are_the_colours_pages_files() {
    use samplekit::config::project_config::COLOR_ROLES;
    use samplekit::tui::theme::{BUILT_IN, Theme, Themes, color};
    assert_eq!(
        BUILT_IN[..3],
        ["samplekit-dark", "samplekit-light", "terminal"]
    );
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files: Vec<String> = fs::read_dir(root.join("docs/templates/colours"))
        .unwrap()
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension()? == "toml").then(|| path.file_stem()?.to_str().map(String::from))?
        })
        .collect();
    files.sort();
    let mut built: Vec<String> = BUILT_IN
        .iter()
        .filter(|name| **name != "terminal")
        .map(|name| name.to_string())
        .collect();
    built.sort();
    assert_eq!(
        files, built,
        "every theme file is one SampleKit has, and no other"
    );
    let themes = Themes::read(None);
    assert_eq!(themes.names(), BUILT_IN);
    // Each theme its file, every role in #rrggbb.
    for name in BUILT_IN.iter().filter(|name| **name != "terminal") {
        let table: toml::Table =
            fs::read_to_string(root.join(format!("docs/templates/colours/{name}.toml")))
                .unwrap()
                .parse()
                .unwrap();
        let colors = table["colors"].as_table().unwrap();
        assert_eq!(colors.len(), COLOR_ROLES.len(), "{name}");
        let theme = themes.theme(name, None).unwrap();
        assert_eq!(
            theme.accent,
            color(colors["accent"].as_str().unwrap()),
            "{name}"
        );
        assert_eq!(
            theme.background,
            color(colors["background"].as_str().unwrap()),
            "{name}"
        );
        for (role, written) in colors {
            let written = written.as_str().unwrap();
            assert!(
                written.len() == 7 && written.starts_with('#'),
                "{name}: {role}"
            );
        }
    }
    // `terminal` is the sixteen colours, painting nothing.
    assert_eq!(themes.theme("terminal", None).unwrap(), Theme::of(None));
    assert_eq!(
        themes.theme("terminal", None).unwrap().background,
        ratatui::style::Color::Reset
    );
}

/// WCAG's relative luminance of a colour drawn in `#rrggbb`.
fn luminance(color: ratatui::style::Color) -> f64 {
    let ratatui::style::Color::Rgb(red, green, blue) = color else {
        panic!("{color:?} is no #rrggbb");
    };
    let linear = |channel: u8| {
        let channel = f64::from(channel) / 255.0;
        if channel <= 0.03928 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue)
}

/// WCAG's contrast between two colours, 1 to 21.
fn contrast(one: ratatui::style::Color, other: ratatui::style::Color) -> f64 {
    let (one, other) = (luminance(one), luminance(other));
    (one.max(other) + 0.05) / (one.min(other) + 0.05)
}

#[test]
fn samplekit_themes_meet_their_contrasts() {
    use samplekit::config::project_config::COLOR_ROLES;
    use samplekit::tui::theme::{Themes, color};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let themes = Themes::read(None);
    let mut failing = Vec::new();
    for name in ["samplekit-dark", "samplekit-light"] {
        let table: toml::Table =
            fs::read_to_string(root.join(format!("docs/templates/colours/{name}.toml")))
                .unwrap()
                .parse()
                .unwrap();
        let colors = table["colors"].as_table().unwrap();
        let role = |role: &str| color(colors[role].as_str().unwrap());
        let (background, line) = (role("background"), role("line"));
        assert_eq!(themes.theme(name, None).unwrap().background, background);
        println!("{name}: role, colour, against the background, against the line");
        for (each, _) in COLOR_ROLES {
            if *each == "background" {
                continue;
            }
            let on_background = contrast(role(each), background);
            let on_line = contrast(role(each), line);
            println!(
                "  {each:<10} {}  {on_background:5.2}  {on_line:5.2}",
                colors[*each].as_str().unwrap()
            );
            // The text and the values read at 7:1, on the cursor's line too;
            // every other role at 4.5:1, the muted at 3:1.
            let (wanted, on) = match *each {
                "foreground" | "value" => (7.0, on_background.min(on_line)),
                "muted" => (3.0, on_background),
                "line" => continue,
                _ => (4.5, on_background),
            };
            if on < wanted {
                failing.push(format!("{name}: {each} at {on:.2}, under {wanted}"));
            }
        }
        // No two roles one colour: where roles meet on a screen, each is told
        // apart.
        let mut seen: Vec<(&str, &str)> = Vec::new();
        for (each, written) in colors {
            let written = written.as_str().unwrap();
            if let Some((other, _)) = seen.iter().find(|(_, held)| *held == written) {
                failing.push(format!("{name}: {each} and {other} are both {written}"));
            }
            seen.push((each, written));
        }
    }
    assert!(failing.is_empty(), "{}", failing.join("\n"));
    // What waits is told by its italics too, whatever the theme.
    let scratch = Scratch::new("contrast-italics");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.theme_chosen = Some("samplekit-dark".to_string());
    workbench.themed();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "alloy");
    keys(&mut workbench, "enickel\n\u{1b}");
    let buffer = buffer_of(&mut workbench, 100, 30);
    let (x, y) = found(&buffer, "nickel");
    assert_eq!(buffer[(x, y)].fg, workbench.theme.waiting);
    assert!(
        buffer[(x, y)]
            .modifier
            .contains(ratatui::style::Modifier::ITALIC)
    );
}

#[test]
fn a_terminal_drawing_sixteen_colours_defaults_to_them() {
    use samplekit::tui::theme::draws_every_colour;
    for every in ["truecolor", "24bit", "TrueColor", " truecolor "] {
        assert!(draws_every_colour(Some(every), false), "{every}");
    }
    // Windows Terminal draws every colour and does not say it.
    assert!(draws_every_colour(None, true));
    for sixteen in [Some("256color"), Some(""), Some("yes"), None] {
        assert!(!draws_every_colour(sixteen, false), "{sixteen:?}");
    }
}

#[test]
fn the_default_theme_is_dark_unless_colorfgbg_says_light() {
    use samplekit::tui::theme::default_theme;
    for light in ["0;15", "0;7", "12;9", "0;default;15"] {
        assert_eq!(default_theme(Some(light)), "samplekit-light", "{light}");
    }
    for dark in ["15;0", "7;8", "default", ""] {
        assert_eq!(default_theme(Some(dark)), "samplekit-dark", "{dark}");
    }
    assert_eq!(default_theme(None), "samplekit-dark");
}

#[test]
fn ones_own_themes_are_read_from_the_themes_folder() {
    use samplekit::tui::theme::{BUILT_IN, Themes};
    let scratch = Scratch::new("own-themes");
    let folder = scratch.0.join("themes");
    fs::create_dir_all(&folder).unwrap();
    for (name, text) in [
        ("mine", "base = \"nord\"\n[colors]\naccent = \"#ff79c6\"\n"),
        ("plain", "[colors]\nforeground = \"#eeeeee\"\n"),
        ("above", "base = \"mine\"\n[colors]\nkey = \"#00ff00\"\n"),
        ("broken", "[colors\n"),
        ("stray", "base = \"nrod\"\n"),
        ("nord", "[colors]\naccent = \"#000000\"\n"),
        ("one", "base = \"two\"\n"),
        ("two", "base = \"one\"\n"),
    ] {
        fs::write(folder.join(format!("{name}.toml")), text).unwrap();
    }
    fs::write(folder.join("notes.txt"), "not a theme").unwrap();
    let themes = Themes::read(Some(&folder));
    let names = themes.names();
    assert_eq!(names[..BUILT_IN.len()], *BUILT_IN);
    assert_eq!(names[BUILT_IN.len()..], ["above", "mine", "plain"]);
    assert!(themes.own("mine") && !themes.own("nord"));
    let nord = themes.theme("nord", None).unwrap();
    let mine = themes.theme("mine", None).unwrap();
    assert_eq!(mine.accent, ratatui::style::Color::Rgb(0xff, 0x79, 0xc6));
    assert_eq!(
        (mine.background, mine.failed),
        (nord.background, nord.failed)
    );
    // A base of one's own is followed.
    let above = themes.theme("above", None).unwrap();
    assert_eq!(
        (above.accent, above.background),
        (mine.accent, nord.background)
    );
    // No base: SampleKit's dark.
    let dark = themes.theme("samplekit-dark", None).unwrap();
    let plain = themes.theme("plain", None).unwrap();
    assert_eq!(plain.background, dark.background);
    assert_ne!(plain.foreground, dark.foreground);
    // Each left out, said.
    let said = themes.said().join("\n");
    assert_eq!(themes.said().len(), 5, "{said}");
    for (name, why) in [
        ("broken.toml", ""),
        ("stray", "did you mean nord?"),
        ("nord.toml", "a theme SampleKit has"),
        ("one", "lead back to it"),
        ("two", "lead back to it"),
    ] {
        assert!(
            themes
                .said()
                .iter()
                .any(|line| line.contains(name) && line.contains(why)),
            "{name}: {said}"
        );
    }
    // A folder that is not there is no theme, and nothing to say.
    assert!(
        Themes::read(Some(&scratch.0.join("none")))
            .said()
            .is_empty()
    );
    // The colours page's example, a theme as one writes it.
    let example = Themes::read(Some(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/templates/themes"),
    ));
    assert!(example.said().is_empty(), "{:?}", example.said());
    let mine = example.theme("my-nord", None).unwrap();
    assert_eq!(mine.background, nord.background);
    assert_ne!(mine.accent, nord.accent);
}

#[test]
fn the_theme_in_force_is_the_folders_then_the_projects_then_the_last_then_the_default() {
    use samplekit::tui::theme::{Choices, Themes};
    let scratch = Scratch::new("theme-order");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[colors]\ntheme = \"dracula\"\naccent = \"#123456\"\n",
    )
    .unwrap();
    let named = samplekit::config::project_config::load(&scratch.0.join(".samplekitrc")).unwrap();
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[colors]\naccent = \"#123456\"\n",
    )
    .unwrap();
    let unnamed = samplekit::config::project_config::load(&scratch.0.join(".samplekitrc")).unwrap();
    let themes = Themes::read(None);
    let accent = ratatui::style::Color::Rgb(0x12, 0x34, 0x56);
    let resolved = |folder: Option<&str>, config, last: Option<&str>| {
        let (name, theme, said) = themes.resolve(
            Choices {
                folder,
                last,
                default: "samplekit-light",
            },
            Some(config),
        );
        // The project's roles over each.
        assert_eq!(theme.accent, accent, "{name}");
        assert_eq!(
            theme.background,
            themes.theme(&name, None).unwrap().background
        );
        (name, said)
    };
    let last = Some("gruvbox-dark");
    assert_eq!(resolved(Some("nord"), &named, last).0, "nord");
    assert_eq!(resolved(None, &named, last).0, "dracula");
    assert_eq!(resolved(None, &unnamed, last).0, "gruvbox-dark");
    assert_eq!(resolved(None, &unnamed, None).0, "samplekit-light");
    // A name no theme bears, said, and the next taken.
    let (name, said) = resolved(Some("nordic"), &named, last);
    assert_eq!(name, "dracula");
    assert!(
        said.len() == 1
            && said[0].contains("chosen in this folder, nordic")
            && said[0].contains("did you mean nord?")
            && said[0].ends_with("dracula instead"),
        "{said:?}"
    );
}

/// A cell's width and height in a picture, and the baseline of its text.
const CELL: (u16, u16, u16) = (9, 18, 13);

/// A colour as a picture writes it: `#rrggbb`, or `default` for one left to
/// the terminal.
fn hex(color: ratatui::style::Color) -> String {
    match color {
        ratatui::style::Color::Rgb(red, green, blue) => format!("#{red:02x}{green:02x}{blue:02x}"),
        _ => panic!("{color:?} is no #rrggbb"),
    }
}

/// A frame as an SVG picture: its rows on the theme's background.
fn picture(buffer: &ratatui::buffer::Buffer, background: &str, foreground: &str) -> String {
    let (columns, rows) = (buffer.area.width, buffer.area.height);
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" \
         viewBox=\"0 0 {w} {h}\" {FONT}>\n<rect width=\"{w}\" height=\"{h}\" \
         fill=\"{background}\"/>\n",
        w = columns * CELL.0,
        h = rows * CELL.1,
    );
    for row in picture_rows(buffer, background, foreground) {
        svg.push_str(&row);
    }
    svg.push_str("</svg>\n");
    svg
}

/// The font of every picture, and the spaces of its text kept: a browser
/// collapses them otherwise.
const FONT: &str = "font-family=\"'DejaVu Sans Mono', Menlo, Consolas, monospace\" \
                    font-size=\"15\" xml:space=\"preserve\" style=\"white-space:pre\"";

/// A frame's rows as SVG, each at its place: a rectangle per run of cells
/// sharing a background other than the theme's, a text per run sharing a
/// style, and the frames' lines; each colour as the terminal would draw it,
/// the theme's own background and text for one left to it. One row's
/// elements are one string, so that frames sharing a row share it.
fn picture_rows(
    buffer: &ratatui::buffer::Buffer,
    background: &str,
    foreground: &str,
) -> Vec<String> {
    use ratatui::style::{Color, Modifier};
    let (width, height, baseline) = CELL;
    let resolve = |color: Color, default: &str| -> String {
        match color {
            Color::Rgb(red, green, blue) => format!("#{red:02x}{green:02x}{blue:02x}"),
            Color::Reset => default.to_string(),
            // The typing badge's text, on the accent: the theme's background.
            Color::Black => background.to_string(),
            other => panic!(
                "the view draws {other:?} literally: give it a colour of the theme in picture()"
            ),
        }
    };
    let escaped = |text: &str| {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let drawn = Modifier::BOLD | Modifier::ITALIC | Modifier::UNDERLINED | Modifier::REVERSED;
    // Each cell's colours and the style its text is drawn in.
    let cell = |x: u16, y: u16| {
        let cell = &buffer[(x, y)];
        assert!(
            drawn.contains(cell.modifier),
            "the view draws {:?}: give it a form in picture()",
            cell.modifier
        );
        let (mut fg, mut bg) = (resolve(cell.fg, foreground), resolve(cell.bg, background));
        if cell.modifier.contains(Modifier::REVERSED) {
            std::mem::swap(&mut fg, &mut bg);
        }
        let mut attributes = format!(" fill=\"{fg}\"");
        if cell.modifier.contains(Modifier::BOLD) {
            attributes.push_str(" font-weight=\"bold\"");
        }
        if cell.modifier.contains(Modifier::ITALIC) {
            attributes.push_str(" font-style=\"italic\"");
        }
        if cell.modifier.contains(Modifier::UNDERLINED) {
            attributes.push_str(" text-decoration=\"underline\"");
        }
        // A frame's line is drawn as a line, not a glyph: no font joins
        // them from one row to the next.
        let blank = (cell.symbol() == " " && !cell.modifier.contains(Modifier::UNDERLINED))
            || arms(cell.symbol()).is_some();
        (bg, attributes, blank, cell.symbol().to_string(), fg)
    };
    // The arms of a frame's character: left, right, up, down.
    fn arms(symbol: &str) -> Option<[bool; 4]> {
        Some(match symbol {
            "─" => [true, true, false, false],
            "│" => [false, false, true, true],
            "┌" => [false, true, false, true],
            "┐" => [true, false, false, true],
            "└" => [false, true, true, false],
            "┘" => [true, false, true, false],
            "├" => [false, true, true, true],
            "┤" => [true, false, true, true],
            "┬" => [true, true, false, true],
            "┴" => [true, true, true, false],
            "┼" => [true, true, true, true],
            _ => return None,
        })
    }
    let (columns, rows) = (buffer.area.width, buffer.area.height);
    let mut drawn_rows = Vec::new();
    for y in 0..rows {
        let mut svg = String::new();
        // The backgrounds first, so that the text is drawn over them.
        let mut x = 0;
        while x < columns {
            let (bg, ..) = cell(x, y);
            let mut end = x + 1;
            while end < columns && cell(end, y).0 == bg {
                end += 1;
            }
            if bg != background {
                svg.push_str(&format!(
                    "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{height}\" fill=\"{bg}\"/>\n",
                    x * width,
                    y * height,
                    (end - x) * width
                ));
            }
            x = end;
        }
        // A run of text: the cells from one drawn to the last drawn in its
        // style, the blanks between them its spaces.
        let mut x = 0;
        while x < columns {
            let (_, style, blank, ..) = cell(x, y);
            if blank {
                x += 1;
                continue;
            }
            // Two blanks end it: a browser collapses spaces, and a run
            // spanning a gap was stretched over it, its letters huge.
            let mut last = x;
            let mut next = x + 1;
            while next < columns {
                let (_, other, blank, ..) = cell(next, y);
                if !blank && other != style {
                    break;
                }
                if blank && next > last + 1 {
                    break;
                }
                if !blank {
                    last = next;
                }
                next += 1;
            }
            let text: String = (x..=last)
                .map(|at| match cell(at, y) {
                    (_, _, true, symbol, _) if symbol != " " => " ".to_string(),
                    (_, _, _, symbol, _) => symbol,
                })
                .collect();
            svg.push_str(&format!(
                "<text x=\"{}\" y=\"{}\" textLength=\"{}\" lengthAdjust=\"spacing\"{style}>{}</text>\n",
                x * width,
                y * height + baseline,
                (last - x + 1) * width,
                escaped(&text)
            ));
            x = last + 1;
        }
        // The frames' lines, one path per colour, each a half cell from the
        // middle of its cell: a horizontal run of them drawn as one segment.
        let mut paths: Vec<(String, String)> = Vec::new();
        for x in 0..columns {
            let (_, _, _, symbol, fg) = cell(x, y);
            let Some([left, right, up, down]) = arms(&symbol) else {
                continue;
            };
            let (middle_x, middle_y) = (x * width * 2 + width, y * height * 2 + height);
            let mut segments = String::new();
            // Coordinates doubled, so that a half cell is a whole number:
            // halved again by the path's transform.
            if left || right {
                let from = if left { x * width * 2 } else { middle_x };
                let to = if right { (x + 1) * width * 2 } else { middle_x };
                segments.push_str(&format!("M{from} {middle_y}H{to}"));
            }
            if up || down {
                let from = if up { y * height * 2 } else { middle_y };
                let to = if down { (y + 1) * height * 2 } else { middle_y };
                segments.push_str(&format!("M{middle_x} {from}V{to}"));
            }
            match paths.iter_mut().find(|(colour, _)| *colour == fg) {
                Some((_, held)) => held.push_str(&segments),
                None => paths.push((fg, segments)),
            }
        }
        for (colour, segments) in paths {
            svg.push_str(&format!(
                "<path transform=\"scale(0.5)\" stroke=\"{colour}\" stroke-width=\"2\" fill=\"none\" d=\"{segments}\"/>\n"
            ));
        }
        drawn_rows.push(svg);
    }
    drawn_rows
}

/// A folder copied, what a run leaves beside the demo left out.
fn copied(from: &std::path::Path, to: &std::path::Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let name = entry.file_name();
        if [".samplekit", ".venv", "__pycache__", "out"]
            .iter()
            .any(|left| name == *left)
        {
            continue;
        }
        if entry.file_type().unwrap().is_dir() {
            copied(&entry.path(), &to.join(&name));
        } else {
            fs::copy(entry.path(), to.join(&name)).unwrap();
        }
    }
}

#[test]
fn the_colours_pages_pictures_are_drawn_by_the_tui() {
    use samplekit::config::project_config::COLOR_ROLES;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let pictures = root.join("docs/images/colours");
    let mut themes: Vec<PathBuf> = fs::read_dir(root.join("docs/templates/colours"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect();
    themes.sort();
    assert!(!themes.is_empty());
    let writing = std::env::var_os("SAMPLEKIT_WRITE_REFERENCE").is_some();
    let mut differing = Vec::new();
    for theme in &themes {
        let stem = theme.file_stem().unwrap().to_str().unwrap();
        let written = fs::read_to_string(theme).unwrap();
        // Every role, in the theme's own colours, its background and its text
        // among them.
        let table: toml::Table = written.parse().unwrap();
        let colors = table["colors"].as_table().unwrap();
        for (role, _) in COLOR_ROLES {
            let colour = colors
                .get(*role)
                .and_then(toml::Value::as_str)
                .unwrap_or_else(|| panic!("{stem} sets no {role}"));
            assert!(
                colour.len() == 7 && colour.starts_with('#'),
                "{stem}: {role} = {colour}"
            );
        }
        assert_eq!(colors.len(), COLOR_ROLES.len(), "{stem}: {colors:?}");
        let background = colors["background"].as_str().unwrap().to_string();
        let foreground = colors["foreground"].as_str().unwrap().to_string();

        // The demo's last step, in a folder of its own, the theme chosen by
        // its name: the built-in one, the file embedded.
        let scratch = Scratch::new(&format!("colours-{stem}"));
        let project = scratch.0.join("tensile");
        copied(&root.join("examples/tensile/04-results"), &project);
        let mut workbench = Workbench::open(&project).unwrap();
        workbench.theme_chosen = Some(stem.to_string());
        assert!(
            workbench.themed().is_empty(),
            "{stem} is a theme SampleKit has"
        );
        assert_eq!(
            workbench.theme.accent,
            samplekit::tui::theme::color(colors["accent"].as_str().unwrap()),
            "{stem}: {}",
            workbench.message
        );
        // On steel-01, its width typed again: the values it feeds outdated,
        // the change waiting, said in the message.
        open_named(&mut workbench, "steel-01");
        move_to(&mut workbench, "width");
        keys(&mut workbench, "e12.6\n\u{1b}");
        let mut terminal = Terminal::new(TestBackend::new(100, 28)).unwrap();
        terminal
            .draw(|frame| samplekit::tui::view::draw(frame, &mut workbench))
            .unwrap();
        let svg = picture(terminal.backend().buffer(), &background, &foreground);
        assert!(svg.contains("steel-01"), "{stem}");
        assert!(
            !svg.contains(&*scratch.0.to_string_lossy()) && !svg.contains("samplekit-workbench"),
            "{stem}: the picture names the folder drawn"
        );
        let path = pictures.join(format!("{stem}.svg"));
        if writing {
            fs::create_dir_all(&pictures).unwrap();
            fs::write(&path, &svg).unwrap();
        } else if fs::read_to_string(&path).unwrap_or_default() != svg {
            differing.push(format!("docs/images/colours/{stem}.svg"));
        }
    }
    // A picture whose theme is gone goes with it.
    for picture in fs::read_dir(&pictures).into_iter().flatten().flatten() {
        let stem = picture.path().file_stem().unwrap().to_owned();
        assert!(
            themes.iter().any(|theme| theme.file_stem() == Some(&stem)),
            "{} has no theme in docs/templates/colours",
            picture.path().display()
        );
    }
    assert!(
        differing.is_empty(),
        "{} not what the TUI draws: run\n  \
         SAMPLEKIT_WRITE_REFERENCE=1 cargo test --test tui_view the_colours_pages_pictures\n\
         and commit them",
        differing.join(", ")
    );
}

/// The demo's results step, copied into `scratch`, opened in `theme`.
fn demo_in(scratch: &Scratch, theme: &str) -> Workbench {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let project = scratch.0.join("tensile");
    copied(&root.join("examples/tensile/04-results"), &project);
    let mut workbench = Workbench::open(&project).unwrap();
    workbench.theme_chosen = Some(theme.to_string());
    assert!(workbench.themed().is_empty(), "{theme}");
    workbench
}

/// The project's folder where a screen says it — the collection's title —
/// written as the reader's own would be, `~/tensile`, not the scratch folder
/// it was drawn in: its cells written over, the same style kept, the rest
/// of the folder's room left blank.
fn at_home(buffer: &mut ratatui::buffer::Buffer, folder: &std::path::Path) {
    let whole: Vec<String> = folder
        .display()
        .to_string()
        .chars()
        .map(String::from)
        .collect();
    // The folder as a line shows it: whole, or its end after `…` where the
    // line is too narrow for it, as a long temporary folder is.
    let shown: Vec<Vec<String>> = std::iter::once(whole.clone())
        .chain((1..whole.len().saturating_sub(8)).map(|from| {
            std::iter::once("…".to_string())
                .chain(whole[from..].iter().cloned())
                .collect()
        }))
        .collect();
    let (width, height) = (buffer.area.width as usize, buffer.area.height);
    for y in 0..height {
        let Some((at, folder)) = shown.iter().find_map(|folder| {
            (0..=width.saturating_sub(folder.len()))
                .find(|&x| {
                    (0..folder.len()).all(|i| buffer[((x + i) as u16, y)].symbol() == folder[i])
                })
                .map(|at| (at, folder))
        }) else {
            continue;
        };
        for (offset, character) in format!("{:<1$}", "~/tensile", folder.len())
            .chars()
            .enumerate()
        {
            buffer[((at + offset) as u16, y)].set_char(character);
        }
    }
}

/// The screen drawn at `width` × `height`, as a picture in its theme.
fn drawn_picture(workbench: &mut Workbench, width: u16, height: u16) -> String {
    let mut buffer = buffer_of(workbench, width, height);
    at_home(&mut buffer, &workbench.root);
    picture(
        &buffer,
        &hex(workbench.theme.background),
        &hex(workbench.theme.foreground),
    )
}

/// `svg` checked against the file committed at `path`, under the code's
/// folder, or written there with `SAMPLEKIT_WRITE_REFERENCE`: what differs
/// is added to `differing`. A picture names no folder it was drawn in.
fn committed(path: &str, svg: &str, differing: &mut Vec<String>) {
    assert!(
        !svg.contains("samplekit-workbench")
            && !svg.contains(&*std::env::temp_dir().to_string_lossy()),
        "{path} names the folder it was drawn in:\n{}",
        svg.lines()
            .filter(|line| line.contains("samplekit-workbench") || line.contains("/tmp"))
            .collect::<Vec<_>>()
            .join("\n")
    );
    let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    if std::env::var_os("SAMPLEKIT_WRITE_REFERENCE").is_some() {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, svg).unwrap();
    } else if fs::read_to_string(&file).unwrap_or_default() != svg {
        differing.push(path.to_string());
    }
}

fn none_differ(differing: &[String], test: &str) {
    assert!(
        differing.is_empty(),
        "{} not what the TUI draws: run\n  \
         SAMPLEKIT_WRITE_REFERENCE=1 cargo test --test tui_view {test}\n\
         and commit them",
        differing.join(", ")
    );
}

/// Pictures side by side, two to a row, each under its title: one SVG.
fn tiled(tiles: &[(&str, String)], background: &str, title: &str) -> String {
    let size = |svg: &str, attribute: &str| -> u32 {
        let at = svg.find(&format!(" {attribute}=\"")).unwrap() + attribute.len() + 3;
        svg[at..at + svg[at..].find('"').unwrap()].parse().unwrap()
    };
    let (width, height) = (size(&tiles[0].1, "width"), size(&tiles[0].1, "height"));
    let (gap, head) = (18, 28);
    let across = 2;
    let down = tiles.len().div_ceil(across) as u32;
    let total = (
        across as u32 * width + gap,
        down * (height + head) + gap / 2,
    );
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" \
         viewBox=\"0 0 {w} {h}\" {FONT}>\n<rect width=\"{w}\" height=\"{h}\" \
         fill=\"{background}\"/>\n",
        w = total.0,
        h = total.1,
    );
    for (at, (name, tile)) in tiles.iter().enumerate() {
        let (x, y) = (
            (at % across) as u32 * (width + gap),
            (at / across) as u32 * (height + head),
        );
        let body = &tile[tile.find("<rect").unwrap()..tile.rfind("</svg>").unwrap()];
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" fill=\"{title}\" font-weight=\"bold\">{name}</text>\n\
             <svg x=\"{x}\" y=\"{}\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n{body}</svg>\n",
            x + 2,
            y + 19,
            y + head,
        ));
    }
    svg.push_str("</svg>\n");
    svg
}

#[test]
fn the_readmes_pictures_are_drawn_by_the_tui() {
    let mut differing = Vec::new();
    // The sample the README shows, the very file its yaml block holds, read
    // from the README: in the demo's second step, whose model takes each
    // size's mean, computed, what it gives waiting to be written.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let readme = fs::read_to_string(root.join("README.md")).unwrap();
    let file = readme
        .split("```yaml\n")
        .nth(1)
        .and_then(|after| after.split("```").next())
        .expect("the README shows a sample's file");
    let scratch = Scratch::new("readme-copper");
    let project = scratch.0.join("tensile/02-readings");
    copied(&root.join("examples/tensile/02-readings"), &project);
    // The model runs in the repository's environment, named by its path.
    let settings = project.join(".samplekitrc");
    let named = fs::read_to_string(&settings).unwrap().replace(
        "python = \"../.venv\"",
        &format!("python = '{}'", root.join(".venv").display()),
    );
    assert!(named.contains(".venv'"), "{named}");
    fs::write(&settings, named).unwrap();
    fs::write(project.join("samples/copper-01.md"), file).unwrap();
    let mut workbench = Workbench::open(&project).unwrap();
    workbench.theme_chosen = Some("samplekit-dark".to_string());
    open_named(&mut workbench, "copper-01");
    keys(&mut workbench, "Cy");
    for _ in 0..600 {
        workbench.tick();
        if workbench
            .run
            .as_ref()
            .is_some_and(|run| run.outcome.is_some())
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    // The outcome's sentence counts what ran, as any run says it: the
    // status line keeps to what is shown.
    workbench.run = None;
    workbench.message.clear();
    let svg = drawn_picture(&mut workbench, 100, 18);
    assert!(
        svg.contains("copper-01") && svg.contains("12.5043"),
        "{svg}"
    );
    assert!(svg.contains(&hex(workbench.theme.waiting)));
    committed("docs/images/readme/copper-01.svg", &svg, &mut differing);

    // The collection.
    let scratch = Scratch::new("readme-collection");
    let mut workbench = demo_in(&scratch, "samplekit-dark");
    let svg = drawn_picture(&mut workbench, 100, 12);
    assert!(svg.contains("aluminium-01") && svg.contains("steel-01"));
    committed("docs/images/readme/collection.svg", &svg, &mut differing);

    // A value's card.
    let scratch = Scratch::new("readme-card");
    let mut workbench = demo_in(&scratch, "samplekit-dark");
    open_named(&mut workbench, "steel-01");
    move_to(&mut workbench, "area");
    keys(&mut workbench, "i");
    let svg = drawn_picture(&mut workbench, 100, 28);
    assert!(svg.contains("area · its card"), "{svg}");
    committed("docs/images/readme/card.svg", &svg, &mut differing);

    // A table.
    let scratch = Scratch::new("readme-table");
    let mut workbench = demo_in(&scratch, "samplekit-dark");
    open_named(&mut workbench, "steel-01");
    move_to(&mut workbench, "curve");
    keys(&mut workbench, "\n");
    let svg = drawn_picture(&mut workbench, 100, 28);
    assert!(svg.contains("strain"), "{svg}");
    committed("docs/images/readme/table.svg", &svg, &mut differing);

    // The typing mode on the collection: a column typed for a sample.
    let scratch = Scratch::new("readme-typing");
    let mut workbench = demo_in(&scratch, "samplekit-dark");
    keys(&mut workbench, "e");
    workbench.key(Key::Right);
    keys(&mut workbench, "bronz");
    let svg = drawn_picture(&mut workbench, 100, 12);
    assert!(svg.contains("TYPING"), "{svg}");
    committed("docs/images/readme/typing.svg", &svg, &mut differing);

    // A few of the themes, on the same screen.
    let mut tiles = Vec::new();
    for theme in [
        "samplekit-dark",
        "samplekit-light",
        "nord",
        "catppuccin-latte",
    ] {
        let scratch = Scratch::new(&format!("readme-theme-{theme}"));
        let mut workbench = demo_in(&scratch, theme);
        open_named(&mut workbench, "steel-01");
        tiles.push((theme, drawn_picture(&mut workbench, 96, 20)));
    }
    let svg = tiled(&tiles, "#ffffff00", "#808890");
    committed("docs/images/readme/themes.svg", &svg, &mut differing);
    none_differ(&differing, "the_readmes_pictures");
}

/// Frames shown in turn, one SVG: each row drawn once and used by every
/// frame showing it at its place; each frame a group, visible for its
/// `ticks` of `TICK` seconds by one `animate` of its visibility, looping,
/// the first visible where nothing animates. Under each, a caption: the key
/// pressed, then what it did.
fn animation(
    frames: &[(ratatui::buffer::Buffer, &str, &str, u32)],
    theme: &samplekit::tui::theme::Theme,
) -> String {
    const TICK: f64 = 1.2;
    const CAPTION: u16 = 26;
    let (background, foreground) = (hex(theme.background), hex(theme.foreground));
    let (columns, rows) = (frames[0].0.area.width, frames[0].0.area.height);
    let (width, height) = (columns * CELL.0, rows * CELL.1 + CAPTION);
    let mut defined: Vec<String> = Vec::new();
    let mut shown: Vec<Vec<usize>> = Vec::new();
    for (buffer, ..) in frames {
        let mut uses = Vec::new();
        for row in picture_rows(buffer, &background, &foreground) {
            if row.is_empty() {
                continue;
            }
            let at = match defined.iter().position(|held| *held == row) {
                Some(at) => at,
                None => {
                    defined.push(row);
                    defined.len() - 1
                }
            };
            uses.push(at);
        }
        shown.push(uses);
    }
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
         viewBox=\"0 0 {width} {height}\" {FONT}>\n<rect width=\"{width}\" height=\"{height}\" \
         fill=\"{background}\"/>\n<defs>\n"
    );
    for (at, row) in defined.iter().enumerate() {
        svg.push_str(&format!("<g id=\"r{at}\">\n{row}</g>\n"));
    }
    svg.push_str("</defs>\n");
    let total: u32 = frames.iter().map(|(.., ticks)| ticks).sum();
    let mut begun = 0;
    let line = rows * CELL.1;
    for (at, ((_, key, said, ticks), uses)) in frames.iter().zip(&shown).enumerate() {
        let (from, to) = (
            f64::from(begun) / f64::from(total),
            f64::from(begun + ticks) / f64::from(total),
        );
        begun += ticks;
        // Discrete: hidden until its time, visible through it, hidden after.
        let (values, times) = if at == 0 {
            ("visible;hidden".to_string(), format!("0;{to:.4}"))
        } else if at == frames.len() - 1 {
            ("hidden;visible".to_string(), format!("0;{from:.4}"))
        } else {
            (
                "hidden;visible;hidden".to_string(),
                format!("0;{from:.4};{to:.4}"),
            )
        };
        svg.push_str(&format!(
            "<g visibility=\"{}\">\n<animate attributeName=\"visibility\" values=\"{values}\" \
             keyTimes=\"{times}\" dur=\"{:.1}s\" calcMode=\"discrete\" repeatCount=\"indefinite\"/>\n",
            if at == 0 { "visible" } else { "hidden" },
            f64::from(total) * TICK,
        ));
        for used in uses {
            svg.push_str(&format!("<use href=\"#r{used}\"/>\n"));
        }
        svg.push_str(&format!(
            "<line x1=\"0\" y1=\"{y}\" x2=\"{width}\" y2=\"{y}\" stroke=\"{muted}\"/>\n\
             <text x=\"{x}\" y=\"{baseline}\"><tspan fill=\"{key_colour}\" font-weight=\"bold\">{key}</tspan>\
             <tspan fill=\"{foreground}\">  {said}</tspan></text>\n</g>\n",
            y = line + 1,
            muted = hex(theme.muted),
            x = CELL.0,
            baseline = line + 19,
            key_colour = hex(theme.key),
        ));
    }
    svg.push_str("</svg>\n");
    svg
}

// Drawn as on Linux and macOS: Windows shows a written file's path with its
// own separator, which the committed picture does not.
#[test]
#[cfg(unix)]
fn the_animation_is_drawn_frame_by_frame_by_the_tui() {
    let scratch = Scratch::new("at-work");
    let mut workbench = demo_in(&scratch, "samplekit-dark");
    let mut frames: Vec<(ratatui::buffer::Buffer, &str, &str, u32)> = Vec::new();
    let mut frame = |workbench: &mut Workbench, key, said, ticks| {
        let mut buffer = buffer_of(workbench, 100, 28);
        at_home(&mut buffer, &workbench.root.clone());
        frames.push((buffer, key, said, ticks));
    };
    frame(
        &mut workbench,
        "samplekit",
        "the terminal interface, on the demo's specimens",
        3,
    );
    keys(&mut workbench, "j");
    frame(&mut workbench, "j", "down to copper-01", 1);
    keys(&mut workbench, "\n");
    frame(&mut workbench, "Enter", "opens it: its values, its note", 2);
    move_to(&mut workbench, "width");
    frame(&mut workbench, "j", "down to its width", 1);
    keys(&mut workbench, "e");
    frame(
        &mut workbench,
        "e",
        "the typing mode: what is typed replaces the cell",
        2,
    );
    keys(&mut workbench, "12.6 +- 0.02");
    frame(
        &mut workbench,
        "12.6 +- 0.02",
        "a value and its uncertainty, in one cell",
        2,
    );
    keys(&mut workbench, "\n");
    frame(
        &mut workbench,
        "Enter",
        "kept, waiting to be written: what it feeds is outdated at once",
        3,
    );
    keys(&mut workbench, "\u{1b}");
    workbench.key(Key::Down);
    keys(&mut workbench, "i");
    frame(
        &mut workbench,
        "i",
        "area's card: what it reads, and what reads it",
        3,
    );
    keys(&mut workbench, "\u{1b}w");
    frame(
        &mut workbench,
        "w",
        "every change waiting, previewed before it is written",
        3,
    );
    keys(&mut workbench, "y");
    frame(
        &mut workbench,
        "y",
        "written: one change of the project's history, u takes it back",
        3,
    );
    let svg = animation(&frames, &workbench.theme);
    // Each frame is one the TUI drew.
    assert!(svg.contains("12.6 +- 0.02") && svg.contains("area · its card"));
    assert_eq!(svg.matches("<animate ").count(), frames.len());
    assert!(svg.len() < 500_000, "{} bytes", svg.len());
    let mut differing = Vec::new();
    committed("docs/images/readme/at-work.svg", &svg, &mut differing);
    none_differ(&differing, "the_animation_is_drawn");
}

/// The terminal the command line is drawn at work in: its columns, which
/// the commands are told, as wide as the widest line they print so that
/// none wraps, and its rows.
#[cfg(target_os = "linux")]
const TERMINAL: (u16, u16) = (137, 28);

/// A character on a terminal's screen, in the style it is drawn in.
#[cfg(target_os = "linux")]
type Drawn = (char, ratatui::style::Style);

/// What a command printed to a terminal, as the terminal shows it: a line per
/// line, each character in its style. The terminal's sixteen colours are
/// drawn as `theme` draws the role the command line paints in each by
/// default; its own colour is `foreground`, bold in `value`'s, faint in
/// `muted`'s. A carriage return goes back to the line's start and an erase
/// clears it, as a computation draws its progress then clears it. A line
/// not ended yet, a question waiting for its answer, is the last.
#[cfg(target_os = "linux")]
fn on_screen(printed: &str, theme: &samplekit::tui::theme::Theme) -> Vec<Vec<Drawn>> {
    use ratatui::style::{Modifier, Style};
    use unicode_width::UnicodeWidthChar;
    let mut lines = Vec::new();
    let mut line: Vec<Drawn> = Vec::new();
    let mut column = 0;
    let (mut bold, mut faint, mut underlined) = (false, false, false);
    let mut colour = None;
    let mut characters = printed.chars();
    while let Some(character) = characters.next() {
        match character {
            '\u{1b}' => {
                assert_eq!(characters.next(), Some('['), "an escape no picture draws");
                let mut parameters = String::new();
                let command = loop {
                    match characters.next() {
                        Some(digit) if digit.is_ascii_digit() || digit == ';' => {
                            parameters.push(digit)
                        }
                        Some(command) => break command,
                        None => panic!("an escape cut short"),
                    }
                };
                match command {
                    'm' => {
                        for code in parameters.split(';') {
                            match code {
                                "" | "0" => {
                                    (bold, faint, underlined, colour) = (false, false, false, None)
                                }
                                "1" => bold = true,
                                "2" => faint = true,
                                "4" => underlined = true,
                                "22" => (bold, faint) = (false, false),
                                "24" => underlined = false,
                                "39" => colour = None,
                                // The roles the command line paints in each
                                // colour when the project's `[colors]` names
                                // none: `error:`, a value current or written,
                                // outdated, an attribute, edited, a unit,
                                // what is muted.
                                "31" => colour = Some(theme.error),
                                "32" => colour = Some(theme.current),
                                "33" => colour = Some(theme.outdated),
                                "34" => colour = Some(theme.attribute),
                                "35" => colour = Some(theme.edited),
                                "36" => colour = Some(theme.unit),
                                "90" => colour = Some(theme.muted),
                                other => panic!(
                                    "the command line paints {other}: give it a colour in on_screen()"
                                ),
                            }
                        }
                    }
                    'K' if parameters == "2" => {
                        line.clear();
                        column = 0;
                    }
                    'K' if parameters.is_empty() || parameters == "0" => line.truncate(column),
                    other => panic!("an escape no picture draws: {parameters}{other}"),
                }
            }
            '\r' => column = 0,
            '\n' => {
                lines.push(std::mem::take(&mut line));
                column = 0;
            }
            _ => {
                assert_eq!(
                    character.width(),
                    Some(1),
                    "{character:?} is not one cell wide: place it in on_screen()"
                );
                let own = if faint {
                    theme.muted
                } else if bold {
                    theme.value
                } else {
                    theme.foreground
                };
                let mut style = Style::default().fg(colour.unwrap_or(own));
                if bold {
                    style = style.add_modifier(Modifier::BOLD);
                }
                if underlined {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                match line.get_mut(column) {
                    Some(held) => *held = (character, style),
                    None => line.push((character, style)),
                }
                column += 1;
            }
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// A command typed in `folder` at a terminal [`TERMINAL`] columns wide, its
/// colours on, read by the shell as it is written, the binary in place of
/// `samplekit`; each of `answers` typed once its question is asked, a line
/// ending `[y/N] `, as a person types it. What the terminal was sent, the
/// answers echoed in it, where each answer was typed in it, and the
/// command's exit code.
#[cfg(target_os = "linux")]
fn at_a_terminal(
    folder: &std::path::Path,
    typed: &str,
    answers: &[&str],
) -> (String, Vec<usize>, i32) {
    use std::io::{Read, Write};
    let words = typed
        .strip_prefix("samplekit ")
        .expect("a samplekit command");
    let command = format!(
        "stty cols {} rows 60; exec '{}' {words}",
        TERMINAL.0,
        env!("CARGO_BIN_EXE_samplekit")
    );
    let mut child = std::process::Command::new("script")
        .args(["--quiet", "--return", "--command", &command, "/dev/null"])
        .current_dir(folder)
        .env("SHELL", "/bin/sh")
        .env("TERM", "xterm-256color")
        .env("COLUMNS", TERMINAL.0.to_string())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR")
        .env_remove("CLICOLOR_FORCE")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("script creates a pseudoterminal");
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let reader = {
        let seen = std::sync::Arc::clone(&seen);
        let mut stream = child.stdout.take().unwrap();
        std::thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            while let Ok(read) = stream.read(&mut buffer) {
                if read == 0 {
                    break;
                }
                seen.lock().unwrap().extend_from_slice(&buffer[..read]);
            }
        })
    };
    let mut stdin = child.stdin.take().unwrap();
    let mut asked = Vec::new();
    for answer in answers {
        // The question asked, and nothing typed before it: typed sooner,
        // the terminal would echo the answer wherever the output stood.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        // A question after the last one answered, not that one again.
        let after = asked.last().map_or(0, |last| last + answer.len());
        let at = loop {
            let printed = seen.lock().unwrap();
            if printed.len() > after && printed.ends_with(b"[y/N] ") {
                break printed.len();
            }
            drop(printed);
            assert!(
                std::time::Instant::now() < deadline,
                "{typed} asks nothing for {answer:?} to answer:\n{}",
                String::from_utf8_lossy(&seen.lock().unwrap())
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        asked.push(at);
        stdin.write_all(format!("{answer}\n").as_bytes()).unwrap();
    }
    drop(stdin);
    let status = child.wait().unwrap();
    reader.join().unwrap();
    let printed = String::from_utf8(seen.lock().unwrap().clone()).unwrap();
    (
        printed,
        asked,
        status.code().expect("an exit code, not a signal"),
    )
}

/// A terminal window, its frames shown in turn: each frame the screen, its
/// rows drawn once and used by every frame showing them at their place, and
/// at its foot the prompt or a question, where the next command or the
/// answer is typed a letter a tick after its frame has been held for its
/// ticks. Every `animate` runs on one
/// loop of `TICK`s, each frame visible for its time; where nothing animates,
/// the frame `STILL`, a command and what it printed.
#[cfg(target_os = "linux")]
fn terminal_animation(
    frames: &[(ratatui::buffer::Buffer, (u16, u16, u16), u32)],
    theme: &samplekit::tui::theme::Theme,
) -> String {
    const TICK: f64 = 0.05;
    // The ticks between a command typed and what it prints.
    const ENTER: u32 = 8;
    const STILL: usize = 1;
    // A terminal's window and nothing over it: the prompt says the folder.
    let (pad, corner) = (14u16, 10u16);
    let (background, foreground) = (hex(theme.background), hex(theme.foreground));
    let (columns, rows) = (frames[0].0.area.width, frames[0].0.area.height);
    let (width, height) = (columns * CELL.0 + 2 * pad, rows * CELL.1 + 2 * pad);
    let mut defined: Vec<String> = Vec::new();
    let mut shown: Vec<Vec<usize>> = Vec::new();
    for (buffer, ..) in frames {
        let mut uses = Vec::new();
        for row in picture_rows(buffer, &background, &foreground) {
            if row.is_empty() {
                continue;
            }
            let at = match defined.iter().position(|held| *held == row) {
                Some(at) => at,
                None => {
                    defined.push(row);
                    defined.len() - 1
                }
            };
            uses.push(at);
        }
        shown.push(uses);
    }
    let lasting = |frame: &(ratatui::buffer::Buffer, (u16, u16, u16), u32)| {
        let typed = u32::from(frame.1.2);
        frame.2 + typed + if typed > 0 { ENTER } else { 0 }
    };
    let total: u32 = frames.iter().map(lasting).sum();
    let dur = format!("{:.2}s", f64::from(total) * TICK);
    let at_tick = |tick: u32| format!("{:.5}", f64::from(tick) / f64::from(total));
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
         viewBox=\"0 0 {width} {height}\" {FONT}>\n\
         <rect width=\"{width}\" height=\"{height}\" rx=\"{corner}\" fill=\"{background}\"/>\n\
         <defs>\n",
    );
    for (at, row) in defined.iter().enumerate() {
        svg.push_str(&format!("<g id=\"r{at}\">\n{row}</g>\n"));
    }
    svg.push_str(&format!(
        "</defs>\n<g transform=\"translate({pad} {pad})\">\n"
    ));
    let mut begun = 0;
    for (at, (frame, uses)) in frames.iter().zip(&shown).enumerate() {
        let (_, (row, prompt, typed), hold) = frame;
        let (from, to) = (begun, begun + lasting(frame));
        // Discrete: hidden until its time, visible through it, hidden after.
        let (values, times) = if at == 0 {
            ("visible;hidden", format!("0;{}", at_tick(to)))
        } else if at == frames.len() - 1 {
            ("hidden;visible", format!("0;{}", at_tick(from)))
        } else {
            (
                "hidden;visible;hidden",
                format!("0;{};{}", at_tick(from), at_tick(to)),
            )
        };
        svg.push_str(&format!(
            "<g visibility=\"{}\">\n<animate attributeName=\"visibility\" values=\"{values}\" \
             keyTimes=\"{times}\" dur=\"{dur}\" calcMode=\"discrete\" repeatCount=\"indefinite\"/>\n",
            if at == STILL { "visible" } else { "hidden" },
        ));
        for used in uses {
            svg.push_str(&format!("<use href=\"#r{used}\"/>\n"));
        }
        // The command not yet typed hidden, the cursor after what is: both
        // a cell further at each letter.
        let (x, y) = (prompt * CELL.0, row * CELL.1);
        let stepping = if *typed == 0 {
            String::new()
        } else {
            let places: Vec<String> = (0..=*typed)
                .map(|letters| (x + letters * CELL.0).to_string())
                .collect();
            let times: Vec<String> = std::iter::once("0".to_string())
                .chain((0..u32::from(*typed)).map(|letter| at_tick(from + hold + letter)))
                .collect();
            format!(
                "<animate attributeName=\"x\" values=\"{}\" keyTimes=\"{}\" dur=\"{dur}\" \
                 calcMode=\"discrete\" repeatCount=\"indefinite\"/>",
                places.join(";"),
                times.join(";"),
            )
        };
        if *typed > 0 {
            svg.push_str(&format!(
                "<rect x=\"{x}\" y=\"{y}\" width=\"{}\" height=\"{}\" fill=\"{background}\">{stepping}</rect>\n",
                typed * CELL.0,
                CELL.1,
            ));
        }
        svg.push_str(&format!(
            "<rect x=\"{x}\" y=\"{y}\" width=\"{}\" height=\"{}\" fill=\"{}\">{stepping}</rect>\n</g>\n",
            CELL.0,
            CELL.1,
            hex(theme.accent),
        ));
        begun = to;
    }
    svg.push_str("</g>\n</svg>\n");
    svg
}

#[test]
#[cfg(target_os = "linux")]
fn the_command_line_at_work_is_drawn_from_what_it_prints() {
    use ratatui::style::{Modifier, Style};
    let theme = samplekit::tui::theme::Themes::read(None)
        .theme("samplekit-dark", None)
        .unwrap();
    // The demo's last step, a project of its own as the reader's would be,
    // `~/tensile`, the environment its model names beside it.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    isolate();
    let scratch = Scratch(
        dunce::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!(
                "samplekit-workbench-cli-at-work-{}",
                std::process::id()
            )),
    );
    let _ = fs::remove_dir_all(&scratch.0);
    let project = scratch.0.join("tensile");
    copied(&root.join("examples/tensile/06-links"), &project);
    std::os::unix::fs::symlink(root.join(".venv"), scratch.0.join(".venv")).unwrap();
    // Its history begun, as a project in use has it: a copy's first write
    // says otherwise that it takes its values as computed.
    let begun = std::process::Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["samples", "compute", "--write"])
        .current_dir(&project)
        .env("NO_COLOR", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(
        begun.status.success(),
        "{}",
        String::from_utf8_lossy(&begun.stderr)
    );
    // Each command, what its questions are answered, the code it exits
    // with, and a line of what it prints.
    let commands: [(&str, &[&str], i32, &str); 5] = [
        (
            "samplekit -f 'elongation > 20' -c material,elongation,tensile_strength",
            &[],
            0,
            "2 of 3 samples",
        ),
        (
            "samplekit samples/copper-01.md set 'curve.strain[44]=45' --write",
            &[],
            0,
            "this makes 3 values not current",
        ),
        // Previewed, asked, computed, shown, asked again, written.
        ("samplekit compute", &["y", "y"], 0, "44.0 %   45.0 %"),
        ("samplekit validate", &[], 2, "error: 1 defect in 1 file"),
        (
            "samplekit -c name,elongation -s -elongation --csv",
            &[],
            0,
            "copper-01,45.0",
        ),
    ];
    let (columns, rows) = TERMINAL;
    let prompt: Vec<Drawn> = "~/tensile"
        .chars()
        .map(|character| {
            (
                character,
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )
        })
        .chain(
            " $ "
                .chars()
                .map(|character| (character, Style::default().fg(theme.muted))),
        )
        .collect();
    let text = |lines: &[Vec<Drawn>]| -> Vec<String> {
        lines
            .iter()
            .map(|line| line.iter().map(|(character, _)| character).collect())
            .collect()
    };
    // No line wraps: one wider than the terminal fails here, rather than
    // being cut where the terminal would wrap it.
    let fits = |lines: &[Vec<Drawn>], typed: &str| {
        for (line, said) in lines.iter().zip(text(lines)) {
            assert!(
                line.len() <= usize::from(columns),
                "{typed} prints a line of {} characters, wider than the terminal's {columns}: \
                 widen TERMINAL\n{said}",
                line.len()
            );
        }
    };
    let mut frames: Vec<(ratatui::buffer::Buffer, (u16, u16, u16), u32)> = Vec::new();
    // A frame of the rows a terminal of `rows` shows, the last once they
    // fill it, `typed` typed at the end of the last from its column `from`;
    // held longer the more there is to read above it.
    let mut frame = |screen: &[Vec<Drawn>], from: usize, typed: usize, hold: u32| {
        let mut buffer =
            ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, columns, rows));
        let top = screen.len().saturating_sub(usize::from(rows));
        for (y, row) in screen[top..].iter().enumerate() {
            for (x, (character, style)) in row.iter().enumerate() {
                buffer[(x as u16, y as u16)]
                    .set_char(*character)
                    .set_style(*style);
            }
        }
        frames.push((
            buffer,
            ((screen.len() - top - 1) as u16, from as u16, typed as u16),
            hold,
        ));
    };
    let mut screen: Vec<Vec<Drawn>> = Vec::new();
    let mut new_rows = 0;
    for at in 0..=commands.len() {
        let typed = commands.get(at).map_or("", |(typed, ..)| *typed);
        let mut line = prompt.clone();
        line.extend(
            typed
                .chars()
                .map(|character| (character, Style::default().fg(theme.foreground))),
        );
        screen.push(line);
        fits(&screen[screen.len() - 1..], typed);
        let hold = if at == 0 { 20 } else { 30 + 5 * new_rows }
            + if at == commands.len() { 40 } else { 0 };
        frame(&screen, prompt.len(), typed.chars().count(), hold);
        let Some((typed, answers, code, says)) = commands.get(at) else {
            break;
        };
        let (printed, asked, exited) = at_a_terminal(&project, typed, answers);
        // Each answer, as the terminal echoed it after its question.
        let mut shown = 0;
        for (answer, at) in answers.iter().zip(&asked) {
            assert!(
                printed[*at..].starts_with(&format!("{answer}\r\n")),
                "{typed}: {answer:?} is not echoed after its question"
            );
            let lines = on_screen(&printed[..at + answer.len()], &theme);
            fits(&lines, typed);
            let question = lines.last().unwrap().len() - answer.chars().count();
            let mut asking = screen.clone();
            asking.extend(lines.iter().cloned());
            frame(
                &asking,
                question,
                answer.chars().count(),
                30 + 5 * (lines.len() - shown) as u32,
            );
            shown = lines.len();
        }
        assert!(printed.ends_with('\n'), "{typed} ends its last line");
        let lines = on_screen(&printed, &theme);
        let said = text(&lines);
        assert_eq!(exited, *code, "{typed}:\n{}", said.join("\n"));
        assert!(
            said.iter()
                .any(|line| line.trim_end() == *says || line.ends_with(says)),
            "{typed} says no {says:?}:\n{}",
            said.join("\n")
        );
        assert!(
            !said.iter().any(|line| line.contains("warning:")),
            "{typed}:\n{}",
            said.join("\n")
        );
        fits(&lines, typed);
        new_rows = (lines.len() + 1 - shown.max(1)) as u32;
        screen.extend(lines);
    }
    let svg = terminal_animation(&frames, &theme);
    // Each frame a visibility shown in turn, every animation on one loop.
    assert_eq!(
        svg.matches("<animate attributeName=\"visibility\"").count(),
        frames.len()
    );
    let mut loops: Vec<&str> = svg
        .split(" dur=\"")
        .skip(1)
        .map(|after| &after[..after.find('"').unwrap()])
        .collect();
    loops.dedup();
    assert_eq!(loops.len(), 1, "{loops:?}");
    assert!(svg.len() < 500_000, "{} bytes", svg.len());
    let mut differing = Vec::new();
    committed("docs/images/readme/cli-at-work.svg", &svg, &mut differing);
    none_differ(&differing, "the_command_line_at_work");
}

/// The start page's name as a picture: each `█` a square, a run of them one
/// rectangle; each of the shadow's double lines two strokes, from the edge of
/// its cell to the middle of its turn. In `colour`, on nothing.
fn logo(art: &[&str], colour: &str) -> String {
    const WIDTH: f64 = 10.0;
    const HEIGHT: f64 = 20.0;
    // The two strokes, either side of the cell's middle.
    const APART: f64 = 2.5;
    let columns = art.iter().map(|line| line.chars().count()).max().unwrap();
    let (width, height) = (columns as f64 * WIDTH, art.len() as f64 * HEIGHT);
    let mut blocks = String::new();
    let mut strokes = String::new();
    for (y, line) in art.iter().enumerate() {
        let characters: Vec<char> = line.chars().collect();
        let top = y as f64 * HEIGHT;
        let mut x = 0;
        while x < characters.len() {
            if characters[x] == '█' {
                let from = x;
                while x < characters.len() && characters[x] == '█' {
                    x += 1;
                }
                blocks.push_str(&format!(
                    "M{} {top}h{}v{HEIGHT}h-{}z",
                    from as f64 * WIDTH,
                    (x - from) as f64 * WIDTH,
                    (x - from) as f64 * WIDTH
                ));
                continue;
            }
            let left = x as f64 * WIDTH;
            let (middle, centre, right, bottom) = (
                left + WIDTH / 2.0,
                top + HEIGHT / 2.0,
                left + WIDTH,
                top + HEIGHT,
            );
            let (near, far) = (APART, -APART);
            // Each stroke: where it starts, where it turns, where it ends.
            let lines: Vec<[(f64, f64); 3]> = match characters[x] {
                '═' => vec![
                    [
                        (left, centre - near),
                        (middle, centre - near),
                        (right, centre - near),
                    ],
                    [
                        (left, centre + near),
                        (middle, centre + near),
                        (right, centre + near),
                    ],
                ],
                '║' => vec![
                    [
                        (middle - near, top),
                        (middle - near, centre),
                        (middle - near, bottom),
                    ],
                    [
                        (middle + near, top),
                        (middle + near, centre),
                        (middle + near, bottom),
                    ],
                ],
                // Down and to the left: the outer stroke the longer.
                '╗' => vec![
                    [
                        (left, centre + far),
                        (middle - far, centre + far),
                        (middle - far, bottom),
                    ],
                    [
                        (left, centre + near),
                        (middle - near, centre + near),
                        (middle - near, bottom),
                    ],
                ],
                '╔' => vec![
                    [
                        (right, centre + far),
                        (middle + far, centre + far),
                        (middle + far, bottom),
                    ],
                    [
                        (right, centre + near),
                        (middle + near, centre + near),
                        (middle + near, bottom),
                    ],
                ],
                '╝' => vec![
                    [
                        (left, centre - far),
                        (middle - far, centre - far),
                        (middle - far, top),
                    ],
                    [
                        (left, centre - near),
                        (middle - near, centre - near),
                        (middle - near, top),
                    ],
                ],
                '╚' => vec![
                    [
                        (right, centre - far),
                        (middle + far, centre - far),
                        (middle + far, top),
                    ],
                    [
                        (right, centre - near),
                        (middle + near, centre - near),
                        (middle + near, top),
                    ],
                ],
                ' ' => Vec::new(),
                other => panic!("the logo has no shape for {other:?}"),
            };
            for [(x1, y1), (x2, y2), (x3, y3)] in lines {
                strokes.push_str(&format!("M{x1} {y1}L{x2} {y2}L{x3} {y3}"));
            }
            x += 1;
        }
    }
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" \
         viewBox=\"0 0 {width} {height}\">\n<title>SampleKit</title>\n\
         <path fill=\"{colour}\" shape-rendering=\"crispEdges\" d=\"{blocks}\"/>\n\
         <path fill=\"none\" stroke=\"{colour}\" stroke-width=\"1.5\" d=\"{strokes}\"/>\n</svg>\n"
    )
}

#[test]
fn the_logo_is_drawn_from_the_start_pages_art() {
    use samplekit::tui::start::LOGO;
    use samplekit::tui::theme::Themes;
    let themes = Themes::read(None);
    let mut differing = Vec::new();
    // The colour the start page draws it in, SampleKit's own dark theme's
    // accent, on nothing: one logo, the same on a light page and a dark one.
    let accent = hex(themes.theme("samplekit-dark", None).unwrap().accent);
    {
        let file = "docs/images/logo.svg";
        let svg = logo(&LOGO, &accent);
        // A rectangle for each run of blocks, nothing behind them.
        let runs: usize = LOGO
            .iter()
            .map(|line| {
                line.split(|character| character != '█')
                    .filter(|run| !run.is_empty())
                    .count()
            })
            .sum();
        assert_eq!(svg.matches('z').count(), runs);
        assert!(!svg.contains("<rect"), "{svg}");
        assert_eq!(svg.matches(&format!("\"{accent}\"")).count(), 2);
        committed(file, &svg, &mut differing);
    }
    none_differ(&differing, "the_logo_is_drawn");
}
