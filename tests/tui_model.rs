//! The TUI's keys, on a model that draws nothing.

mod workbench_fixtures;

use workbench_fixtures::*;

#[test]
fn a_filter_narrows_the_collection_and_is_remembered() {
    let scratch = Scratch::new("filter");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    assert_eq!(workbench.view.len(), 3);
    keys(&mut workbench, "/beer == schwarz\n");
    assert_eq!(workbench.view.len(), 2, "{}", workbench.message);
    // A filter that does not read keeps the prompt open, with its error.
    keys(&mut workbench, "/ &&\n");
    assert!(matches!(
        workbench.mode,
        Mode::Filter { error: Some(_), .. }
    ));
    keys(&mut workbench, "\u{1b}");
    samplekit::tui::model::remember(&workbench);
    let again = Workbench::open(&scratch.0).unwrap();
    assert_eq!(again.filter, "beer == schwarz");
    assert_eq!(again.view.len(), 2);
}

#[test]
fn the_filter_is_edited_where_its_cursor_is() {
    let scratch = Scratch::new("filter-cursor");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "/beer == schwarz || beer == gose");
    // Back to the start, a parenthesis put before the clause already typed.
    workbench.key(Key::Home);
    keys(&mut workbench, "(");
    workbench.key(Key::End);
    keys(&mut workbench, ")");
    for _ in 0..3 {
        workbench.key(Key::Left);
    }
    workbench.key(Key::Backspace);
    workbench.key(Key::Delete);
    keys(&mut workbench, "é");
    let Mode::Filter { text, .. } = &workbench.mode else {
        panic!("filter");
    };
    assert_eq!(text, "(beer == schwarz || beer == gée)");
    assert_eq!(&text.text()[text.at()..], "e)");
    // Tab completes what precedes the cursor and keeps what follows.
    keys(&mut workbench, "\u{1b}/ && beer");
    workbench.key(Key::Home);
    keys(&mut workbench, "bee");
    keys(&mut workbench, "\t");
    let Mode::Filter { text: line, .. } = &workbench.mode else {
        panic!("filter");
    };
    let text = line.text();
    assert!(text.starts_with("beer"), "{text}");
    assert!(text.ends_with(" && beer"), "{text}");
    assert!(!text[..line.at()].ends_with(" && beer"), "{text}");
}

#[test]
fn every_line_typed_is_edited_where_its_cursor_is() {
    let scratch = rich("cursor-everywhere");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    // A value, in the window it shares with its uncertainty.
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e");
    workbench.key(Key::End);
    for _ in 0..20 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "12.5");
    workbench.key(Key::Left);
    workbench.key(Key::Left);
    keys(&mut workbench, "0");
    let Mode::Quantity { slots, .. } = &workbench.mode else {
        panic!("the quantity's window");
    };
    assert_eq!(slots[0].text, "120.5");
    // A prompt: the tags, written whole.
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    keys(&mut workbench, "e");
    assert!(
        matches!(workbench.mode, Mode::Prompt { .. }),
        "the tags' prompt"
    );
    workbench.key(Key::End);
    for _ in 0..40 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "a, c");
    workbench.key(Key::Home);
    for _ in 0..3 {
        workbench.key(Key::Right);
    }
    keys(&mut workbench, "b, ");
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("the tags' prompt");
    };
    assert_eq!((text.text(), text.at()), ("a, b, c", 6));
    workbench.key(Key::Delete);
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("the tags' prompt");
    };
    assert_eq!(text, "a, b, ");
    // A list narrowed with `/`.
    keys(&mut workbench, "\u{1b}\u{1b}\u{1b}c/mlt");
    for _ in 0..2 {
        workbench.key(Key::Left);
    }
    keys(&mut workbench, "a");
    let Mode::Picker { query, typing, .. } = &workbench.mode else {
        panic!("the picker");
    };
    assert_eq!(query, "malt");
    assert_eq!((query.at(), *typing), (2, true));
}

#[test]
fn the_columns_kept_keep_their_labels() {
    // Accepting the columns picker unchanged turned a profile's `Malt` back
    // into the field's name: a column rebuilt from its field alone.
    let scratch = Scratch::new("columns-labels");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"name\"}, \
         {field = \"malt\", label = \"Malt\"}]\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let labelled = |workbench: &Workbench| workbench.profile.columns()[1].label.clone();
    assert_eq!(labelled(&workbench).as_deref(), Some("Malt"));
    keys(&mut workbench, "c\n");
    assert!(matches!(workbench.mode, Mode::Normal));
    assert_eq!(labelled(&workbench).as_deref(), Some("Malt"));
}

#[test]
fn a_sort_is_chosen_and_reversed() {
    let scratch = Scratch::new("sort");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let malt = workbench
        .profile
        .columns()
        .iter()
        .position(|column| column.field == "malt")
        .expect("malt is shown");
    keys(&mut workbench, "s");
    for _ in 0..malt {
        workbench.key(Key::Down);
    }
    keys(&mut workbench, " \n");
    let first = |workbench: &Workbench| {
        workbench
            .view
            .get(0)
            .unwrap()
            .sample
            .borrow()
            .name()
            .unwrap()
            .to_string()
    };
    assert_eq!(first(&workbench), "C2");
    keys(&mut workbench, "r");
    assert_eq!(first(&workbench), "C1");
}

#[test]
fn the_basket_is_tagged() {
    let scratch = Scratch::new("basket");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "  ");
    assert_eq!(workbench.basket.len(), 2);
    keys(&mut workbench, "tchecked\n");
    assert!(matches!(workbench.mode, Mode::Confirm { .. }));
    keys(&mut workbench, "y");
    assert!(
        workbench.message.contains("2 samples tagged"),
        "{}",
        workbench.message
    );
    keys(&mut workbench, "/tags has checked\n");
    assert_eq!(workbench.view.len(), 2, "{}", workbench.message);
}

#[test]
fn a_picker_is_narrowed_with_a_slash() {
    let scratch = rich("narrow");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "c/bee");
    let Mode::Picker { items, query, .. } = &workbench.mode else {
        panic!("the picker stays open");
    };
    assert_eq!(query, "bee");
    let shown = samplekit::tui::model::visible(items, query.text());
    assert_eq!(shown.len(), 1, "{items:?}");
    // Space ticks the item under the cursor among those shown.
    let before = items[shown[0]].1;
    keys(&mut workbench, " ");
    let Mode::Picker { items, .. } = &workbench.mode else {
        panic!("still open");
    };
    assert_ne!(items[shown[0]].1, before);
    // Esc clears the narrowing first, then closes.
    keys(&mut workbench, "\u{1b}");
    assert!(matches!(&workbench.mode, Mode::Picker { query, .. } if query.is_empty()));
    keys(&mut workbench, "\u{1b}");
    assert!(matches!(workbench.mode, Mode::Normal));
}

#[test]
fn narrowing_and_completion_find_letters_in_order() {
    let scratch = rich("fuzzy");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    // A picker: the letters of `beer`, not side by side.
    keys(&mut workbench, "c/br");
    let Mode::Picker { items, query, .. } = &workbench.mode else {
        panic!("the picker stays open");
    };
    let shown = samplekit::tui::model::visible(items, query.text());
    assert!(
        shown
            .first()
            .is_some_and(|at| items[*at].0.contains("beer")),
        "{items:?} {shown:?}"
    );
    // Side by side comes before apart.
    assert!(
        samplekit::tui::model::fuzzy("mash_water", "mw")
            > samplekit::tui::model::fuzzy("mashing_water", "mw")
    );
    assert_eq!(samplekit::tui::model::fuzzy("brix", "hr"), None);
    // The filter's completion likewise.
    keys(&mut workbench, "\u{1b}\u{1b}/br");
    let Mode::Filter { candidates, .. } = &workbench.mode else {
        panic!("filter");
    };
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.starts_with("beer")),
        "{candidates:?}"
    );
}

#[test]
fn the_note_is_edited_in_place_and_previewed() {
    let scratch = rich("note");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    keys(&mut workbench, "N");
    workbench.key(Key::End);
    keys(&mut workbench, " Amended.");
    workbench.key(Key::Down);
    workbench.key(Key::Home);
    workbench.key(Key::Delete);
    keys(&mut workbench, "\u{1b}");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines.contains(&"+ First line. Amended.".to_string()),
        "{lines:?}"
    );
    assert!(lines.contains(&"- Second line.".to_string()), "{lines:?}");
    // A line replaced reads `-` then `+`, as a diff does.
    let at = |line: &str| lines.iter().position(|held| held == line).unwrap();
    assert!(
        at("- First line.") < at("+ First line. Amended."),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    let written = fs::read_to_string(scratch.0.join("more/D1.md")).unwrap();
    assert!(
        written.ends_with("First line. Amended.\necond line.\n"),
        "{written}"
    );
}

#[test]
fn readings_with_no_statistic_say_so_and_are_not_computed() {
    let scratch = rich("nostatistic");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "malt");
    let malt = workbench
        .entries()
        .into_iter()
        .find(|entry| entry.field == "malt")
        .unwrap();
    assert_eq!(malt.state, "no statistic");
    keys(&mut workbench, "c");
    assert!(workbench.running.is_none());
    assert!(matches!(workbench.mode, Mode::Normal));
    assert!(
        // No mean stands in for them, nor is offered.
        workbench.message.contains("no statistic chosen")
            && !workbench.message.contains("their mean"),
        "{}",
        workbench.message
    );
}

#[test]
fn the_selection_and_the_basket_are_remembered() {
    let scratch = rich("kept");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "a");
    assert_eq!(workbench.basket.len(), 4);
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, " ");
    assert!(workbench.selected.contains("malt"));
    samplekit::tui::model::remember(&workbench);
    let mut again = Workbench::open(&scratch.0).unwrap();
    assert_eq!(again.basket.len(), 4);
    assert!(again.selected.contains("malt"));
    // x empties each where it applies.
    keys(&mut again, "x");
    assert!(again.basket.is_empty());
    keys(&mut again, "\nx");
    assert!(again.selected.is_empty());
}

#[test]
fn a_value_and_its_uncertainty_are_edited_together() {
    let scratch = rich("quantity");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e");
    let Mode::Quantity { slots, .. } = &workbench.mode else {
        panic!("one window for both");
    };
    let texts: Vec<&str> = slots.iter().map(|slot| slot.text.text()).collect();
    // Its readings too, a property's: none here.
    assert_eq!(texts, ["12", "0.1", ""]);
    workbench.key(Key::Backspace);
    keys(&mut workbench, "3\t");
    for _ in 0..3 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "0.2\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines.iter().any(|line| line.contains("12.0  →  13.0")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.starts_with("malt.u")),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    let written = fs::read_to_string(scratch.0.join("C1.md")).unwrap();
    assert!(
        written.contains("v: 13") && written.contains("u: 0.2"),
        "{written}"
    );
    // An emptied uncertainty is cleared.
    keys(&mut workbench, "e\t");
    for _ in 0..3 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "\ny");
    let written = fs::read_to_string(scratch.0.join("C1.md")).unwrap();
    assert!(!written.contains("u: 0.2"), "{written}");
}

#[test]
fn f_offers_the_saved_queries_and_profiles_and_no_project() {
    // Another project is reached from the start page; `f` keeps to what this
    // one declares — `g`, which went there, groups.
    let scratch = rich("goto");
    fs::create_dir_all(scratch.0.join("more/deeper")).unwrap();
    fs::write(
        scratch.0.join("more/deeper/.samplekitrc"),
        "schema_version = 1\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "f");
    let Mode::Picker { items, .. } = &workbench.mode else {
        panic!("a picker");
    };
    assert!(
        items
            .iter()
            .any(|(item, _)| item.starts_with("query      schwarz")),
        "{items:?}"
    );
    assert!(
        !items.iter().any(|(item, _)| item.starts_with("project")),
        "{items:?}"
    );
}

#[test]
fn f_offers_the_configuration_of_the_folder_opened() {
    // A project holding two collections: what the root decides is what `f`
    // offers there, not each collection's.
    let scratch = rich("goto-offered");
    for (project, beer, import) in [("ana", "schwarz", true), ("tom", "bock", false)] {
        fs::create_dir_all(scratch.0.join(project)).unwrap();
        fs::write(
            scratch.0.join(project).join(".samplekitrc"),
            format!(
                "schema_version = 1\n{}[query.{beer}_{project}]\n\
                 filter = 'beer == \"{beer}\"'\n\
                 [profile.light]\ncolumns = [{{ field = \"name\" }}, {{ field = \"malt\" }}]\n\
                 sort = [\"malt\"]\n",
                if import { "import = \"..\"\n" } else { "" }
            ),
        )
        .unwrap();
        fs::write(
            scratch.0.join(project).join("S.md"),
            format!(
                "---\nschema_version: 1\nname: S-{project}\nbeer: {beer}\n\
                 properties:\n  malt: {{v: 2.0, unit: g}}\n---\n"
            ),
        )
        .unwrap();
    }
    let labels = |workbench: &mut Workbench| -> Vec<String> {
        keys(workbench, "f");
        let Mode::Picker { items, .. } = &workbench.mode else {
            panic!("a picker");
        };
        // A path as Unix writes it, whatever the system said it with.
        items
            .iter()
            .map(|(item, _)| item.replace('\\', "/"))
            .collect()
    };
    let mut root = Workbench::open(&scratch.0).unwrap();
    let offered = labels(&mut root);
    assert!(
        offered
            .iter()
            .any(|item| item.starts_with("query      schwarz")),
        "{offered:?}"
    );
    assert!(
        !offered
            .iter()
            .any(|item| item.contains("_ana") || item.contains("_tom") || item.contains("light")),
        "{offered:?}"
    );
    // In the collection importing the root: its own, and the root's, marked.
    let mut ana = Workbench::open(&scratch.0.join("ana")).unwrap();
    let offered = labels(&mut ana);
    assert!(
        offered
            .iter()
            .any(|item| item.starts_with("query      schwarz ")
                && item.ends_with("· ../.samplekitrc")),
        "{offered:?}"
    );
    assert!(
        offered
            .iter()
            .any(|item| item.starts_with("query      schwarz_ana") && item.ends_with("· local")),
        "{offered:?}"
    );
    // A profile applied is the one offered.
    let at = offered
        .iter()
        .position(|item| item.starts_with("profile    light"))
        .unwrap();
    for _ in 0..at {
        ana.key(Key::Down);
    }
    ana.key(Key::Enter);
    assert_eq!(ana.sort, vec!["malt".to_string()], "{}", ana.message);
}

#[test]
fn a_selection_of_entered_values_computes_nothing_and_says_so() {
    let scratch = rich("entered");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, " c");
    assert!(workbench.running.is_none());
    assert!(
        workbench.message.contains("nothing computes malt"),
        "{}",
        workbench.message
    );
}

