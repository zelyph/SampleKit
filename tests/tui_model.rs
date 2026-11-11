//! The TUI's keys, on a model that draws nothing.

mod workbench_fixtures;

use samplekit::tui::model::CardGroup;
use workbench_fixtures::*;

/// The theme list's items and the cursor, where it is open.
fn theme_list(workbench: &Workbench) -> (Vec<String>, usize) {
    match &workbench.mode {
        Mode::Picker {
            purpose: samplekit::tui::model::Choosing::Theme { .. },
            items,
            cursor,
            ..
        } => (
            items.iter().map(|(name, _)| name.clone()).collect(),
            *cursor,
        ),
        other => panic!("no list of themes: {}", matches!(other, Mode::Normal)),
    }
}

#[test]
fn t_lists_the_themes_and_the_cursor_tries_each() {
    use samplekit::tui::theme::Themes;
    let scratch = Scratch::new("themes-list");
    let folder = scratch.0.join("themes");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("mine.toml"),
        "base = \"nord\"\n[colors]\naccent = \"#ff79c6\"\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.themes = Themes::read(Some(&folder));
    workbench.themed();
    let before = workbench.theme;
    keys(&mut workbench, "T");
    let (items, cursor) = theme_list(&workbench);
    assert_eq!(items.first().map(String::as_str), Some("samplekit-dark"));
    assert_eq!(items.last().map(String::as_str), Some("mine"));
    assert_eq!(items[cursor], workbench.theme_name);
    // One's own said so, the whole list in sight.
    let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
    terminal
        .draw(|frame| samplekit::tui::view::draw(frame, &mut workbench))
        .unwrap();
    let drawn: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(drawn.contains("mine  yours"), "{drawn}");
    // The cursor tries the next one at once.
    workbench.key(Key::Down);
    let (_, moved) = theme_list(&workbench);
    let tried = workbench.themes.theme(&items[moved], None).unwrap();
    assert_eq!(moved, cursor + 1);
    assert_eq!(workbench.theme, tried);
    assert_ne!(workbench.theme, before);
    // Esc gives the one before back.
    keys(&mut workbench, "\u{1b}");
    assert!(matches!(workbench.mode, Mode::Normal));
    assert_eq!(workbench.theme, before);
    // On a sample, the same list.
    keys(&mut workbench, "\nT");
    assert_eq!(theme_list(&workbench).0, items);
}

#[test]
fn enter_keeps_the_theme_for_the_folder_and_as_the_last_chosen() {
    let scratch = Scratch::new("themes-kept");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let last = scratch.0.join("last-theme");
    workbench.last_theme = Some(last.clone());
    keys(&mut workbench, "T/nord\n");
    assert!(
        matches!(workbench.mode, Mode::Normal),
        "{}",
        workbench.message
    );
    assert_eq!(workbench.theme_name, "nord");
    assert_eq!(
        workbench.theme,
        workbench.themes.theme("nord", None).unwrap()
    );
    assert!(
        workbench.message.contains("theme nord, kept"),
        "{}",
        workbench.message
    );
    assert_eq!(fs::read_to_string(&last).unwrap(), "nord\n");
    // The folder's choice, remembered with it.
    samplekit::tui::model::remember(&workbench).unwrap();
    let again = Workbench::open(&scratch.0).unwrap();
    assert_eq!(again.theme_name, "nord");
    // The last choice, read where the folder chose none, under a project's
    // own theme.
    let other = Scratch::new("themes-kept-other");
    let mut elsewhere = Workbench::open(&other.0).unwrap();
    elsewhere.last_theme = Some(last.clone());
    elsewhere.themed();
    assert_eq!(elsewhere.theme_name, "nord");
    fs::write(
        other.0.join(".samplekitrc"),
        "schema_version = 1\n[colors]\ntheme = \"dracula\"\n",
    )
    .unwrap();
    let mut named = Workbench::open(&other.0).unwrap();
    named.last_theme = Some(last);
    named.themed();
    assert_eq!(named.theme_name, "dracula");
}

#[test]
fn the_themes_list_says_the_theme_the_project_names() {
    let scratch = Scratch::new("themes-named");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[colors]\ntheme = \"dracula\"\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.last_theme = Some(scratch.0.join("last-theme"));
    assert_eq!(workbench.theme_name, "dracula");
    keys(&mut workbench, "T");
    assert!(
        screen(&mut workbench).contains("theme · this project names dracula"),
        "{}",
        screen(&mut workbench)
    );
    // Chosen over the project's, then the project's again: no choice kept.
    keys(&mut workbench, "/nord\n");
    assert_eq!(workbench.theme_chosen.as_deref(), Some("nord"));
    samplekit::tui::model::remember(&workbench).unwrap();
    keys(&mut workbench, "T/dracula\n");
    assert_eq!(workbench.theme_name, "dracula");
    assert_eq!(workbench.theme_chosen, None);
    assert!(
        workbench.message.contains("the project's own"),
        "{}",
        workbench.message
    );
    // The choice kept before is forgotten with the session's.
    samplekit::tui::model::remember(&workbench).unwrap();
    let again = Workbench::open(&scratch.0).unwrap();
    assert_eq!(again.theme_chosen, None);
    assert_eq!(again.theme_name, "dracula");
}

#[test]
fn a_filter_narrows_the_collection_and_is_remembered() {
    let scratch = Scratch::new("filter");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    assert_eq!(workbench.view.len(), 3);
    keys(&mut workbench, "/alloy == cobalt\n");
    assert_eq!(workbench.view.len(), 2, "{}", workbench.message);
    // A filter that does not read keeps the prompt open, with its error.
    keys(&mut workbench, "/ &&\n");
    assert!(matches!(
        workbench.mode,
        Mode::Filter { error: Some(_), .. }
    ));
    keys(&mut workbench, "\u{1b}");
    samplekit::tui::model::remember(&workbench).unwrap();
    let again = Workbench::open(&scratch.0).unwrap();
    assert_eq!(again.filter, "alloy == cobalt");
    assert_eq!(again.view.len(), 2);
}

#[test]
fn the_filter_is_edited_where_its_cursor_is() {
    let scratch = Scratch::new("filter-cursor");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "/alloy == cobalt || alloy == gold");
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
    assert_eq!(text, "(alloy == cobalt || alloy == géd)");
    assert_eq!(&text.text()[text.at()..], "d)");
    // Tab completes what precedes the cursor and keeps what follows.
    keys(&mut workbench, "\u{1b}/ && alloy");
    workbench.key(Key::Home);
    keys(&mut workbench, "allo");
    keys(&mut workbench, "\t");
    let Mode::Filter { text: line, .. } = &workbench.mode else {
        panic!("filter");
    };
    let text = line.text();
    assert!(text.starts_with("alloy"), "{text}");
    assert!(text.ends_with(" && alloy"), "{text}");
    assert!(!text[..line.at()].ends_with(" && alloy"), "{text}");
}

#[test]
fn every_line_typed_is_edited_where_its_cursor_is() {
    let scratch = rich("cursor-everywhere");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    // A value, typed in its cell: F2 opens it with its text kept.
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, "e");
    workbench.key(Key::F2);
    workbench.key(Key::End);
    for _ in 0..20 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "12.5");
    workbench.key(Key::Left);
    workbench.key(Key::Left);
    keys(&mut workbench, "0");
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("the value's line");
    };
    assert_eq!(text.text(), "120.5");
    // A prompt: the name, one value on its line, which Enter opens.
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    keys(&mut workbench, "\n");
    assert!(
        matches!(workbench.mode, Mode::Prompt { .. }),
        "the name's prompt"
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
        panic!("the name's prompt");
    };
    assert_eq!((text.text(), text.at()), ("a, b, c", 6));
    workbench.key(Key::Delete);
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("the name's prompt");
    };
    assert_eq!(text, "a, b, ");
    // A list narrowed with `/`.
    keys(&mut workbench, "\u{1b}\u{1b}\u{1b}c/diamter");
    for _ in 0..3 {
        workbench.key(Key::Left);
    }
    keys(&mut workbench, "e");
    let Mode::Picker { query, typing, .. } = &workbench.mode else {
        panic!("the picker");
    };
    assert_eq!(query, "diameter");
    assert_eq!((query.at(), *typing), (5, true));
}

#[test]
fn the_columns_kept_keep_their_labels() {
    // Accepting the columns picker unchanged turned a profile's `Diameter` back
    // into the field's name: a column rebuilt from its field alone.
    let scratch = Scratch::new("columns-labels");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[profile.p]\ncolumns = [{field = \"name\"}, \
         {field = \"diameter\", label = \"Diameter\"}]\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let labelled = |workbench: &Workbench| workbench.profile.columns()[1].label.clone();
    assert_eq!(labelled(&workbench).as_deref(), Some("Diameter"));
    keys(&mut workbench, "c\n");
    assert!(matches!(workbench.mode, Mode::Normal));
    assert_eq!(labelled(&workbench).as_deref(), Some("Diameter"));
}

#[test]
fn a_sort_is_chosen_and_reversed() {
    let scratch = Scratch::new("sort");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let diameter = workbench
        .profile
        .columns()
        .iter()
        .position(|column| column.field == "diameter")
        .expect("diameter is shown");
    keys(&mut workbench, "s");
    for _ in 0..diameter {
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
    // Reversed in the menu, its key first there.
    keys(&mut workbench, "sr\n");
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
    keys(&mut workbench, "c/allo");
    let Mode::Picker { items, query, .. } = &workbench.mode else {
        panic!("the picker stays open");
    };
    assert_eq!(query, "allo");
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
    // A picker: the letters of `alloy`, not side by side.
    keys(&mut workbench, "c/ay");
    let Mode::Picker { items, query, .. } = &workbench.mode else {
        panic!("the picker stays open");
    };
    let shown = samplekit::tui::model::visible(items, query.text());
    assert!(
        shown
            .first()
            .is_some_and(|at| items[*at].0.contains("alloy")),
        "{items:?} {shown:?}"
    );
    // Side by side comes before apart.
    assert!(
        samplekit::tui::model::fuzzy("preheat_water", "tw")
            > samplekit::tui::model::fuzzy("preheating_water", "tw")
    );
    assert_eq!(samplekit::tui::model::fuzzy("porosity", "hr"), None);
    // The filter's completion likewise.
    keys(&mut workbench, "\u{1b}\u{1b}/ay");
    let Mode::Filter { candidates, .. } = &workbench.mode else {
        panic!("filter");
    };
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate.starts_with("alloy")),
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
    write_waiting(&mut workbench);
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
    move_to(&mut workbench, "diameter");
    let diameter = workbench
        .entries()
        .into_iter()
        .find(|entry| entry.field == "diameter")
        .unwrap();
    assert_eq!(diameter.state, "no statistic");
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
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, " ");
    assert!(workbench.selected.contains("diameter"));
    samplekit::tui::model::remember(&workbench).unwrap();
    let mut again = Workbench::open(&scratch.0).unwrap();
    assert_eq!(again.basket.len(), 4);
    assert!(again.selected.contains("diameter"));
    // x empties each where it applies.
    keys(&mut again, "x");
    assert!(again.basket.is_empty());
    keys(&mut again, "\nx");
    assert!(again.selected.is_empty());
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
            .any(|(item, _)| item.split_whitespace().take(2).eq(["query", "cobalt"])),
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
    for (project, alloy, import) in [("ana", "cobalt", true), ("tom", "chrome", false)] {
        fs::create_dir_all(scratch.0.join(project)).unwrap();
        fs::write(
            scratch.0.join(project).join(".samplekitrc"),
            format!(
                "schema_version = 1\n{}[query.{alloy}_{project}]\n\
                 filter = 'alloy == \"{alloy}\"'\n\
                 [profile.light]\ncolumns = [{{ field = \"name\" }}, {{ field = \"diameter\" }}]\n\
                 sort = [\"diameter\"]\n",
                if import { "import = \"..\"\n" } else { "" }
            ),
        )
        .unwrap();
        fs::write(
            scratch.0.join(project).join("S.md"),
            format!(
                "---\nschema_version: 1\nname: S-{project}\nalloy: {alloy}\n\
                 properties:\n  diameter: {{v: 2.0, unit: g}}\n---\n"
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
            .any(|item| item.split_whitespace().take(2).eq(["query", "cobalt"])),
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
        offered.iter().any(
            |item| item.split_whitespace().take(2).eq(["query", "cobalt"])
                && item.ends_with("· ../.samplekitrc")
        ),
        "{offered:?}"
    );
    assert!(
        offered.iter().any(|item| {
            item.split_whitespace().take(2).eq(["query", "cobalt_ana"]) && item.ends_with("· local")
        }),
        "{offered:?}"
    );
    // A profile applied is the one offered.
    let at = offered
        .iter()
        .position(|item| item.split_whitespace().take(2).eq(["profile", "light"]))
        .unwrap();
    for _ in 0..at {
        ana.key(Key::Down);
    }
    ana.key(Key::Enter);
    assert_eq!(ana.sort, vec!["diameter".to_string()], "{}", ana.message);
}

#[test]
fn a_selection_of_entered_values_computes_nothing_and_says_so() {
    let scratch = rich("entered");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, " c");
    assert!(workbench.running.is_none());
    assert!(
        workbench.message.contains("nothing computes diameter"),
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
    // Alloy shown, so that it can be sorted by.
    let Mode::Picker { items, .. } = &workbench.mode else {
        panic!("columns");
    };
    let (alloy, shown) = items
        .iter()
        .enumerate()
        .find(|(_, (label, _))| label == "alloy")
        .map(|(at, (_, ticked))| (at, *ticked))
        .unwrap();
    for _ in 0..alloy {
        workbench.key(Key::Down);
    }
    keys(&mut workbench, if shown { "\n" } else { " \n" });
    keys(&mut workbench, "s/diameter r\u{1b}/alloy \u{1b}");
    // Alloy moved above diameter: it sorts first.
    let Mode::Picker { items, cursor, .. } = &workbench.mode else {
        panic!("sort");
    };
    let at = items
        .iter()
        .position(|(label, _)| label == "alloy")
        .unwrap();
    assert_eq!(*cursor, at);
    for _ in 0..at {
        workbench.key(Key::MoveUp);
    }
    keys(&mut workbench, "\n");
    assert_eq!(workbench.sort, ["alloy", "-diameter"]);
    let names: Vec<String> = workbench
        .view
        .iter()
        .map(|entry| entry.sample.borrow().name().unwrap().to_string())
        .collect();
    assert_eq!(names, ["C2", "D1", "C1", "C3"]);
}

#[test]
fn the_project_colours_the_workbench() {
    let scratch = rich("colours");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[colors]\noutdated = \"#ff8800\"\n",
    )
    .unwrap();
    let workbench = Workbench::open(&scratch.0).unwrap();
    assert_eq!(
        workbench.theme.outdated,
        ratatui::style::Color::Rgb(0xff, 0x88, 0x00)
    );
    // A role it leaves out is its theme's.
    let theme = samplekit::tui::theme::Themes::read(None)
        .theme(&workbench.theme_name, None)
        .unwrap();
    assert_eq!(workbench.theme.failed, theme.failed);
    assert_ne!(workbench.theme.outdated, theme.outdated);
}

#[test]
fn an_export_is_previewed_then_written() {
    let scratch = rich("export");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[collection]\nrecursive = true\n\
         [profile.diameters]\ncolumns = [{ field = \"diameter\" }]\nsort = [\"diameter\"]\n\
         [export.diameters]\nprofile = \"diameters\"\nformat = \"csv\"\noutput = \"out/diameters.csv\"\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "p/diameters\n");
    let Mode::Confirm { title, lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    // A path as the system writes it, `out\diameters.csv` on Windows.
    assert!(
        title.replace('\\', "/").ends_with("out/diameters.csv"),
        "{title}"
    );
    assert!(lines[0].starts_with("4 rows"), "{lines:?}");
    keys(&mut workbench, "y");
    assert!(
        workbench.message.starts_with("4 rows written"),
        "{}",
        workbench.message
    );
    let written = fs::read_to_string(scratch.0.join("out/diameters.csv")).unwrap();
    // Its profile's order: the lightest first.
    let first = written.lines().nth(1).unwrap_or_default();
    assert!(first.contains('9'), "{written}");
}

#[test]
#[cfg(unix)]
fn readings_the_model_declares_no_statistic_of_are_said_to_give_no_value() {
    // The preview said an empty value lets them give it: with no statistic
    // declared, they give none.
    let venv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".venv");
    assert!(venv.join("bin/python").is_file(), "the repository's .venv");
    let scratch = Scratch::new("readings-none-declared");
    std::os::unix::fs::symlink(&venv, scratch.0.join(".venv")).unwrap();
    fs::write(
        scratch.0.join("model.py"),
        "import samplekit as sk\nclass Piece(sk.Sample):\n    def __init__(self):\n        \
         self.ore = sk.Property(unit=\"kg\")\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join("G.md"),
        "---\nschema_version: 1\nname: G\nproperties:\n  ore: {v: 4.6, unit: kg}\n---\n",
    )
    .unwrap();
    let config = samplekit::config::project_config::load_for(&scratch.0)
        .unwrap()
        .unwrap();
    samplekit::config::model_runtime::describe(&config, &scratch.0).unwrap();
    for (emptied, said) in [
        // The value kept first: emptying it is written.
        (
            false,
            "the model declares no statistic of them: the value written beside them stays the value",
        ),
        (
            true,
            "the model declares no statistic of them: they give no value",
        ),
    ] {
        let mut workbench = Workbench::open(&scratch.0).unwrap();
        open_named(&mut workbench, "G");
        go_to(&mut workbench, "ore");
        // In the typing mode, the value emptied first in its cell, then
        // readings typed in theirs.
        keys(&mut workbench, "e");
        if emptied {
            workbench.key(Key::F2);
            workbench.key(ctrl('a'));
            workbench.key(Key::Backspace);
            keys(&mut workbench, "\n");
            go_to(&mut workbench, "ore");
        }
        workbench.key(Key::Right);
        keys(&mut workbench, "5.1, 5.2\n");
        assert!(workbench.message.contains(said), "{}", workbench.message);
        // Taken back, for the next.
        keys(&mut workbench, "\u{1b}");
        while workbench.waiting_count() > 0 {
            keys(&mut workbench, "u");
        }
    }
}

#[test]
fn a_tag_comes_off_the_basket_with_a_minus() {
    let scratch = rich("untag");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "a");
    keys(&mut workbench, "tchecked\ny");
    write_waiting(&mut workbench);
    assert!(
        fs::read_to_string(scratch.0.join("C1.md"))
            .unwrap()
            .contains("checked")
    );
    keys(&mut workbench, "t-checked\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(lines[0].starts_with("'checked' off 4"), "{lines:?}");
    keys(&mut workbench, "y");
    write_waiting(&mut workbench);
    let written = fs::read_to_string(scratch.0.join("C1.md")).unwrap();
    assert!(!written.contains("checked"), "{written}");
}

#[test]
fn a_new_sample_is_written_beside_the_one_under_the_cursor() {
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
    // Nothing of D1 is taken: neither its attributes nor its tables.
    assert!(lines[0].contains("D2.md"), "{lines:?}");
    assert!(
        !lines.iter().any(|line| line.contains("chrome")),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    assert_eq!(workbench.collection.len(), 5);
    write_waiting(&mut workbench);
    let written = fs::read_to_string(scratch.0.join("more/D2.md")).unwrap();
    assert!(
        !written.contains("chrome") && !written.contains("runs"),
        "{written}"
    );
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
    // The empty row at the foot, filled cell by cell: kept until its index
    // is given, then waiting whole.
    keys(&mut workbench, "jjle5.5\n");
    assert!(
        matches!(workbench.mode, Mode::Normal),
        "{}",
        workbench.message
    );
    assert!(
        workbench.message.contains("once its index"),
        "{}",
        workbench.message
    );
    // Its index given, it waits at once: `w` previews it.
    workbench.key(Key::Left);
    keys(&mut workbench, "3\n\u{1b}w");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    // Each cell a change from nothing, as `set --add-row` says it.
    assert_eq!(lines[1], "  runs.run[3]    —  →  3", "{lines:?}");
    assert_eq!(lines[2], "  runs.load[3]   —  →  5.5 N", "{lines:?}");
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
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, "e9\n\u{1b}");
    write_waiting(&mut workbench);
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
        "---\nschema_version: 1\nname: C3\nalloy: cobalt\nproperties:\n  \
         diameter: {v: 11.0, u: 0.1, unit: g, fingerprint: '000000000000'}\n  \
         twice: {v: 22.0, computed: {diameter: '111111111111'}, fingerprint: '222222222222'}\n---\n",
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
    keys(&mut workbench, "\n");
    // What will be done is its page, the preview: Enter sets it up, not asked
    // a second time.
    assert_eq!(
        workbench
            .setup
            .as_ref()
            .unwrap()
            .plan(&root)
            .missing()
            .count(),
        5
    );
    keys(&mut workbench, "\n");
    assert!(matches!(workbench.mode, Mode::Normal));
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
    keys(&mut workbench, "azwickel.filter = alloy == \"zwickel\"\n");
    keys(&mut workbench, "d");
    let rows = workbench.workspace_rows();
    assert!(
        rows.contains(&samplekit::tui::model::ConfigRow::Name(
            "cobalt".to_string()
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
    keys(&mut workbench, "achrome.filter = alloy == \"chrome\"\n");
    let rows = workbench.workspace_rows();
    let at = rows
        .iter()
        .position(|row| row == &samplekit::tui::model::ConfigRow::Name("chrome".to_string()))
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
                if name == "chrome" && key == "directory"
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
    keys(&mut workbench, "chrome.filter = alloy == \"chrome\"\n");
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
        lines.iter().any(|line| line.contains("[query.chrome]")),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    let written = fs::read_to_string(scratch.0.join(".samplekitrc")).unwrap();
    assert!(
        written.contains("[query.chrome]") && !written.contains("bad"),
        "{written}"
    );
    // What was there stays as it was.
    assert!(written.contains("[query.cobalt]"), "{written}");
    assert!(!workbench.workspace.as_ref().unwrap().dirty);
    // Undone, the lines it takes away and brings back are said: `restore
    // .samplekitrc` said neither.
    keys(&mut workbench, "\u{1b}u");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert_eq!(lines[0], "restore .samplekitrc", "{lines:?}");
    assert!(
        lines.iter().any(|line| line == "  - [query.chrome]"),
        "{lines:?}"
    );
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
    keys(&mut workbench, "chrome.filter = alloy == \"chrome\"\n");
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
        workspace.edit.text().contains("[query.chrome]"),
        "{}",
        workspace.edit.text()
    );

    // What is shown, saved by `W`, is refused the same way.
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.filter = "alloy == cobalt".to_string();
    workbench.refresh().unwrap();
    keys(&mut workbench, "W\nchromes\n");
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
    workbench.sort = vec!["diameter".to_string()];
    workbench.refresh().unwrap();
    // D1 (9) first by diameter: made the heaviest, it moves to the end.
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, "e99\n\u{1b}");
    let (_, sample) = workbench.current().expect("still a sample open");
    assert_eq!(sample.name(), Some("D1"));
}

#[test]
fn a_remembered_sort_no_longer_resolving_is_set_aside_not_the_filter() {
    let scratch = rich("stalesort");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.filter = "alloy == cobalt".to_string();
    workbench.sort = vec!["body".to_string()];
    samplekit::tui::model::remember(&workbench).unwrap();
    let again = Workbench::open(&scratch.0).unwrap();
    assert_eq!(again.filter, "alloy == cobalt", "{}", again.message);
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
fn undo_takes_back_its_change_and_keeps_one_made_since() {
    // A colleague's work pulled, or an edit by hand, on the same sample: `u`
    // refused the whole file, and took nothing back.
    let scratch = rich("undoguard");
    let diameter_line = |text: &str| {
        text.lines()
            .find(|line| line.trim_start().starts_with("diameter:"))
            .unwrap_or_default()
            .to_string()
    };
    let original = diameter_line(&fs::read_to_string(scratch.0.join("C1.md")).unwrap());
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, "e9\n\u{1b}");
    write_waiting(&mut workbench);
    let path = scratch.0.join("C1.md");
    let edited = fs::read_to_string(&path)
        .unwrap()
        .replace("A note on C1.", "Mine.");
    fs::write(&path, &edited).unwrap();
    keys(&mut workbench, "u");
    assert!(
        matches!(workbench.mode, Mode::Confirm { .. }),
        "{}",
        workbench.message
    );
    keys(&mut workbench, "y");
    let after = fs::read_to_string(&path).unwrap();
    assert!(after.contains("Mine."), "{after}");
    assert_ne!(diameter_line(&edited), original, "{edited}");
    assert_eq!(diameter_line(&after), original, "{after}");
}

#[test]
fn undo_says_what_it_makes_not_current() {
    // `y` read `x` at 2: x set to 2 made it current, and `u` taking that back
    // left it outdated again without a word, where `restore` says it.
    let scratch = Scratch::new("undo-stale");
    fs::write(scratch.0.join(".samplekitrc"), "schema_version = 1\n").unwrap();
    fs::write(
        scratch.0.join("A.md"),
        "---\nschema_version: 1\nname: A\nproperties:\n  x: 3.0\n  \
         y: {v: 4.0, computed: {x: bd408504d4b1}, fingerprint: 8babce7201f4}\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "A");
    move_to(&mut workbench, "x");
    keys(&mut workbench, "e2\n\u{1b}");
    write_waiting(&mut workbench);
    keys(&mut workbench, "u");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("asked: {}", workbench.message);
    };
    assert!(
        lines.iter().any(|line| line.contains("not current"))
            && lines
                .iter()
                .any(|line| line.contains('y') && line.contains("outdated")),
        "{lines:?}"
    );
}

#[test]
fn undo_says_a_value_changed_since_and_lets_it_go_when_asked_again() {
    let scratch = rich("undoconflict");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, "e19\n\u{1b}");
    write_waiting(&mut workbench);
    // The same value changed since, by someone else.
    let path = scratch.0.join("C1.md");
    let written = fs::read_to_string(&path).unwrap();
    let edited = written.replacen("diameter: {v: 19", "diameter: {v: 17", 1);
    assert_ne!(edited, written, "{written}");
    fs::write(&path, &edited).unwrap();
    keys(&mut workbench, "u");
    assert!(
        workbench.message.contains("changed since") && workbench.message.contains("u again"),
        "{}",
        workbench.message
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), edited);
    keys(&mut workbench, "u");
    assert!(
        workbench.message.starts_with("let go"),
        "{}",
        workbench.message
    );
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
    workbench.selected.insert("diameter".to_string());
    keys(&mut workbench, "aC");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a confirmation");
    };
    assert!(lines[0].starts_with("diameter over 4 samples"), "{lines:?}");
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

/// The sample a collection holds under `name`.
fn held_sample(
    workbench: &Workbench,
    name: &str,
) -> std::rc::Rc<std::cell::RefCell<samplekit::core::sample::Sample>> {
    workbench
        .collection
        .iter()
        .find(|entry| entry.sample.borrow().name() == Some(name))
        .map(|entry| entry.sample.clone())
        .unwrap()
}

#[test]
fn a_reload_reads_again_only_the_files_that_changed() {
    let scratch = Scratch::new("reload-changed");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let (first, second) = (held_sample(&workbench, "C1"), held_sample(&workbench, "C2"));
    // One character, the length kept: as a USB key keeps a time, the same.
    let file = scratch.0.join("C2.md");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, text.replace("v: 10.0", "v: 10.5")).unwrap();
    workbench.reload();
    assert!(std::rc::Rc::ptr_eq(&first, &held_sample(&workbench, "C1")));
    let read = held_sample(&workbench, "C2");
    assert!(!std::rc::Rc::ptr_eq(&second, &read));
    let diameter = samplekit::core::identifier::Identifier::new("diameter").unwrap();
    let value = read.borrow().property(&diameter).unwrap().value().unwrap();
    assert_eq!(value, samplekit::core::value::Value::Number(10.5));
}

#[test]
fn a_sample_changed_in_memory_is_read_again_from_its_file() {
    let scratch = Scratch::new("reload-memory");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let alloy = samplekit::core::identifier::Identifier::new("alloy").unwrap();
    let first = held_sample(&workbench, "C1");
    first.borrow_mut().remove_attribute(&alloy).unwrap();
    workbench.reload();
    let read = held_sample(&workbench, "C1");
    assert!(!std::rc::Rc::ptr_eq(&first, &read));
    assert!(read.borrow().attribute_if(&alloy).is_some());
}

#[test]
fn a_changed_configuration_reads_every_sample_again() {
    let scratch = Scratch::new("reload-settings");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let first = held_sample(&workbench, "C1");
    let rc = scratch.0.join(".samplekitrc");
    let text = fs::read_to_string(&rc).unwrap();
    fs::write(&rc, text.replace(".1f", ".2f")).unwrap();
    workbench.reload();
    assert!(!std::rc::Rc::ptr_eq(&first, &held_sample(&workbench, "C1")));
}

#[test]
fn a_file_changed_outside_is_read_again_on_the_systems_word() {
    let scratch = Scratch::new("watched");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    if !workbench.watch_files() {
        // A system with no watch to give looks at the files as before.
        eprintln!("no watch on this system: the files are looked at instead");
        return;
    }
    // Nothing changed, nothing told: the files are not looked at.
    assert!(!workbench.catch_up());
    let file = scratch.0.join("more.md");
    fs::write(
        &file,
        "---\nschema_version: 1\nname: C4\nalloy: cobalt\n---\n",
    )
    .unwrap();
    // The system's word comes from its own thread: a moment, at most.
    let start = std::time::Instant::now();
    while !workbench.catch_up() {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "the system never told of the file written"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(workbench.collection.len(), 4);
}

#[test]
fn a_new_samples_name_is_no_path() {
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
    move_to(&mut workbench, "alloy");
    keys(&mut workbench, "e8 \n\u{1b}");
    write_waiting(&mut workbench);
    let written = fs::read_to_string(scratch.0.join("C1.md")).unwrap();
    assert!(written.contains("alloy: 8\n"), "{written}");
}

#[test]
fn an_attribute_holding_a_wikilink_is_edited() {
    // `[[Other note]]` was read as a list, and refused as readings of `notch`.
    let scratch = Scratch::new("wikilink");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "alloy");
    keys(&mut workbench, "e[[Piece log]] and [[Inspection]]\n\u{1b}");
    write_waiting(&mut workbench);
    let written = fs::read_to_string(scratch.0.join("C1.md")).unwrap();
    assert!(
        written.contains("alloy: \"[[Piece log]] and [[Inspection]]\""),
        "{written}"
    );
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
            .any(|column| column.field == "diameter")
    );
    samplekit::tui::model::remember(&workbench).unwrap();
    for name in ["C1", "C2", "C3"] {
        let path = scratch.0.join(format!("{name}.md"));
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace("diameter:", "swarf:");
        fs::write(&path, text).unwrap();
    }
    workbench = Workbench::open(&scratch.0).unwrap();
    assert!(
        !workbench
            .profile
            .columns
            .iter()
            .any(|column| column.field == "diameter")
    );
    assert!(
        workbench.message.contains("no sample holds diameter"),
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
    pick(&mut workbench, "diameter");
    assert!(matches!(workbench.mode, Mode::Figure));
    keys(&mut workbench, "j\n");
    pick(&mut workbench, "alloy");
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
    keys(&mut workbench, "\nDiameter by alloy\n");
    let form = workbench.figure.as_ref().unwrap();
    assert_eq!(form.title, "Diameter by alloy");
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
            form.declaration("alloy", "diameter"),
        )),
        samples: Vec::new(),
        output: None,
        overrides: form.overrides(),
        project_style: true,
        said: None,
        snapshot: None,
        workbench: true,
        accept_outdated: false,
    };
    let sent = samplekit::presentation::plotting::request_json(&request);
    assert!(sent.contains("\"y_scale\":\"log\""), "{sent}");
    assert!(sent.contains("\"title\":\"Diameter by alloy\""), "{sent}");
    assert!(sent.contains("\"style\":\"plain\""), "{sent}");
    // From the workbench: an advice to write the figure names `w`, not `-o`.
    assert!(sent.contains("\"workbench\":true"), "{sent}");
    // Drawn: no environment here, said, the window kept; closed, then
    // opened again on what was set up.
    keys(&mut workbench, "p");
    assert!(workbench.drawing.is_none());
    assert!(!workbench.message.is_empty());
    keys(&mut workbench, "\u{1b}p\n");
    assert_eq!(
        workbench.figure.as_ref().unwrap().y.as_deref(),
        Some("diameter")
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
    assert_eq!(workbench.view.len(), 2, "{}", workbench.message);
    write_waiting(&mut workbench);
    assert!(!path.exists());
    assert_eq!(workbench.view.len(), 2, "{}", workbench.message);
    assert!(scratch.0.join("images/C1_sem.png").exists());
    keys(&mut workbench, "uy");
    assert_eq!(fs::read_to_string(&path).unwrap(), held);
    assert_eq!(workbench.view.len(), 3);
}