#[test]
fn shift_tab_goes_back_through_the_completions() {
    let scratch = rich("backtab");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "/");
    let Mode::Filter { candidates, .. } = &workbench.mode else {
        panic!("filter");
    };
    let candidates = candidates.clone();
    assert!(candidates.len() > 2, "{candidates:?}");
    keys(&mut workbench, "\t\t");
    workbench.key(Key::BackTab);
    let Mode::Filter { text, .. } = &workbench.mode else {
        panic!("filter");
    };
    assert_eq!(text, &candidates[0]);
    // Back from nothing chosen is the last.
    keys(&mut workbench, "\u{1b}/");
    workbench.key(Key::BackTab);
    let Mode::Filter { text, .. } = &workbench.mode else {
        panic!("filter");
    };
    assert_eq!(text, candidates.last().unwrap());
}

#[test]
fn a_sort_takes_several_keys_each_reversible_and_in_order() {
    let scratch = rich("multisort");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "c");
    // Beer shown, so that it can be sorted by.
    let Mode::Picker { items, .. } = &workbench.mode else {
        panic!("columns");
    };
    let (beer, shown) = items
        .iter()
        .enumerate()
        .find(|(_, (label, _))| label == "beer")
        .map(|(at, (_, ticked))| (at, *ticked))
        .unwrap();
    for _ in 0..beer {
        workbench.key(Key::Down);
    }
    keys(&mut workbench, if shown { "\n" } else { " \n" });
    keys(&mut workbench, "s/malt r\u{1b}/beer \u{1b}");
    // Beer moved above malt: it sorts first.
    let Mode::Picker { items, cursor, .. } = &workbench.mode else {
        panic!("sort");
    };
    let at = items.iter().position(|(label, _)| label == "beer").unwrap();
    assert_eq!(*cursor, at);
    for _ in 0..at {
        workbench.key(Key::MoveUp);
    }
    keys(&mut workbench, "\n");
    assert_eq!(workbench.sort, ["beer", "-malt"]);
    let names: Vec<String> = workbench
        .view
        .iter()
        .map(|entry| entry.sample.borrow().name().unwrap().to_string())
        .collect();
    assert_eq!(names, ["C2", "D1", "C1", "C3"]);
}

#[test]
fn a_tables_cell_is_edited_explained_and_selected_as_a_value_is() {
    let scratch = rich("cells");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "runs");
    workbench.key(Key::Right);
    let (cell, column) = workbench.cell().expect("a cell under the cursor");
    assert_eq!(
        (cell.field.as_str(), column.as_str()),
        ("runs.load[1]", "runs.load")
    );
    // Its value and its uncertainty, in the one window a value has.
    keys(&mut workbench, "e");
    let Mode::Quantity { slots, .. } = &workbench.mode else {
        panic!("the cell's window: {}", workbench.message);
    };
    // A cell has no readings to edit.
    assert_eq!(slots.len(), 2);
    assert_eq!(slots[0].text, "3.5");
    workbench.key(Key::Backspace);
    keys(&mut workbench, "6\t0.1\ny");
    let written = fs::read_to_string(scratch.0.join("more/D1.md")).unwrap();
    assert!(written.contains("load: {v: 3.6, u: 0.1"), "{written}");
    keys(&mut workbench, "\n");
    let Mode::Panel { title, .. } = &workbench.mode else {
        panic!("explained");
    };
    assert!(title.starts_with("runs.load[1]"), "{title}");
    keys(&mut workbench, "\u{1b}");
    // Entered, so nothing computes it; selected, as a column.
    keys(&mut workbench, "c");
    // No column selected: the whole table, which nothing computes here.
    assert!(
        workbench.message.contains("nothing computes runs:"),
        "{}",
        workbench.message
    );
    keys(&mut workbench, " ");
    assert!(workbench.selected.contains("runs.load"));
    // ← from the first column is the sample again.
    workbench.key(Key::Left);
    assert!(matches!(workbench.screen, Screen::Sample { .. }));
}

#[test]
fn the_project_colours_the_workbench() {
    let scratch = rich("colours");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[workbench.colors]\noutdated = \"#ff8800\"\n",
    )
    .unwrap();
    let workbench = Workbench::open(&scratch.0).unwrap();
    assert_eq!(
        workbench.theme.outdated,
        ratatui::style::Color::Rgb(0xff, 0x88, 0x00)
    );
    assert_eq!(workbench.theme.failed, ratatui::style::Color::Red);
}

#[test]
fn an_export_is_previewed_then_written() {
    let scratch = rich("export");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[collection]\nrecursive = true\n\
         [profile.malts]\ncolumns = [{ field = \"malt\" }]\nsort = [\"malt\"]\n\
         [export.malts]\nprofile = \"malts\"\nformat = \"csv\"\noutput = \"out/malts.csv\"\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "p/malts\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(lines[0].ends_with("out/malts.csv"), "{lines:?}");
    assert!(lines[1].starts_with("4 rows"), "{lines:?}");
    keys(&mut workbench, "y");
    assert!(
        workbench.message.starts_with("4 rows written"),
        "{}",
        workbench.message
    );
    let written = fs::read_to_string(scratch.0.join("out/malts.csv")).unwrap();
    // Its profile's order: the lightest first.
    let first = written.lines().nth(1).unwrap_or_default();
    assert!(first.contains('9'), "{written}");
}

#[test]
fn a_quantitys_readings_are_edited_in_its_window() {
    let scratch = rich("readings");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e\t\t");
    let Mode::Quantity { slots, on, .. } = &workbench.mode else {
        panic!("its window");
    };
    assert_eq!((slots[2].text.text(), *on), ("8, 10", 2));
    for _ in 0..5 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "11, 12, 13\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(lines[0].starts_with("malt.readings"), "{lines:?}");
    keys(&mut workbench, "y");
    let written = fs::read_to_string(scratch.0.join("more/D1.md")).unwrap();
    assert!(
        written.contains("readings: [11.0, 12.0, 13.0]"),
        "{written}"
    );
    // No statistic is declared for them, so no value is written.
    assert!(!written.contains("v: 12"), "no mean: {written}");
}

#[test]
fn a_tag_comes_off_the_basket_with_a_minus() {
    let scratch = rich("untag");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "a");
    keys(&mut workbench, "tchecked\ny");
    keys(&mut workbench, "t-checked\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(lines[0].starts_with("'checked' off 4"), "{lines:?}");
    keys(&mut workbench, "y");
    let written = fs::read_to_string(scratch.0.join("C1.md")).unwrap();
    assert!(!written.contains("checked"), "{written}");
}

#[test]
fn a_new_sample_is_shaped_like_the_one_under_the_cursor() {
    let scratch = rich("new");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let at = workbench
        .view
        .iter()
        .position(|entry| entry.sample.borrow().name() == Some("D1"))
        .unwrap();
    workbench.cursor = at;
    keys(&mut workbench, "ND2\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(lines[0].contains("D2.md, shaped like D1"), "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|line| line.contains("carried") && line.contains("bock")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("tables, empty") && line.contains("runs")),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    assert!(
        scratch.0.join("more/D2.md").is_file(),
        "{}",
        workbench.message
    );
    assert_eq!(workbench.collection.len(), 5);
    // And u takes it away again.
    keys(&mut workbench, "uy");
    assert!(
        !scratch.0.join("more/D2.md").exists(),
        "{}",
        workbench.message
    );
}

#[test]
fn a_row_is_added_to_an_unfolded_table() {
    let scratch = rich("row");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "runs");
    workbench.key(Key::Right);
    keys(&mut workbench, "+run=3, load=5.5\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines[0].starts_with("a row in runs: run = 3, load = 5.5"),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    let written = fs::read_to_string(scratch.0.join("more/D1.md")).unwrap();
    assert!(written.contains("load: 5.5"), "{written}");
}

#[test]
fn u_gives_the_last_change_back_and_n_keeps_it_for_later() {
    let scratch = rich("undo");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let before = fs::read_to_string(scratch.0.join("C1.md")).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e");
    workbench.key(Key::Backspace);
    keys(&mut workbench, "9\ny");
    assert_ne!(fs::read_to_string(scratch.0.join("C1.md")).unwrap(), before);
    keys(&mut workbench, "un");
    assert_eq!(workbench.undo.len(), 1, "declined, still there");
    keys(&mut workbench, "uy");
    assert_eq!(fs::read_to_string(scratch.0.join("C1.md")).unwrap(), before);
    keys(&mut workbench, "u");
    assert_eq!(workbench.message, "nothing to undo");
}

#[test]
fn the_control_screen_lists_what_is_behind_and_opens_it() {
    let scratch = rich("control");
    // A formula's value whose input moved: stale.
    fs::write(
        scratch.0.join("C3.md"),
        "---\nschema_version: 1\nname: C3\nbeer: schwarz\nproperties:\n  \
         malt: {v: 11.0, u: 0.1, unit: g, fingerprint: 000000000000}\n  \
         twice: {v: 22.0, computed: {malt: 111111111111}, fingerprint: 222222222222}\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "v");
    assert!(matches!(workbench.screen, Screen::Control { .. }));
    let first = workbench.control.first().expect("something to say").clone();
    assert_eq!(first.sample, "C3", "{:?}", workbench.control);
    assert_eq!(first.field.as_deref(), Some("twice"));
    // Enter opens the sample on that value.
    keys(&mut workbench, "\n");
    assert!(matches!(workbench.screen, Screen::Sample { .. }));
    let entry = workbench
        .entries()
        .into_iter()
        .nth(match workbench.screen {
            Screen::Sample { cursor, .. } => cursor,
            _ => 0,
        })
        .unwrap();
    assert_eq!(entry.field, "twice");
}

#[test]
fn an_empty_directory_opens_on_its_setup_and_its_answers_are_done() {
    // Questions, one at a time; the example chosen, no environment.
    let root = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-workbench-setup-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    Scratch::new("setup-state");
    let mut workbench = Workbench::open(&root).unwrap();
    assert!(matches!(workbench.screen, Screen::Setup { cursor: 0 }));
    // The example, then no environment: the model question is not asked.
    workbench.key(Key::Down);
    keys(&mut workbench, "\n");
    assert_eq!(workbench.setup.as_ref().unwrap().question, 1);
    workbench.key(Key::Down);
    keys(&mut workbench, "\n\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert_eq!(
        lines.iter().filter(|line| line.starts_with("  ")).count(),
        5,
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    assert!(
        root.join("samples/EXAMPLE.md").is_file(),
        "{}",
        workbench.message
    );
    assert!(!root.join(".venv").exists());
    assert!(
        workbench.message.starts_with("5 files written"),
        "{}",
        workbench.message
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn what_is_added_to_the_configuration_is_under_the_cursor() {
    // After `a`, the cursor stayed on the first entry, and `d` then removed a
    // query that was there before rather than the one just added.
    let scratch = rich("added-under-cursor");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "P");
    for _ in 0..5 {
        workbench.key(Key::Down);
    }
    workbench.key(Key::Right);
    keys(&mut workbench, "azwickel.filter = beer == \"zwickel\"\n");
    keys(&mut workbench, "d");
    let rows = workbench.workspace_rows();
    assert!(
        rows.contains(&samplekit::tui::model::ConfigRow::Name(
            "schwarz".to_string()
        )),
        "{rows:?}"
    );
    assert!(
        !rows.iter().any(|row| matches!(
            row,
            samplekit::tui::model::ConfigRow::Key { name: Some(name), .. } if name == "zwickel"
        )),
        "{rows:?}"
    );
}

#[test]
fn an_entrys_name_takes_a_key_of_its_own() {
    let scratch = rich("entry-key");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "P");
    // Queries, fifth below the first; an entry made, then its name chosen.
    for _ in 0..5 {
        workbench.key(Key::Down);
    }
    workbench.key(Key::Right);
    keys(&mut workbench, "abock.filter = beer == \"bock\"\n");
    let rows = workbench.workspace_rows();
    let at = rows
        .iter()
        .position(|row| row == &samplekit::tui::model::ConfigRow::Name("bock".to_string()))
        .unwrap();
    workbench.workspace.as_mut().unwrap().row = at;
    workbench.key(Key::Enter);
    assert!(
        matches!(workbench.mode, Mode::Prompt { .. }),
        "{}",
        workbench.message
    );
    keys(&mut workbench, "directory = \"data\"\n");
    let rows = workbench.workspace_rows();
    assert!(
        rows.iter().any(|row| matches!(
            row,
            samplekit::tui::model::ConfigRow::Key { name: Some(name), key, .. }
                if name == "bock" && key == "directory"
        )),
        "{rows:?} {}",
        workbench.message
    );
}

#[test]
fn the_configuration_is_edited_by_section_and_written_checked() {
    let scratch = rich("configure");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "P");
    assert!(matches!(workbench.screen, Screen::Configure));
    // Queries, fifth below the first.
    for _ in 0..5 {
        workbench.key(Key::Down);
    }
    workbench.key(Key::Right);
    keys(&mut workbench, "a");
    keys(&mut workbench, "bock.filter = beer == \"bock\"\n");
    assert!(
        workbench.workspace.as_ref().unwrap().dirty,
        "{}",
        workbench.message
    );
    // A setting that would not load is said at once.
    keys(&mut workbench, "abad.nonsense = 1\n");
    assert!(
        workbench.message.contains("would not load"),
        "{}",
        workbench.message
    );
    // Taken away again: its entry under the cursor.
    let rows = workbench.workspace_rows();
    let at = rows
        .iter()
        .position(|row| row == &samplekit::tui::model::ConfigRow::Name("bad".to_string()))
        .unwrap();
    workbench.workspace.as_mut().unwrap().row = at;
    keys(&mut workbench, "d");
    keys(&mut workbench, "w");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines.iter().any(|line| line.contains("[query.bock]")),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    let written = fs::read_to_string(scratch.0.join(".samplekitrc")).unwrap();
    assert!(
        written.contains("[query.bock]") && !written.contains("bad"),
        "{written}"
    );
    // What was there stays as it was.
    assert!(written.contains("[query.schwarz]"), "{written}");
    assert!(!workbench.workspace.as_ref().unwrap().dirty);
}

#[test]
fn a_configuration_changed_since_it_was_read_is_not_saved() {
    // The configuration screen's save and `W`'s read `.samplekitrc` again as
    // they write it, as a sample's save does; refused, the message says so, the
    // other writer's text stays, and the unsaved edit is kept.
    let scratch = rich("configure-since");
    let rc = scratch.0.join(".samplekitrc");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "P");
    for _ in 0..5 {
        workbench.key(Key::Down);
    }
    workbench.key(Key::Right);
    keys(&mut workbench, "a");
    keys(&mut workbench, "bock.filter = beer == \"bock\"\n");
    keys(&mut workbench, "w");
    assert!(
        matches!(workbench.mode, Mode::Confirm { .. }),
        "{}",
        workbench.message
    );
    let theirs = format!("{}# another hand\n", fs::read_to_string(&rc).unwrap());
    fs::write(&rc, &theirs).unwrap();
    keys(&mut workbench, "y");
    assert!(
        workbench.message.contains("changed since it was read"),
        "{}",
        workbench.message
    );
    assert_eq!(fs::read_to_string(&rc).unwrap(), theirs);
    let workspace = workbench.workspace.as_ref().unwrap();
    assert!(workspace.dirty);
    assert!(
        workspace.edit.text().contains("[query.bock]"),
        "{}",
        workspace.edit.text()
    );

    // What is shown, saved by `W`, is refused the same way.
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.filter = "beer == schwarz".to_string();
    workbench.refresh().unwrap();
    keys(&mut workbench, "W\nbocks\n");
    assert!(
        matches!(workbench.mode, Mode::Confirm { .. }),
        "{}",
        workbench.message
    );
    let again = format!("{theirs}# and again\n");
    fs::write(&rc, &again).unwrap();
    keys(&mut workbench, "y");
    assert!(
        workbench.message.contains("changed since it was read"),
        "{}",
        workbench.message
    );
    assert_eq!(fs::read_to_string(&rc).unwrap(), again);
}

#[test]
fn the_sample_open_stays_open_when_a_write_reorders_the_view() {
    let scratch = rich("reorder");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.sort = vec!["malt".to_string()];
    workbench.refresh().unwrap();
    // D1 (9) first by malt: made the heaviest, it moves to the end.
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e");
    workbench.key(Key::Backspace);
    keys(&mut workbench, "99\ny");
    let (_, sample) = workbench.current().expect("still a sample open");
    assert_eq!(sample.name(), Some("D1"));
}

#[test]
fn a_remembered_sort_no_longer_resolving_is_set_aside_not_the_filter() {
    let scratch = rich("stalesort");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.filter = "beer == schwarz".to_string();
    workbench.sort = vec!["body".to_string()];
    samplekit::tui::model::remember(&workbench);
    let again = Workbench::open(&scratch.0).unwrap();
    assert_eq!(again.filter, "beer == schwarz", "{}", again.message);
    assert!(again.sort.is_empty());
    assert!(
        again.message.contains("sort was set aside"),
        "{}",
        again.message
    );
    assert_eq!(again.rendered.len(), again.view.len());
    assert_eq!(again.view.len(), 2);
}

#[test]
fn undo_leaves_a_file_changed_since_as_it_is() {
    let scratch = rich("undoguard");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e");
    workbench.key(Key::Backspace);
    keys(&mut workbench, "9\ny");
    // Edited by hand after the write.
    let path = scratch.0.join("C1.md");
    let edited = fs::read_to_string(&path)
        .unwrap()
        .replace("A note on C1.", "Mine.");
    fs::write(&path, &edited).unwrap();
    keys(&mut workbench, "u");
    assert!(
        workbench.message.contains("changed since"),
        "{}",
        workbench.message
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), edited);
    assert_eq!(workbench.undo.len(), 1, "still offered");
}

#[test]
fn keys_on_the_sections_list_do_not_act_on_their_settings() {
    let scratch = rich("sectionkeys");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "P");
    for _ in 0..5 {
        workbench.key(Key::Down);
    }
    keys(&mut workbench, "d");
    assert!(!workbench.workspace.as_ref().unwrap().dirty);
    keys(&mut workbench, "e");
    assert!(matches!(workbench.mode, Mode::Normal));
}