#[test]
fn d_says_the_links_a_removal_leaves_leading_nowhere() {
    let scratch = Scratch::new("remove-links");
    let other = scratch.0.join("C2.md");
    let held = fs::read_to_string(&other).unwrap();
    fs::write(
        &other,
        held.replacen(
            "schema_version: 1\n",
            "schema_version: 1\ntwin: \"[[C1]]\"\n",
            1,
        ),
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.cursor = workbench
        .view
        .iter()
        .position(|entry| entry.sample.borrow().name() == Some("C1"))
        .unwrap();
    keys(&mut workbench, "D");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("asked first: {}", workbench.message);
    };
    let at = lines
        .iter()
        .position(|line| line == "1 link leads to it, and will lead nowhere:")
        .unwrap_or_else(|| panic!("{lines:?}"));
    assert_eq!(lines[at + 1], "  C2.md", "{lines:?}");
    assert_eq!(lines[at + 2], "    twin  [[C1]]", "{lines:?}");
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
    write_waiting(&mut workbench);
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
            now: "C2 · diameter".to_string(),
        })
        .unwrap();
    workbench.tick();
    let run = workbench.run.clone().unwrap();
    assert_eq!(
        (run.done, run.total, run.now.as_str()),
        (1, 3, "C2 · diameter")
    );
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
    workbench.filter = "alloy == cobalt".to_string();
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
    keys(&mut workbench, "\nchromes\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines.iter().any(|line| line.contains("[export.chromes]")),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    let written = fs::read_to_string(&rc).unwrap();
    for part in [
        "[query.chromes]",
        "[profile.chromes]",
        "[export.chromes]",
        "out/chromes.csv",
    ] {
        assert!(written.contains(part), "{part}: {written}");
    }
    samplekit::config::project_config::load(&rc).unwrap();
    // A name taken is refused.
    keys(&mut workbench, "W\nchromes\n");
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
    figure.y = Some("diameter".to_string());
    figure.x = Some("alloy".to_string());
    keys(&mut workbench, "wdiameters\ny");
    let written = fs::read_to_string(&rc).unwrap();
    assert!(
        written.contains("[figure.diameters]") && written.contains("y = \"diameter\""),
        "{written}"
    );
}

#[test]
fn n_makes_a_sample_with_values_and_fields_kept() {
    // `new NAME field=value --keep SAMPLE fields` was the command line's
    // alone: `N` took a name only.
    let scratch = Scratch::new("new-with-values");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    // Under the cursor, C1: its alloy kept, the diameter given.
    keys(&mut workbench, "NC9 diameter=14.5 keep:alloy\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines
            .iter()
            .any(|line| line.contains("kept from") && line.contains("alloy")),
        "{lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("diameter   14.5")),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    write_waiting(&mut workbench);
    let written = fs::read_to_string(scratch.0.join("C9.md")).unwrap();
    assert!(
        written.contains("alloy:") && written.contains("14.5"),
        "{written}"
    );
}

#[test]
fn t_renames_a_tag_over_what_is_shown() {
    // `tag rename` was the command line's alone.
    let scratch = Scratch::new("tag-rename");
    for file in ["C1.md", "C2.md"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .args([file, "tag", "add", "draft", "--write"])
            .current_dir(&scratch.0)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "tdraft=checked\n");
    let Mode::Confirm { title, lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert_eq!(title, "rename a tag");
    assert!(
        lines[0].contains("'draft' renamed 'checked' in 2 samples"),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    write_waiting(&mut workbench);
    for file in ["C1.md", "C2.md"] {
        let written = fs::read_to_string(scratch.0.join(file)).unwrap();
        assert!(
            written.contains("checked") && !written.contains("draft"),
            "{written}"
        );
    }
    // A tag added still needs the basket.
    keys(&mut workbench, "tother\n");
    assert!(
        workbench.message.contains("the basket is empty"),
        "{}",
        workbench.message
    );
}

#[test]
fn the_history_restores_a_snapshot_and_compares_two() {
    // `restore --at` and `diff --from --to` were the command line's alone.
    let scratch = Scratch::new("history-restore");
    for value in ["12.5", "13.0"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .args(["C1.md", "set", &format!("diameter={value}"), "--write"])
            .current_dir(&scratch.0)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    keys(&mut workbench, "H");
    // The newest first: the second, 12.5 → 13.0; then the first.
    assert!(
        workbench.history[0].message.contains("13.0"),
        "{:?}",
        workbench.history[0].message
    );
    // `d` alone shows what the first changed, from the state before it,
    // and marks it; `d` on the second compares the two.
    keys(&mut workbench, "jd");
    assert!(
        workbench.message.contains("marked"),
        "{}",
        workbench.message
    );
    let Mode::Panel { title, .. } = &workbench.mode else {
        panic!("what #2 changed: {}", workbench.message);
    };
    assert!(title.starts_with("diff #3 → #2"), "{title}");
    keys(&mut workbench, "\u{1b}kd");
    let Mode::Panel { title, lines, .. } = &workbench.mode else {
        panic!("a comparison: {}", workbench.message);
    };
    assert!(title.starts_with("diff #2 → #1"), "{title}");
    assert!(
        lines
            .iter()
            .any(|line| line.contains("12.5") && line.contains("13")),
        "{lines:?}"
    );
    keys(&mut workbench, "\u{1b}");
    // Taken back to the first change: 12.5, previewed, then written.
    keys(&mut workbench, "jr");
    let Mode::Confirm { title, lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert_eq!(title, "restore #2");
    assert!(
        lines.iter().any(|line| line.contains("diameter")),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    assert!(
        workbench.message.starts_with("restored"),
        "{}",
        workbench.message
    );
    keys(&mut workbench, "\u{1b}");
    write_waiting(&mut workbench);
    let written = fs::read_to_string(scratch.0.join("C1.md")).unwrap();
    assert!(written.contains("12.5"), "{written}");
}

#[test]
fn a_change_the_tui_writes_is_a_snapshot_saying_tui() {
    // A change written from the TUI is a snapshot saying what the TUI did,
    // after one of what changed before it — `tui`, its name.
    let scratch = Scratch::new("history");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "\n");
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, "e12.5\n\u{1b}");
    write_waiting(&mut workbench);
    assert!(
        workbench.message.starts_with("written"),
        "{}",
        workbench.message
    );
    let output = std::process::Command::new("git")
        .arg("--git-dir")
        .arg(scratch.0.join(".samplekit/history"))
        // Every machine's line: tests run together draw the machine's id
        // once, and two drew two where they met.
        .args(["log", "--all", "--format=%s"])
        .output()
        .unwrap();
    let log = String::from_utf8_lossy(&output.stdout).into_owned();
    let messages: Vec<&str> = log.lines().collect();
    assert_eq!(
        messages.len(),
        2,
        "{log}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(messages[0].starts_with("tui · "), "{log}");
    // The value changed named, not only the file.
    assert_eq!(
        messages[0], "tui · change: diameter 12.0 ± 0.1 g → 12.5 ± 0.1 g, written C1.md",
        "{log}"
    );
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
        move_to(&mut workbench, "diameter");
        keys(&mut workbench, "e12.5\n\u{1b}");
        write_waiting(&mut workbench);
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
        lines[0].contains("tui · change: diameter 12.0 ± 0.1 g → 12.5 ± 0.1 g, written C1.md"),
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
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, "e12.5\n\u{1b}");
    write_waiting(&mut workbench);
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
    assert_eq!(
        newest.message,
        "tui · change: diameter 12.0 ± 0.1 g → 12.5 ± 0.1 g, written C1.md"
    );
    assert!(
        newest
            .lines
            .iter()
            .any(|line| line.contains("diameter") && line.contains("12.5")),
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
    // `eps20` did not find `measurements.hv[20]`: only the word after the
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
    keys(&mut workbench, "\u{1b}/diameter > 1 && ld1");
    let Mode::Filter { candidates, .. } = &workbench.mode else {
        panic!("filtering");
    };
    assert!(
        candidates.contains(&"diameter > 1 && runs.load[1]".to_string()),
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
            now: "C2 · diameter".to_string(),
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
            .any(|line| line.contains("a computation, 1 of 3 values done · C2 · diameter")),
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
    keys(&mut workbench, "/diametr");
    assert!(workbench.described.get().is_some());
    assert!(
        !workbench
            .described_fields()
            .contains(&"ductility".to_string())
    );
    keys(&mut workbench, "\u{1b}");
    // A field added in an editor: the collection read again, its fields too.
    let path = scratch.0.join("C1.md");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        text.replace("properties:\n", "properties:\n  ductility: {v: 7.8}\n"),
    )
    .unwrap();
    workbench.reload();
    assert!(workbench.described.get().is_none());
    keys(&mut workbench, "/dutility");
    let Mode::Filter { candidates, .. } = &workbench.mode else {
        panic!("the filter");
    };
    assert!(
        candidates.iter().any(|candidate| candidate == "ductility"),
        "{candidates:?}"
    );
}

#[test]
fn e_on_a_table_says_how_a_table_is_changed() {
    let scratch = rich("e-table");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "runs");
    // The typing mode, on a table's line, says where its cells are typed; a
    // key typed there says it again.
    keys(&mut workbench, "e");
    assert!(workbench.typing);
    assert!(matches!(workbench.mode, Mode::Normal));
    assert!(
        workbench.message.contains("→ or Enter opens it")
            && workbench.message.contains("empty row"),
        "{}",
        workbench.message
    );
    keys(&mut workbench, "4");
    assert!(matches!(workbench.mode, Mode::Normal));
    assert!(
        workbench.message.contains("→ or Enter opens it"),
        "{}",
        workbench.message
    );
    assert!(!workbench.message.contains("samplekit set"));
}

#[test]
fn a_decimal_comma_or_a_unit_typed_is_said() {
    let scratch = rich("decimal-comma");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    // A row's cell: `4,5` read as a cell `5`, no change of anything.
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "runs");
    keys(&mut workbench, "ljjle4,5\n");
    assert!(
        workbench.message.contains("decimal comma") && workbench.message.contains("4.5"),
        "{}",
        workbench.message
    );
    // A quantity's value, in its cell: a decimal comma, then a unit, each
    // refused with the number it means, the cell kept open.
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, "e");
    workbench.key(Key::F2);
    assert!(matches!(workbench.mode, Mode::Prompt { .. }));
    workbench.key(ctrl('a'));
    workbench.key(Key::Backspace);
    keys(&mut workbench, "21,5\n");
    assert!(
        workbench.message.contains("decimal comma") && workbench.message.contains("21.5"),
        "{}",
        workbench.message
    );
    assert!(matches!(workbench.mode, Mode::Prompt { .. }));
    workbench.key(ctrl('a'));
    workbench.key(Key::Backspace);
    keys(&mut workbench, "21 g\n");
    assert!(
        workbench.message.contains("measured in g")
            && workbench.message.contains("write the number alone, as 21"),
        "{}",
        workbench.message
    );
    assert!(matches!(workbench.mode, Mode::Prompt { .. }));
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
    move_to(&mut workbench, "diameter");
    // In the typing mode, the readings' cell beside the value's.
    keys(&mut workbench, "e");
    workbench.key(Key::Right);
    keys(&mut workbench, "11, 13\n");
    let said = &workbench.message;
    assert!(said.contains("the value 12.0 stays"), "{said}");
    assert!(!said.contains("their statistic is its value"), "{said}");
    let waiting = read_waiting(&scratch.0.join("C1.md"));
    assert!(
        waiting.contains("v: 12.0") && waiting.contains("readings: [11.0, 13.0]"),
        "{waiting}"
    );
}

#[test]
fn c_over_a_value_typed_asks_first() {
    let scratch = Scratch::new("c-override");
    fs::write(
        scratch.0.join("S.md"),
        "---\nschema_version: 1\nname: S\nproperties:\n  \
         diameter: {v: 13.0, fingerprint: aaaaaaaaaaaa}\n  twice:\n    \
         v: 3.0\n    computed: {diameter: bbbbbbbbbbbb}\n    fingerprint: cccccccccccc\n---\n",
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
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, "e12.5\n\u{1b}");
    write_waiting(&mut workbench);
    keys(&mut workbench, "u");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("an undo: {}", workbench.message);
    };
    assert_eq!(lines[0], "restore C1.md", "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|line| line.contains("diameter") && line.contains("12.5") && line.contains('→')),
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
                    planned("modulus", Reason::NeverComputed, "never computed"),
                    planned("current_one", Reason::Current, "current"),
                ],
            ),
            (
                c2.clone(),
                vec![
                    planned("diameter", Reason::NeverComputed, "never computed"),
                    planned("notch", Reason::Waiting, "waits for radius"),
                ],
            ),
        ],
        unread: Vec::new(),
        ..Default::default()
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
        owed.contains(&"D1 modulus  never computed".to_string()),
        "{owed:?}"
    );
    // An entered value a formula owes: its uncertainty.
    assert!(
        owed.contains(&"C2 diameter  uncertainty never computed".to_string()),
        "{owed:?}"
    );
    assert!(
        owed.contains(&"C2 notch  waits for radius".to_string()),
        "{owed:?}"
    );
    assert!(
        !owed.iter().any(|said| said.contains("current_one")),
        "{owed:?}"
    );
    // C computes the samples with something never computed — here nothing
    // runs, no model being declared, which is said — and not one only waiting.
    keys(&mut workbench, "Cy");
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
    assert_eq!(state("modulus"), "never computed", "{entries:?}");
    // Readings with no statistic recorded, the model read: `c` no longer
    // says that a model chooses one, beside the model that does.
    assert_eq!(state("diameter"), "no statistic", "{entries:?}");
    move_to(&mut workbench, "diameter");
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
            .any(|entry| entry.field == "diameter" && entry.state == "uncertainty never computed"),
        "{entries:?}"
    );
    assert!(
        entries
            .iter()
            .any(|entry| entry.field == "notch" && entry.state == "waits for radius"),
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
    keys(&mut workbench, "e3.75\n\u{1b}");
    write_waiting(&mut workbench);
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
    keys(&mut workbench, "jjle4,5\n");
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("the prompt stays: {}", workbench.message);
    };
    assert_eq!(text, "4,5");
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
    // A name refused, the same: the typing mode left first.
    keys(&mut workbench, "\u{1b}\u{1b}\u{1b}\u{1b}N../away\n");
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
    keys(&mut workbench, "/alloy == cobalt\n");
    keys(&mut workbench, "/diameter > 10,5");
    let Mode::Filter {
        candidates, error, ..
    } = &workbench.mode
    else {
        panic!("the filter");
    };
    // No completion of its own after the comma: `diameter > 10,10.0`.
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
        error.contains("the filter in force is still alloy == cobalt"),
        "{error}"
    );
    assert!(!error.contains("set aside"), "{error}");
}

#[test]
fn numbers_are_said_at_their_precision_not_as_a_sum_left_them() {
    let scratch = Scratch::new("precision");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[property.modulus]\nprecision = \".2f\"\n\
         [property.diameter]\nprecision = \".1f\"\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join("S.md"),
        "---\nschema_version: 1\nname: S\nproperties:\n  \
         modulus: {v: 4.375000000000013, u: 0.2886751345948129}\n  \
         diameter: {v: 9.0, readings: [8.0, 10.0]}\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "S");
    move_to(&mut workbench, "modulus");
    // Its cell opens at the precision the screen shows it, the sum's noise
    // dropped, and its card says it so.
    keys(&mut workbench, "e");
    workbench.key(Key::F2);
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("a value typed: {}", workbench.message);
    };
    assert_eq!(text.text(), "4.38 +- 0.29");
    keys(&mut workbench, "\u{1b}\u{1b}i");
    assert_eq!(workbench.card_lines()[0].shown, "4.38 ± 0.29");
    // The mean of readings, at its own precision.
    keys(&mut workbench, "\u{1b}");
    go_to(&mut workbench, "diameter");
    keys(&mut workbench, "i");
    let lines = workbench.card_lines();
    assert_eq!(lines[0].field, "diameter");
    assert!(lines[0].shown.starts_with("9.0"), "{lines:?}");
    keys(&mut workbench, "\u{1b}");
    // A completion compared by order, at the field's precision: from the
    // sample, then the collection.
    keys(&mut workbench, "\u{1b}/modulus > ");
    let Mode::Filter { candidates, .. } = &workbench.mode else {
        panic!("the filter");
    };
    assert!(
        candidates
            .iter()
            .any(|candidate| candidate == "modulus > 4.38"),
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
    keys(&mut workbench, "y");
    assert_eq!(
        workbench.message,
        "removed 1 sample — not written yet: 1 file waiting, w writes, u takes it back"
    );
    write_waiting(&mut workbench);
    assert_eq!(workbench.message, "written: C1.md · u takes it back");
    keys(&mut workbench, "uy");
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
    assert_eq!(
        workbench.message,
        "removed 1 sample — not written yet: 1 file waiting, w writes, u takes it back"
    );
}

#[test]
fn results_name_paths_from_the_workbenchs_folder() {
    let scratch = rich("paths-said");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "alloy");
    // A cell kept in the typing mode waits without a word: the write says
    // its path.
    keys(&mut workbench, "ecobalt\n\u{1b}");
    assert_eq!(workbench.waiting_count(), 1, "{}", workbench.message);
    write_waiting(&mut workbench);
    assert_eq!(
        workbench.message.replace('\\', "/"),
        "written: more/D1.md · u takes it back"
    );
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
        workbench.message.starts_with("made "),
        "{}",
        workbench.message
    );
    assert!(
        !workbench.message.contains(&scratch.0.display().to_string()),
        "{}",
        workbench.message
    );
    write_waiting(&mut workbench);
    assert!(
        workbench.message.starts_with("written: "),
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
        "---\nschema_version: 1\nname: C3\nalloy: cobalt\nproperties:\n  \
         diameter: {v: 11.0, u: 0.1, unit: g, fingerprint: '000000000000'}\n  \
         twice: {v: 22.0, computed: {diameter: '111111111111'}, fingerprint: '222222222222'}\n---\n",
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
        "---\nschema_version: 1\nname: H\ncarbon: [pitch, hard coke, fullerene]\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "c");
    let Mode::Picker { items, .. } = &workbench.mode else {
        panic!("the columns");
    };
    let labels: Vec<&str> = items.iter().map(|(label, _)| label.as_str()).collect();
    assert!(labels.contains(&"carbon"), "{labels:?}");
    assert!(
        !labels.iter().any(|label| label.contains("[#")),
        "{labels:?}"
    );
    keys(&mut workbench, "\u{1b}s");
    let Mode::Picker { items, .. } = &workbench.mode else {
        panic!("the sort");
    };
    let labels: Vec<&str> = items.iter().map(|(label, _)| label.as_str()).collect();
    assert!(labels.contains(&"carbon[#0]"), "{labels:?}");
    assert!(!labels.contains(&"carbon[#1]"), "{labels:?}");
    // And `p` offers a figure of one's own in plain words.
    keys(&mut workbench, "\u{1b}p");
    let Mode::Picker { items, .. } = &workbench.mode else {
        panic!("the figures");
    };
    assert!(!items[0].0.contains("plot -x"), "{:?}", items[0]);
}

#[test]
fn c_previews_then_runs_the_model() {
    // It ran at once, where `compute` previews and asks; the message names
    // the model's file once it runs.
    let scratch = Scratch::new("no-asking");
    fs::write(scratch.0.join("interpreter"), "").unwrap();
    fs::write(
        scratch.0.join("model.py"),
        "import samplekit as sk\nclass M(sk.Sample):\n    def __init__(self):\n        \
         self.diameter = sk.Property()\n        self.ductility = sk.Property(compute_value=self._d)\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \"interpreter\"\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "C");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview first: {}", workbench.message);
    };
    assert!(
        lines.iter().any(|line| line.contains("known once it runs")),
        "{lines:?}"
    );
    assert!(workbench.running.is_none());
    keys(&mut workbench, "y");
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
            now: "C2 · diameter".to_string(),
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
fn a_filter_matching_nothing_offers_the_nearest_value() {
    // `alloy == cobalr` said `0 of 3 samples`, and nothing of why.
    let scratch = Scratch::new("filter-nearest");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "/alloy == cobalr\n");
    assert!(workbench.view.is_empty());
    assert!(
        workbench
            .message
            .contains("alloy == cobalr: did you mean cobalt?"),
        "{}",
        workbench.message
    );
    // Within a longer filter, the clause that matched nothing is named.
    keys(&mut workbench, "\u{1b}/diameter > 1 && alloy == cobalr\n");
    assert!(
        workbench
            .message
            .contains("· alloy == cobalr: did you mean cobalt?"),
        "{}",
        workbench.message
    );
    // Nothing near, nothing offered; a number is offered nothing.
    keys(&mut workbench, "\u{1b}/alloy == zirconium\n");
    assert!(workbench.view.is_empty());
    assert!(
        !workbench.message.contains("did you mean"),
        "{}",
        workbench.message
    );
    keys(&mut workbench, "\u{1b}/diameter == 12.5\n");
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
        "---\nschema_version: 1\nname: D0\nalloy: gold\nproperties:\n  \
         diameter: {v: 11.5, unit: g}\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    // Sorted by diameter, heaviest first: each project sorted within itself.
    let names = |workbench: &Workbench| -> Vec<String> {
        workbench
            .view
            .iter()
            .map(|entry| entry.sample.borrow().name().unwrap_or("").to_string())
            .collect()
    };
    workbench.sort = vec!["-diameter".to_string()];
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
    keys(&mut workbench, "g/alloy \n");
    assert_eq!(workbench.group, ["alloy"], "{}", workbench.message);
    assert_eq!(view_names(&workbench), ["C2", "C1", "C3"]);
    let headings = workbench.headings();
    assert_eq!(headings[0].as_deref(), Some("alloy = chrome"));
    assert_eq!(headings[1].as_deref(), Some("alloy = cobalt"));
    assert!(headings[2].is_none(), "{headings:?}");
    // Sorted within each group: the lightest is chrome's, whose group stays
    // first.
    keys(&mut workbench, "s/diameter\n");
    assert_eq!(view_names(&workbench), ["C2", "C3", "C1"]);
    samplekit::tui::model::remember(&workbench).unwrap();
    let mut again = Workbench::open(&scratch.0).unwrap();
    assert_eq!(again.group, ["alloy"]);
    assert_eq!(view_names(&again)[0], "C2");
    // Grouped, `g` opens on the item that ungroups.
    keys(&mut again, "g");
    let Mode::Picker { items, .. } = &again.mode else {
        panic!("a picker");
    };
    assert_eq!(items[0], ("no grouping".to_string(), false));
    assert_eq!(items[1], ("alloy".to_string(), true));
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
    keys(&mut workbench, "g/alloy \n");
    assert_eq!(view_names(&workbench), ["C2", "C1", "C3"]);
    // Heaviest first: C1 is cobalt's, so cobalt comes first.
    keys(&mut workbench, "s/diameter\nsr\n");
    assert_eq!(view_names(&workbench), ["C1", "C3", "C2"]);
    let headings = workbench.headings();
    assert_eq!(headings[0].as_deref(), Some("alloy = cobalt"));
    assert_eq!(headings[2].as_deref(), Some("alloy = chrome"));
    keys(&mut workbench, "S");
    let drawn = screen(&mut workbench);
    // A row per group, its value in the first column.
    let cobalt = drawn.find("│ cobalt").expect(&drawn);
    let chrome = drawn.find("│ chrome").expect(&drawn);
    assert!(cobalt < chrome, "{drawn}");
}

#[test]
fn a_profile_applied_comes_back_with_its_labels() {
    // Only the columns' fields were remembered: reopened, a profile's
    // columns came back under their fields' names, its labels lost.
    let scratch = Scratch::new("profile-labels");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n\
         [profile.labelled]\ncolumns = [{ field = \"name\", label = \"Billet\" }, \
         { field = \"diameter\", label = \"Diameter\", precision = \".2f\" }]\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "f/labelled\n");
    let applied = workbench.profile.columns().to_vec();
    samplekit::tui::model::remember(&workbench).unwrap();
    let again = Workbench::open(&scratch.0).unwrap();
    assert_eq!(again.profile.columns(), applied.as_slice());
    assert_eq!(again.profile.columns()[0].label.as_deref(), Some("Billet"));
}

#[test]
fn f_applies_a_profile_with_its_columns_its_sort_and_its_groups() {
    // A profile applied from `f` brings its grouping as it brings its columns
    // and its sort; one grouping by nothing ungroups.
    let scratch = Scratch::new("profile-groups");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n\
         [profile.by_alloy]\ncolumns = [{ field = \"name\" }, { field = \"diameter\" }]\n\
         sort = [\"-diameter\"]\ngroup = [\"alloy\"]\n\
         [profile.plain]\ncolumns = [{ field = \"name\" }]\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "f/by_alloy\n");
    assert_eq!(workbench.group, ["alloy"], "{}", workbench.message);
    assert_eq!(workbench.sort, ["-diameter"]);
    let fields: Vec<&str> = workbench
        .profile
        .columns()
        .iter()
        .map(|column| column.field.as_str())
        .collect();
    assert_eq!(fields, ["name", "diameter"]);
    // Sorted heaviest first, cobalt's C1 leads, and its group with it.
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
    keys(&mut workbench, "g/alloy \n");
    assert_eq!(workbench.group, ["alloy"]);
    save_as_profile(&mut workbench, "grouped");
    let written = fs::read_to_string(scratch.0.join(".samplekitrc")).unwrap();
    assert!(written.contains("[profile.grouped]"), "{written}");
    assert!(written.contains("group = [\"alloy\"]"), "{written}");
    keys(&mut workbench, "g\n");
    assert!(workbench.group.is_empty());
    save_as_profile(&mut workbench, "flat");
    let written = fs::read_to_string(scratch.0.join(".samplekitrc")).unwrap();
    let flat = &written[written.find("[profile.flat]").expect(&written)..];
    assert!(!flat.contains("group"), "{written}");
}

#[test]
fn w_keeps_each_columns_precision_and_header() {
    // `W` wrote a column's field and label alone: an export of the profile
    // then rounded otherwise than the screen.
    let scratch = Scratch::new("save-precision");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[profile.paper]\ncolumns = [\
         {field = \"diameter\", header = \"Diameter\", precision = [\".2f\", \".3f\"]}, \
         {field = \"alloy\", precision = \".1f\"}]\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.profile = workbench
        .collection
        .config()
        .unwrap()
        .profile("paper")
        .unwrap()
        .clone();
    save_as_profile(&mut workbench, "again");
    let written = fs::read_to_string(scratch.0.join(".samplekitrc")).unwrap();
    let again = &written[written.find("[profile.again]").expect(&written)..];
    assert!(
        again.contains("header = \"Diameter\"") && again.contains("precision = [\".2f\", \".3f\"]"),
        "{written}"
    );
    assert!(again.contains("precision = \".1f\""), "{written}");
}

#[test]
fn the_configuration_screen_edits_the_tui_colours() {
    // `P`'s colours are `[colors]`.
    let scratch = Scratch::new("tui-colours");
    fs::write(scratch.0.join(".samplekitrc"), "schema_version = 1\n").unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.open_workspace();
    let section = samplekit::tui::model::SECTIONS
        .iter()
        .position(|(title, ..)| *title == "TUI colours")
        .expect("a colours section");
    assert!(matches!(
        samplekit::tui::model::SECTIONS[section].1,
        samplekit::tui::model::Section::Setting("colors")
    ));
    let workspace = workbench.workspace.as_mut().unwrap();
    workspace.edit.set_setting(
        "colors",
        "outdated",
        samplekit::config::configuration_edit::value_of("yellow").unwrap(),
    );
    let text = workspace.edit.text();
    assert!(text.contains("[colors]\noutdated = \"yellow\""), "{text}");
    workspace.edit.checked().unwrap();
}

#[test]
fn a_remembered_grouping_no_sample_holds_is_set_aside_and_said() {
    // As a sort is, where the field is gone; the filter stays.
    let scratch = Scratch::new("stalegroup");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "/diameter > 10.5\n");
    workbench.group = vec!["alloy".to_string()];
    workbench.refresh().unwrap();
    samplekit::tui::model::remember(&workbench).unwrap();
    for name in ["C1", "C2", "C3"] {
        let path = scratch.0.join(format!("{name}.md"));
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace("alloy:", "brand:");
        fs::write(&path, text).unwrap();
    }
    let again = Workbench::open(&scratch.0).unwrap();
    assert!(again.group.is_empty());
    assert!(
        again.message.contains("grouping was set aside"),
        "{}",
        again.message
    );
    assert_eq!(again.filter, "diameter > 10.5");
}

#[test]
fn groups_are_made_within_each_project() {
    // The projects apart first, each one's groups within it.
    let scratch = rich("groups-projects");
    fs::write(scratch.0.join("more/.samplekitrc"), "schema_version = 1\n").unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.group = vec!["alloy".to_string()];
    workbench.refresh().unwrap();
    assert_eq!(view_names(&workbench), ["D1", "C2", "C1", "C3"]);
    let headings = workbench.headings();
    assert_eq!(headings[0].as_deref(), Some("more · alloy = chrome"));
    let own = headings[1].clone().unwrap();
    assert!(own.ends_with(" · alloy = chrome"), "{own}");
    assert!(
        headings[2]
            .as_deref()
            .unwrap()
            .ends_with(" · alloy = cobalt")
    );
    assert!(headings[3].is_none(), "{headings:?}");
}

#[test]
fn the_filter_reads_state_and_completes_its_words() {
    // `state` is a field the workbench's filter reads.
    let scratch = Scratch::new("filter-state");
    std::fs::write(
        scratch.0.join("C2.md"),
        "---\nschema_version: 1\nname: C2\nalloy: chrome\nproperties:\n  \
         diameter: {v: 10.0}\n  volume: {computed: {diameter: bbbbbbbbbbbb}, \
         fingerprint: {failed: ZeroDivisionError}}\n---\nN.\n",
    )
    .unwrap();
    std::fs::write(
        scratch.0.join("C3.md"),
        "---\nschema_version: 1\nname: C3\nalloy: cobalt\nproperties:\n  diameter: {v: \"2.0\"}\n---\nN.\n",
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
        "---\nschema_version: 1\nname: C2\nalloy: chrome\nproperties:\n  \
         diameter: {v: 10.0}\n  carbon: {v: \"2.0\"}\n  volume: {computed: {diameter: bbbbbbbbbbbb}, \
         fingerprint: {failed: ZeroDivisionError}}\n---\nN.\n",
    )
    .unwrap();
    std::fs::write(
        scratch.0.join("C3.md"),
        "---\nschema_version: 1\nname: C3\nalloy: cobalt\n---\nN.\n",
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
         has a cursor that `←` `→` `Home` `End` move; a sample's value opens \
         selected, so that what is typed replaces it, and an arrow or `End` keeps \
         it to edit.\n",
    );

    page.push_str(
        "\n## Typing\n\nIn every line typed (the filter, a prompt, a cell of the typing \
         mode, a list's `/`, the start page's name and narrowing) and in the note. A \
         window's `Enter`, `Esc` and `Tab` stay the window's.\n\n",
    );
    page.push_str(HEAD);
    for (key, said) in samplekit::tui::typing::TYPING {
        page.push_str(&format!("| {} | {} |\n", keys(key), text(said)));
    }
    page.push_str("\n**In the note** (`N` on a sample), besides:\n\n");
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
            "What a project opens on: its samples, one per line. The basket, the \
             samples `Space` puts in it, is kept from one session to the next, \
             with the filter, the sort and the columns: `C`, `p`, `t` and `D` act \
             on it alone while it holds any, and say so; `x` empties it.",
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
            Place::Items,
            "A list's window",
            "`Enter` on an attribute: its items, one per row.",
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
        // The card opens over a sample and a table, with keys of its own.
        if place == Place::Table {
            page.push_str(
                "\n## A value's card\n\n`i` on a value of a sample, or on a table's cell \
                 holding one: the value, its readings, what it reads and what reads it, each \
                 with its value and its state.\n\n",
            );
            page.push_str(HEAD);
            for (key, said) in [
                ("j k ↓ ↑", "the next line, the line above"),
                (
                    "e Enter F2",
                    "type over the line under the cursor: what is typed waits, and the card \
                     is drawn again as it makes it",
                ),
                ("w", "the changes waiting, previewed, written by y"),
                ("u", "take back the last change"),
                ("Esc q i", "close the card"),
            ] {
                page.push_str(&format!("| {} | {} |\n", keys(key), text(said)));
            }
        }
    }

    page.push_str(
        "\n## Questions and lists\n\n\
         **A question before a write** shows what would change, as the command \
         line's preview does:\n\n",
    );
    page.push_str(HEAD);
    for (key, said) in [
        ("y", "write it"),
        ("n Esc", "leave it: nothing is written"),
        ("j k ↓ ↑", "read a long question to its end"),
    ] {
        page.push_str(&format!("| {} | {} |\n", keys(key), text(said)));
    }
    page.push_str(
        "\n**A list to choose from**, as `s`, `c`, `g` or `p` open \
         one:\n\n",
    );
    page.push_str(HEAD);
    for (key, said) in [
        ("j k ↓ ↑", "move"),
        ("Home End PgUp PgDn", "first, last, a page"),
        (
            "Space",
            "in the sort, the columns and the groups, tick or untick",
        ),
        (
            "Shift+↑ Shift+↓ K J",
            "in the sort, the columns and the groups, the item under the cursor up or down: its order",
        ),
        (
            "r",
            "in the sort, the field under the cursor descending, or back, and ticked",
        ),
        ("/", "narrow the list to what holds the letters typed"),
        ("Tab", "in `p`, the next group: scripts, figures, exports"),
        ("w", "in `p`, a figure to a file"),
        ("Enter", "accept"),
        ("Esc", "clear the narrowing, then leave the list"),
    ] {
        page.push_str(&format!("| {} | {} |\n", keys(key), text(said)));
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
    // Its line first, opened by `Enter`, it changes what the file writes and
    // never the file's name; emptied, `name:` goes and the file's name stands
    // for it.
    let scratch = Scratch::new("name-edited");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    assert_eq!(workbench.entries()[0].field, "name");
    move_to(&mut workbench, "name");
    keys(&mut workbench, "\n");
    for _ in 0..4 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "Billet one\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines.iter().any(|line| line.contains("C1  →  Billet one")),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    let path = scratch.0.join("C1.md");
    assert!(read_waiting(&path).contains("name: Billet one"));
    move_to(&mut workbench, "name");
    keys(&mut workbench, "\n");
    for _ in 0..12 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines.iter().any(|line| line.contains("Billet one  →  C1")),
        "{lines:?}"
    );
    // What it means, beneath.
    assert!(
        lines.iter().any(|line| line.contains("the file's name")),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    write_waiting(&mut workbench);
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
    samplekit::tui::model::remember(&workbench).unwrap();
    let mut again = Workbench::open(&scratch.0).unwrap();
    assert!(again.note_hidden);
    open_named(&mut again, "D1");
    keys(&mut again, "n");
    assert!(!again.note_hidden);
    samplekit::tui::model::remember(&again).unwrap();
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
    assert!(shown.contains(&"diameter".to_string()), "{shown:?}");
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
    keys(&mut workbench, "/diameter alpha beta");
    workbench.key(held(KeyCode::Left, KeyModifiers::CONTROL));
    assert_eq!(typed_line(&workbench).1, "diameter alpha ");
    workbench.key(Key::End);
    workbench.key(held(KeyCode::Backspace, KeyModifiers::CONTROL));
    assert_eq!(typed_line(&workbench).0, "diameter alpha ");
    workbench.key(ctrl('z'));
    assert_eq!(typed_line(&workbench).0, "diameter alpha beta");
    workbench.key(held(
        KeyCode::Char('Z'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ));
    assert_eq!(typed_line(&workbench).0, "diameter alpha ");
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
    keys(&mut workbench, "\u{1b}/allo\t");
    assert!(
        typed_line(&workbench).0.starts_with("alloy"),
        "{:?}",
        typed_line(&workbench)
    );
    // A prompt: the name, opened by `Enter`, a word taken back.
    keys(&mut workbench, "\u{1b}");
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "name");
    keys(&mut workbench, "\n");
    workbench.key(ctrl('a'));
    keys(&mut workbench, "reference checked");
    workbench.key(held(KeyCode::Backspace, KeyModifiers::CONTROL));
    assert_eq!(typed_line(&workbench).0, "reference ");
    // A value's cell in the typing mode, its text kept: all selected, then
    // typed over.
    keys(&mut workbench, "\u{1b}");
    workbench.key(Key::Home);
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, "e");
    workbench.key(Key::F2);
    assert!(matches!(workbench.mode, Mode::Prompt { .. }));
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
        // The last line's end the editor holds, as the file does.
        let text = text.text();
        text.strip_suffix('\n').map_or(text.clone(), str::to_string)
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
    // Ctrl+Y on a confirmation does nothing: the sample's removal, asked
    // first.
    open_named(&mut workbench, "D1");
    keys(&mut workbench, "D");
    assert!(
        matches!(workbench.mode, Mode::Confirm { .. }),
        "{}",
        workbench.message
    );
    workbench.key(ctrl('y'));
    assert!(matches!(workbench.mode, Mode::Confirm { .. }));
    assert_eq!(workbench.waiting_count(), 0, "{}", workbench.message);
    // Declined: the sample stays open.
    keys(&mut workbench, "n");
    assert!(matches!(workbench.screen, Screen::Sample { .. }));
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
    keys(&mut workbench, "/alloy");
    assert!(!workbench.copy());
    workbench.key(ctrl('a'));
    assert!(workbench.copy());
    keys(&mut workbench, "\u{1b}/");
    workbench.key(ctrl('v'));
    assert_eq!(typed_line(&workbench).0, "alloy");
    // A paste, where the cursor is.
    workbench.key(Key::Home);
    workbench.paste("diameter > 1 && ");
    assert_eq!(typed_line(&workbench).0, "diameter > 1 && alloy");
    // Into the note, and nowhere on the collection.
    keys(&mut workbench, "\u{1b}");
    workbench.paste("alloy");
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

#[test]
fn a_refused_value_keeps_its_window_and_is_said_as_typed() {
    // Text where a number goes closed the window, the typing lost, and the
    // advice was `set`'s: `diameter=1.05`.
    let scratch = rich("refused-typing");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, "e");
    for _ in 0..4 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "abc\n");
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("the window closed: {}", workbench.message);
    };
    assert_eq!(text.text(), "abc");
    assert!(
        workbench.message.contains("write the number alone, as"),
        "{}",
        workbench.message
    );
}

#[test]
fn a_value_previewed_is_the_one_written() {
    // At the declared `.1f`, `13.25` was previewed `→ 13.2` and written 13.25.
    let scratch = rich("preview-whole");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, "e13.25\n\u{1b}");
    let waiting = read_waiting(&scratch.0.join("C1.md"));
    assert!(waiting.contains("v: 13.25"), "{waiting}");
    // What waits is previewed as it will be written.
    keys(&mut workbench, "w");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines.iter().any(|line| line.contains("→  13.25")),
        "{lines:?}"
    );
}