#[test]
fn quitting_with_the_configuration_unwritten_asks_first() {
    let scratch = rich("quitdirty");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "P");
    workbench.key(Key::Right);
    keys(&mut workbench, "arecursive = false\n");
    keys(&mut workbench, "\u{1b}");
    let effects = keys(&mut workbench, "q");
    assert!(matches!(workbench.mode, Mode::Confirm { .. }));
    assert!(!matches!(effects[0], Effect::Quit));
    let effects = keys(&mut workbench, "y");
    assert!(matches!(effects[0], Effect::Quit));
}

#[test]
fn a_selection_kept_turns_c_into_a_confirmed_computation() {
    let scratch = rich("forced");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.selected.insert("malt".to_string());
    keys(&mut workbench, "aC");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a confirmation");
    };
    assert!(lines[0].starts_with("malt over 4 samples"), "{lines:?}");
    keys(&mut workbench, "n");
    assert!(workbench.running.is_none());
}

#[test]
fn a_sample_gone_leaves_the_basket() {
    let scratch = rich("prune");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "a");
    fs::remove_file(scratch.0.join("C2.md")).unwrap();
    workbench.reload();
    assert_eq!(workbench.basket.len(), 3);
}

#[test]
fn a_new_samples_name_is_no_path_and_a_rows_cells_are_separated() {
    let scratch = rich("names");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "N../escaped\n");
    assert!(
        workbench.message.contains("is a path"),
        "{}",
        workbench.message
    );
    assert!(!scratch.0.parent().unwrap().join("escaped.md").exists());
    // The prompt stays open on its text, to correct it.
    assert!(matches!(workbench.mode, Mode::Prompt { .. }));
    keys(&mut workbench, "\u{1b}");
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "runs");
    workbench.key(Key::Right);
    keys(&mut workbench, "+run=4 load=5\n");
    assert!(
        workbench.message.contains("holds two cells"),
        "{}",
        workbench.message
    );
}

#[test]
fn a_long_confirmation_scrolls_and_an_unread_configuration_is_said() {
    let scratch = rich("scrollconfirm");
    for at in 0..30 {
        fs::write(
            scratch.0.join(format!("X{at:02}.md")),
            format!("---\nschema_version: 1\nname: X{at:02}\n---\n"),
        )
        .unwrap();
    }
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "atmany\n");
    keys(&mut workbench, "jjj");
    assert_eq!(workbench.confirm_scroll, 3);
    keys(&mut workbench, "n");
    assert_eq!(workbench.confirm_scroll, 0);
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[nonsense]\n",
    )
    .unwrap();
    let broken = Workbench::open(&scratch.0).unwrap();
    assert!(
        broken.message.contains("configuration is not read"),
        "{}",
        broken.message
    );
}

#[test]
fn a_configuration_changed_underneath_is_said_and_read_again_with_r() {
    let scratch = rich("reread");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "P");
    workbench.key(Key::Right);
    keys(&mut workbench, "arecursive = false\n");
    let path = scratch.0.join(".samplekitrc");
    let changed = format!("{}# by hand\n", fs::read_to_string(&path).unwrap());
    fs::write(&path, &changed).unwrap();
    keys(&mut workbench, "w");
    assert!(matches!(workbench.mode, Mode::Normal));
    assert!(
        workbench.message.contains("r reads it again"),
        "{}",
        workbench.message
    );
    keys(&mut workbench, "r");
    let workspace = workbench.workspace.as_ref().unwrap();
    assert!(!workspace.dirty);
    assert_eq!(workspace.section, 0);
    assert_eq!(fs::read_to_string(&path).unwrap(), changed);
    keys(&mut workbench, "w");
    assert!(
        workbench.message.contains("nothing changed"),
        "{}",
        workbench.message
    );
}

#[test]
fn an_attribute_typed_with_a_trailing_space_is_written_without_it() {
    let scratch = Scratch::new("trim");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "beer");
    keys(&mut workbench, "e");
    for _ in 0..10 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "8 \ny");
    let written = fs::read_to_string(scratch.0.join("C1.md")).unwrap();
    assert!(written.contains("beer: 8\n"), "{written}");
}

#[test]
fn a_model_figure_draws_none_of_another_projects_samples() {
    // Python reads each sample with its own model: another project's is
    // not this figure's.
    let scratch = rich("modelfigure");
    let rc = fs::read_to_string(scratch.0.join(".samplekitrc")).unwrap();
    fs::write(
        scratch.0.join(".samplekitrc"),
        rc.replace(
            "schema_version = 1\n",
            "schema_version = 1\n[model]\npath = \"model.py\"\n",
        ),
    )
    .unwrap();
    fs::write(
        scratch.0.join("model.py"),
        "import samplekit as sk\nclass M(sk.Sample):\n    @sk.figure\n    def look(self, ax):\n        pass\n",
    )
    .unwrap();
    fs::write(scratch.0.join("more/.samplekitrc"), "schema_version = 1\n").unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.cursor = workbench
        .view
        .iter()
        .position(|entry| entry.sample.borrow().name() == Some("D1"))
        .unwrap();
    keys(&mut workbench, "pj\n");
    assert!(
        workbench
            .message
            .contains("none of another project's samples"),
        "{}",
        workbench.message
    );
    assert!(workbench.drawing.is_none());
}

#[test]
fn a_remembered_column_no_sample_holds_is_set_aside_and_said() {
    let scratch = Scratch::new("stalecolumn");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    assert!(
        workbench
            .profile
            .columns
            .iter()
            .any(|column| column.field == "malt")
    );
    samplekit::tui::model::remember(&workbench);
    for name in ["C1", "C2", "C3"] {
        let path = scratch.0.join(format!("{name}.md"));
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace("malt:", "grist:");
        fs::write(&path, text).unwrap();
    }
    workbench = Workbench::open(&scratch.0).unwrap();
    assert!(
        !workbench
            .profile
            .columns
            .iter()
            .any(|column| column.field == "malt")
    );
    assert!(
        workbench.message.contains("no sample holds malt"),
        "{}",
        workbench.message
    );
}

#[test]
fn a_property_named_with_dots_takes_its_settings_by_the_last_dot() {
    let scratch = rich("dottedname");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "P");
    // Properties: the section list's order, found by name.
    let at = samplekit::tui::model::SECTIONS
        .iter()
        .position(|(title, _, _)| title.starts_with("propert"))
        .unwrap();
    for _ in 0..at {
        workbench.key(Key::Down);
    }
    workbench.key(Key::Right);
    keys(&mut workbench, "arun.load.symbol = \"L\"\n");
    let text = workbench.workspace.as_ref().unwrap().edit.text();
    assert!(text.contains("[property.\"run.load\"]"), "{text}");
    assert!(text.contains("symbol = \"L\""), "{text}");
}

#[test]
fn a_figure_of_ones_own_is_set_up_in_one_window_and_sorts_take_hidden_fields() {
    use samplekit::tui::model::{Choosing, FigureRow};
    let scratch = rich("ownfigure");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "p\n");
    assert!(matches!(workbench.mode, Mode::Figure));
    let pick = |workbench: &mut Workbench, field: &str| {
        let Mode::Picker { items, cursor, .. } = &mut workbench.mode else {
            panic!("a picker");
        };
        *cursor = items.iter().position(|(item, _)| item == field).unwrap();
        keys(workbench, "\n");
    };
    // y: values, then a table's whole columns, then its cells.
    keys(&mut workbench, "\n");
    let Mode::Picker { purpose, items, .. } = &workbench.mode else {
        panic!("a picker");
    };
    assert!(matches!(purpose, Choosing::FigureField(FigureRow::Y)));
    let column = items.iter().position(|(field, _)| field == "runs.load");
    let cell = items.iter().position(|(field, _)| field == "runs.load[1]");
    assert!(column.unwrap() < cell.unwrap(), "{items:?}");
    pick(&mut workbench, "malt");
    assert!(matches!(workbench.mode, Mode::Figure));
    keys(&mut workbench, "j\n");
    pick(&mut workbench, "beer");
    // The kind turns with the arrows; a title is typed.
    keys(&mut workbench, "jl");
    let form = workbench.figure.as_ref().unwrap();
    assert_eq!(form.kind.as_str(), "line");
    let title = form
        .rows()
        .iter()
        .position(|row| *row == FigureRow::Title)
        .unwrap();
    workbench.figure.as_mut().unwrap().row = title;
    keys(&mut workbench, "\nMalt by beer\n");
    let form = workbench.figure.as_ref().unwrap();
    assert_eq!(form.title, "Malt by beer");
    // What the window sets travels to Python: set on the declaration, the
    // scale, the style and the title were never sent.
    let scale = form
        .rows()
        .iter()
        .position(|row| *row == FigureRow::YScale)
        .unwrap();
    workbench.figure.as_mut().unwrap().row = scale;
    keys(&mut workbench, "l");
    let form = workbench.figure.as_ref().unwrap();
    let request = samplekit::presentation::plotting::FigureRequest {
        choice: samplekit::presentation::plotting::FigureChoice::AdHoc(Box::new(
            form.declaration("beer", "malt"),
        )),
        samples: Vec::new(),
        output: None,
        overrides: form.overrides(),
        project_style: true,
        said: None,
        snapshot: None,
    };
    let sent = samplekit::presentation::plotting::request_json(&request);
    assert!(sent.contains("\"y_scale\":\"log\""), "{sent}");
    assert!(sent.contains("\"title\":\"Malt by beer\""), "{sent}");
    assert!(sent.contains("\"style\":\"plain\""), "{sent}");
    // Drawn: no environment here, said, the window kept; closed, then
    // opened again on what was set up.
    keys(&mut workbench, "p");
    assert!(workbench.drawing.is_none());
    assert!(!workbench.message.is_empty());
    keys(&mut workbench, "\u{1b}p\n");
    assert_eq!(
        workbench.figure.as_ref().unwrap().y.as_deref(),
        Some("malt")
    );
    // A sort offers the fields no column shows.
    keys(&mut workbench, "\u{1b}s");
    let Mode::Picker { items, .. } = &workbench.mode else {
        panic!("the sort picker");
    };
    assert!(
        items.iter().any(|(field, _)| field == "runs.load[1]"),
        "{items:?}"
    );
}