#[test]
fn a_list_to_choose_from_moves_by_its_ends_and_pages() {
    // Home, End, PgUp and PgDn moved every list but these.
    let scratch = rich("picker-ends");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "s");
    workbench.key(Key::End);
    let Mode::Picker { cursor, items, .. } = &workbench.mode else {
        panic!("a list to choose from");
    };
    assert_eq!(*cursor, items.len() - 1);
    workbench.key(Key::Home);
    let Mode::Picker { cursor, .. } = &workbench.mode else {
        panic!("a list to choose from");
    };
    assert_eq!(*cursor, 0);
}

#[test]
fn a_file_not_read_is_counted_on_the_collection() {
    // A file with broken YAML showed on no line, and only `v` said it.
    let scratch = rich("unread-counted");
    fs::write(scratch.0.join("broken.md"), "---\nname: [unclosed\n---\n").unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let drawn = screen(&mut workbench);
    assert!(drawn.contains("1 file not read — v says why"), "{drawn}");
}

#[test]
fn a_file_changed_outside_is_read_again() {
    // An edit made in another editor stayed unseen on the screen until the
    // workbench next wrote, and the preview of that write judged the old file.
    let scratch = rich("changed-outside");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    assert!(!workbench.catch_up());
    let file = scratch.0.join("C1.md");
    let text = fs::read_to_string(&file).unwrap();
    fs::write(&file, text.replace("cobalt", "duplex-cobalt")).unwrap();
    assert!(workbench.catch_up());
    assert!(
        workbench.message.contains("read again"),
        "{}",
        workbench.message
    );
    let held = workbench.collection.iter().any(|entry| {
        let sample = entry.sample.borrow();
        sample.name() == Some("C1")
            && sample
                .attribute(&samplekit::core::identifier::Identifier::new("alloy").unwrap())
                .is_ok_and(|value| format!("{value:?}").contains("duplex-cobalt"))
    });
    assert!(held);
    assert!(!workbench.catch_up());
}

#[test]
fn a_configuration_or_a_model_changed_outside_is_read_again() {
    // Only the samples were looked at: `.samplekitrc` and the model edited
    // in another editor stayed unseen until the workbench was opened again.
    let scratch = Scratch::new("rc-changed-outside");
    fs::create_dir_all(scratch.0.join("model")).unwrap();
    fs::write(scratch.0.join("model/piece.py"), "import helper\n").unwrap();
    fs::write(scratch.0.join("model/helper.py"), "RATE = 1\n").unwrap();
    let rc = scratch.0.join(".samplekitrc");
    let declared = fs::read_to_string(&rc).unwrap()
        + "[model]\npath = \"model/piece.py\"\npython = \".venv\"\n";
    fs::write(&rc, &declared).unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    assert!(!workbench.catch_up(), "{}", workbench.message);
    fs::write(&rc, declared.replace("\".1f\"", "\".2f\"")).unwrap();
    assert!(workbench.catch_up());
    assert!(!workbench.catch_up());
    fs::write(
        scratch.0.join("model/piece.py"),
        "import helper\n# a formula\n",
    )
    .unwrap();
    assert!(workbench.catch_up());
    fs::write(scratch.0.join("model/helper.py"), "RATE = 2\n").unwrap();
    assert!(workbench.catch_up());
    assert!(!workbench.catch_up());
}

#[test]
fn the_notes_last_line_takes_the_editors_line_keys() {
    // Held without its end, the last line broke the editor's line keys:
    // Enter at its end then `ABC` wrote `A` alone and `BC` on the next line,
    // and Ctrl+D doubled it onto itself.
    let scratch = rich("note-last-line");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    keys(&mut workbench, "N");
    workbench.key(held(
        crossterm::event::KeyCode::End,
        crossterm::event::KeyModifiers::CONTROL,
    ));
    workbench.key(Key::Up);
    workbench.key(Key::End);
    workbench.key(Key::Enter);
    keys(&mut workbench, "ABC");
    workbench.key(ctrl('d'));
    keys(&mut workbench, "\u{1b}y");
    write_waiting(&mut workbench);
    let written = fs::read_to_string(scratch.0.join("more/D1.md")).unwrap();
    assert!(
        written.ends_with("Second line.\nABC\nABC\n"),
        "{written}\n{}",
        workbench.message
    );
}

#[test]
fn a_table_typed_in_the_note_goes_from_cell_to_cell() {
    // A header's Enter left the cursor at the end of the dashes, where what
    // was typed next went; Tab stopped before a cell's space, and left the
    // table from its last cell.
    let scratch = rich("note-table");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    keys(&mut workbench, "N");
    workbench.key(held(
        crossterm::event::KeyCode::End,
        crossterm::event::KeyModifiers::CONTROL,
    ));
    keys(&mut workbench, "\n| h1 | h2 |\n1\t2\t3");
    workbench.key(Key::BackTab);
    keys(&mut workbench, "x");
    keys(&mut workbench, "\u{1b}y");
    write_waiting(&mut workbench);
    let written = fs::read_to_string(scratch.0.join("more/D1.md")).unwrap();
    let table: Vec<&str> = written
        .lines()
        .skip_while(|line| !line.starts_with("| h1"))
        .collect();
    assert_eq!(table.len(), 4, "{written}");
    assert!(table[1].starts_with("|----|"), "{written}");
    assert!(table[2].starts_with("| 1"), "{written}");
    // Shift+Tab from the new row's first cell: the row above's last.
    assert!(table[2].contains("| x2"), "{written}");
    assert!(table[3].starts_with("| 3"), "{written}");
}

#[test]
fn enter_at_a_rows_end_begins_the_next_in_its_first_cell() {
    // The new row's cursor stayed in the column it had, and what was typed
    // went into its last cell.
    let scratch = rich("note-table-enter");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    keys(&mut workbench, "N");
    workbench.key(held(
        crossterm::event::KeyCode::End,
        crossterm::event::KeyModifiers::CONTROL,
    ));
    keys(&mut workbench, "\n| h1 | h2 | h3 |\na\tb\tc\nZ");
    keys(&mut workbench, "\u{1b}y");
    write_waiting(&mut workbench);
    let written = fs::read_to_string(scratch.0.join("more/D1.md")).unwrap();
    let table: Vec<&str> = written
        .lines()
        .skip_while(|line| !line.starts_with("| h1"))
        .collect();
    assert!(table[3].starts_with("| Z"), "{written}");
}

#[test]
fn a_value_cut_by_its_pane_ends_with_an_ellipsis() {
    // At 40 columns a value's unit was cut without a sign, `g` read as whole.
    let scratch = rich("value-cut");
    fs::write(
        scratch.0.join("C1.md"),
        "---\nschema_version: 1\nname: C1\nproperties:\n  diameter: {v: 12.0, u: 0.1, unit: grams_of_diameter_per_litre}\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    let drawn = drawn_at(&mut workbench, 40);
    let line = drawn
        .lines()
        .find(|line| line.contains("diameter"))
        .unwrap_or_default()
        .to_string();
    assert!(line.contains('…'), "{drawn}");
}

#[test]
fn a_line_typed_after_the_notes_end_takes_the_line_keys() {
    // Typed on the line after the note's end, which has no end of its own,
    // `Last` then Ctrl+D wrote `LastLast`, and Alt+2 made no heading.
    let scratch = rich("note-typed-last");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    keys(&mut workbench, "N");
    workbench.key(held(
        crossterm::event::KeyCode::End,
        crossterm::event::KeyModifiers::CONTROL,
    ));
    keys(&mut workbench, "Last");
    workbench.key(ctrl('d'));
    workbench.key(alt('2'));
    keys(&mut workbench, "\u{1b}y");
    write_waiting(&mut workbench);
    let written = fs::read_to_string(scratch.0.join("more/D1.md")).unwrap();
    assert!(
        written.contains("\nLast\n## Last\n"),
        "{written}\n{}",
        workbench.message
    );
}

#[test]
fn a_cells_refusal_is_said_in_its_windows_words() {
    // A table's cell refused text with `set`'s advice:
    // `runs.load[…]=1.05`, where a value's window said *as 1.05*.
    let scratch = rich("cell-refusal");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "runs");
    workbench.key(Key::Right);
    keys(&mut workbench, "eabc\n");
    assert!(
        workbench.message.contains("write the number alone, as"),
        "{}",
        workbench.message
    );
    assert!(!workbench.message.contains("]="), "{}", workbench.message);
}

#[test]
fn a_computer_not_known_yet_is_named_on_the_history_screen() {
    let scratch = rich("name-machine");
    // Another computer, its own state directory, writes here.
    let state = scratch.0.join("..").join("name-machine-state");
    let _ = fs::remove_dir_all(&state);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["C1.md", "tag", "add", "seen", "--write"])
        .current_dir(&scratch.0)
        .env_remove("SAMPLEKIT_MACHINE")
        .env("SAMPLEKIT_STATE_DIR", &state)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let identifier = fs::read_to_string(state.join("machine"))
        .unwrap()
        .trim()
        .to_string();
    let _ = fs::remove_dir_all(&state);
    // Then this one.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["C1.md", "tag", "add", "again", "--write"])
        .current_dir(&scratch.0)
        .env_remove("SAMPLEKIT_MACHINE")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "H");
    assert!(matches!(workbench.screen, Screen::History { .. }));
    assert!(
        matches!(workbench.mode, Mode::Prompt { .. }),
        "its name is asked"
    );
    for _ in 0..80 {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "the other one\n");
    assert!(matches!(workbench.mode, Mode::Normal));
    let names = samplekit::config::version_control::machine_names();
    assert_eq!(
        names.get(&identifier).map(String::as_str),
        Some("the other one")
    );
    assert!(
        workbench.history.iter().any(|row| row
            .machine
            .as_deref()
            .is_some_and(|m| m.contains("the other one"))),
        "shown by its name"
    );
    // In the list too, after *when*, as `log` writes it.
    let drawn = drawn_at(&mut workbench, 160);
    let listed = drawn
        .lines()
        .filter(|line| line.contains("the other one"))
        .count();
    assert!(listed >= 2, "{drawn}");
    // Asked once: the history opened again asks nothing.
    keys(&mut workbench, "\u{1b}H");
    assert!(matches!(workbench.mode, Mode::Normal));
    // This machine's list of names is left as it was.
    samplekit::config::version_control::name_machine(&identifier, "").unwrap();
}

#[test]
fn ctrl_c_over_unsaved_typing_asks_first() {
    // Ctrl+C in the note or a line closed it at once, what was typed lost.
    let scratch = rich("ctrl-c-typing");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "Nhalf-typed");
    assert!(matches!(workbench.mode, Mode::Prompt { .. }));
    assert!(!workbench.interrupt(), "not at once");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("asked: {}", workbench.message);
    };
    assert!(
        lines.iter().any(|line| line.contains("not saved")),
        "{lines:?}"
    );
    // n gives the line back as it was left.
    keys(&mut workbench, "n");
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("the line back");
    };
    assert_eq!(text.text(), "half-typed");
    // Asked again, y quits.
    assert!(!workbench.interrupt());
    assert!(matches!(
        keys(&mut workbench, "y").last(),
        Some(Effect::Quit)
    ));
    // Nothing typed: Ctrl+C quits at once, as ever.
    let mut idle = Workbench::open(&scratch.0).unwrap();
    assert!(idle.interrupt());
}

#[test]
fn w_in_p_writes_a_figure_to_a_file() {
    // `p` opened a window only; without a display, the message sent to
    // `-o figure.pdf`, an option of the command line.
    let scratch = rich("figure-file");
    let rc = fs::read_to_string(scratch.0.join(".samplekitrc")).unwrap();
    fs::write(
        scratch.0.join(".samplekitrc"),
        format!("{rc}[figure.diameters]\nx = \"diameter\"\ny = \"diameter\"\n"),
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    // The declared figure, after one's own.
    keys(&mut workbench, "pjw");
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("its file asked: {}", workbench.message);
    };
    assert_eq!(text.text(), "diameters.pdf");
    keys(&mut workbench, "\n");
    // Here no Python is declared: said, and nothing half-drawn.
    assert!(
        matches!(workbench.mode, Mode::Normal),
        "{}",
        workbench.message
    );
    assert!(
        workbench.message.contains("diameters") || workbench.message.contains("interpreter"),
        "{}",
        workbench.message
    );
    // w on one's own figure says what it is for.
    keys(&mut workbench, "pw");
    assert!(
        workbench.message.contains("w writes a figure"),
        "{}",
        workbench.message
    );
}

#[test]
#[cfg(unix)]
fn w_on_a_figure_of_one_sample_writes_a_file_each() {
    // `p` then `w` on a figure the model draws for one sample wrote the
    // sample under the cursor alone, saying 3 samples.
    let venv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".venv");
    assert!(venv.join("bin/python").is_file(), "the repository's .venv");
    let scratch = Scratch::new("figure-each");
    std::os::unix::fs::symlink(&venv, scratch.0.join(".venv")).unwrap();
    fs::write(
        scratch.0.join("model.py"),
        "import samplekit as sk\nclass M(sk.Sample):\n    @sk.figure\n    def look(self, ax):\n        \
         ax.plot([0, 1], [0, 1])\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
    )
    .unwrap();
    for name in ["A", "B"] {
        fs::write(
            scratch.0.join(format!("{name}.md")),
            format!("---\nschema_version: 1\nname: {name}\n---\n"),
        )
        .unwrap();
    }
    let config = samplekit::config::project_config::load_for(&scratch.0)
        .unwrap()
        .unwrap();
    samplekit::config::model_runtime::describe(&config, &scratch.0).unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "pjw");
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("its file asked: {}", workbench.message);
    };
    assert_eq!(text.text(), "look-{name}.pdf");
    // A name without `{name}` would hold one sample of two: refused.
    for _ in 0..text.text().chars().count() {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "look.pdf\n");
    assert!(
        workbench.message.contains("name the file with {name}"),
        "{}",
        workbench.message
    );
}

#[test]
#[cfg(unix)]
fn ps_menu_says_of_each_figure_what_list_figures_says() {
    // A figure of the model was said *drawn by the model*, with nothing of
    // what it acts on, and a declared one without a title had no line.
    let venv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".venv");
    assert!(venv.join("bin/python").is_file(), "the repository's .venv");
    let scratch = Scratch::new("menu-figures");
    let project = scratch.0.join("p");
    fs::create_dir(&project).unwrap();
    std::os::unix::fs::symlink(&venv, project.join(".venv")).unwrap();
    fs::write(
        project.join("model.py"),
        "import samplekit as sk\nclass M(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        self.a = sk.Property()\n\n    \
         @sk.figure\n    def look(self, ax):\n        \"\"\"The specimen at a glance.\n\n        More.\"\"\"\n        \
         ax.plot([0, 1], [0, 1])\n",
    )
    .unwrap();
    fs::write(
        project.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n\
         [figure.spread]\nx = \"a\"\ny = \"a\"\n",
    )
    .unwrap();
    fs::write(
        project.join("s1.md"),
        "---\nschema_version: 1\nname: s1\nproperties:\n  a: 1.0\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&project).unwrap();
    keys(&mut workbench, "p");
    let drawn = drawn_at(&mut workbench, 120);
    let look = drawn
        .lines()
        .find(|line| line.contains("look"))
        .unwrap_or_else(|| panic!("{drawn}"));
    assert!(look.contains("each sample"), "{drawn}");
    assert!(look.contains("The specimen at a glance."), "{drawn}");
    assert!(!look.contains("More."), "{drawn}");
    let spread = drawn
        .lines()
        .find(|line| line.contains("spread"))
        .unwrap_or_else(|| panic!("{drawn}"));
    assert!(spread.contains("selection"), "{drawn}");
    assert!(spread.contains("of a against a"), "{drawn}");
}

#[test]
fn w_over_a_figures_file_there_already_asks_first() {
    // `w` drew over a file of that name without a word.
    let scratch = rich("figure-over");
    let rc = fs::read_to_string(scratch.0.join(".samplekitrc")).unwrap();
    fs::write(
        scratch.0.join(".samplekitrc"),
        format!("{rc}[figure.diameters]\nx = \"diameter\"\ny = \"diameter\"\n"),
    )
    .unwrap();
    fs::write(scratch.0.join("diameters.pdf"), "kept").unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "pjw\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("asked: {}", workbench.message);
    };
    assert!(
        lines.iter().any(|line| line.contains("diameters.pdf")),
        "{lines:?}"
    );
    keys(&mut workbench, "n");
    assert!(matches!(workbench.mode, Mode::Normal));
    assert_eq!(
        fs::read_to_string(scratch.0.join("diameters.pdf")).unwrap(),
        "kept"
    );
}

#[test]
fn a_tables_line_says_its_cells_outdated() {
    // Each cell was marked in the table, and the sample's line for the
    // table said nothing.
    let scratch = rich("table-line-outdated");
    fs::write(
        scratch.0.join("T.md"),
        "---\nschema_version: 1\nname: T\ntables:\n  tempering:\n    index: day\n    \
         columns:\n      day: {}\n      co2: {unit: vol}\n      recovery: {unit: \"%\"}\n    \
         rows:\n      - day: 1\n        co2: 2.0\n        recovery: {v: 90.0, \
         computed: {row.co2: aaaaaaaaaaaa}, fingerprint: bbbbbbbbbbbb}\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "T");
    let line = workbench
        .entries()
        .into_iter()
        .find(|entry| entry.field == "tempering")
        .expect("the table's line");
    // Its fingerprint no longer its value's: edited by hand, as the table
    // marks the cell; an input changed says outdated alike.
    assert_eq!(
        line.state,
        "tempering.recovery edited — typed over its formula"
    );
}

#[test]
fn a_figure_declared_without_a_name_is_said_as_a_figure() {
    // Its empty name was refused as "an unnamed property".
    let scratch = rich("figure-no-name");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "p\n");
    let figure = workbench.figure.as_mut().unwrap();
    figure.y = Some("diameter".to_string());
    figure.x = Some("alloy".to_string());
    keys(&mut workbench, "w");
    let Mode::Prompt { .. } = &workbench.mode else {
        panic!("its name asked: {}", workbench.message);
    };
    workbench.key(ctrl('d'));
    keys(&mut workbench, "\n");
    assert!(
        workbench.message.contains("a figure needs a name"),
        "{}",
        workbench.message
    );
}

#[test]
fn an_index_is_changed_as_any_cell() {
    // Its own key, `i`, reached it: the cursor goes over the index as over
    // any column, and the typing mode changes it, the row moving with it.
    let scratch = rich("index-edit");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "runs");
    keys(&mut workbench, "le");
    workbench.key(Key::F2);
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("the index asked: {}", workbench.message);
    };
    assert_eq!(text.text(), "1");
    workbench.key(Key::Backspace);
    // Kept at once, its preview confirmed: the change waits.
    keys(&mut workbench, "5\n");
    assert!(
        matches!(workbench.mode, Mode::Normal),
        "kept: {}",
        workbench.message
    );
    assert!(read_waiting(&scratch.0.join("more/D1.md")).contains("run: 5"));
    keys(&mut workbench, "\u{1b}");
    write_waiting(&mut workbench);
    let written = fs::read_to_string(scratch.0.join("more/D1.md")).unwrap();
    assert!(written.contains("run: 5"), "{written}");
    assert!(
        written.contains("load: 3.5"),
        "the row's cells kept: {written}"
    );
    // `i` is a value's card, and the index holds none: said, and the typing
    // mode not entered.
    keys(&mut workbench, "i");
    assert!(workbench.card.is_none());
    assert!(!workbench.typing);
    assert!(
        workbench
            .message
            .contains("a cell holding a value has a card"),
        "{}",
        workbench.message
    );
}

#[test]
#[cfg(unix)]
fn a_table_with_no_row_unfolds_to_be_filled() {
    // Only a prompt for a whole row reached a table the model declares and
    // the file holds none of: it unfolds with its columns, the empty row to
    // fill, its computed columns marked.
    let venv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".venv");
    assert!(venv.join("bin/python").is_file(), "the repository's .venv");
    let scratch = Scratch::new("table-to-fill");
    std::os::unix::fs::symlink(&venv, scratch.0.join(".venv")).unwrap();
    fs::write(
        scratch.0.join("model.py"),
        "import samplekit as sk\nclass Piece(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        self.ageing = sk.Table(\n            \
         {'day': sk.Property(unit='d'), 'hardness': sk.Property(), \
         'drop': sk.Property(compute=lambda row: {'drop': 1.0 - row.hardness.value}, \
         depends_on=['row.hardness'])},\n            index='day')\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join("F.md"),
        "---\nschema_version: 1\nname: F\n---\n",
    )
    .unwrap();
    let config = samplekit::config::project_config::load_for(&scratch.0)
        .unwrap()
        .unwrap();
    samplekit::config::model_runtime::describe(&config, &scratch.0).unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "F");
    move_to(&mut workbench, "ageing");
    workbench.key(Key::Right);
    assert!(
        matches!(workbench.screen, Screen::Table { .. }),
        "{}",
        workbench.message
    );
    assert!(workbench.on_the_empty_row());
    let drawn = drawn_at(&mut workbench, 100);
    assert!(
        drawn.contains("day") && drawn.contains("hardness"),
        "{drawn}"
    );
    // A computed column is the model's, not typed.
    keys(&mut workbench, "llex");
    assert!(
        workbench.message.contains("computed by the model"),
        "{}",
        workbench.message
    );
    // Its row typed cell by cell, kept once its index is given.
    workbench.key(Key::Left);
    keys(&mut workbench, "1.050\n");
    workbench.key(Key::Left);
    keys(&mut workbench, "0\n");
    assert!(
        read_waiting(&scratch.0.join("F.md")).contains("hardness: 1.05"),
        "{}",
        workbench.message
    );
    keys(&mut workbench, "\u{1b}");
    write_waiting(&mut workbench);
    let written = fs::read_to_string(scratch.0.join("F.md")).unwrap();
    assert!(written.contains("hardness: 1.05"), "{written}");
}

#[test]
fn c_in_the_collection_computes_every_sample_shown() {
    // On a current line, `C` computed that sample alone and answered *nothing
    // to compute*, the line below it not current: *C compute all* was not.
    let scratch = Scratch::new("compute-all-shown");
    fs::write(scratch.0.join("interpreter"), "").unwrap();
    fs::write(
        scratch.0.join("model.py"),
        "import samplekit as sk\nclass M(sk.Sample):\n    def __init__(self):\n        \
         self.diameter = sk.Property()\n        self.ductility = sk.Property(compute_value=self._d)\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \"interpreter\"\n",
    )
    .unwrap();
    for name in ["a", "b", "c"] {
        fs::write(
            scratch.0.join(format!("{name}.md")),
            format!("---\nschema_version: 1\nname: {name}\nproperties:\n  diameter: 1.0\n---\n"),
        )
        .unwrap();
    }
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "C");
    // The samples it may write, as its preview holds them: every one shown.
    // What it writes waits, so no file is held aside as it runs.
    let reached = |workbench: &Workbench| -> usize {
        let Mode::Confirm { pending, .. } = &workbench.mode else {
            return 0;
        };
        let samplekit::tui::model::Pending::Compute(computing) = pending.as_ref() else {
            return 0;
        };
        computing
            .paths
            .iter()
            .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
            .count()
    };
    assert!(workbench.view.len() > 1);
    assert_eq!(
        reached(&workbench),
        workbench.view.len(),
        "{}",
        workbench.message
    );
    keys(&mut workbench, "y");
    // The basket first, where it holds any.
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, " C");
    // Kept from a session to the next, the basket is said where `C` acts on
    // it alone, in its preview.
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines
            .iter()
            .any(|line| line.contains("the basket's 1 sample")),
        "{lines:?}"
    );
    assert_eq!(reached(&workbench), 1, "{}", workbench.message);
}

#[test]
fn readings_with_no_statistic_say_they_give_no_value() {
    // The preview said their statistic was the value, where no statistic is
    // recorded and the model declares none: the value stayed empty.
    let scratch = Scratch::new("readings-no-statistic");
    fs::write(
        scratch.0.join("C4.md"),
        "---\nschema_version: 1\nname: C4\nproperties:\n  ore: {unit: kg}\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C4");
    move_to(&mut workbench, "ore");
    // In the typing mode, the readings' cell.
    keys(&mut workbench, "e");
    workbench.key(Key::Right);
    keys(&mut workbench, "4.1, 4.2\n");
    let said = &workbench.message;
    assert!(said.contains("no statistic of them is recorded"), "{said}");
    assert!(!said.contains("their statistic is its value"), "{said}");
    assert!(
        read_waiting(&scratch.0.join("C4.md")).contains("readings: [4.1, 4.2]"),
        "kept, waiting"
    );
}

#[test]
fn a_sample_renamed_to_another_samples_name_is_refused() {
    // Written, it made two samples of one name, which `validate` then found.
    let scratch = Scratch::new("rename-taken");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "name");
    keys(&mut workbench, "\n");
    workbench.key(ctrl('d'));
    keys(&mut workbench, "C2\n");
    assert!(
        !matches!(workbench.mode, Mode::Confirm { .. }),
        "{}",
        workbench.message
    );
    assert!(
        workbench.message.contains("C2 is already a sample's name"),
        "{}",
        workbench.message
    );
    // Its own name, or a new one, is not refused.
    keys(&mut workbench, "\u{1b}");
    move_to(&mut workbench, "name");
    keys(&mut workbench, "\n");
    workbench.key(ctrl('d'));
    keys(&mut workbench, "C9\n");
    assert!(
        matches!(workbench.mode, Mode::Confirm { .. }),
        "{}",
        workbench.message
    );
}

#[test]
fn a_groups_heading_reads_in_its_precision() {
    // `radius = 1.0539999999999998`, where the column showed `1.054`.
    let scratch = Scratch::new("group-precision");
    fs::write(
        scratch.0.join("C2.md"),
        "---\nschema_version: 1\nname: C2\nalloy: chrome\nproperties:\n  \
         diameter: {v: 10.04, u: 0.1, unit: g}\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.group = vec!["diameter".to_string()];
    workbench.refresh().unwrap();
    let headings: Vec<String> = workbench.headings().into_iter().flatten().collect();
    assert!(
        headings.contains(&"diameter = 10.0".to_string()),
        "{headings:?}"
    );
    assert!(
        !headings.iter().any(|heading| heading.contains("10.04")),
        "{headings:?}"
    );
}

#[test]
fn a_folder_made_by_samplekit_is_no_change_outside() {
    // At a project's first opening, SampleKit's own folder was taken for a
    // file changed outside: `read again` was said with nothing changed.
    let scratch = Scratch::new("first-opening");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    fs::create_dir_all(scratch.0.join(".samplekit")).unwrap();
    assert!(!workbench.catch_up(), "{}", workbench.message);
    // A sample added is one.
    fs::copy(scratch.0.join("C1.md"), scratch.0.join("C9.md")).unwrap();
    fs::write(
        scratch.0.join("C9.md"),
        fs::read_to_string(scratch.0.join("C9.md"))
            .unwrap()
            .replace("name: C1", "name: C9"),
    )
    .unwrap();
    assert!(workbench.catch_up());
}

#[test]
fn the_sort_list_offers_no_field_without_an_order() {
    // `state` was offered, then refused once chosen.
    let scratch = Scratch::new("sort-state");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "s");
    let Mode::Picker { items, .. } = &workbench.mode else {
        panic!("the sort list: {}", workbench.message);
    };
    let offered: Vec<&str> = items.iter().map(|(item, _)| item.as_str()).collect();
    assert!(!offered.contains(&"state"), "{offered:?}");
    assert!(!offered.contains(&"tags"), "{offered:?}");
    assert!(offered.contains(&"diameter"), "{offered:?}");
}

#[test]
fn the_basket_shown_alone_and_removed_shows_every_sample_again() {
    // The list was left empty, the basket no longer said.
    let scratch = Scratch::new("basket-removed");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, " ");
    keys(&mut workbench, "b");
    assert!(workbench.basket_only);
    assert_eq!(workbench.view.len(), 1);
    keys(&mut workbench, "D");
    keys(&mut workbench, "y");
    assert!(!workbench.basket_only, "{}", workbench.message);
    assert_eq!(workbench.view.len(), 2, "{}", workbench.message);
    assert!(
        workbench
            .message
            .contains("the basket is empty: every sample is shown"),
        "{}",
        workbench.message
    );
}

#[test]
fn alt_z_redoes_where_the_terminal_sends_ctrl_shift_z_as_ctrl_z() {
    let scratch = Scratch::new("redo");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "/abc");
    workbench.key(ctrl('z'));
    let text = |workbench: &Workbench| {
        let Mode::Filter { text, .. } = &workbench.mode else {
            panic!("filter");
        };
        text.to_string()
    };
    let undone = text(&workbench);
    assert_ne!(undone, "abc");
    workbench.key(alt('z'));
    assert_eq!(text(&workbench), "abc");
    // As a terminal telling the keys apart says Ctrl+Shift+Z: `z`, Shift held.
    workbench.key(ctrl('z'));
    workbench.key(held(
        crossterm::event::KeyCode::Char('z'),
        crossterm::event::KeyModifiers::CONTROL | crossterm::event::KeyModifiers::SHIFT,
    ));
    assert_eq!(text(&workbench), "abc");
}

#[test]
#[cfg(unix)]
fn a_computations_outcome_names_what_it_computed() {
    // Kept in the title over every screen, `✓ 1 computed` read on the
    // collection as said of it.
    let venv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".venv");
    assert!(venv.join("bin/python").is_file(), "the repository's .venv");
    let scratch = Scratch::new("outcome-names");
    std::os::unix::fs::symlink(&venv, scratch.0.join(".venv")).unwrap();
    fs::write(
        scratch.0.join("model.py"),
        "import samplekit as sk\nclass Piece(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        self.radius = sk.Property(value=sk.stats.mean)\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join("beta-bronze.md"),
        "---\nschema_version: 1\nname: beta-bronze\nproperties:\n  radius: {readings: [1.050, 1.052]}\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "beta-bronze");
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
    let outcome = workbench.run.as_ref().and_then(|run| run.outcome.clone());
    assert_eq!(
        outcome.as_deref(),
        Some("computed 1 value in 1 sample"),
        "{}",
        workbench.message
    );
}

#[test]
#[cfg(unix)]
fn c_says_the_edited_values_it_kept() {
    // The command line says a value typed over its formula was kept, and how
    // it is given back; `C` said nothing of it.
    let venv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".venv");
    assert!(venv.join("bin/python").is_file(), "the repository's .venv");
    let scratch = Scratch::new("kept-said");
    let project = scratch.0.join("p");
    fs::create_dir(&project).unwrap();
    std::os::unix::fs::symlink(&venv, project.join(".venv")).unwrap();
    fs::write(
        project.join("model.py"),
        "import samplekit as sk\nclass M(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        self.a = sk.Property()\n        \
         self.b = sk.Property(compute_value=self._b, depends_on=[\"a\"])\n        \
         self.d = sk.Property(compute_value=self._d, depends_on=[\"a\"])\n\n    \
         def _b(self):\n        return 2 * self.a.value\n\n    \
         def _d(self):\n        return 3 * self.a.value\n",
    )
    .unwrap();
    fs::write(
        project.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
    )
    .unwrap();
    let file = project.join("s1.md");
    fs::write(
        &file,
        "---\nschema_version: 1\nname: s1\nproperties:\n  a: 1.0\n---\n",
    )
    .unwrap();
    let outcome_of_c = |workbench: &mut Workbench| {
        keys(workbench, "Cy");
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
        workbench.run.as_ref().and_then(|run| run.outcome.clone())
    };
    let mut workbench = Workbench::open(&project).unwrap();
    assert_eq!(
        outcome_of_c(&mut workbench).as_deref(),
        Some("computed 2 values in 1 sample"),
        "{}",
        workbench.message
    );
    // What it computed waits: written, so the file holds it.
    write_waiting(&mut workbench);
    // `b` typed over by hand, in the file.
    let written = fs::read_to_string(&file).unwrap();
    let b = written.find("  b:").unwrap();
    let edited = format!(
        "{}{}",
        &written[..b],
        written[b..].replacen("v: 2.0", "v: 5.0", 1)
    );
    fs::write(&file, edited).unwrap();
    let mut workbench = Workbench::open(&project).unwrap();
    assert_eq!(
        outcome_of_c(&mut workbench).as_deref(),
        Some(
            "nothing to change — nothing to compute in s1 · 1 value typed over its formula kept — \
             c on it gives it back to its formula"
        ),
        "{}",
        workbench.message
    );
    let written = fs::read_to_string(&file).unwrap();
    assert!(written.contains("v: 5.0"), "{written}");
}

#[test]
#[cfg(unix)]
fn vs_title_counts_cells() {
    // A column outdated in its rows counted one, where `status` counts its
    // cells.
    let venv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".venv");
    assert!(venv.join("bin/python").is_file(), "the repository's .venv");
    let scratch = Scratch::new("v-cells");
    let project = scratch.0.join("p");
    fs::create_dir(&project).unwrap();
    std::os::unix::fs::symlink(&venv, project.join(".venv")).unwrap();
    fs::write(
        project.join("model.py"),
        "import samplekit as sk\nclass M(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        self.k = sk.Property()\n        \
         self.t = sk.Table({\"x\": sk.Property(), \"y\": sk.Property(compute_value=lambda row: \
         row.x.value * self.k.value, depends_on=[\"row.x\", \"k\"])}, index=\"x\")\n",
    )
    .unwrap();
    fs::write(
        project.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
    )
    .unwrap();
    let file = project.join("s1.md");
    fs::write(
        &file,
        "---\nschema_version: 1\nname: s1\nproperties:\n  k: 1.0\ntables:\n  t:\n    index: x\n    \
         columns:\n      x: {}\n    rows:\n      - {x: 1}\n      - {x: 2}\n      - {x: 3}\n---\n",
    )
    .unwrap();
    let computed = std::process::Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["s1.md", "compute", "--write"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        computed.status.success(),
        "{}",
        String::from_utf8_lossy(&computed.stderr)
    );
    let written = fs::read_to_string(&file).unwrap();
    fs::write(
        &file,
        written
            .replacen("k: {v: 1.0", "k: {v: 2.0", 1)
            .replacen("k: 1.0", "k: 2.0", 1),
    )
    .unwrap();
    let mut workbench = Workbench::open(&project).unwrap();
    keys(&mut workbench, "v");
    let drawn = drawn_at(&mut workbench, 100);
    assert!(drawn.contains("⚠ 3 outdated"), "{drawn}");
}

#[test]
#[cfg(unix)]
fn c_previews_what_it_computes() {
    // `C` computed and wrote at once, where `compute` lists the values and
    // asks; the list, counted by cells, and the question.
    let venv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".venv");
    assert!(venv.join("bin/python").is_file(), "the repository's .venv");
    let scratch = Scratch::new("c-preview");
    let project = scratch.0.join("p");
    fs::create_dir(&project).unwrap();
    std::os::unix::fs::symlink(&venv, project.join(".venv")).unwrap();
    fs::write(
        project.join("model.py"),
        "import samplekit as sk\nclass M(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        self.a = sk.Property()\n        \
         self.b = sk.Property(compute_value=self._b, depends_on=[\"a\"])\n        \
         self.c = sk.Property(compute_value=self._c, depends_on=[\"b\"])\n\n    \
         def _b(self):\n        return 2 * self.a.value\n\n    \
         def _c(self):\n        return self.b.value + 1\n",
    )
    .unwrap();
    fs::write(
        project.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
    )
    .unwrap();
    let file = project.join("s1.md");
    fs::write(
        &file,
        "---\nschema_version: 1\nname: s1\nproperties:\n  a: 1.0\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&project).unwrap();
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
    // What it computed waits: written, so the file holds it.
    write_waiting(&mut workbench);
    let written = fs::read_to_string(&file).unwrap();
    fs::write(&file, written.replacen("v: 1.0", "v: 4.0", 1)).unwrap();
    let mut workbench = Workbench::open(&project).unwrap();
    keys(&mut workbench, "C");
    let Mode::Confirm { title, lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert_eq!(title, "compute these 2 values?", "{lines:?}");
    assert!(
        lines.iter().any(|line| line.contains("b  outdated — a")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("2 values in 1 sample would be computed")),
        "{lines:?}"
    );
    // Nothing written until it is answered.
    assert!(fs::read_to_string(&file).unwrap().contains("v: 4.0"));
    keys(&mut workbench, "n");
    assert!(matches!(workbench.mode, Mode::Normal));
    assert!(workbench.running.is_none());
}

#[test]
#[cfg(unix)]
fn v_writes_each_state_whole() {
    // `v` wrote `outdated — b…` where `status` writes `b (itself not
    // current)`.
    let venv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".venv");
    assert!(venv.join("bin/python").is_file(), "the repository's .venv");
    let scratch = Scratch::new("v-whole");
    let project = scratch.0.join("p");
    fs::create_dir(&project).unwrap();
    std::os::unix::fs::symlink(&venv, project.join(".venv")).unwrap();
    fs::write(
        project.join("model.py"),
        "import samplekit as sk\nclass M(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        self.a = sk.Property()\n        \
         self.b = sk.Property(compute_value=self._b, depends_on=[\"a\"])\n        \
         self.c = sk.Property(compute_value=self._c, depends_on=[\"b\"])\n\n    \
         def _b(self):\n        return 2 * self.a.value\n\n    \
         def _c(self):\n        return self.b.value + 1\n",
    )
    .unwrap();
    fs::write(
        project.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
    )
    .unwrap();
    let file = project.join("s1.md");
    fs::write(
        &file,
        "---\nschema_version: 1\nname: s1\nproperties:\n  a: 1.0\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&project).unwrap();
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
    // What it computed waits: written, so the file holds it.
    write_waiting(&mut workbench);
    let written = fs::read_to_string(&file).unwrap();
    fs::write(&file, written.replacen("v: 1.0", "v: 4.0", 1)).unwrap();
    let mut workbench = Workbench::open(&project).unwrap();
    keys(&mut workbench, "v");
    let said: Vec<String> = workbench
        .control
        .iter()
        .map(|item| item.said.clone())
        .collect();
    assert_eq!(
        said,
        ["b  outdated — a", "c  outdated — b (itself not current)"],
        "{}",
        workbench.message
    );
}

#[test]
fn a_computed_value_opens_without_readings() {
    // Readings say what was measured: a value a formula gives has none to
    // enter, and neither its cell nor its card offers them.
    let scratch = rich("computed-window");
    fs::write(
        scratch.0.join("C3.md"),
        "---\nschema_version: 1\nname: C3\nalloy: cobalt\nproperties:\n  \
         diameter: {v: 11.0, u: 0.1, unit: g, fingerprint: '000000000000'}\n  \
         twice: {v: 22.0, computed: {diameter: '111111111111'}, fingerprint: '222222222222'}\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C3");
    go_to(&mut workbench, "twice");
    // Its readings' cell, reached and opened: refused, said.
    keys(&mut workbench, "e");
    workbench.key(Key::Right);
    workbench.key(Key::Right);
    assert!(
        workbench
            .message
            .contains("twice is a formula's: it holds no readings"),
        "{}",
        workbench.message
    );
    workbench.key(Key::F2);
    assert!(matches!(workbench.mode, Mode::Normal), "nothing opened");
    // Its card: what it reads, and no readings.
    keys(&mut workbench, "\u{1b}i");
    let lines = workbench.card_lines();
    assert!(
        !lines.iter().any(|line| line.group == CardGroup::Readings),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.group == CardGroup::Reads && line.field == "diameter"),
        "{lines:?}"
    );
    // A measured one takes them.
    keys(&mut workbench, "\u{1b}");
    go_to(&mut workbench, "diameter");
    keys(&mut workbench, "e");
    workbench.key(Key::Right);
    workbench.key(Key::Right);
    assert!(
        workbench
            .message
            .contains("diameter holds no readings: what you type gives them"),
        "{}",
        workbench.message
    );
    workbench.key(Key::F2);
    assert!(
        matches!(workbench.mode, Mode::Prompt { .. }),
        "{}",
        workbench.message
    );
}

#[test]
fn an_attribute_opens_as_a_list_and_turns_single_and_back() {
    // An attribute's list was one line of text to type again whole, and a
    // list could not be written at all.
    let scratch = rich("attribute-list");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "alloy");
    keys(&mut workbench, "\n");
    let Some((_, items, list)) = workbench.items() else {
        panic!("its window: {}", workbench.message);
    };
    assert_eq!((items, list), (vec!["chrome".to_string()], false));
    // A second item makes a list.
    keys(&mut workbench, "jesuperalloy\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("previewed: {}", workbench.message);
    };
    assert!(
        lines.iter().any(|line| line.contains("becomes a list")),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    // Each change waits, read as written until `w` writes them all.
    let written = read_waiting(&scratch.0.join("more/D1.md"));
    assert!(written.contains("alloy: [chrome, superalloy]"), "{written}");
    // Moved up, then one taken out: a list of one stays a list.
    keys(&mut workbench, "Ky");
    let written = read_waiting(&scratch.0.join("more/D1.md"));
    assert!(written.contains("alloy: [superalloy, chrome]"), "{written}");
    keys(&mut workbench, "jdy");
    let written = read_waiting(&scratch.0.join("more/D1.md"));
    assert!(written.contains("alloy: [superalloy]"), "{written}");
    // `L` turns a list of one into its single value.
    // A list of one turned single says what it was: the list.
    keys(&mut workbench, "L");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines
            .iter()
            .any(|line| line.contains("[superalloy]  →  superalloy")),
        "{lines:?}"
    );
    keys(&mut workbench, "y\u{1b}");
    assert!(matches!(workbench.screen, Screen::Sample { .. }));
    write_waiting(&mut workbench);
    let written = fs::read_to_string(scratch.0.join("more/D1.md")).unwrap();
    assert!(written.contains("alloy: superalloy"), "{written}");
}

#[test]
fn the_tags_are_a_list_and_the_name_one_value() {
    // The tags were a line typed whole, and the name a list's window: the
    // name is the one attribute that is never a list, the tags always one.
    let scratch = rich("tags-list");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "name");
    keys(&mut workbench, "\n");
    assert!(
        matches!(workbench.mode, Mode::Prompt { .. }),
        "{}",
        workbench.message
    );
    keys(&mut workbench, "\u{1b}");
    move_to(&mut workbench, "tags");
    keys(&mut workbench, "\n");
    let Screen::Items { field, .. } = &workbench.screen else {
        panic!("the tags' list");
    };
    assert_eq!(field, "tags");
    // An item added on the empty row, previewed as tags change.
    keys(&mut workbench, "ereference\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert_eq!(lines, &["  tags   +reference"]);
    keys(&mut workbench, "y");
    let written = read_waiting(&scratch.0.join("more/D1.md"));
    assert!(written.contains("tags: [reference]"), "{written}");
    // A list they stay.
    keys(&mut workbench, "L");
    assert_eq!(workbench.message, "tags are a list, and stay one");
    keys(&mut workbench, "\u{1b}");
    assert!(matches!(workbench.screen, Screen::Sample { .. }));
    write_waiting(&mut workbench);
    let written = fs::read_to_string(scratch.0.join("more/D1.md")).unwrap();
    assert!(written.contains("tags: [reference]"), "{written}");
}