#[test]
fn a_sample_is_removed_asked_first_and_given_back_by_u() {
    let scratch = Scratch::new("remove");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.cursor = workbench
        .view
        .iter()
        .position(|entry| entry.sample.borrow().name() == Some("C1"))
        .unwrap();
    let path = scratch.0.join("C1.md");
    let held = fs::read_to_string(&path).unwrap();
    keys(&mut workbench, "D");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("asked first: {}", workbench.message);
    };
    // Its image stays, and says so.
    assert!(
        lines.iter().any(|line| line.contains("own files stay")),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    assert!(!path.exists());
    assert_eq!(workbench.view.len(), 2, "{}", workbench.message);
    assert!(scratch.0.join("images/C1_sem.png").exists());
    keys(&mut workbench, "uy");
    assert_eq!(fs::read_to_string(&path).unwrap(), held);
    assert_eq!(workbench.view.len(), 3);
}

#[test]
fn an_empty_project_is_the_default_and_its_first_sample_made() {
    let root = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-workbench-empty-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let mut workbench = Workbench::open(&root).unwrap();
    assert!(matches!(workbench.screen, Screen::Setup { .. }));
    // Empty and a model, as the defaults; Esc goes back a question, its
    // answer kept; no environment: a test makes none.
    keys(&mut workbench, "\n");
    workbench.key(Key::Esc);
    assert_eq!(workbench.setup.as_ref().unwrap().question, 0);
    keys(&mut workbench, "\n\n");
    workbench.key(Key::Down);
    keys(&mut workbench, "\n");
    keys(&mut workbench, "\ny");
    assert!(root.join("model/main.py").exists(), "{}", workbench.message);
    assert!(root.join("samples").is_dir());
    assert!(!root.join("samples/EXAMPLE.md").exists());
    // Two files, samples/ not counted among them.
    assert!(
        workbench.message.starts_with("2 files written"),
        "{}",
        workbench.message
    );
    assert!(
        workbench.message.contains("N makes a first sample"),
        "{}",
        workbench.message
    );
    // The first sample, from nothing, into samples/.
    keys(&mut workbench, "NS-01\ny");
    assert!(
        root.join("samples/S-01.md").exists(),
        "{}",
        workbench.message
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_table_is_drawn_from_its_view_and_a_figure_takes_a_cell() {
    use samplekit::tui::model::{Choosing, FigureRow};
    let scratch = rich("tablefigure");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    // Over several samples, a table's cell is an axis, a point per sample.
    keys(&mut workbench, "p\n\n");
    let Mode::Picker { purpose, items, .. } = &workbench.mode else {
        panic!("a picker");
    };
    assert!(matches!(purpose, Choosing::FigureField(FigureRow::Y)));
    assert!(
        items.iter().any(|(field, _)| field == "runs.load[1]"),
        "{items:?}"
    );
    keys(&mut workbench, "\u{1b}\u{1b}");
    // Over one sample's table: its columns alone, the one selected first, and
    // no group to choose.
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "runs");
    keys(&mut workbench, "l");
    workbench.selected.insert("runs.load".to_string());
    keys(&mut workbench, "p");
    assert!(
        matches!(workbench.mode, Mode::Figure),
        "{}",
        workbench.message
    );
    let form = workbench.figure.as_ref().unwrap();
    assert!(!form.rows().contains(&FigureRow::Group));
    keys(&mut workbench, "\n");
    let Mode::Picker { items, .. } = &workbench.mode else {
        panic!("a picker: {}", workbench.message);
    };
    let offered: Vec<&str> = items.iter().map(|(field, _)| field.as_str()).collect();
    assert_eq!(offered, ["runs.load", "runs.run"]);
    // runs.load for y; then x, runs.run.
    keys(&mut workbench, "\nj\nj\n");
    let form = workbench.figure.as_ref().unwrap();
    assert_eq!(form.y.as_deref(), Some("runs.load"));
    assert_eq!(form.x.as_deref(), Some("runs.run"));
    // Drawn: no environment here, said, and the window stays.
    keys(&mut workbench, "p");
    assert!(matches!(workbench.mode, Mode::Figure));
    assert!(!workbench.message.is_empty());
}

#[test]
fn a_runs_progress_and_outcome_stay_whatever_is_pressed() {
    // What a computation says was a message, taken away by the next key: its
    // progress and its outcome are the status line's now.
    use samplekit::tui::model::{Progress, Running};
    let scratch = Scratch::new("runstatus");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    workbench.running = Some(Running {
        receiver,
        before: None,
    });
    sender
        .send(Progress::At {
            done: 1,
            total: 3,
            now: "C2 · malt".to_string(),
        })
        .unwrap();
    workbench.tick();
    let run = workbench.run.clone().unwrap();
    assert_eq!((run.done, run.total, run.now.as_str()), (1, 3, "C2 · malt"));
    keys(&mut workbench, "j\n");
    assert_eq!(workbench.run.as_ref().unwrap().done, 1);
    sender
        .send(Progress::Done("2 computed, 1 failed".to_string()))
        .unwrap();
    workbench.tick();
    keys(&mut workbench, "\u{1b}jk");
    let run = workbench.run.as_ref().unwrap();
    assert_eq!(run.outcome.as_deref(), Some("2 computed, 1 failed"));
    assert!(workbench.running.is_none());
}

#[test]
fn what_is_shown_is_saved_as_a_query_a_profile_an_export_and_a_figure() {
    // An export of the filter, its profile and its query beside it, previewed,
    // written, and taken back by u; then the figure set up.
    use samplekit::tui::model::{Choosing, Declare};
    let scratch = Scratch::new("declare");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.filter = "beer == schwarz".to_string();
    workbench.refresh().unwrap();
    let rc = scratch.0.join(".samplekitrc");
    let before = fs::read_to_string(&rc).unwrap();
    keys(&mut workbench, "W");
    let Mode::Picker {
        purpose: Choosing::Declare(offered),
        cursor,
        ..
    } = &mut workbench.mode
    else {
        panic!("the choices: {}", workbench.message);
    };
    *cursor = offered
        .iter()
        .position(|what| *what == Declare::Export("csv"))
        .unwrap();
    keys(&mut workbench, "\nbocks\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines.iter().any(|line| line.contains("[export.bocks]")),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    let written = fs::read_to_string(&rc).unwrap();
    for part in [
        "[query.bocks]",
        "[profile.bocks]",
        "[export.bocks]",
        "out/bocks.csv",
    ] {
        assert!(written.contains(part), "{part}: {written}");
    }
    samplekit::config::project_config::load(&rc).unwrap();
    // A name taken is refused.
    keys(&mut workbench, "W\nbocks\n");
    assert!(
        workbench.message.contains("declared already"),
        "{}",
        workbench.message
    );
    // u takes it back.
    keys(&mut workbench, "\u{1b}uy");
    assert_eq!(fs::read_to_string(&rc).unwrap(), before);
    // The figure's window saves the figure.
    keys(&mut workbench, "p\n");
    let figure = workbench.figure.as_mut().unwrap();
    figure.y = Some("malt".to_string());
    figure.x = Some("beer".to_string());
    keys(&mut workbench, "wmalts\ny");
    let written = fs::read_to_string(&rc).unwrap();
    assert!(
        written.contains("[figure.malts]") && written.contains("y = \"malt\""),
        "{written}"
    );
}

#[test]
fn a_change_the_tui_writes_is_a_snapshot_saying_tui() {
    // A change written from the TUI is a snapshot saying what the TUI did,
    // after one of what changed before it — `tui`, its name.
    let scratch = Scratch::new("history");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "\n");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e");
    for _ in 0..8 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "12.5\ny");
    assert!(
        workbench.message.starts_with("written"),
        "{}",
        workbench.message
    );
    let output = std::process::Command::new("git")
        .arg("--git-dir")
        .arg(scratch.0.join(".samplekit/history"))
        .args(["log", "--format=%s"])
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&output.stdout).into_owned();
    let messages: Vec<&str> = log.lines().collect();
    assert_eq!(messages.len(), 2, "{log}");
    assert!(messages[0].starts_with("tui · "), "{log}");
    assert_eq!(messages[0], "tui · change: written C1.md", "{log}");
    assert_eq!(messages[1], "the project as SampleKit first kept it");
}

#[test]
fn an_undo_outlives_the_workbench() {
    // Closed and opened again, `u` offers the last change the history kept,
    // restores its files, and does not offer it twice.
    let scratch = Scratch::new("history-undo");
    let before = fs::read_to_string(scratch.0.join("C1.md")).unwrap();
    {
        let mut workbench = Workbench::open(&scratch.0).unwrap();
        keys(&mut workbench, "\n");
        move_to(&mut workbench, "malt");
        keys(&mut workbench, "e");
        for _ in 0..8 {
            workbench.key(Key::Backspace);
        }
        keys(&mut workbench, "12.5\ny");
        assert!(
            workbench.message.starts_with("written"),
            "{}",
            workbench.message
        );
    }
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "u");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("an undo offered: {}", workbench.message);
    };
    assert!(lines[0].starts_with("kept in the history, "), "{lines:?}");
    assert!(
        lines[0].contains("tui · change: written C1.md"),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    assert_eq!(fs::read_to_string(scratch.0.join("C1.md")).unwrap(), before);
    let mut again = Workbench::open(&scratch.0).unwrap();
    keys(&mut again, "u");
    assert_eq!(again.message, "nothing to undo");
}

#[test]
fn the_history_is_browsed_snapshot_by_snapshot() {
    // `H` lists the snapshots newest first, each with what it changed; from a
    // sample, only those that changed it; Back returns where it was.
    let scratch = Scratch::new("history-screen");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "H");
    assert!(
        workbench.message.contains("no history is kept here yet"),
        "{}",
        workbench.message
    );
    keys(&mut workbench, "\n");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e");
    for _ in 0..8 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "12.5\ny");
    keys(&mut workbench, "H");
    assert!(matches!(
        workbench.screen,
        Screen::History {
            sample: Some(_),
            ..
        }
    ));
    assert_eq!(workbench.history.len(), 2, "{:?}", workbench.history);
    let newest = &workbench.history[0];
    assert_eq!(newest.number, 1);
    assert_eq!(newest.message, "tui · change: written C1.md");
    assert!(
        newest
            .lines
            .iter()
            .any(|line| line.contains("malt") && line.contains("12.5")),
        "{:?}",
        newest.lines
    );
    keys(&mut workbench, "j");
    assert!(matches!(
        workbench.screen,
        Screen::History { cursor: 1, .. }
    ));
    workbench.key(Key::Esc);
    assert!(matches!(workbench.screen, Screen::Sample { .. }));
    workbench.key(Key::Esc);
    keys(&mut workbench, "H");
    assert!(matches!(
        workbench.screen,
        Screen::History { sample: None, .. }
    ));
    assert_eq!(
        workbench.history[1].message,
        "the project as SampleKit first kept it"
    );
}

#[test]
fn the_filter_finds_a_cell_by_letters_of_its_whole_address() {
    // `eps20` did not find `measurements.ebc[20]`: only the word after the
    // last `.` was matched, against the names at that level.
    let scratch = rich("filter-deep");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "/load2");
    let Mode::Filter { candidates, .. } = &workbench.mode else {
        panic!("filtering");
    };
    assert_eq!(
        candidates.first().map(String::as_str),
        Some("runs.load[2]"),
        "{candidates:?}"
    );
    // After a clause, what is before the word is kept.
    keys(&mut workbench, "\u{1b}/malt > 1 && ld1");
    let Mode::Filter { candidates, .. } = &workbench.mode else {
        panic!("filtering");
    };
    assert!(
        candidates.contains(&"malt > 1 && runs.load[1]".to_string()),
        "{candidates:?}"
    );
}

#[test]
fn no_key_does_two_things_in_one_place() {
    // `H`, the history, beside `h`, going back: a key is one action where it
    // is read, and the history is said at the foot where it is reached.
    use samplekit::tui::model::{Action, BINDINGS, Place, hinted};
    let mut seen = std::collections::HashSet::new();
    for (place, key, action, _) in BINDINGS {
        assert!(
            seen.insert((format!("{place:?}"), format!("{key:?}"))),
            "{place:?} {key:?} is bound twice, the second to {action:?}"
        );
    }
    for place in [Place::Collection, Place::Sample, Place::Table] {
        assert!(hinted(place).contains(&Action::History), "{place:?}");
    }
    let scratch = rich("history-from-table");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "runs");
    workbench.key(Key::Right);
    assert!(matches!(workbench.screen, Screen::Table { .. }));
    // Nothing written yet: the history says so, rather than `H` doing nothing.
    keys(&mut workbench, "H");
    assert!(
        matches!(
            workbench.screen,
            Screen::History {
                sample: Some(_),
                ..
            }
        ) || workbench.message.starts_with("no history is kept here yet"),
        "{}",
        workbench.message
    );
}