#[test]
fn p_from_a_root_offers_the_selections_projects() {
    // At a root above several projects, `p` offered the root's figures and
    // exports alone, whatever sample was open.
    let scratch = Scratch::new("produce-root");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[collection]\nrecursive = true\n",
    )
    .unwrap();
    fs::create_dir_all(scratch.0.join("p")).unwrap();
    fs::write(
        scratch.0.join("p/.samplekitrc"),
        "schema_version = 1\nimport = \"..\"\n[figure.fp]\nx = \"diameter\"\ny = \"diameter\"\n\
         [profile.pp]\ncolumns = [{field = \"diameter\"}]\n\
         [export.ep]\nprofile = \"pp\"\nformat = \"csv\"\noutput = \"out/ep.csv\"\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join("p/a.md"),
        "---\nschema_version: 1\nname: a\nproperties:\n  diameter: 1.0\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    // `a` alone in the basket: `p`'s project.
    workbench.cursor = workbench
        .view
        .iter()
        .position(|entry| entry.sample.borrow().name() == Some("a"))
        .unwrap();
    keys(&mut workbench, " p");
    let Mode::Picker { items, .. } = &workbench.mode else {
        panic!("the picker: {}", workbench.message);
    };
    let offered: Vec<&String> = items.iter().map(|(item, _)| item).collect();
    assert!(
        offered.iter().any(|item| item.contains("fp")),
        "{offered:?}"
    );
    assert!(
        offered.iter().any(|item| item.contains("ep")),
        "{offered:?}"
    );
    // Every sample shown, the root's own and `p`'s, of different models:
    // the root's alone, said.
    workbench.key(Key::Esc);
    keys(&mut workbench, "x");
    keys(&mut workbench, "p");
    let Mode::Picker { items, .. } = &workbench.mode else {
        panic!("the picker: {}", workbench.message);
    };
    assert!(
        !items.iter().any(|(item, _)| item.contains("fp")),
        "{items:?}"
    );
    assert!(
        workbench
            .message
            .contains("several projects of different models"),
        "{}",
        workbench.message
    );
}

#[test]
fn p_in_a_sample_open_offers_the_whole_menu_grouped() {
    let scratch = Scratch::new("produce-sample");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[figure.fp]\nx = \"diameter\"\ny = \"diameter\"\n\
         [profile.pp]\ncolumns = [{field = \"diameter\"}]\n\
         [export.ep]\nprofile = \"pp\"\nformat = \"csv\"\noutput = \"out/ep.csv\"\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join("a.md"),
        "---\nschema_version: 1\nname: a\nproperties:\n  diameter: 1.0\n---\n",
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.key(Key::Enter);
    assert!(matches!(workbench.screen, Screen::Sample { .. }));
    keys(&mut workbench, "p");
    let Mode::Picker { items, cursor, .. } = &workbench.mode else {
        panic!("the menu: {}", workbench.message);
    };
    let offered: Vec<&str> = items.iter().map(|(item, _)| item.as_str()).collect();
    let offered_len = offered.len();
    let figures = offered.iter().position(|item| *item == "figures");
    let exports = offered.iter().position(|item| *item == "exports");
    assert!(figures.is_some() && exports.is_some(), "{offered:?}");
    assert!(
        offered[figures.unwrap()..exports.unwrap()]
            .iter()
            .any(|item| item.starts_with("  fp")),
        "{offered:?}"
    );
    assert!(
        offered[exports.unwrap()..]
            .iter()
            .any(|item| item.starts_with("  ep")),
        "{offered:?}"
    );
    // The cursor on the first line under the first heading; `Tab` to the
    // first line of the next group, and a heading chosen does nothing.
    assert_eq!(*cursor, figures.unwrap() + 1);
    workbench.key(Key::Tab);
    let Mode::Picker { cursor, .. } = &workbench.mode else {
        panic!("the menu: {}", workbench.message);
    };
    assert_eq!(*cursor, exports.unwrap() + 1);
    // `Up` from a group's first line goes to the line above its heading.
    workbench.key(Key::Up);
    let Mode::Picker { cursor, .. } = &mut workbench.mode else {
        panic!("the menu: {}", workbench.message);
    };
    assert_eq!(*cursor, exports.unwrap() - 1);
    // A heading chosen runs nothing, and leaves the menu whole.
    *cursor = figures.unwrap();
    workbench.key(Key::Enter);
    let Mode::Picker { items, .. } = &workbench.mode else {
        panic!("the menu: {}", workbench.message);
    };
    assert_eq!(items.len(), offered_len);
}

#[test]
fn p_writes_what_is_shown_to_a_file_one_names() {
    let scratch = Scratch::new("produce-shown");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.sort = vec!["-diameter".to_string()];
    workbench.refresh().unwrap();
    // The exports' first line, `Tab` past the figures.
    keys(&mut workbench, "p");
    workbench.key(Key::Tab);
    let Mode::Picker { items, cursor, .. } = &workbench.mode else {
        panic!("the menu: {}", workbench.message);
    };
    assert!(items[*cursor].0.starts_with("  what is shown"), "{items:?}");
    // A file whose extension says no format is refused, naming those that do.
    workbench.key(Key::Enter);
    for _ in 0.."out/shown.csv".len() {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "out/shown.doc\n");
    assert!(
        workbench.message.contains(".csv, .tsv, .json"),
        "{}",
        workbench.message
    );
    for _ in 0.."out/shown.doc".len() {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "out/shown.csv\n");
    let Mode::Confirm { title, lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        title.replace('\\', "/").ends_with("out/shown.csv"),
        "{title}"
    );
    assert!(lines[0].starts_with("3 rows"), "{lines:?}");
    keys(&mut workbench, "y");
    let written = fs::read_to_string(scratch.0.join("out/shown.csv")).unwrap();
    // The columns shown, in the order shown: the most diameter first.
    let mut rows = written.lines();
    let header: Vec<&str> = rows.next().unwrap().split(',').collect();
    let diameter = header
        .iter()
        .position(|column| column.starts_with("diameter_value"));
    let diameter = diameter.unwrap_or_else(|| panic!("{written}"));
    let diameters: Vec<f64> = rows
        .map(|row| row.split(',').nth(diameter).unwrap().parse().unwrap())
        .collect();
    assert!(
        diameters.windows(2).all(|pair| pair[0] >= pair[1]),
        "{written}"
    );
}

#[test]
fn p_over_several_projects_writes_each_its_declared_file() {
    // From a root, a declared export wrote one file of every project's
    // samples, at the first project's output.
    let scratch = Scratch::new("produce-projects");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[collection]\nrecursive = true\n\
         [profile.pp]\ncolumns = [{field = \"name\"}, {field = \"diameter\"}]\n\
         [export.e]\nprofile = \"pp\"\nformat = \"csv\"\noutput = \"out/e.csv\"\n",
    )
    .unwrap();
    // `p` and `q` import the root's export; `r` does not.
    for (project, sample, imports) in [("p", "a", true), ("q", "b", true), ("r", "c", false)] {
        fs::create_dir_all(scratch.0.join(project)).unwrap();
        let import = if imports { "import = \"..\"\n" } else { "" };
        fs::write(
            scratch.0.join(project).join(".samplekitrc"),
            format!("schema_version = 1\n{import}"),
        )
        .unwrap();
        fs::write(
            scratch.0.join(project).join(format!("{sample}.md")),
            format!("---\nschema_version: 1\nname: {sample}\nproperties:\n  diameter: 1.0\n---\n"),
        )
        .unwrap();
    }
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let choose = |workbench: &mut Workbench, line: &str| {
        keys(workbench, "p");
        let Mode::Picker { items, cursor, .. } = &mut workbench.mode else {
            panic!("the menu: {}", workbench.message);
        };
        let at = items.iter().position(|(item, _)| item.starts_with(line));
        *cursor = at.unwrap_or_else(|| panic!("{line} in {items:?}"));
        workbench.key(Key::Enter);
    };
    // One file for two projects is refused before either is written.
    choose(&mut workbench, "  e ");
    assert!(
        workbench
            .message
            .replace('\\', "/")
            .contains("would write out/e.csv for p and for q"),
        "{}",
        workbench.message
    );
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[collection]\nrecursive = true\n\
         [profile.pp]\ncolumns = [{field = \"name\"}, {field = \"diameter\"}]\n\
         [export.e]\nprofile = \"pp\"\nformat = \"csv\"\n\
         output = \"out/{project}/e.csv\"\n",
    )
    .unwrap();
    workbench.catch_up();
    workbench.mode = Mode::Normal;
    choose(&mut workbench, "  e ");
    let Mode::Confirm { title, lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    // The root's own samples, `p`'s and `q`'s: three files.
    assert_eq!(title, "export e · 3 files", "{lines:?}");
    assert!(
        lines
            .iter()
            .map(|line| line.replace('\\', "/"))
            .any(|line| line.starts_with("  out/p/e.csv · 1 row · p")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .map(|line| line.replace('\\', "/"))
            .any(|line| line == "1 sample not exported: r declares no export 'e'"),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    let p = fs::read_to_string(scratch.0.join("out/p/e.csv")).unwrap();
    let q = fs::read_to_string(scratch.0.join("out/q/e.csv")).unwrap();
    assert!(p.contains("a,") && !p.contains("b,"), "{p}");
    assert!(q.contains("b,") && !q.contains("a,"), "{q}");
    // What is shown, over four projects: no file offered, none refused.
    choose(&mut workbench, "  what is shown");
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("a prompt: {}", workbench.message);
    };
    assert_eq!(text.text(), "");
    workbench.key(Key::Enter);
    assert!(
        workbench.message.contains("spans 4 projects"),
        "{}",
        workbench.message
    );
    keys(&mut workbench, "all.csv\ny");
    let all = fs::read_to_string(scratch.0.join("all.csv")).unwrap();
    // Its header, and the six samples shown.
    assert_eq!(all.lines().count(), 7, "{all}");
}

#[test]
fn what_is_shown_over_a_declared_profile_is_made_again_by_it() {
    // A column with a header, or a label holding a comma, is no `-c`: the
    // file was made again by nothing. Shown as the project declares it, the
    // profile is what the command names.
    let scratch = Scratch::new("shown-profile-again");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[profile.paper]\n\
         columns = [{field = \"name\", header = \"Piece\"}, \
         {field = \"diameter\", label = \"Diameter, dry\", precision = \".2f\"}]\n",
    )
    .unwrap();
    for (name, diameter) in [("C1", 12.0), ("C2", 10.0)] {
        fs::write(
            scratch.0.join(format!("{name}.md")),
            format!(
                "---\nschema_version: 1\nname: {name}\nproperties:\n  \
                 diameter: {{v: {diameter:?}, u: 0.1, unit: g}}\n---\n"
            ),
        )
        .unwrap();
    }
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.profile = workbench
        .collection
        .config()
        .unwrap()
        .profile("paper")
        .unwrap()
        .clone();
    workbench.sort = vec!["-diameter".to_string()];
    workbench.refresh().unwrap();
    keys(&mut workbench, "p\t\n");
    for _ in 0.."out/shown.csv".len() {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "out/paper.csv\n");
    let Mode::Confirm { pending, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    let samplekit::tui::model::Pending::Export { files, .. } = pending.as_ref() else {
        panic!("an export");
    };
    let said = files[0].said.clone();
    assert!(said.contains("--profile paper"), "{said}");
    keys(&mut workbench, "y");
    let written = fs::read(scratch.0.join("out/paper.csv")).unwrap();
    assert!(String::from_utf8_lossy(&written).starts_with("Piece,"));
    let words = shell_words::split(&said).unwrap();
    let again = std::process::Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(&words[1..])
        .output()
        .unwrap();
    assert!(
        again.status.success(),
        "{said}\n{}",
        String::from_utf8_lossy(&again.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&fs::read(scratch.0.join("out/paper.csv")).unwrap()),
        String::from_utf8_lossy(&written),
        "{said}"
    );
}

#[test]
fn what_is_shown_is_made_again_by_the_command_it_records() {
    // Written from the workbench, it was made again by nothing: its record
    // named the workbench's action, which no command runs.
    let scratch = Scratch::new("shown-again");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[profile.p]\n\
         columns = [{field = \"name\"}, {field = \"diameter\", precision = \".2f\"}, {field = \"carbon\"}]\n",
    )
    .unwrap();
    for (name, diameter, alloy) in [
        ("C1", 12.0, "cobalt"),
        ("C2", 10.0, "chrome"),
        ("C3", 11.0, "cobalt"),
    ] {
        fs::write(
            scratch.0.join(format!("{name}.md")),
            format!(
                "---\nschema_version: 1\nname: {name}\nalloy: {alloy}\nproperties:\n  \
                 diameter: {{v: {diameter:?}, u: 0.1, unit: g}}\n  carbon: {{v: 3.0, unit: g}}\n---\n"
            ),
        )
        .unwrap();
    }
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.profile = workbench
        .collection
        .config()
        .unwrap()
        .profile("p")
        .unwrap()
        .clone();
    workbench.sort = vec!["-diameter".to_string()];
    workbench.group = vec!["alloy".to_string()];
    workbench.refresh().unwrap();
    for file in ["out/shown.csv", "out/shown.tex"] {
        keys(&mut workbench, "p\t\n");
        for _ in 0.."out/shown.csv".len() {
            workbench.key(Key::Backspace);
        }
        keys(&mut workbench, &format!("{file}\n"));
        let Mode::Confirm { lines, pending, .. } = &workbench.mode else {
            panic!("a preview: {}", workbench.message);
        };
        let samplekit::tui::model::Pending::Export { files, .. } = pending.as_ref() else {
            panic!("an export");
        };
        let said = files[0].said.clone();
        assert!(said.starts_with("samplekit "), "{said}");
        // The column no sample fills is left out, and said.
        if file.ends_with(".csv") {
            assert!(
                lines
                    .iter()
                    .any(|line| line.starts_with("carbon_uncertainty")
                        && line.ends_with("is left out: it is empty for every sample")),
                "{lines:?}"
            );
        }
        keys(&mut workbench, "y");
        let written = fs::read(scratch.0.join(file)).unwrap();
        // The groups marked in the document, `\midrule` between two.
        if file.ends_with(".tex") {
            let text = String::from_utf8_lossy(&written);
            assert_eq!(text.matches("\\midrule").count(), 2, "{text}");
        }
        // The command it records writes the same file, byte for byte.
        let words = shell_words::split(&said).unwrap();
        let again = std::process::Command::new(env!("CARGO_BIN_EXE_samplekit"))
            .args(&words[1..])
            .output()
            .unwrap();
        assert!(
            again.status.success(),
            "{said}\n{}",
            String::from_utf8_lossy(&again.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&fs::read(scratch.0.join(file)).unwrap()),
            String::from_utf8_lossy(&written),
            "{said}"
        );
    }
}

#[test]
#[cfg(unix)]
fn a_figure_no_window_can_show_asks_its_file() {
    // Without a display, a figure of one's own reached no file undeclared:
    // the advice named `w`, which declares it.
    let scratch = Scratch::new("figure-no-window");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    workbench.drawing = Some(samplekit::tui::model::Running {
        receiver,
        before: None,
    });
    workbench.windowed = Some(samplekit::tui::model::Output::Own);
    sender
        .send(samplekit::tui::model::Progress::Done(
            "matplotlib has no interactive backend here (it chose 'agg'), so a window would \
             show nothing: install tkinter or PyQt, or write the figure to a file (the \
             workbench asks its file next)"
                .to_string(),
        ))
        .unwrap();
    workbench.tick();
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("its file asked: {}", workbench.message);
    };
    assert_eq!(text.text(), "figure.pdf");
    assert!(
        workbench.message.ends_with("its file is asked"),
        "{}",
        workbench.message
    );
    // A drawing that ends otherwise says what it said, and asks nothing.
    let (sender, receiver) = std::sync::mpsc::channel();
    workbench.mode = Mode::Normal;
    workbench.drawing = Some(samplekit::tui::model::Running {
        receiver,
        before: None,
    });
    workbench.windowed = Some(samplekit::tui::model::Output::Own);
    sender
        .send(samplekit::tui::model::Progress::Done(
            "'diameter against alloy' closed".to_string(),
        ))
        .unwrap();
    workbench.tick();
    assert!(matches!(workbench.mode, Mode::Normal));
    assert_eq!(workbench.message, "'diameter against alloy' closed");
}

#[test]
#[cfg(unix)]
fn an_export_written_from_values_never_computed_says_so() {
    // Written empty, they were said by the command line alone: the
    // workbench's preview said nothing of them.
    let venv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".venv");
    assert!(venv.join("bin/python").is_file(), "the repository's .venv");
    let scratch = Scratch::new("export-never-computed");
    std::os::unix::fs::symlink(&venv, scratch.0.join(".venv")).unwrap();
    fs::write(
        scratch.0.join("model.py"),
        "import samplekit as sk\nclass Piece(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        \
         self.ore = sk.Property(unit=\"kg\")\n        \
         self.double = sk.Property(unit=\"kg\", compute_value=lambda: self.ore.value * 2)\n        \
         self.set_dependencies(\"double\", depends_on=[\"ore\"])\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n\
         [profile.p]\ncolumns = [{field = \"name\"}, {field = \"double\"}]\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join("G.md"),
        "---\nschema_version: 1\nname: G\nproperties:\n  ore: {v: 4.6, unit: kg}\n---\n",
    )
    .unwrap();
    let config = samplekit::config::project_config::load_for(&scratch.0)
        .unwrap()
        .unwrap();
    samplekit::config::model_runtime::describe(&config, &scratch.0).unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.profile = workbench
        .collection
        .config()
        .unwrap()
        .profile("p")
        .unwrap()
        .clone();
    workbench.refresh().unwrap();
    keys(&mut workbench, "p\t\n");
    for _ in 0.."out/shown.csv".len() {
        workbench.key(Key::Backspace);
    }
    keys(&mut workbench, "out/g.csv\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(
        lines
            .iter()
            .any(|line| line.contains("1 never computed, written empty")),
        "{lines:?}"
    );
}

#[test]
#[cfg(unix)]
fn a_declared_export_from_a_failed_value_says_so() {
    // The preview of a declared export counted outdated, edited and never
    // computed values, and left a failed one out: written with no word.
    let venv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".venv");
    assert!(venv.join("bin/python").is_file(), "the repository's .venv");
    let scratch = Scratch::new("export-failed");
    let project = scratch.0.join("p");
    fs::create_dir(&project).unwrap();
    std::os::unix::fs::symlink(&venv, project.join(".venv")).unwrap();
    fs::write(
        project.join("model.py"),
        "import samplekit as sk\nclass Piece(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        \
         self.ore = sk.Property(unit=\"kg\")\n        \
         self.double = sk.Property(unit=\"kg\", compute_value=self._double, depends_on=[\"ore\"])\n\n    \
         def _double(self):\n        raise ValueError(\"no\")\n",
    )
    .unwrap();
    fs::write(
        project.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n\
         [profile.p]\ncolumns = [{field = \"name\"}, {field = \"double\"}]\n\
         [export.e]\nprofile = \"p\"\nformat = \"csv\"\noutput = \"out/e.csv\"\n",
    )
    .unwrap();
    fs::write(
        project.join("G.md"),
        "---\nschema_version: 1\nname: G\nproperties:\n  ore: {v: 4.6, unit: kg}\n---\n",
    )
    .unwrap();
    let computed = std::process::Command::new(env!("CARGO_BIN_EXE_samplekit"))
        .args(["G.md", "compute", "--write"])
        .current_dir(&project)
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&computed.stdout).contains("failed")
            || String::from_utf8_lossy(&computed.stderr).contains("failed"),
        "{}",
        String::from_utf8_lossy(&computed.stderr)
    );
    let mut workbench = Workbench::open(&project).unwrap();
    keys(&mut workbench, "p\tj");
    let menu = drawn_at(&mut workbench, 120);
    keys(&mut workbench, "\n");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}\n{menu}", workbench.message);
    };
    assert!(
        lines.iter().any(|line| line.contains("1 failed")),
        "{lines:?}"
    );
}

#[test]
fn a_first_profile_without_the_name_opens_with_it() {
    // A profile made for a figure's export, its quantity alone, opened the
    // project on a column of numbers no row named.
    let scratch = Scratch::new("first-profile-name");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[profile.figure]\ncolumns = [{field = \"diameter\"}]\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join("A.md"),
        "---\nschema_version: 1\nname: A\nproperties:\n  diameter: {v: 1.0, unit: g}\n---\n",
    )
    .unwrap();
    let workbench = Workbench::open(&scratch.0).unwrap();
    let fields: Vec<&str> = workbench
        .profile
        .columns()
        .iter()
        .map(|column| column.field.as_str())
        .collect();
    assert_eq!(fields, ["name", "diameter"]);
}

#[test]
fn a_note_changed_in_its_file_meanwhile_is_not_written_over_unseen() {
    // The preview compared the note with the one opened, and the write went
    // from the file read again: a line another hand added was dropped unseen.
    let scratch = rich("note-meanwhile");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    keys(&mut workbench, "N");
    workbench.key(Key::End);
    keys(&mut workbench, " Mine.");
    let file = scratch.0.join("more/D1.md");
    let held = fs::read_to_string(&file).unwrap();
    fs::write(&file, format!("{held}Theirs.\n")).unwrap();
    keys(&mut workbench, "\u{1b}");
    assert!(
        matches!(workbench.mode, Mode::Note { .. }),
        "{}",
        workbench.message
    );
    assert!(
        workbench.message.contains("changed in the file"),
        "{}",
        workbench.message
    );
    keys(&mut workbench, "\u{1b}");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("a preview: {}", workbench.message);
    };
    assert!(lines.contains(&"- Theirs.".to_string()), "{lines:?}");
}

#[test]
fn yes_during_a_computation_keeps_the_question() {
    // The change previewed, a note typed whole among them, was dropped by a
    // `y` while a computation ran, said *write this once it has finished*.
    use samplekit::tui::model::Running;
    let scratch = rich("yes-while-running");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    keys(&mut workbench, "N");
    workbench.key(Key::End);
    keys(&mut workbench, " Kept.");
    keys(&mut workbench, "\u{1b}");
    assert!(matches!(workbench.mode, Mode::Confirm { .. }));
    let (_sender, receiver) = std::sync::mpsc::channel();
    workbench.running = Some(Running {
        receiver,
        before: None,
    });
    keys(&mut workbench, "y");
    assert!(
        matches!(workbench.mode, Mode::Confirm { .. }),
        "{}",
        workbench.message
    );
    assert!(
        workbench.message.contains("y again"),
        "{}",
        workbench.message
    );
}

#[test]
fn ctrl_c_asks_where_the_configuration_is_not_written() {
    // `q` asked, and Ctrl+C left with the configuration's changes lost.
    let scratch = rich("ctrl-c-configuration");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "P");
    workbench.workspace.as_mut().unwrap().dirty = true;
    assert!(!workbench.interrupt());
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("asked: {}", workbench.message);
    };
    assert!(
        lines[0].contains("configuration's changes are not written"),
        "{lines:?}"
    );
    // A second Ctrl+C quits.
    assert!(workbench.interrupt());
}

#[test]
fn every_remembered_setting_set_aside_is_said_and_the_others_kept() {
    // A remembered column set aside cleared the filter too, said as the
    // filter's; a sort and a grouping went unsaid behind a filter refused.
    let scratch = rich("set-asides");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    workbench.filter = "diameter > 1".to_string();
    workbench.sort = vec!["nosuchsort".to_string()];
    workbench.group = vec!["nosuchgroup".to_string()];
    let said = workbench.refresh().unwrap_err();
    assert!(said.contains("the sort was set aside"), "{said}");
    assert!(said.contains("the grouping was set aside"), "{said}");
    assert_eq!(workbench.filter, "diameter > 1");
}

#[test]
fn capital_a_adds_an_attribute_its_name_then_its_value() {
    // A sample made empty could not be given an attribute in the workbench:
    // `e` changes only what the sample holds.
    let scratch = Scratch::new("add-attribute");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    keys(&mut workbench, "Aalloy\n");
    assert!(
        workbench
            .message
            .contains("alloy is held already: e changes it"),
        "{}",
        workbench.message
    );
    workbench.key(Key::Esc);
    keys(&mut workbench, "Astyle\ntin\ny");
    write_waiting(&mut workbench);
    let written = fs::read_to_string(scratch.0.join("C1.md")).unwrap();
    assert!(written.contains("style: tin\n"), "{written}");
}

#[test]
fn a_value_opens_selected_and_what_is_typed_replaces_it() {
    // Opened with the cursor at its end, `66` typed over `64` gave `6466`:
    // in the typing mode, what is typed replaces the cell.
    let scratch = Scratch::new("value-selected");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "alloy");
    keys(&mut workbench, "e66");
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("its value typed: {}", workbench.message);
    };
    assert_eq!(text.text(), "66");
    keys(&mut workbench, "\n");
    let written = read_waiting(&scratch.0.join("C1.md"));
    assert!(written.contains("alloy: 66\n"), "{written}");
    // `F2` keeps it, to edit, the cursor at its end.
    go_to(&mut workbench, "alloy");
    workbench.key(Key::F2);
    keys(&mut workbench, "7\n\u{1b}");
    write_waiting(&mut workbench);
    let written = fs::read_to_string(scratch.0.join("C1.md")).unwrap();
    assert!(written.contains("alloy: 667\n"), "{written}");
}

#[test]
fn a_folder_made_a_project_brings_its_history_as_the_workbench_opens() {
    // The workbench asked, and a first write there ended the offer: brought
    // without asking, and said.
    use samplekit::config::{project_config, version_control};
    let scratch = Scratch::new("bring-history");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[collection]\nrecursive = true\n",
    )
    .unwrap();
    fs::create_dir_all(scratch.0.join("sub")).unwrap();
    let root = project_config::load(&scratch.0.join(".samplekitrc")).unwrap();
    for diameter in ["1.0", "2.0"] {
        fs::write(
            scratch.0.join("sub/s.md"),
            format!("---\nschema_version: 1\nname: s\nproperties:\n  diameter: {diameter}\n---\n"),
        )
        .unwrap();
        version_control::snapshot(&root, "a change").unwrap();
    }
    fs::write(scratch.0.join("sub/.samplekitrc"), "schema_version = 1\n").unwrap();
    let workbench = Workbench::open(&scratch.0).unwrap();
    assert!(
        workbench.message.contains("snapshots of")
            && workbench.message.contains("brought from the history of"),
        "{}",
        workbench.message
    );
    let sub = project_config::load(&scratch.0.join("sub/.samplekitrc")).unwrap();
    assert!(
        version_control::entries(&sub)
            .unwrap()
            .iter()
            .any(|entry| entry.message == "a change"),
    );
    // The folder kept as made a project is no change to undo: `u` took back
    // the `.samplekitrc` the researcher wrote. The last change brought is.
    let undoable = version_control::last_undoable(&sub).unwrap().unwrap();
    assert_eq!(undoable.message, "a change");
}

#[test]
fn p_offers_a_figures_export_and_asks_before_replacing_its_file() {
    let scratch = Scratch::new("produce-figure-export");
    fs::write(
        scratch.0.join(".samplekitrc"),
        "schema_version = 1\n[figure.fp]\nx = \"diameter\"\ny = \"diameter\"\n\
         [export.ef]\nfigure = \"fp\"\noutput = \"out/ef.png\"\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join("a.md"),
        "---\nschema_version: 1\nname: a\nproperties:\n  diameter: 1.0\n---\n",
    )
    .unwrap();
    fs::create_dir_all(scratch.0.join("out")).unwrap();
    fs::write(scratch.0.join("out/ef.png"), "before").unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "p");
    let Mode::Picker { items, cursor, .. } = &mut workbench.mode else {
        panic!("the menu: {}", workbench.message);
    };
    let line = items
        .iter()
        .position(|(item, _)| item.starts_with("  ef"))
        .unwrap_or_else(|| panic!("{items:?}"));
    assert!(items[line].0.contains("figure fp"), "{:?}", items[line]);
    *cursor = line;
    workbench.key(Key::Enter);
    let Mode::Confirm { title, lines, .. } = &workbench.mode else {
        panic!("asked before replacing: {}", workbench.message);
    };
    assert_eq!(title, "replace a figure's file");
    assert!(lines[0].contains("y draws 'fp' over it"), "{lines:?}");
    workbench.key(Key::Char('n'));
    assert_eq!(
        fs::read_to_string(scratch.0.join("out/ef.png")).unwrap(),
        "before"
    );
}

#[test]
fn enter_on_a_link_opens_its_sample_or_its_address() {
    let scratch = Scratch::new("link");
    let file = scratch.0.join("C1.md");
    let text = fs::read_to_string(&file).unwrap().replace(
        "alloy: cobalt\n",
        "alloy: cobalt\nreference: \"[[C3]]\"\nsupplier: https://example.org/c1\nmissing: \"[[C9]]\"\n",
    );
    fs::write(&file, text).unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "\n");
    move_to(&mut workbench, "reference");
    keys(&mut workbench, "\n");
    let Screen::Sample { at, .. } = workbench.screen else {
        panic!("a sample's screen");
    };
    assert_eq!(
        workbench
            .view
            .get(at)
            .and_then(|entry| entry.path.clone())
            .and_then(|path| path.file_name().map(|name| name.to_os_string())),
        Some(std::ffi::OsString::from("C3.md")),
        "{}",
        workbench.message
    );
    assert_eq!(workbench.message, "[[C3]] → C3.md — Esc goes back to C1");
    // The title bar says the way taken; Esc goes back to C1, on its link.
    assert!(
        screen(&mut workbench).contains("C1 › C3"),
        "{}",
        screen(&mut workbench)
    );
    keys(&mut workbench, "\u{1b}");
    let Screen::Sample { at, cursor } = workbench.screen else {
        panic!("C1's screen");
    };
    assert_eq!(
        workbench
            .view
            .get(at)
            .and_then(|entry| entry.path.clone())
            .and_then(|path| path.file_name().map(|name| name.to_os_string())),
        Some(std::ffi::OsString::from("C1.md"))
    );
    assert_eq!(workbench.entries()[cursor].field, "reference");
    assert!(workbench.trail.is_empty());
    // On C1 again: an address opens in the browser, by `o` as by Enter.
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "\n");
    move_to(&mut workbench, "supplier");
    let effects = keys(&mut workbench, "o");
    assert!(
        matches!(effects.as_slice(), [Effect::OpenAddress(address)] if address == "https://example.org/c1"),
        "{}",
        workbench.message
    );
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "\n");
    move_to(&mut workbench, "missing");
    keys(&mut workbench, "\n");
    assert_eq!(
        workbench.message,
        "[[C9]] leads to no sample: no file of the project is named C9, and no sample's name is"
    );
}

#[test]
fn a_link_to_a_sample_the_view_leaves_out_shows_it_until_esc() {
    let scratch = Scratch::new("link-outside");
    let other = scratch.0.join("other");
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join(".samplekitrc"), "schema_version = 1\n").unwrap();
    fs::write(
        other.join("X1.md"),
        "---\nschema_version: 1\nname: X1\nalloy: nickel\n---\n",
    )
    .unwrap();
    let file = scratch.0.join("C1.md");
    let text = fs::read_to_string(&file).unwrap().replace(
        "alloy: cobalt\n",
        "alloy: cobalt\nreference: \"[[C2]]\"\nelsewhere: \"[x](other/X1.md)\"\n",
    );
    fs::write(&file, text).unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    // C2 is chrome: the filter leaves it out.
    keys(&mut workbench, "/alloy == cobalt\n");
    assert_eq!(workbench.view.len(), 2, "{}", workbench.message);
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "reference");
    keys(&mut workbench, "\n");
    assert_eq!(
        workbench.message,
        "[[C2]] → C2.md, not among those shown — Esc goes back to C1"
    );
    assert_eq!(
        workbench
            .current()
            .map(|(_, sample)| sample.name().map(str::to_string)),
        Some(Some("C2".to_string()))
    );
    assert!(
        screen(&mut workbench).contains("C1 › C2"),
        "{}",
        screen(&mut workbench)
    );
    // Read again, as after a write: still shown, as the file now says.
    let twin = scratch.0.join("C2.md");
    fs::write(
        &twin,
        fs::read_to_string(&twin).unwrap().replace("chrome", "iron"),
    )
    .unwrap();
    workbench.reload();
    assert!(
        screen(&mut workbench).contains("iron"),
        "{}",
        screen(&mut workbench)
    );
    keys(&mut workbench, "\u{1b}");
    assert_eq!(workbench.view.len(), 2);
    assert_eq!(
        workbench
            .current()
            .map(|(_, sample)| sample.name().map(str::to_string)),
        Some(Some("C1".to_string()))
    );
    // Another project's sample, which the collection does not read, too.
    if let Screen::Sample { cursor, .. } = &mut workbench.screen {
        *cursor = 0;
    }
    move_to(&mut workbench, "elsewhere");
    keys(&mut workbench, "\n");
    assert!(
        workbench
            .message
            .contains("not among those shown — Esc goes back to C1"),
        "{}",
        workbench.message
    );
    assert_eq!(
        workbench
            .current()
            .map(|(_, sample)| sample.name().map(str::to_string)),
        Some(Some("X1".to_string()))
    );
    keys(&mut workbench, "\u{1b}\u{1b}");
    assert!(matches!(workbench.screen, Screen::Collection));
    assert_eq!(workbench.view.len(), 2);
    assert!(workbench.trail.is_empty());
}

#[test]
fn a_sample_lists_the_samples_linking_to_it_and_follows_them() {
    let scratch = Scratch::new("backlinks");
    let file = scratch.0.join("C1.md");
    let text = fs::read_to_string(&file).unwrap().replace(
        "alloy: cobalt\n",
        "alloy: cobalt\nreference: \"[[C3]]\"\nparents: [\"[c](C3.md)\", \"[[C2]]\"]\n",
    );
    fs::write(&file, text).unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C3");
    let entries = workbench.entries();
    let last = entries.last().unwrap();
    assert_eq!(last.field, "← C1");
    assert_eq!(last.shown, "reference, parents");
    assert_eq!(last.state, "linked from");
    // Followed as a link is, the way kept.
    move_to(&mut workbench, "← C1");
    keys(&mut workbench, "\n");
    assert_eq!(
        workbench
            .current()
            .map(|(_, sample)| sample.name().map(str::to_string)),
        Some(Some("C1".to_string()))
    );
    assert!(
        screen(&mut workbench).contains("C3 › C1"),
        "{}",
        screen(&mut workbench)
    );
    keys(&mut workbench, "\u{1b}");
    assert_eq!(
        workbench
            .current()
            .map(|(_, sample)| sample.name().map(str::to_string)),
        Some(Some("C3".to_string()))
    );
    // C2 is linked from C1 too; C1 from nothing.
    keys(&mut workbench, "\u{1b}");
    open_named(&mut workbench, "C1");
    assert!(
        !workbench
            .entries()
            .iter()
            .any(|entry| entry.state == "linked from")
    );
}

#[test]
fn a_link_is_shown_as_markdown_shows_it_and_typed_as_written() {
    let scratch = Scratch::new("link-shown");
    let file = scratch.0.join("C1.md");
    let text = fs::read_to_string(&file).unwrap().replace(
        "alloy: cobalt\n",
        "alloy: cobalt\nreference: \"[[C3]]\"\ntwin: \"[the twin](C2.md)\"\n\
         page: https://example.org/c1\nlost: \"[[C9]]\"\nparents: [\"[[C2]]\", \"[[C3|third]]\"]\n",
    );
    fs::write(&file, text).unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "\n");
    let shown = |field: &str| {
        workbench
            .entries()
            .into_iter()
            .find(|entry| entry.field == field)
            .map(|entry| (entry.shown, entry.link))
            .unwrap()
    };
    use samplekit::presentation::links::Leads;
    assert_eq!(shown("reference"), ("C3".to_string(), Some(Leads::Inside)));
    assert_eq!(shown("twin"), ("the twin".to_string(), Some(Leads::Inside)));
    assert_eq!(
        shown("page"),
        ("https://example.org/c1 ↗".to_string(), Some(Leads::Web))
    );
    assert_eq!(shown("lost"), ("C9 ✗".to_string(), Some(Leads::Nowhere)));
    assert_eq!(
        shown("parents"),
        ("C2, third".to_string(), Some(Leads::Inside))
    );
    // Typed, the text as written.
    move_to(&mut workbench, "twin");
    keys(&mut workbench, "e");
    workbench.key(Key::F2);
    match &workbench.mode {
        Mode::Prompt { text, .. } => assert_eq!(text.text(), "[the twin](C2.md)"),
        _ => panic!("typing: {}", workbench.message),
    }
}