/// The history's messages, newest first.
fn history_log(root: &std::path::Path) -> Vec<String> {
    let output = std::process::Command::new("git")
        .arg("--git-dir")
        .arg(root.join(".samplekit/history"))
        .args(["log", "--format=%s"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn quitting_while_something_runs_asks_and_waits_for_its_snapshot() {
    // Quit mid-computation, the workbench was dropped: the run's snapshot
    // lost, its worker writing on unseen.
    use samplekit::config::version_control;
    use samplekit::tui::model::{Progress, Running};
    let scratch = Scratch::new("quit-running");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    workbench.running = Some(Running {
        receiver,
        before: None,
    });
    let (writing, _) = version_control::before_writing(std::slice::from_ref(&scratch.0));
    workbench.computing_history = Some(writing);
    sender
        .send(Progress::At {
            done: 1,
            total: 3,
            now: "C2 · malt".to_string(),
        })
        .unwrap();
    workbench.tick();
    // Asked, what is under way said; `n` stays.
    assert_eq!(workbench.key(Key::Char('q')), Effect::None);
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("asked first");
    };
    assert!(
        lines
            .iter()
            .any(|line| line.contains("a computation, 1 of 3 values done · C2 · malt")),
        "{lines:?}"
    );
    keys(&mut workbench, "n");
    assert!(!workbench.leaving);
    // `y` leaves once the run has ended, not before; a key meanwhile stays.
    assert_eq!(keys(&mut workbench, "qy"), [Effect::None, Effect::None]);
    assert!(workbench.leaving && !workbench.left());
    keys(&mut workbench, "j");
    assert!(!workbench.leaving, "{}", workbench.message);
    keys(&mut workbench, "qy");
    // The worker writes, then ends: its snapshot is taken, then it leaves.
    let path = scratch.0.join("C1.md");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(&path, text.replace("v: 12.0", "v: 12.25")).unwrap();
    sender
        .send(Progress::Done("1 computed".to_string()))
        .unwrap();
    workbench.tick();
    assert!(workbench.left());
    assert!(workbench.computing_history.is_none());
    let log = history_log(&scratch.0);
    assert_eq!(log[0], "tui · compute: 1 computed", "{log:?}");
    // Nothing under way, `q` quits at once; a figure's window is said.
    let mut idle = Workbench::open(&scratch.0).unwrap();
    assert_eq!(idle.key(Key::Char('q')), Effect::Quit);
    let (_sender, receiver) = std::sync::mpsc::channel::<Progress>();
    idle.drawing = Some(Running {
        receiver,
        before: None,
    });
    idle.key(Key::Char('q'));
    let Mode::Confirm { lines, .. } = &idle.mode else {
        panic!("asked first");
    };
    assert!(
        lines.iter().any(|line| line.contains("a figure's window")),
        "{lines:?}"
    );
}

#[test]
fn the_filters_fields_are_read_once_per_load() {
    let scratch = Scratch::new("fields-once");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    assert!(workbench.described.get().is_none());
    keys(&mut workbench, "/mal");
    assert!(workbench.described.get().is_some());
    assert!(!workbench.described_fields().contains(&"plato".to_string()));
    keys(&mut workbench, "\u{1b}");
    // A field added in an editor: the collection read again, its fields too.
    let path = scratch.0.join("C1.md");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        text.replace("properties:\n", "properties:\n  plato: {v: 7.8}\n"),
    )
    .unwrap();
    workbench.reload();
    assert!(workbench.described.get().is_none());
    keys(&mut workbench, "/plto");
    let Mode::Filter { candidates, .. } = &workbench.mode else {
        panic!("the filter");
    };
    assert!(
        candidates.iter().any(|candidate| candidate == "plato"),
        "{candidates:?}"
    );
}

#[test]
fn e_on_a_table_says_how_a_table_is_changed() {
    let scratch = rich("e-table");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "runs");
    keys(&mut workbench, "e");
    assert!(matches!(workbench.mode, Mode::Normal));
    assert!(
        workbench.message.contains("→ opens it") && workbench.message.contains("+ adds a row"),
        "{}",
        workbench.message
    );
    assert!(!workbench.message.contains("samplekit set"));
}

#[test]
fn a_decimal_comma_or_a_unit_typed_is_said() {
    let scratch = rich("decimal-comma");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    // A row: `load=4,5` read as a cell `5`, no change of anything.
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "runs");
    keys(&mut workbench, "l+run=3, load=4,5\n");
    assert!(
        workbench.message.contains("decimal comma") && workbench.message.contains("load=4.5"),
        "{}",
        workbench.message
    );
    // A quantity's value: a decimal comma, then a unit.
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e");
    let Mode::Quantity { slots, .. } = &workbench.mode else {
        panic!("the quantity's window");
    };
    assert_eq!(slots[0].unit, "g");
    workbench.key(Key::End);
    for _ in 0..8 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "21,5\n");
    assert!(
        workbench
            .message
            .contains("use a point for decimals — 21.5"),
        "{}",
        workbench.message
    );
    assert!(matches!(workbench.mode, Mode::Quantity { .. }));
    for _ in 0..4 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "21 g\n");
    assert!(
        workbench
            .message
            .contains("write the number alone, 21: it is in g"),
        "{}",
        workbench.message
    );
}

#[test]
fn a_sample_screen_moves_by_page_and_to_its_ends() {
    let scratch = rich("sample-ends");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    let last = workbench.entries().len() - 1;
    let cursor = |workbench: &Workbench| match workbench.screen {
        Screen::Sample { cursor, .. } => cursor,
        _ => panic!("the sample"),
    };
    workbench.key(Key::End);
    assert_eq!(cursor(&workbench), last);
    workbench.key(Key::Home);
    assert_eq!(cursor(&workbench), 0);
    workbench.page = 2;
    workbench.key(Key::PageDown);
    assert_eq!(cursor(&workbench), last.min(2));
    workbench.key(Key::PageUp);
    assert_eq!(cursor(&workbench), 0);
}

#[test]
fn readings_beside_a_value_typed_say_it_stays() {
    // The preview said their statistic became the value, where the value
    // typed stays and outranks them, as `set --readings` says.
    let scratch = Scratch::new("readings-stay");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e\t\t11, 13\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    let said = lines.join("\n");
    assert!(said.contains("the value 12.0 stays"), "{said}");
    assert!(!said.contains("their statistic is its value"), "{said}");
}

#[test]
fn c_over_a_value_typed_asks_first() {
    let scratch = Scratch::new("c-override");
    fs::write(
        scratch.0.join("S.md"),
        "---\nschema_version: 1\nname: S\nproperties:\n  \
         malt: {v: 13.0, fingerprint: aaaaaaaaaaaa}\n  twice:\n    \
         v: 3.0\n    computed: {malt: bbbbbbbbbbbb}\n    fingerprint: cccccccccccc\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "S");
    move_to(&mut workbench, "twice");
    keys(&mut workbench, "c");
    let Mode::Confirm { title, lines, .. } = &workbench.mode else {
        panic!("asked first: {}", workbench.message);
    };
    assert_eq!(title, "compute over a value typed");
    assert!(lines.iter().any(|line| line.contains("twice")), "{lines:?}");
    keys(&mut workbench, "n");
    assert!(workbench.running.is_none());
    assert!(
        fs::read_to_string(scratch.0.join("S.md"))
            .unwrap()
            .contains("v: 3.0")
    );
}

#[test]
fn an_undo_names_the_values_going_back_and_is_kept_as_the_tuis() {
    // Its snapshot says `tui · undo`, the TUI's name.
    let scratch = Scratch::new("undo-values");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e");
    workbench.key(Key::End);
    for _ in 0..8 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "12.5\ny");
    keys(&mut workbench, "u");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("an undo: {}", workbench.message);
    };
    assert_eq!(lines[0], "restore C1.md", "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|line| line.contains("malt") && line.contains("12.5") && line.contains('→')),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    // Kept as an undo: never offered back, never read as a change made
    // outside SampleKit.
    let log = history_log(&scratch.0);
    assert!(log[0].starts_with("tui · undo · "), "{log:?}");
    assert!(
        !log.iter().any(|message| message.contains("outside")),
        "{log:?}"
    );
    let mut again = Workbench::open(&scratch.0).unwrap();
    keys(&mut again, "u");
    assert_eq!(again.message, "nothing to undo");
}

#[test]
fn help_says_what_q_and_g_do_and_what_the_marks_mean() {
    let scratch = Scratch::new("help-says");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let said = |workbench: &Workbench, key: &str| {
        workbench
            .help_sections()
            .into_iter()
            .flat_map(|(_, rows)| rows)
            .find(|(keys, _)| keys == key)
            .map(|(_, said)| said)
            .unwrap()
    };
    assert_eq!(said(&workbench, "f"), "apply a saved query or a profile");
    assert_eq!(
        said(&workbench, "g"),
        "group by fields, a heading above each group"
    );
    assert_eq!(said(&workbench, "q"), "quit");
    workbench.from_start = true;
    assert!(said(&workbench, "q").starts_with("back to the start page"));
    // The marks, in a section of their own, on the collection and a sample.
    for place in ["", "\n"] {
        keys(&mut workbench, place);
        let sections = workbench.help_sections();
        let (_, marks) = sections
            .iter()
            .find(|(title, _)| *title == "marks")
            .expect("the marks");
        for mark in ["●", "⚠", "✎", "✗", "∅", "·"] {
            assert!(marks.iter().any(|(shown, _)| shown == mark), "{marks:?}");
        }
    }
}

/// A plan as the model's worker gives it, for a test that runs no Python.
fn planned(
    value: &str,
    reason: samplekit::config::model_runtime::Reason,
    said: &str,
) -> samplekit::config::model_runtime::Planned {
    samplekit::config::model_runtime::Planned {
        value: value.to_string(),
        reason,
        said: said.to_string(),
    }
}

#[test]
fn what_the_model_owes_is_said_never_computed_or_waiting() {
    // `status` listed values never computed and values waiting; the workbench,
    // reading the files alone, showed none of them. `∅` is said one way: never
    // computed, or waits for an input.
    use samplekit::config::model_runtime::Reason;
    use samplekit::tui::model::{Concern, Planning};
    let scratch = rich("owed");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let d1 = scratch.0.join("more/D1.md");
    let c2 = scratch.0.join("C2.md");
    workbench.take_plan(Planning {
        plans: vec![
            (
                d1.clone(),
                vec![
                    planned("abv", Reason::NeverComputed, "never computed"),
                    planned("current_one", Reason::Current, "current"),
                ],
            ),
            (
                c2.clone(),
                vec![
                    planned("malt", Reason::NeverComputed, "never computed"),
                    planned("fg", Reason::Waiting, "waits for og"),
                ],
            ),
        ],
        unread: Vec::new(),
    });
    // Marked in the collection, where the file says nothing worse.
    let mark = |workbench: &Workbench, name: &str| {
        let at = workbench
            .view
            .iter()
            .position(|entry| entry.sample.borrow().name() == Some(name))
            .unwrap();
        workbench.rendered[at].mark
    };
    assert_eq!(mark(&workbench, "D1"), "∅");
    assert_eq!(mark(&workbench, "C2"), "∅");
    assert_eq!(mark(&workbench, "C1"), " ");
    // Listed and counted on the control screen.
    keys(&mut workbench, "v");
    let owed: Vec<String> = workbench
        .control
        .iter()
        .filter(|item| item.kind == Concern::Owed)
        .map(|item| format!("{} {}", item.sample, item.said))
        .collect();
    assert!(
        owed.contains(&"D1 abv  never computed".to_string()),
        "{owed:?}"
    );
    // An entered value a formula owes: its uncertainty.
    assert!(
        owed.contains(&"C2 malt  uncertainty never computed".to_string()),
        "{owed:?}"
    );
    assert!(
        owed.contains(&"C2 fg  waits for og".to_string()),
        "{owed:?}"
    );
    assert!(
        !owed.iter().any(|said| said.contains("current_one")),
        "{owed:?}"
    );
    // C computes the samples with something never computed — here nothing
    // runs, no model being declared, which is said — and not one only waiting.
    keys(&mut workbench, "C");
    assert!(
        workbench.message.contains("no model is declared"),
        "{}",
        workbench.message
    );
    // The sample shows each, a value it does not hold on a line of its own.
    keys(&mut workbench, "\u{1b}");
    open_named(&mut workbench, "D1");
    let entries = workbench.entries();
    let state = |field: &str| {
        entries
            .iter()
            .find(|entry| entry.field == field)
            .map(|entry| entry.state.clone())
            .unwrap_or_default()
    };
    assert_eq!(state("abv"), "never computed", "{entries:?}");
    // Readings with no statistic recorded, the model read: `c` no longer
    // says that a model chooses one, beside the model that does.
    assert_eq!(state("malt"), "no statistic", "{entries:?}");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "c");
    assert!(
        workbench.message.contains("the model's") && !workbench.message.contains("chooses one"),
        "{}",
        workbench.message
    );
    keys(&mut workbench, "\u{1b}");
    open_named(&mut workbench, "C2");
    let entries = workbench.entries();
    assert!(
        entries
            .iter()
            .any(|entry| entry.field == "malt" && entry.state == "uncertainty never computed"),
        "{entries:?}"
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.field == "fg" && entry.state == "waits for og"),
        "{entries:?}"
    );
}

#[test]
fn esc_from_the_history_returns_where_h_was_pressed() {
    // Esc came back to the sample's first line, and from a table to the
    // sample rather than to the table.
    let scratch = rich("history-back");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "runs");
    workbench.key(Key::Right);
    keys(&mut workbench, "e");
    workbench.key(Key::End);
    for _ in 0..8 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "3.75\ny");
    keys(&mut workbench, "jl");
    let Screen::Table { cursor, column, .. } = &workbench.screen else {
        panic!("the table");
    };
    let (cursor, column) = (*cursor, *column);
    assert_eq!(cursor, 1);
    keys(&mut workbench, "H");
    assert!(
        matches!(workbench.screen, Screen::History { .. }),
        "{}",
        workbench.message
    );
    workbench.key(Key::Esc);
    let Screen::Table {
        cursor: back,
        column: back_column,
        ..
    } = &workbench.screen
    else {
        panic!("back to the table");
    };
    assert_eq!((*back, *back_column), (cursor, column));
    // From the sample, on the line it was on.
    keys(&mut workbench, "\u{1b}");
    let Screen::Sample { cursor, .. } = workbench.screen else {
        panic!("the sample");
    };
    assert!(cursor > 0);
    keys(&mut workbench, "H\u{1b}");
    assert!(matches!(workbench.screen, Screen::Sample { cursor: back, .. } if back == cursor));
}

#[test]
fn a_refused_line_keeps_its_prompt_and_its_text() {
    // Closed on a refusal, the text was lost, and what was typed again fell
    // on the screen as its keys.
    let scratch = rich("prompt-kept");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "runs");
    workbench.key(Key::Right);
    keys(&mut workbench, "+run=3, load=4,5\n");
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("the prompt stays: {}", workbench.message);
    };
    assert_eq!(text, "run=3, load=4,5");
    assert!(
        workbench.message.contains("decimal comma"),
        "{}",
        workbench.message
    );
    // Typed on, into the prompt: `D` removes nothing.
    keys(&mut workbench, "D");
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("still the prompt");
    };
    assert!(text.text().ends_with('D'), "{text}");
    // A name refused, the same.
    keys(&mut workbench, "\u{1b}\u{1b}\u{1b}N../away\n");
    assert!(
        matches!(workbench.mode, Mode::Prompt { .. }),
        "{}",
        workbench.message
    );
}

#[test]
fn a_filters_decimal_comma_is_said_and_the_filter_in_force_named() {
    let scratch = Scratch::new("filter-comma");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "/beer == schwarz\n");
    keys(&mut workbench, "/malt > 10,5");
    let Mode::Filter {
        candidates, error, ..
    } = &workbench.mode
    else {
        panic!("the filter");
    };
    // No completion of its own after the comma: `malt > 10,10.0`.
    assert!(candidates.is_empty(), "{candidates:?}");
    let error = error.clone().unwrap_or_default();
    assert!(error.contains("use a point for decimals"), "{error}");
    assert!(error.contains("10.5"), "{error}");
    keys(&mut workbench, "\n");
    let Mode::Filter { error, .. } = &workbench.mode else {
        panic!("the filter stays open");
    };
    let error = error.clone().unwrap_or_default();
    assert!(error.contains("use a point for decimals"), "{error}");
    assert!(
        error.contains("the filter in force is still beer == schwarz"),
        "{error}"
    );
    assert!(!error.contains("set aside"), "{error}");
}

#[test]
fn numbers_are_said_at_their_precision_not_as_a_sum_left_them() {
    let scratch = Scratch::new("precision");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[property.abv]\nprecision = \".2f\"\n\
         [property.malt]\nprecision = \".1f\"\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join("S.md"),
        "---\nschema_version: 1\nname: S\nproperties:\n  \
         abv: {v: 4.375000000000013, u: 0.2886751345948129}\n  \
         malt: {v: 9.0, readings: [8.0, 10.0]}\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "S");
    move_to(&mut workbench, "abv");
    // The window holds what the file holds, its noise dropped.
    keys(&mut workbench, "e");
    let Mode::Quantity { slots, .. } = &workbench.mode else {
        panic!("the quantity's window");
    };
    assert_eq!(slots[0].text, "4.375");
    assert_eq!(slots[1].text, "0.288675134595");
    // The preview, at the precision the screen shows it.
    workbench.key(Key::End);
    for _ in 0..8 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "5.5\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines.iter().any(|line| line.contains("4.38  →  5.50")),
        "{lines:?}"
    );
    keys(&mut workbench, "n");
    // A completion compared by order, at the field's precision.
    keys(&mut workbench, "\u{1b}/abv > ");
    let Mode::Filter { candidates, .. } = &workbench.mode else {
        panic!("the filter");
    };
    assert!(
        candidates.iter().any(|candidate| candidate == "abv > 4.38"),
        "{candidates:?}"
    );
    assert!(
        !candidates
            .iter()
            .any(|candidate| candidate.contains("4.375000000000013")),
        "{candidates:?}"
    );
}

#[test]
fn plurals_follow_their_count() {
    let scratch = Scratch::new("plurals");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "D");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("asked first");
    };
    assert!(
        lines
            .iter()
            .any(|line| line == "u gives it back, as it was"),
        "{lines:?}"
    );
    keys(&mut workbench, "yuy");
    assert_eq!(
        workbench.message, "undone: 1 file as it was",
        "{}",
        workbench.message
    );
}

#[test]
fn removing_the_sample_open_says_so_as_the_collection_does() {
    let scratch = Scratch::new("remove-open");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C2");
    keys(&mut workbench, "Dy");
    assert!(matches!(workbench.screen, Screen::Collection));
    assert_eq!(workbench.message, "removed 1 sample · u gives it back");
}

#[test]
fn results_name_paths_from_the_workbenchs_folder() {
    let scratch = rich("paths-said");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "beer");
    keys(&mut workbench, "e");
    workbench.key(Key::End);
    for _ in 0..8 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "schwarz\ny");
    assert_eq!(workbench.message.replace('\\', "/"), "written: more/D1.md");
    keys(&mut workbench, "\u{1b}NC9\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        !lines[0].contains(&scratch.0.display().to_string()),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    assert!(
        workbench.message.starts_with("created: "),
        "{}",
        workbench.message
    );
    assert!(
        !workbench.message.contains(&scratch.0.display().to_string()),
        "{}",
        workbench.message
    );
}