#[test]
fn r_renames_a_sample_and_the_links_leading_to_it() {
    let scratch = Scratch::new("tui-rename");
    let file = scratch.0.join("C1.md");
    let text = fs::read_to_string(&file)
        .unwrap()
        .replace("alloy: cobalt\n", "alloy: cobalt\nreference: \"[[C3]]\"\n");
    fs::write(&file, text).unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C3");
    keys(&mut workbench, "R");
    match &workbench.mode {
        Mode::Prompt { text, .. } => assert_eq!(text.text(), "C3"),
        _ => panic!("asked its name: {}", workbench.message),
    }
    keys(&mut workbench, "C7\n");
    let Mode::Confirm { title, lines, .. } = &workbench.mode else {
        panic!("asked first: {}", workbench.message);
    };
    assert_eq!(title, "rename");
    assert!(
        lines.iter().any(|line| line.contains("[[C3]]  →  [[C7]]")),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    assert!(
        workbench.message.starts_with("renamed C3.md → C7.md"),
        "{}",
        workbench.message
    );
    // Still on it, under its new name.
    assert_eq!(
        workbench
            .current()
            .map(|(_, sample)| sample.name().map(str::to_string)),
        Some(Some("C7".to_string()))
    );
    write_waiting(&mut workbench);
    assert!(!scratch.0.join("C3.md").exists());
    assert!(
        fs::read_to_string(&file)
            .unwrap()
            .contains("reference: \"[[C7]]\"")
    );
    assert_eq!(
        workbench
            .current()
            .map(|(_, sample)| sample.name().map(str::to_string)),
        Some(Some("C7".to_string()))
    );
    // u gives the samples back.
    keys(&mut workbench, "u");
    if matches!(workbench.mode, Mode::Confirm { .. }) {
        keys(&mut workbench, "y");
    }
    assert!(scratch.0.join("C3.md").exists(), "{}", workbench.message);
    assert!(
        fs::read_to_string(&file)
            .unwrap()
            .contains("reference: \"[[C3]]\"")
    );
}

#[test]
fn tab_completes_a_link_being_typed_with_a_samples_name() {
    // An attribute's value asked by `A`, and one typed in the typing mode.
    let scratch = Scratch::new("link-tab");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    keys(&mut workbench, "\n");
    keys(&mut workbench, "Areference\n[[");
    workbench.key(Key::Tab);
    let typed = |workbench: &Workbench| match &workbench.mode {
        Mode::Prompt { text, .. } => text.text().to_string(),
        _ => panic!("still typing: {}", workbench.message),
    };
    // Several answer: as far as they agree, and which they are.
    assert_eq!(typed(&workbench), "[[C");
    assert_eq!(workbench.message, "C1, C2, C3");
    keys(&mut workbench, "3");
    workbench.key(Key::Tab);
    assert_eq!(typed(&workbench), "[[C3]]");
    // In the typing mode's cell too: Tab completes a link being typed, and
    // keeps the cell once it is whole.
    keys(&mut workbench, "\ny");
    move_to_line(&mut workbench, "alloy");
    keys(&mut workbench, "e[[C");
    workbench.key(Key::Tab);
    assert_eq!(typed(&workbench), "[[C");
    keys(&mut workbench, "2");
    workbench.key(Key::Tab);
    assert_eq!(typed(&workbench), "[[C2]]");
}

/// The cursor on `field`'s line, from the sample's first.
fn move_to_line(workbench: &mut Workbench, field: &str) {
    workbench.key(Key::Home);
    move_to(workbench, field);
}

#[test]
fn a_pdfs_page_and_a_file_by_its_name_are_opened_from_their_links() {
    let scratch = Scratch::new("page-link");
    fs::create_dir_all(scratch.0.join("papers")).unwrap();
    fs::write(scratch.0.join("papers/method.pdf"), "%PDF-1.4\n").unwrap();
    fs::create_dir_all(scratch.0.join("data/2024")).unwrap();
    fs::write(scratch.0.join("data/2024/curve.csv"), "step,force\n").unwrap();
    let file = scratch.0.join("C1.md");
    let text = fs::read_to_string(&file).unwrap().replace(
        "alloy: cobalt\n",
        "alloy: cobalt\nmethod: \"[the method](papers/method.pdf#page=12)\"\n\
         again: \"[[method.pdf#page=3]]\"\ncurve: \"[[curve.csv]]\"\ntwin: \"[[c2.md]]\"\n",
    );
    fs::write(&file, text).unwrap();
    let follow = |field: &str| {
        let mut workbench = Workbench::open(&scratch.0).unwrap();
        keys(&mut workbench, "\n");
        move_to(&mut workbench, field);
        let effects = keys(&mut workbench, "\n");
        (workbench, effects)
    };
    let (_, effects) = follow("method");
    assert!(
        matches!(effects.as_slice(), [Effect::OpenPage { file, page: 12 }] if file.ends_with("papers/method.pdf")),
        "{effects:?}"
    );
    let (_, effects) = follow("again");
    assert!(
        matches!(effects.as_slice(), [Effect::OpenPage { file, page: 3 }] if file.ends_with("papers/method.pdf")),
        "{effects:?}"
    );
    let (_, effects) = follow("curve");
    assert!(
        matches!(effects.as_slice(), [Effect::Open(path)] if path.ends_with("data/2024/curve.csv")),
        "{effects:?}"
    );
    // A sample by its file's name in another case, its extension written.
    let (workbench, _) = follow("twin");
    assert_eq!(workbench.message, "[[c2.md]] → C2.md — Esc goes back to C1");
}

#[test]
fn a_markdown_link_opens_its_file_or_its_sample_and_a_list_its_items() {
    let scratch = Scratch::new("markdown-link");
    let file = scratch.0.join("C1.md");
    let text = fs::read_to_string(&file).unwrap().replace(
        "alloy: cobalt\n",
        "alloy: cobalt\nimage: \"[the scan](images/C1_sem.png)\"\ntwin: \"[the twin](C2.md)\"\n\
         parents: [\"[[C2]]\", \"[[C3]]\"]\n",
    );
    fs::write(&file, text).unwrap();
    let open_on = |field: &str| {
        let mut workbench = Workbench::open(&scratch.0).unwrap();
        keys(&mut workbench, "\n");
        move_to(&mut workbench, field);
        workbench
    };
    // A file opens in its application.
    let mut workbench = open_on("image");
    let effects = keys(&mut workbench, "\n");
    assert!(
        matches!(effects.as_slice(), [Effect::Open(path)] if path.ends_with("images/C1_sem.png")),
        "{}",
        workbench.message
    );
    // A sample's file, on its screen.
    let mut workbench = open_on("twin");
    keys(&mut workbench, "o");
    assert_eq!(
        workbench.message,
        "[the twin](C2.md) → C2.md — Esc goes back to C1"
    );
    // A list opens its items, and `o` follows the one under the cursor.
    let mut workbench = open_on("parents");
    keys(&mut workbench, "\n");
    assert!(matches!(workbench.screen, Screen::Items { .. }));
    // Enter on an item follows it too.
    keys(&mut workbench, "j\n");
    assert_eq!(workbench.message, "[[C3]] → C3.md — Esc goes back to C1");
    let Screen::Sample { at, .. } = workbench.screen else {
        panic!("C3's screen");
    };
    assert_eq!(
        workbench
            .view
            .get(at)
            .and_then(|entry| entry.path.clone())
            .and_then(|path| path.file_name().map(|name| name.to_os_string())),
        Some(std::ffi::OsString::from("C3.md"))
    );
    // Back to the list the link was in, its item under the cursor.
    keys(&mut workbench, "\u{1b}");
    assert!(
        matches!(workbench.screen, Screen::Items { cursor: 1, .. }),
        "{}",
        workbench.message
    );
}

// ------------------------------------------------------------ changes waiting

/// The history's lines, newest first.
fn history_lines(root: &std::path::Path) -> Vec<String> {
    let output = std::process::Command::new("git")
        .arg("--git-dir")
        .arg(root.join(".samplekit/history"))
        .args(["log", "--all", "--format=%s"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

/// `A` on the sample open: an attribute, its value, confirmed.
fn add_attribute(workbench: &mut Workbench, name: &str, value: &str) {
    keys(workbench, &format!("A{name}\n{value}\ny"));
}

#[test]
fn a_change_waits_until_w_writes_it_one_change_of_the_history() {
    let scratch = Scratch::new("waiting-w");
    let c1 = scratch.0.join("C1.md");
    let c2 = scratch.0.join("C2.md");
    let (was_c1, was_c2) = (fs::read(&c1).unwrap(), fs::read(&c2).unwrap());
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    add_attribute(&mut workbench, "style", "tin");
    assert!(
        workbench.message.contains("not written yet"),
        "{}",
        workbench.message
    );
    assert_eq!(fs::read(&c1).unwrap(), was_c1, "the file is untouched");
    assert!(
        screen(&mut workbench).contains("style"),
        "the screen shows it"
    );
    keys(&mut workbench, "\u{1b}");
    let at = workbench
        .view
        .iter()
        .position(|entry| entry.sample.borrow().name() == Some("C2"))
        .unwrap();
    workbench.cursor = at;
    keys(&mut workbench, "Dy");
    assert_eq!(workbench.view.len(), 2, "C2 is no longer shown");
    assert_eq!(fs::read(&c2).unwrap(), was_c2, "nor removed yet");
    write_waiting(&mut workbench);
    assert!(
        workbench.message.starts_with("written: 2 files"),
        "{}",
        workbench.message
    );
    assert!(fs::read_to_string(&c1).unwrap().contains("style: tin"));
    assert!(!c2.exists());
    let lines = history_lines(&scratch.0);
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(
        lines[0].starts_with("tui · ") && lines[0].contains("; "),
        "each change said: {lines:?}"
    );
    // One `u` gives both back.
    keys(&mut workbench, "uy");
    assert_eq!(fs::read(&c1).unwrap(), was_c1);
    assert_eq!(fs::read(&c2).unwrap(), was_c2);
}

#[test]
fn u_takes_back_a_change_waiting_then_a_write() {
    let scratch = Scratch::new("waiting-u");
    let c1 = scratch.0.join("C1.md");
    let was = fs::read_to_string(&c1).unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    add_attribute(&mut workbench, "style", "tin");
    add_attribute(&mut workbench, "finish", "matt");
    keys(&mut workbench, "u");
    assert!(
        workbench.message.contains("still waiting"),
        "{}",
        workbench.message
    );
    assert!(read_waiting(&c1).contains("style: tin"));
    assert!(!read_waiting(&c1).contains("finish"));
    keys(&mut workbench, "u");
    assert_eq!(
        workbench.message, "taken back: nothing waits to be written",
        "{}",
        workbench.message
    );
    assert_eq!(fs::read_to_string(&c1).unwrap(), was);
    add_attribute(&mut workbench, "style", "tin");
    write_waiting(&mut workbench);
    assert!(fs::read_to_string(&c1).unwrap().contains("style: tin"));
    // Nothing waits: `u` undoes the write.
    keys(&mut workbench, "uy");
    assert_eq!(fs::read_to_string(&c1).unwrap(), was);
}

#[test]
fn leaving_with_changes_waiting_asks_and_keeps_them_by_default() {
    let scratch = Scratch::new("waiting-leave");
    let c1 = scratch.0.join("C1.md");
    let was = fs::read_to_string(&c1).unwrap();
    {
        let mut workbench = Workbench::open(&scratch.0).unwrap();
        open_named(&mut workbench, "C1");
        add_attribute(&mut workbench, "style", "tin");
        keys(&mut workbench, "\u{1b}");
        assert!(matches!(
            keys(&mut workbench, "q").last(),
            Some(Effect::None)
        ));
        assert!(matches!(workbench.mode, Mode::Confirm { .. }), "asked");
        keys(&mut workbench, "n");
        assert!(matches!(workbench.mode, Mode::Normal), "stays");
        keys(&mut workbench, "q");
        assert!(matches!(
            keys(&mut workbench, "k").last(),
            Some(Effect::Quit)
        ));
    }
    assert_eq!(fs::read_to_string(&c1).unwrap(), was, "kept, not written");
    {
        let mut workbench = Workbench::open(&scratch.0).unwrap();
        let Mode::Confirm { title, lines, .. } = &workbench.mode else {
            panic!("the changes waiting are offered: {}", workbench.message);
        };
        assert_eq!(title, "changes waiting");
        assert!(
            lines[0].starts_with("1 file changed and not written, since"),
            "{lines:?}"
        );
        keys(&mut workbench, "y");
        assert!(screen(&mut workbench).contains("3 of 3") || workbench.view.len() == 3);
        write_waiting(&mut workbench);
        assert!(fs::read_to_string(&c1).unwrap().contains("style: tin"));
        open_named(&mut workbench, "C1");
        add_attribute(&mut workbench, "finish", "matt");
        keys(&mut workbench, "\u{1b}q");
        assert!(matches!(
            keys(&mut workbench, "d").last(),
            Some(Effect::Quit)
        ));
    }
    let workbench = Workbench::open(&scratch.0).unwrap();
    assert!(
        matches!(workbench.mode, Mode::Normal),
        "nothing waits: dropped"
    );
    let written = fs::read_to_string(&c1).unwrap();
    assert!(
        written.contains("style: tin") && !written.contains("finish"),
        "{written}"
    );
}

#[test]
fn a_file_changed_by_another_hand_is_not_written_over() {
    let scratch = Scratch::new("waiting-other-hand");
    let c1 = scratch.0.join("C1.md");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    add_attribute(&mut workbench, "style", "tin");
    // Another hand changes another field: laid again over it, both kept.
    let other = fs::read_to_string(&c1)
        .unwrap()
        .replace("alloy: cobalt", "alloy: nickel");
    fs::write(&c1, &other).unwrap();
    write_waiting(&mut workbench);
    let written = fs::read_to_string(&c1).unwrap();
    assert!(
        written.contains("style: tin") && written.contains("alloy: nickel"),
        "{written}"
    );
    // The same field changed both here and there: nothing is written.
    add_attribute(&mut workbench, "finish", "matt");
    let theirs = fs::read_to_string(&c1)
        .unwrap()
        .replace("style: tin", "style: zinc\nfinish: gloss");
    fs::write(&c1, &theirs).unwrap();
    keys(&mut workbench, "w");
    let Mode::Confirm { lines, .. } = &workbench.mode else {
        panic!("{}", workbench.message)
    };
    assert!(
        lines
            .iter()
            .any(|line| line.contains("changed both here and since by another hand")),
        "{lines:?}"
    );
    keys(&mut workbench, "y");
    assert!(
        workbench.message.contains("nothing written"),
        "{}",
        workbench.message
    );
    assert_eq!(fs::read_to_string(&c1).unwrap(), theirs);
}

#[test]
#[cfg(unix)]
fn a_computation_waits_and_is_dropped_leaving_the_history_as_it_was() {
    let venv = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".venv");
    assert!(venv.join("bin/python").is_file(), "the repository's .venv");
    let scratch = Scratch::new("waiting-compute");
    let project = scratch.0.join("p");
    fs::create_dir(&project).unwrap();
    std::os::unix::fs::symlink(&venv, project.join(".venv")).unwrap();
    fs::write(
        project.join("model.py"),
        "import samplekit as sk\nclass M(sk.Sample):\n    def __init__(self, path=None, name=None):\n        \
         super().__init__(path, name=name)\n        self.a = sk.Property()\n        \
         self.b = sk.Property(compute_value=self._b, depends_on=[\"a\"])\n\n    \
         def _b(self):\n        return 2 * self.a.value\n",
    )
    .unwrap();
    fs::write(
        project.join(".samplekitrc"),
        "schema_version = 1\n[model]\npath = \"model.py\"\npython = \".venv\"\n",
    )
    .unwrap();
    let file = project.join("s1.md");
    let was = "---\nschema_version: 1\nname: s1\nproperties:\n  a: 1.0\n---\n";
    fs::write(&file, was).unwrap();
    let mut workbench = Workbench::open(&project).unwrap();
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
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        was,
        "computed, not written"
    );
    assert!(
        read_waiting(&file).contains("b:"),
        "{}",
        read_waiting(&file)
    );
    let history = history_lines(&project);
    // Dropped, by `u`: nothing of it is left.
    keys(&mut workbench, "u");
    assert_eq!(read_waiting(&file), was);
    assert_eq!(history_lines(&project), history, "no snapshot of it");
}

// ------------------------------------------------------------ the typing mode

/// The cursor on `field`'s line, from the sample's first.
fn go_to(workbench: &mut Workbench, field: &str) {
    workbench.key(Key::Home);
    move_to(workbench, field);
}

#[test]
fn e_enters_the_typing_mode_where_what_is_typed_replaces_a_cell() {
    let scratch = Scratch::new("typing");
    let c1 = scratch.0.join("C1.md");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    go_to(&mut workbench, "alloy");
    let on_alloy = workbench.screen.clone();
    keys(&mut workbench, "e");
    assert!(workbench.typing, "{}", workbench.message);
    keys(&mut workbench, "nickel");
    assert!(
        matches!(workbench.mode, Mode::Prompt { .. }),
        "a cell typed in"
    );
    keys(&mut workbench, "\n");
    assert!(
        read_waiting(&c1).contains("alloy: nickel"),
        "{}",
        read_waiting(&c1)
    );
    assert!(
        !fs::read_to_string(&c1).unwrap().contains("nickel"),
        "waiting"
    );
    assert!(matches!(workbench.mode, Mode::Normal));
    let Screen::Sample { cursor: below, .. } = workbench.screen else {
        panic!()
    };
    let Screen::Sample { cursor: alloy, .. } = on_alloy else {
        panic!()
    };
    assert_eq!(below, alloy + 1, "Enter keeps it and goes down");
    workbench.key(Key::Up);
    workbench.key(Key::Left);
    assert_eq!(workbench.cell, 0, "the name's cell");
    workbench.key(Key::Right);
    assert_eq!(workbench.cell, 1, "back to the value's");
    // `j` is typed in the mode; out of it, it moves.
    keys(&mut workbench, "\u{1b}");
    assert!(!workbench.typing);
    keys(&mut workbench, "j");
    let Screen::Sample { cursor, .. } = workbench.screen else {
        panic!()
    };
    assert_eq!(cursor, alloy + 1);
}

#[test]
fn a_value_and_its_uncertainty_are_typed_in_one_cell() {
    let scratch = Scratch::new("typing-value");
    let c1 = scratch.0.join("C1.md");
    fs::write(
        &c1,
        fs::read_to_string(&c1)
            .unwrap()
            .replace("v: 12.0", "v: 12.34"),
    )
    .unwrap();
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    go_to(&mut workbench, "diameter");
    // Opened as shown, rounded, and kept as it opened: left as it is.
    keys(&mut workbench, "e");
    workbench.key(Key::F2);
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("{}", workbench.message)
    };
    assert_eq!(text.text(), "12.3 +- 0.1");
    keys(&mut workbench, "\n");
    assert_eq!(workbench.waiting_count(), 0, "nothing changed");
    go_to(&mut workbench, "diameter");
    keys(&mut workbench, "12.5 +- 0.2\n");
    let waiting = read_waiting(&c1);
    assert!(
        waiting.contains("v: 12.5") && waiting.contains("u: 0.2"),
        "{waiting}"
    );
    go_to(&mut workbench, "diameter");
    keys(&mut workbench, "13\n");
    let waiting = read_waiting(&c1);
    assert!(
        waiting.contains("v: 13") && waiting.contains("u: 0.2"),
        "kept: {waiting}"
    );
    // `±` typed, or pasted, is read as `+-` is.
    go_to(&mut workbench, "diameter");
    keys(&mut workbench, "13 ± 0.3\n");
    let waiting = read_waiting(&c1);
    assert!(
        waiting.contains("v: 13") && waiting.contains("u: 0.3"),
        "{waiting}"
    );
    go_to(&mut workbench, "diameter");
    keys(&mut workbench, "13 +-\n");
    let waiting = read_waiting(&c1);
    assert!(
        waiting.contains("v: 13") && !waiting.contains("u: 0.3"),
        "taken out: {waiting}"
    );
}

#[test]
fn typing_over_a_name_renames_an_attribute_and_delete_takes_it_out() {
    let scratch = Scratch::new("typing-name");
    let c1 = scratch.0.join("C1.md");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    go_to(&mut workbench, "alloy");
    keys(&mut workbench, "e");
    workbench.key(Key::Left);
    keys(&mut workbench, "style\n");
    let waiting = read_waiting(&c1);
    assert!(
        waiting.contains("style: cobalt") && !waiting.contains("alloy:"),
        "{waiting}"
    );
    go_to(&mut workbench, "style");
    workbench.key(Key::Delete);
    let waiting = read_waiting(&c1);
    assert!(
        !waiting.contains("style") && !waiting.contains("cobalt"),
        "{waiting} {}",
        workbench.message
    );
    assert!(
        workbench.message.contains("not written yet"),
        "{}",
        workbench.message
    );
}

#[test]
fn a_table_is_typed_cell_by_cell_and_delete_takes_a_row_out() {
    let scratch = rich("typing-table");
    let d1 = scratch.0.join("more/D1.md");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    go_to(&mut workbench, "runs");
    workbench.key(Key::Right);
    assert!(matches!(workbench.screen, Screen::Table { .. }));
    keys(&mut workbench, "e");
    // The first row's load, with its uncertainty.
    workbench.key(Key::Right);
    keys(&mut workbench, "4.5 +- 0.1\n");
    let waiting = read_waiting(&d1);
    assert!(waiting.contains("load: {v: 4.5, u: 0.1"), "{waiting}");
    // On the row of run 2 now: taken out.
    workbench.key(Key::Delete);
    let waiting = read_waiting(&d1);
    assert!(!waiting.contains("run: 2"), "{waiting}");
    assert!(
        fs::read_to_string(&d1).unwrap().contains("run: 2"),
        "not written yet"
    );
    // The empty row takes run 3 once its index is given.
    workbench.key(Key::End);
    workbench.key(Key::Left);
    keys(&mut workbench, "3\n");
    let waiting = read_waiting(&d1);
    assert!(waiting.contains("run: 3"), "{waiting}");
}

#[test]
fn the_collection_is_typed_in_a_column_for_the_rows_taken() {
    let scratch = Scratch::new("typing-collection");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    let column = |workbench: &Workbench, field: &str| {
        workbench
            .profile
            .columns()
            .iter()
            .position(|column| column.field == field)
            .unwrap()
    };
    keys(&mut workbench, "e");
    let alloy = column(&workbench, "alloy");
    for _ in 0..alloy {
        workbench.key(Key::Right);
    }
    workbench.key(Key::MoveDown);
    keys(&mut workbench, "tin\n");
    let tin: Vec<String> = ["C1", "C2", "C3"]
        .iter()
        .filter(|name| read_waiting(&scratch.0.join(format!("{name}.md"))).contains("alloy: tin"))
        .map(|name| name.to_string())
        .collect();
    assert_eq!(
        tin.len(),
        2,
        "two rows taken: {tin:?} {}",
        workbench.message
    );
    let diameter = column(&workbench, "diameter");
    while workbench.column > diameter {
        workbench.key(Key::Left);
    }
    while workbench.column < diameter {
        workbench.key(Key::Right);
    }
    keys(&mut workbench, "12.5 ± 0.3\n");
    assert!(
        workbench.view.iter().any(|entry| {
            let text = read_waiting(entry.path.as_deref().unwrap());
            text.contains("v: 12.5") && text.contains("u: 0.3")
        }),
        "{}",
        workbench.message
    );
    while workbench.column > 0 {
        workbench.key(Key::Left);
    }
    keys(&mut workbench, "x");
    assert!(workbench.message.contains("R"), "{}", workbench.message);
}

#[test]
fn a_project_opened_again_while_open_keeps_its_changes_waiting() {
    // The start page opens a project again while the workbench before it is
    // still there: the first one's end leaves the second its changes waiting.
    let scratch = Scratch::new("waiting-again");
    let c1 = scratch.0.join("C1.md");
    let was = fs::read_to_string(&c1).unwrap();
    let first = Workbench::open(&scratch.0).unwrap();
    let mut second = Workbench::open(&scratch.0).unwrap();
    drop(first);
    open_named(&mut second, "C1");
    add_attribute(&mut second, "style", "tin");
    assert_eq!(fs::read_to_string(&c1).unwrap(), was, "it waits");
    assert_eq!(second.waiting_count(), 1);
}

// ------------------------------------------------------------ a value's card

/// A copy of the tensile demo's last stage, `04-results`, in `scratch`: the
/// workbench writes its state, and never into the repository's own copy.
fn tensile(scratch: &Scratch) -> PathBuf {
    fn copied(from: &std::path::Path, to: &std::path::Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copied(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let demo = scratch.0.join("tensile");
    copied(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/tensile/04-results"),
        &demo,
    );
    demo
}

/// The card's line of `field`, in `group`.
fn card_line(
    workbench: &Workbench,
    group: CardGroup,
    field: &str,
) -> samplekit::tui::model::CardLine {
    let lines = workbench.card_lines();
    lines
        .iter()
        .find(|line| line.group == group && line.field == field)
        .cloned()
        .unwrap_or_else(|| panic!("{group:?} {field} on the card: {lines:?}"))
}

#[test]
fn i_opens_a_values_card_with_what_it_reads_and_what_reads_it() {
    // Read from the records the files keep: no Python is asked.
    let scratch = Scratch::new("card");
    let demo = tensile(&scratch);
    let mut workbench = Workbench::open(&demo).unwrap();
    open_named(&mut workbench, "steel-01");
    move_to(&mut workbench, "area");
    keys(&mut workbench, "i");
    let area = card_line(&workbench, CardGroup::Itself, "area");
    assert_eq!(area.shown, "25.056 ± 0.053 mm²");
    assert_eq!(workbench.card_lines()[0], area, "the value first");
    for read in ["thickness", "width"] {
        card_line(&workbench, CardGroup::Reads, read);
    }
    let stress = card_line(&workbench, CardGroup::ReadBy, "curve.stress");
    assert_eq!(stress.shown, "13 rows");
    keys(&mut workbench, "\u{1b}");
    assert!(workbench.card.is_none());
    // A cell of `curve.stress`: its row's `force`, and `area`.
    workbench.key(Key::Home);
    move_to(&mut workbench, "curve");
    workbench.key(Key::Right);
    keys(&mut workbench, "llj");
    assert_eq!(workbench.cell().unwrap().0.field, "curve.stress[0.025]");
    keys(&mut workbench, "i");
    card_line(&workbench, CardGroup::Itself, "curve.stress[0.025]");
    let force = card_line(&workbench, CardGroup::Reads, "curve.force[0.025]");
    assert_eq!(force.shown, "1.281");
    card_line(&workbench, CardGroup::Reads, "area");
    keys(&mut workbench, "\u{1b}");
    assert!(workbench.card.is_none());
    assert!(matches!(workbench.screen, Screen::Table { .. }));
}

#[test]
fn a_line_typed_in_a_card_waits_and_shows_what_it_made_outdated() {
    let scratch = Scratch::new("card-typed");
    let demo = tensile(&scratch);
    let steel = demo.join("samples/steel-01.md");
    let was = fs::read_to_string(&steel).unwrap();
    let mut workbench = Workbench::open(&demo).unwrap();
    open_named(&mut workbench, "steel-01");
    move_to(&mut workbench, "area");
    keys(&mut workbench, "i");
    assert_eq!(
        card_line(&workbench, CardGroup::Itself, "area").state,
        "current"
    );
    // `width`, under the cursor: its value with its uncertainty.
    let at = workbench
        .card_lines()
        .iter()
        .position(|line| line.field == "width")
        .unwrap();
    for _ in 0..at {
        workbench.key(Key::Down);
    }
    keys(&mut workbench, "e");
    let Mode::Prompt { text, .. } = &workbench.mode else {
        panic!("width typed: {}", workbench.message);
    };
    assert_eq!(text.text(), "12.513 +- 0.011");
    keys(&mut workbench, "12.6\n");
    assert!(
        matches!(workbench.mode, Mode::Normal),
        "{}",
        workbench.message
    );
    assert!(workbench.card.is_some(), "the card stays open");
    // It waits, the file untouched.
    assert_eq!(fs::read_to_string(&steel).unwrap(), was);
    let waiting = read_waiting(&steel);
    assert!(waiting.contains("v: 12.6"), "{waiting}");
    assert!(
        waiting.contains("u: 0.01008849730028156"),
        "the uncertainty kept"
    );
    // Drawn again as it made it: what reads `width` is outdated at once.
    assert!(
        card_line(&workbench, CardGroup::Reads, "width")
            .shown
            .starts_with("12.600"),
        "{:?}",
        workbench.card_lines()
    );
    let area = card_line(&workbench, CardGroup::Itself, "area");
    assert!(area.state.starts_with("outdated"), "{area:?}");
    // A whole column is typed in its table: said, nothing opened.
    let at = workbench
        .card_lines()
        .iter()
        .position(|line| line.field == "curve.stress")
        .unwrap();
    for _ in 0..at {
        workbench.key(Key::Down);
    }
    keys(&mut workbench, "e");
    assert!(matches!(workbench.mode, Mode::Normal));
    assert!(
        workbench
            .message
            .contains("curve.stress is a whole column: its cells are typed in its table"),
        "{}",
        workbench.message
    );
    assert_eq!(workbench.waiting_count(), 1);
}

#[test]
fn enter_on_a_value_does_nothing_and_a_line_shows_its_mark_alone() {
    let scratch = Scratch::new("enter-on-a-value");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "C1");
    move_to(&mut workbench, "diameter");
    let before = screen(&mut workbench);
    keys(&mut workbench, "\n");
    assert!(
        matches!(workbench.screen, Screen::Sample { .. }) && matches!(workbench.mode, Mode::Normal),
        "{}",
        workbench.message
    );
    assert!(workbench.card.is_none() && !workbench.typing);
    assert_eq!(screen(&mut workbench), before, "the screen as it was");
    // A value outdated by a change waiting: its mark, never its word.
    let demo = tensile(&scratch);
    let mut workbench = Workbench::open(&demo).unwrap();
    open_named(&mut workbench, "steel-01");
    move_to(&mut workbench, "width");
    keys(&mut workbench, "e12.6\n\u{1b}");
    let area = workbench
        .entries()
        .into_iter()
        .find(|entry| entry.field == "area")
        .unwrap();
    assert!(area.state.starts_with("outdated"), "{}", area.state);
    let drawn = screen(&mut workbench);
    let line = drawn
        .lines()
        .find(|line| line.contains(" area "))
        .unwrap_or_default();
    assert!(line.contains("⚠ area"), "{drawn}");
    assert!(!drawn.contains("outdated"), "{drawn}");
}

#[test]
fn enter_on_a_cards_readings_opens_them_as_a_list() {
    let scratch = rich("card-readings");
    let mut workbench = Workbench::open(&scratch.0).unwrap();
    open_named(&mut workbench, "D1");
    move_to(&mut workbench, "diameter");
    keys(&mut workbench, "i");
    let readings = workbench
        .card_lines()
        .iter()
        .position(|line| line.group == samplekit::tui::model::CardGroup::Readings)
        .expect("the card has its readings");
    for _ in 0..readings {
        workbench.key(Key::Down);
    }
    workbench.key(Key::Enter);
    assert!(workbench.card.is_none(), "the card closes for the list");
    let Screen::Items {
        readings, field, ..
    } = &workbench.screen
    else {
        panic!("the readings' window: {}", workbench.message)
    };
    assert!(*readings && field == "diameter");
    let (_, items, _) = workbench.items().unwrap();
    assert_eq!(items.len(), 2, "{items:?}");
}