#[test]
fn a_sample_opened_from_the_control_screen_goes_back_to_it() {
    let scratch = rich("control-back");
    fs::write(
        scratch.0.join("C3.md"),
        "---\nschema_version: 1\nname: C3\nbeer: schwarz\nproperties:\n  \
         malt: {v: 11.0, u: 0.1, unit: g, fingerprint: 000000000000}\n  \
         twice: {v: 22.0, computed: {malt: 111111111111}, fingerprint: 222222222222}\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "v\n");
    assert!(matches!(workbench.screen, Screen::Sample { .. }));
    workbench.key(Key::Esc);
    assert!(matches!(workbench.screen, Screen::Control { cursor: 0 }));
    // From the collection, back to the collection.
    workbench.key(Key::Esc);
    keys(&mut workbench, "\n\u{1b}");
    assert!(matches!(workbench.screen, Screen::Collection));
}

#[test]
fn the_pickers_offer_a_list_not_its_items() {
    let scratch = Scratch::new("list-fields");
    fs::write(
        scratch.0.join("H.md"),
        "---\nschema_version: 1\nname: H\nhops: [saaz, hallertau, fuggle]\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "c");
    let Mode::Picker { items, .. } = &workbench.mode else {
        panic!("the columns");
    };
    let labels: Vec<&str> = items.iter().map(|(label, _)| label.as_str()).collect();
    assert!(labels.contains(&"hops"), "{labels:?}");
    assert!(
        !labels.iter().any(|label| label.contains("[#")),
        "{labels:?}"
    );
    keys(&mut workbench, "\u{1b}s");
    let Mode::Picker { items, .. } = &workbench.mode else {
        panic!("the sort");
    };
    let labels: Vec<&str> = items.iter().map(|(label, _)| label.as_str()).collect();
    assert!(labels.contains(&"hops[#0]"), "{labels:?}");
    assert!(!labels.contains(&"hops[#1]"), "{labels:?}");
    // And `p` offers a figure of one's own in plain words.
    keys(&mut workbench, "\u{1b}p");
    let Mode::Picker { items, .. } = &workbench.mode else {
        panic!("the figures");
    };
    assert!(!items[0].0.contains("plot -x"), "{:?}", items[0]);
}

#[test]
fn c_runs_the_model_without_asking() {
    // No window before the model runs; the message names its file.
    let scratch = Scratch::new("no-asking");
    fs::write(scratch.0.join("interpreter"), "").unwrap();
    fs::write(
        scratch.0.join("model.py"),
        "import samplekit as sk\nclass M(sk.Sample):\n    def __init__(self):\n        \
         self.malt = sk.Property()\n        self.plato = sk.Property(compute=self._d)\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \"interpreter\"\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "C");
    assert!(matches!(workbench.mode, Mode::Normal), "a window opened");
    assert!(workbench.running.is_some(), "{}", workbench.message);
    assert!(
        workbench.message.contains("running the model model.py"),
        "{}",
        workbench.message
    );
}

#[test]
fn ctrl_c_while_something_runs_asks_and_a_second_quits_at_once() {
    // Ctrl+C dropped a computation as `q` no longer did, its snapshot lost and
    // its worker writing on unseen.
    use samplekit::tui::model::{Progress, Running};
    let scratch = Scratch::new("interrupt");
    // Nothing under way: it ends at once.
    let mut idle = Workbench::open(&scratch.0).unwrap();
    assert!(idle.interrupt());
    assert!(idle.interrupted);
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    workbench.running = Some(Running {
        receiver,
        before: None,
    });
    sender
        .send(Progress::At {
            done: 1,
            total: 3,
            now: "C2 · malt".to_string(),
        })
        .unwrap();
    workbench.tick();
    // Asked as `q` asks, what runs said, and what a second Ctrl+C does.
    assert!(!workbench.interrupt());
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("asked first");
    };
    assert!(
        lines
            .iter()
            .any(|line| line.contains("a computation, 1 of 3 values done")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("Ctrl+C again")),
        "{lines:?}"
    );
    // `n` stays, and the next Ctrl+C is a first again.
    keys(&mut workbench, "n");
    assert!(!workbench.interrupted);
    assert!(!workbench.interrupt());
    // A second, at once.
    assert!(workbench.interrupt());
    // `y` waits for the run to end, then leaves as an interrupt.
    let mut waiting = Workbench::open(&scratch.0).unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    waiting.running = Some(Running {
        receiver,
        before: None,
    });
    assert!(!waiting.interrupt());
    keys(&mut waiting, "y");
    assert!(waiting.leaving && !waiting.left());
    sender
        .send(Progress::Done("1 computed".to_string()))
        .unwrap();
    waiting.tick();
    assert!(waiting.left() && waiting.interrupted);
}

#[test]
fn n_carries_neither_dates_nor_status() {
    // A new sample came with the date its pattern was made and its `status:
    // approved`, as if they were its own.
    let scratch = Scratch::new("new-own");
    let path = scratch.0.join("C1.md");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        text.replace(
            "beer: schwarz\n",
            "beer: schwarz\nmade: 2026-09-01\nstatus: approved\n",
        ),
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    keys(&mut workbench, "\u{1b}NC9\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines
            .iter()
            .any(|line| line.contains("not carried") && line.contains("made, status")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("carried") && line.contains("beer = schwarz")),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    let written = fs::read_to_string(scratch.0.join("C9.md")).unwrap();
    assert!(written.contains("beer: schwarz"), "{written}");
    assert!(!written.contains("made"), "{written}");
    assert!(!written.contains("status"), "{written}");
}

#[test]
fn a_filter_matching_nothing_offers_the_nearest_value() {
    // `beer == schwarc` said `0 of 3 samples`, and nothing of why.
    let scratch = Scratch::new("filter-nearest");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "/beer == schwarc\n");
    assert!(workbench.view.is_empty());
    assert!(
        workbench
            .message
            .contains("beer == schwarc: did you mean schwarz?"),
        "{}",
        workbench.message
    );
    // Within a longer filter, the clause that matched nothing is named.
    keys(&mut workbench, "\u{1b}/malt > 1 && beer == schwarc\n");
    assert!(
        workbench
            .message
            .contains("· beer == schwarc: did you mean schwarz?"),
        "{}",
        workbench.message
    );
    // Nothing near, nothing offered; a number is offered nothing.
    keys(&mut workbench, "\u{1b}/beer == zirconium\n");
    assert!(workbench.view.is_empty());
    assert!(
        !workbench.message.contains("did you mean"),
        "{}",
        workbench.message
    );
    keys(&mut workbench, "\u{1b}/malt == 12.5\n");
    assert!(workbench.view.is_empty());
    assert!(
        !workbench.message.contains("did you mean"),
        "{}",
        workbench.message
    );
}

#[test]
fn several_projects_are_shown_one_after_the_other() {
    // Over several projects, the command line gives a table each; the
    // workbench's list is split the same way.
    let scratch = rich("projects-apart");
    fs::write(scratch.0.join("more/.samplekitrc"), "schema_version = 1\n").unwrap();
    fs::write(
        scratch.0.join("more/D0.md"),
        "---\nschema_version: 1\nname: D0\nbeer: gose\nproperties:\n  \
         malt: {v: 11.5, unit: g}\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    // Sorted by malt, heaviest first: each project sorted within itself.
    let names = |workbench: &Workbench| -> Vec<String> {
        workbench
            .view
            .iter()
            .map(|entry| entry.sample.borrow().name().unwrap_or("").to_string())
            .collect()
    };
    workbench.sort = vec!["-malt".to_string()];
    workbench.refresh().unwrap();
    assert_eq!(names(&workbench), ["D0", "D1", "C1", "C3", "C2"]);
    let headings = workbench.project_headings();
    assert_eq!(headings[0].as_deref(), Some("more"));
    assert!(headings[1].is_none());
    assert!(headings[2].is_some(), "{headings:?}");
    assert!(headings[3..].iter().all(Option::is_none), "{headings:?}");
    // One project alone: no heading.
    let alone = Workbench::open(&Scratch::new("projects-alone").0).unwrap();
    assert!(alone.project_headings().iter().all(Option::is_none));
}

fn view_names(workbench: &Workbench) -> Vec<String> {
    workbench
        .view
        .iter()
        .map(|entry| entry.sample.borrow().name().unwrap_or("").to_string())
        .collect()
}

#[test]
fn g_groups_the_collection_and_the_grouping_is_remembered() {
    // As `--group` splits a table, each group headed by its values.
    let scratch = Scratch::new("grouped");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "g/beer \n");
    assert_eq!(workbench.group, ["beer"], "{}", workbench.message);
    assert_eq!(view_names(&workbench), ["C2", "C1", "C3"]);
    let headings = workbench.headings();
    assert_eq!(headings[0].as_deref(), Some("beer = bock"));
    assert_eq!(headings[1].as_deref(), Some("beer = schwarz"));
    assert!(headings[2].is_none(), "{headings:?}");
    // Sorted within each group: the lightest is bock's, whose group stays
    // first.
    keys(&mut workbench, "s/malt\n");
    assert_eq!(view_names(&workbench), ["C2", "C3", "C1"]);
    samplekit::tui::model::remember(&workbench);
    let mut again = Workbench::open(&scratch.0).unwrap();
    assert_eq!(again.group, ["beer"]);
    assert_eq!(view_names(&again)[0], "C2");
    // Grouped, `g` opens on the item that ungroups.
    keys(&mut again, "g");
    let Mode::Picker { items, .. } = &again.mode else {
        panic!("a picker");
    };
    assert_eq!(items[0], ("no grouping".to_string(), false));
    assert_eq!(items[1], ("beer".to_string(), true));
    keys(&mut again, "\n");
    assert!(again.group.is_empty());
    assert!(again.headings().iter().all(Option::is_none));
}

/// `W`, *profile*, a name, and the preview written.
fn save_as_profile(workbench: &mut Workbench, name: &str) {
    use samplekit::tui::model::{Choosing, Declare};
    keys(workbench, "W");
    let Mode::Picker {
        purpose: Choosing::Declare(offered),
        cursor,
        ..
    } = &mut workbench.mode
    else {
        panic!("the choices: {}", workbench.message);
    };
    *cursor = offered
        .iter()
        .position(|what| *what == Declare::Profile)
        .unwrap();
    keys(workbench, &format!("\n{name}\n"));
    assert!(
        matches!(workbench.mode, Mode::Confirm { .. }),
        "a preview: {}",
        workbench.message
    );
    keys(workbench, "y");
}

#[test]
fn the_groups_follow_the_sort_shown() {
    // With a sort chosen, each group comes where its first sample comes;
    // without one, by the values. `S` summarises in the same order.
    let scratch = Scratch::new("groups-sorted");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "g/beer \n");
    assert_eq!(view_names(&workbench), ["C2", "C1", "C3"]);
    // Heaviest first: C1 is schwarz's, so schwarz comes first.
    keys(&mut workbench, "s/malt\nr");
    assert_eq!(view_names(&workbench), ["C1", "C3", "C2"]);
    let headings = workbench.headings();
    assert_eq!(headings[0].as_deref(), Some("beer = schwarz"));
    assert_eq!(headings[2].as_deref(), Some("beer = bock"));
    keys(&mut workbench, "S");
    let drawn = screen(&mut workbench);
    // A row per group, its value in the first column.
    let schwarz = drawn.find("│ schwarz").expect(&drawn);
    let bock = drawn.find("│ bock").expect(&drawn);
    assert!(schwarz < bock, "{drawn}");
}

#[test]
fn a_profile_applied_comes_back_with_its_labels() {
    // Only the columns' fields were remembered: reopened, a profile's
    // columns came back under their fields' names, its labels lost.
    let scratch = Scratch::new("profile-labels");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n\
         [profile.labelled]\ncolumns = [{ field = \"name\", label = \"Keg\" }, \
         { field = \"malt\", label = \"Malt\", precision = \".2f\" }]\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "f/labelled\n");
    let applied = workbench.profile.columns().to_vec();
    samplekit::tui::model::remember(&workbench);
    let again = Workbench::open(&scratch.0).unwrap();
    assert_eq!(again.profile.columns(), applied.as_slice());
    assert_eq!(again.profile.columns()[0].label.as_deref(), Some("Keg"));
}

#[test]
fn f_applies_a_profile_with_its_columns_its_sort_and_its_groups() {
    // A profile applied from `f` brings its grouping as it brings its columns
    // and its sort; one grouping by nothing ungroups.
    let scratch = Scratch::new("profile-groups");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n\
         [profile.by_beer]\ncolumns = [{ field = \"name\" }, { field = \"malt\" }]\n\
         sort = [\"-malt\"]\ngroup = [\"beer\"]\n\
         [profile.plain]\ncolumns = [{ field = \"name\" }]\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "f/by_beer\n");
    assert_eq!(workbench.group, ["beer"], "{}", workbench.message);
    assert_eq!(workbench.sort, ["-malt"]);
    let fields: Vec<&str> = workbench
        .profile
        .columns()
        .iter()
        .map(|column| column.field.as_str())
        .collect();
    assert_eq!(fields, ["name", "malt"]);
    // Sorted heaviest first, schwarz's C1 leads, and its group with it.
    assert_eq!(view_names(&workbench), ["C1", "C3", "C2"]);
    keys(&mut workbench, "f/plain\n");
    assert!(workbench.group.is_empty(), "{:?}", workbench.group);
    assert!(workbench.headings().iter().all(Option::is_none));
}

#[test]
fn w_saves_the_grouping_shown_into_the_profile() {
    // `W` writes the grouping shown as the profile's `group`, which `f` and
    // `--profile` then follow; ungrouped, it writes none.
    let scratch = Scratch::new("save-groups");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "g/beer \n");
    assert_eq!(workbench.group, ["beer"]);
    save_as_profile(&mut workbench, "grouped");
    let written = fs::read_to_string(scratch.0.join(".samplekitrc")).unwrap();
    assert!(written.contains("[profile.grouped]"), "{written}");
    assert!(written.contains("group = [\"beer\"]"), "{written}");
    keys(&mut workbench, "g\n");
    assert!(workbench.group.is_empty());
    save_as_profile(&mut workbench, "flat");
    let written = fs::read_to_string(scratch.0.join(".samplekitrc")).unwrap();
    let flat = &written[written.find("[profile.flat]").expect(&written)..];
    assert!(!flat.contains("group"), "{written}");
}

#[test]
fn the_configuration_screen_edits_the_tui_colours() {
    // `P`'s colours are `[tui.colors]`; a file holding the former section is
    // said to be read still, and is left as it is.
    let scratch = Scratch::new("tui-colours");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[workbench.colors]\nfailed = \"red\"\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.open_workspace();
    assert!(
        workbench
            .message
            .contains("[workbench.colors] are read still")
            && workbench.message.contains("[tui.colors]"),
        "{}",
        workbench.message
    );
    let section = samplekit::tui::model::SECTIONS
        .iter()
        .position(|(title, ..)| *title == "TUI colours")
        .expect("a colours section");
    assert!(matches!(
        samplekit::tui::model::SECTIONS[section].1,
        samplekit::tui::model::Section::Setting("tui.colors")
    ));
    let workspace = workbench.workspace.as_mut().unwrap();
    workspace.edit.set_setting(
        "tui.colors",
        "outdated",
        samplekit::config::configuration_edit::value_of("yellow").unwrap(),
    );
    let text = workspace.edit.text();
    assert!(
        text.contains("[tui.colors]\noutdated = \"yellow\""),
        "{text}"
    );
    // The former section is not rewritten.
    assert!(
        text.contains("[workbench.colors]\nfailed = \"red\""),
        "{text}"
    );
    workspace.edit.checked().unwrap();
}

#[test]
fn a_remembered_grouping_no_sample_holds_is_set_aside_and_said() {
    // As a sort is, where the field is gone; the filter stays.
    let scratch = Scratch::new("stalegroup");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "/malt > 10.5\n");
    workbench.group = vec!["beer".to_string()];
    workbench.refresh().unwrap();
    samplekit::tui::model::remember(&workbench);
    for name in ["C1", "C2", "C3"] {
        let path = scratch.0.join(format!("{name}.md"));
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace("beer:", "brand:");
        fs::write(&path, text).unwrap();
    }
    let again = Workbench::open(&scratch.0).unwrap();
    assert!(again.group.is_empty());
    assert!(
        again.message.contains("grouping was set aside"),
        "{}",
        again.message
    );
    assert_eq!(again.filter, "malt > 10.5");
}

#[test]
fn groups_are_made_within_each_project() {
    // The projects apart first, each one's groups within it.
    let scratch = rich("groups-projects");
    fs::write(scratch.0.join("more/.samplekitrc"), "schema_version = 1\n").unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.group = vec!["beer".to_string()];
    workbench.refresh().unwrap();
    assert_eq!(view_names(&workbench), ["D1", "C2", "C1", "C3"]);
    let headings = workbench.headings();
    assert_eq!(headings[0].as_deref(), Some("more · beer = bock"));
    let own = headings[1].clone().unwrap();
    assert!(own.ends_with(" · beer = bock"), "{own}");
    assert!(
        headings[2]
            .as_deref()
            .unwrap()
            .ends_with(" · beer = schwarz")
    );
    assert!(headings[3].is_none(), "{headings:?}");
}

#[test]
fn the_filter_reads_state_and_completes_its_words() {
    // `state` is a field the workbench's filter reads.
    let scratch = Scratch::new("filter-state");
    std::fs::write(
        scratch.0.join("C2.md"),
        "---\nschema_version: 1\nname: C2\nbeer: bock\nproperties:\n  \
         malt: {v: 10.0}\n  volume: {computed: {malt: bbbbbbbbbbbb}, \
         fingerprint: {failed: ZeroDivisionError}}\n---\nN.\n",
    )
    .unwrap();
    std::fs::write(
        scratch.0.join("C3.md"),
        "---\nschema_version: 1\nname: C3\nbeer: schwarz\nbeer: iron\n---\nN.\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "/state == failed\n");
    assert_eq!(workbench.view.len(), 1, "{}", workbench.message);
    keys(&mut workbench, "\u{1b}/state == defective\n");
    assert_eq!(workbench.view.len(), 1, "{}", workbench.message);
    assert!(
        workbench
            .view
            .get(0)
            .unwrap()
            .path
            .as_ref()
            .unwrap()
            .ends_with("C3.md")
    );
    keys(&mut workbench, "\u{1b}/state ");
    let Mode::Filter { candidates, .. } = &workbench.mode else {
        panic!("filter");
    };
    for operator in ["==", "!=", "has", "in"] {
        assert!(
            candidates.contains(&format!("state {operator} ")),
            "{candidates:?}"
        );
    }
    keys(&mut workbench, "== ");
    let Mode::Filter { candidates, .. } = &workbench.mode else {
        panic!("filter");
    };
    for word in samplekit::query::field_addressing::StateWord::WORDS {
        assert!(
            candidates.contains(&format!("state == {word}")),
            "{candidates:?}"
        );
    }
}

#[test]
fn the_state_is_a_column_the_picker_offers() {
    // `state` is a column, its words worst first, as `-c name,state`.
    let scratch = Scratch::new("state-column");
    std::fs::write(
        scratch.0.join("C2.md"),
        "---\nschema_version: 1\nname: C2\nbeer: bock\nbeer: iron\nproperties:\n  \
         malt: {v: 10.0}\n  volume: {computed: {malt: bbbbbbbbbbbb}, \
         fingerprint: {failed: ZeroDivisionError}}\n---\nN.\n",
    )
    .unwrap();
    std::fs::write(
        scratch.0.join("C3.md"),
        "---\nschema_version: 1\nname: C3\nbeer: schwarz\n---\nN.\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "c");
    let Mode::Picker { items, cursor, .. } = &workbench.mode else {
        panic!("the columns picker");
    };
    let at = items
        .iter()
        .position(|(field, _)| field == "state")
        .unwrap_or_else(|| panic!("state is offered: {items:?}"));
    for _ in *cursor..at {
        workbench.key(Key::Down);
    }
    keys(&mut workbench, " \n");
    let column = workbench
        .profile
        .columns()
        .iter()
        .position(|column| column.field == "state")
        .expect("state is shown");
    let shown = |name: &str| {
        let row = workbench
            .rendered
            .iter()
            .find(|row| row.name == name)
            .unwrap_or_else(|| panic!("{name} is shown"));
        row.cells[column].clone()
    };
    assert_eq!(shown("C2"), "failed, defective", "{}", workbench.message);
    // No model declared: nothing is owed, and that is known.
    assert_eq!(shown("C3"), "current", "{}", workbench.message);
}

/// `docs/reference/tui.md` is written from the keys themselves: every screen's
/// bindings as `?` groups them, the start page's help, and the marks. Run with
/// `SAMPLEKIT_WRITE_REFERENCE=1` it writes the page; otherwise it fails when
/// the page committed is not the one the keys give.
#[test]
fn the_reference_page_is_written_from_the_bindings() {
    use samplekit::tui::model::{MARKS, Place, keys_of};
    use samplekit::tui::start::HELP;

    fn keys(written: &str) -> String {
        written
            .split(' ')
            .filter(|key| !key.is_empty())
            .map(|key| format!("`{key}`"))
            .collect::<Vec<_>>()
            .join(" ")
    }
    fn text(said: &str) -> String {
        said.replace('|', "\\|")
    }
    fn capitalised(name: &str) -> String {
        let mut letters = name.chars();
        letters
            .next()
            .map(|first| first.to_uppercase().chain(letters).collect())
            .unwrap_or_default()
    }
    const HEAD: &str = "| Keys | What they do |\n| --- | --- |\n";

    let mut page = String::from(
        "<!-- Generated from the TUI's key bindings by the test \
         the_reference_page_is_written_from_the_bindings (tests/tui_model.rs):\n     \
         do not edit; change the bindings, then run\n     \
         SAMPLEKIT_WRITE_REFERENCE=1 cargo test --test tui_model the_reference_page -->\n\n\
         # The TUI's keys\n\n\
         Every key of the TUI, screen by screen, as `?` lists them there. The foot \
         of each screen shows the keys used most.\n\n\
         In a list to choose from, `Space` ticks and `Enter` accepts what is ticked, \
         and `/` narrows the list to what holds the letters typed. Every line typed \
         has a cursor that `←` `→` `Home` `End` move.\n",
    );

    page.push_str(
        "\n## Typing\n\nIn every line typed — the filter, a prompt, a value's window, \
         a list's `/`, the start page's name and narrowing — and in the note. A \
         window's `Enter`, `Esc` and `Tab` stay the window's.\n\n",
    );
    page.push_str(HEAD);
    for (key, said) in samplekit::tui::typing::TYPING {
        page.push_str(&format!("| {} | {} |\n", keys(key), text(said)));
    }
    page.push_str("\n**In the note**, besides — `N` on a sample:\n\n");
    page.push_str(HEAD);
    for (key, said) in samplekit::tui::typing::WRITING {
        page.push_str(&format!("| {} | {} |\n", keys(key), text(said)));
    }

    page.push_str("\n## The start page\n\n`samplekit` alone, in a terminal.\n");
    let mut table_open = false;
    for (at, (key, said)) in HELP.iter().enumerate() {
        if !key.is_empty() {
            if !table_open {
                page.push('\n');
                page.push_str(HEAD);
                table_open = true;
            }
            page.push_str(&format!("| {} | {} |\n", keys(key), text(said)));
        } else if let Some(more) = said.strip_prefix("  ") {
            // A line continuing the entry above it.
            page.truncate(page.len() - " |\n".len());
            page.push_str(&format!("; {} |\n", text(more)));
        } else if said.is_empty() {
            table_open = false;
        } else {
            let heading = HELP.get(at + 1).is_some_and(|(next, _)| !next.is_empty());
            table_open = false;
            if heading && *said != "The start page" {
                page.push_str(&format!("\n### {said}\n"));
            } else if !heading {
                page.push_str(&format!("\n{said}\n"));
            }
        }
    }

    let screens = [
        (
            Place::Collection,
            "The collection",
            "What a project opens on: its samples, one per line.",
        ),
        (
            Place::Sample,
            "A sample",
            "One sample, value by value, and its tables.",
        ),
        (
            Place::Table,
            "A table",
            "One table of a sample, row by row.",
        ),
        (
            Place::Control,
            "The collection's state",
            "`v`: what is not current, what is defective.",
        ),
        (
            Place::History,
            "The history",
            "`H`: each change kept, and what it changed.",
        ),
        (
            Place::Setup,
            "Setting a project up",
            "A folder with no project: a few questions.",
        ),
        (
            Place::Configure,
            "The configuration",
            "`P`: the project's `.samplekitrc`, section by section.",
        ),
    ];
    for (place, title, what) in screens {
        page.push_str(&format!("\n## {title}\n\n{what}\n"));
        for (section, rows) in keys_of(place) {
            page.push_str(&format!("\n**{}**\n\n{HEAD}", capitalised(section)));
            for (bound, said) in rows {
                page.push_str(&format!("| {} | {} |\n", keys(&bound), text(said)));
            }
        }
    }

    page.push_str(
        "\n## Marks\n\nThe marks beside a sample or a value, on the collection, a \
         sample, a table and the collection's state.\n\n| Mark | What it says |\n| --- | --- |\n",
    );
    for (mark, said) in MARKS {
        page.push_str(&format!("| {mark} | {} |\n", text(said)));
    }

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/reference/tui.md");
    if std::env::var_os("SAMPLEKIT_WRITE_REFERENCE").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &page).unwrap();
        return;
    }
    let committed = fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == page,
        "docs/reference/tui.md is not what the bindings give: run\n  \
         SAMPLEKIT_WRITE_REFERENCE=1 cargo test --test tui_model the_reference_page\n\
         and commit it"
    );
}

#[test]
fn the_name_is_edited_as_an_attribute() {
    // Its line first, `e` changes what the file writes and never the file's
    // name; emptied, `name:` goes and the file's name stands for it.
    let scratch = Scratch::new("name-edited");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    assert_eq!(workbench.entries()[0].field, "name");
    move_to(&mut workbench, "name");
    keys(&mut workbench, "e");
    for _ in 0..4 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "Keg one\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines.iter().any(|line| line.contains("C1  →  Keg one")),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    let path = scratch.0.join("C1.md");
    assert!(fs::read_to_string(&path).unwrap().contains("name: Keg one"));
    move_to(&mut workbench, "name");
    keys(&mut workbench, "e");
    for _ in 0..12 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines
            .iter()
            .any(|line| line.contains("Keg one  →  C1") && line.contains("the file's name")),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    assert!(!fs::read_to_string(&path).unwrap().contains("name:"));
    let entries = workbench.entries();
    assert_eq!(entries[0].shown, "C1");
    assert_eq!(entries[0].state, "the file's name");
}

#[test]
fn n_hides_the_note_and_the_choice_is_remembered() {
    let scratch = rich("note-remembered");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    keys(&mut workbench, "n");
    assert!(workbench.note_hidden);
    assert!(
        workbench.message.contains("hidden"),
        "{}",
        workbench.message
    );
    // Hidden, it is not scrolled out of sight in silence.
    keys(&mut workbench, "J");
    assert!(
        workbench.message.contains("n shows it"),
        "{}",
        workbench.message
    );
    samplekit::tui::model::remember(&workbench);
    let mut again = Workbench::open(&scratch.0).unwrap();
    assert!(again.note_hidden);
    open_named(&mut again, "D1");
    keys(&mut again, "n");
    assert!(!again.note_hidden);
    samplekit::tui::model::remember(&again);
    assert!(!Workbench::open(&scratch.0).unwrap().note_hidden);
}

#[test]
fn capital_n_edits_the_note_on_a_sample_and_makes_a_sample_on_the_collection() {
    let scratch = rich("capital-n");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "N");
    assert!(matches!(
        &workbench.mode,
        Mode::Prompt {
            purpose: samplekit::tui::model::Purpose::New { .. },
            ..
        }
    ));
    workbench.key(Key::Esc);
    open_named(&mut workbench, "D1");
    keys(&mut workbench, "N");
    assert!(matches!(workbench.mode, Mode::Note { .. }));
}

#[test]
fn a_model_already_there_is_asked_its_path() {
    // The model's question has three answers; the second asks the file, and the
    // configuration names it where it is.
    let root = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!(
            "samplekit-workbench-setup-own-model-{}",
            std::process::id()
        ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    Scratch::new("setup-own-model-state");
    let mut workbench = Workbench::open(&root).unwrap();
    // An empty project: the model's question next, its default marked.
    keys(&mut workbench, "\n");
    assert_eq!(workbench.setup.as_ref().unwrap().question, 1);
    assert!(matches!(workbench.screen, Screen::Setup { cursor: 0 }));
    // Three answers: `j` stops on the last.
    keys(&mut workbench, "jjj");
    assert!(matches!(workbench.screen, Screen::Setup { cursor: 2 }));
    keys(&mut workbench, "k\n");
    assert!(
        matches!(
            &workbench.mode,
            Mode::Prompt {
                purpose: samplekit::tui::model::Purpose::ModelPath,
                ..
            }
        ),
        "{}",
        workbench.message
    );
    // A file not there is said, the window kept.
    keys(&mut workbench, "nothere.py\n");
    assert!(
        workbench.message.contains("no file at nothere.py"),
        "{}",
        workbench.message
    );
    assert!(matches!(workbench.mode, Mode::Prompt { .. }));
    // Esc goes back to the question.
    keys(&mut workbench, "\u{1b}");
    assert!(matches!(workbench.mode, Mode::Normal));
    assert_eq!(workbench.setup.as_ref().unwrap().question, 1);
    // A file there is the answer, the next question shown.
    fs::create_dir_all(root.join("mine")).unwrap();
    fs::write(
        root.join("mine/model.py"),
        "import samplekit as sk\n\nclass Mine(sk.Sample):\n    pass\n",
    )
    .unwrap();
    keys(&mut workbench, "\nmine/model.py\n");
    assert!(
        matches!(workbench.mode, Mode::Normal),
        "{}",
        workbench.message
    );
    assert_eq!(workbench.setup.as_ref().unwrap().question, 2);
    // No environment, then set up.
    keys(&mut workbench, "j\n\n");
    assert!(
        matches!(workbench.mode, Mode::Confirm { .. }),
        "{}",
        workbench.message
    );
    keys(&mut workbench, "y");
    let config = samplekit::config::project_config::load(&root.join(".samplekitrc")).unwrap();
    let model = config.model().expect("a model named");
    assert!(model.path.ends_with("mine/model.py"), "{model:?}");
    assert!(model.class.is_none());
    assert!(!root.join("model").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_project_column_is_shown_only_over_several_projects() {
    // One project's collection repeated its folder on every row.
    let fields = |workbench: &Workbench| -> Vec<String> {
        workbench
            .profile
            .columns()
            .iter()
            .map(|column| column.field.clone())
            .collect()
    };
    let one = Scratch::new("one-project-columns");
    let workbench = Workbench::open(&one.0).unwrap();
    let shown = fields(&workbench);
    assert!(!shown.contains(&"project".to_string()), "{shown:?}");
    assert!(shown.contains(&"malt".to_string()), "{shown:?}");
    let several = rich("several-projects-columns");
    fs::write(several.0.join("more/.samplekitrc"), "schema_version = 1\n").unwrap();
    let workbench = Workbench::open(&several.0).unwrap();
    let shown = fields(&workbench);
    assert!(shown.contains(&"project".to_string()), "{shown:?}");
}

/// What a line typed says: its text, and the text before its cursor.
fn typed_line(workbench: &Workbench) -> (String, String) {
    let line = match &workbench.mode {
        Mode::Filter { text, .. } | Mode::Prompt { text, .. } => text,
        Mode::Quantity { slots, on, .. } => &slots[*on].text,
        _ => panic!("nothing typed"),
    };
    (
        line.text().to_string(),
        line.text()[..line.at()].to_string(),
    )
}

#[test]
fn a_line_typed_moves_and_deletes_by_word_selects_and_undoes() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let scratch = rich("words");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    // The filter: back a word, a word taken out, undone, done again.
    keys(&mut workbench, "/malt alpha beta");
    workbench.key(held(KeyCode::Left, KeyModifiers::CONTROL));
    assert_eq!(typed_line(&workbench).1, "malt alpha ");
    workbench.key(Key::End);
    workbench.key(held(KeyCode::Backspace, KeyModifiers::CONTROL));
    assert_eq!(typed_line(&workbench).0, "malt alpha ");
    workbench.key(ctrl('z'));
    assert_eq!(typed_line(&workbench).0, "malt alpha beta");
    workbench.key(held(
        KeyCode::Char('Z'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    assert_eq!(typed_line(&workbench).0, "malt alpha ");
    // Ctrl+Delete takes the word after the cursor.
    workbench.key(Key::Home);
    workbench.key(held(KeyCode::Delete, KeyModifiers::CONTROL));
    assert_eq!(typed_line(&workbench).0, " alpha ");
    // Shift+← selects; a character typed replaces what is selected.
    workbench.key(Key::End);
    keys(&mut workbench, "beta");
    workbench.key(held(KeyCode::Left, KeyModifiers::SHIFT));
    workbench.key(held(KeyCode::Left, KeyModifiers::SHIFT));
    keys(&mut workbench, "x");
    assert_eq!(
        typed_line(&workbench),
        (" alpha bex".to_string(), " alpha bex".to_string())
    );
    // Tab still completes.
    keys(&mut workbench, "\u{1b}/bee\t");
    assert!(
        typed_line(&workbench).0.starts_with("beer"),
        "{:?}",
        typed_line(&workbench)
    );
    // A prompt: the tags, a word taken back.
    keys(&mut workbench, "\u{1b}");
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "tags");
    keys(&mut workbench, "e");
    keys(&mut workbench, "reference checked");
    workbench.key(held(KeyCode::Backspace, KeyModifiers::CONTROL));
    assert_eq!(typed_line(&workbench).0, "reference ");
    // The quantity's window: all selected, then typed over.
    keys(&mut workbench, "\u{1b}");
    workbench.key(Key::Home);
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e");
    assert!(matches!(workbench.mode, Mode::Quantity { .. }));
    workbench.key(ctrl('a'));
    keys(&mut workbench, "12.5");
    assert_eq!(typed_line(&workbench).0, "12.5");
    workbench.key(ctrl('z'));
    assert_ne!(typed_line(&workbench).0, "12.5");
}

#[test]
fn the_note_takes_rat_markdowns_keys() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let scratch = rich("markdown-keys");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    keys(&mut workbench, "N");
    let written = |workbench: &Workbench| {
        let Mode::Note { text, .. } = &workbench.mode else {
            panic!("the note");
        };
        text.text()
    };
    // `*` around the first word selected.
    workbench.key(held(
        KeyCode::Right,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    keys(&mut workbench, "*");
    assert!(
        written(&workbench).starts_with("*First* line."),
        "{}",
        written(&workbench)
    );
    workbench.key(ctrl('z'));
    assert!(
        written(&workbench).starts_with("First line."),
        "{}",
        written(&workbench)
    );
    // Alt+2 makes the line a heading.
    workbench.key(Key::Down);
    workbench.key(alt('2'));
    assert!(
        written(&workbench).contains("\n## Second line."),
        "{}",
        written(&workbench)
    );
    // Alt+L a link around what is selected.
    workbench.key(held(KeyCode::End, KeyModifiers::NONE));
    keys(&mut workbench, "\n  - item");
    workbench.key(held(
        KeyCode::Left,
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    workbench.key(alt('l'));
    assert!(
        written(&workbench).ends_with("  - [item]()"),
        "{}",
        written(&workbench)
    );
    // Enter keeps the indentation; the list's marker is not repeated.
    workbench.key(held(KeyCode::End, KeyModifiers::NONE));
    keys(&mut workbench, "\nnext");
    assert!(
        written(&workbench).ends_with("[item]()\n  next"),
        "{}",
        written(&workbench)
    );
}

#[test]
fn a_chord_means_something_only_where_something_is_typed() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let scratch = rich("chords");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let before = workbench.view.len();
    // Alt+D on the collection removes nothing.
    workbench.key(alt('D'));
    assert!(matches!(workbench.mode, Mode::Normal));
    assert_eq!(workbench.view.len(), before);
    // Ctrl+Y on a confirmation writes nothing.
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "malt");
    keys(&mut workbench, "e");
    workbench.key(ctrl('a'));
    keys(&mut workbench, "7\n");
    assert!(
        matches!(workbench.mode, Mode::Confirm { .. }),
        "{}",
        workbench.message
    );
    workbench.key(ctrl('y'));
    assert!(matches!(workbench.mode, Mode::Confirm { .. }));
    let file = fs::read_to_string(scratch.0.join("more/D1.md")).unwrap();
    assert!(file.contains("v: 9.0"), "{file}");
    keys(&mut workbench, "n");
    // Shift+→ on a table's line is →: it unfolds.
    move_to(&mut workbench, "runs");
    workbench.key(held(KeyCode::Right, KeyModifiers::SHIFT));
    assert!(matches!(workbench.screen, Screen::Table { .. }));
}

#[test]
fn ctrl_c_copies_a_selection_and_a_paste_is_typed_where_the_cursor_is() {
    let scratch = rich("clipboard");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    // Nothing selected: nothing copied, Ctrl+C left to interrupt.
    keys(&mut workbench, "/beer");
    assert!(!workbench.copy());
    workbench.key(ctrl('a'));
    assert!(workbench.copy());
    keys(&mut workbench, "\u{1b}/");
    workbench.key(ctrl('v'));
    assert_eq!(typed_line(&workbench).0, "beer");
    // A paste, where the cursor is.
    workbench.key(Key::Home);
    workbench.paste("malt > 1 && ");
    assert_eq!(typed_line(&workbench).0, "malt > 1 && beer");
    // Into the note, and nowhere on the collection.
    keys(&mut workbench, "\u{1b}");
    workbench.paste("beer");
    assert!(matches!(workbench.mode, Mode::Normal));
    assert!(workbench.filter.is_empty());
    open_named(&mut workbench, "D1");
    keys(&mut workbench, "N");
    workbench.paste("Pasted. ");
    let Mode::Note { text, .. } = &workbench.mode else {
        panic!("the note");
    };
    assert!(
        text.text().starts_with("Pasted. First line."),
        "{}",
        text.text()
    );
}
