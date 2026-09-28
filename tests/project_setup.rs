//! The tests of `project_setup`.

use std::fs;
use std::path::PathBuf;

use samplekit::config::project_setup::{
    self, Answers, ModelAnswer, Question, SetupError, Starter, Step,
};

const EXAMPLE: Answers = Answers {
    starter: Starter::Example,
    model: ModelAnswer::Create,
    environment: false,
};
const EMPTY: Answers = Answers {
    starter: Starter::Empty,
    model: ModelAnswer::Create,
    environment: false,
};

fn scratch(name: &str) -> PathBuf {
    let path = dunce::canonicalize(std::env::temp_dir())
        .unwrap()
        .join(format!("samplekit-setup-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn an_empty_directory_is_offered_every_file() {
    let root = scratch("empty");
    let plan = project_setup::plan(&root, &EXAMPLE);
    assert_eq!(plan.missing().count(), 5);
    assert!(plan.proposals.iter().all(|proposal| !proposal.present));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_file_already_there_is_kept() {
    let root = scratch("kept");
    fs::write(
        root.join(".samplekitrc"),
        "schema_version = 1\n[nonsense]\n",
    )
    .unwrap();
    let plan = project_setup::plan(&root, &EXAMPLE);
    let rc = plan
        .proposals
        .iter()
        .find(|proposal| matches!(&proposal.step, Step::File { path, .. } if path.ends_with(".samplekitrc")))
        .unwrap();
    assert!(rc.present);
    assert!(
        rc.note
            .as_deref()
            .is_some_and(|note| note.contains("not read")),
        "{rc:?}"
    );
    project_setup::apply(&plan, |_| true).unwrap();
    assert_eq!(
        fs::read_to_string(root.join(".samplekitrc")).unwrap(),
        "schema_version = 1\n[nonsense]\n"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn only_the_steps_accepted_are_done() {
    let root = scratch("accepted");
    let plan = project_setup::plan(&root, &EXAMPLE);
    let applied = project_setup::apply(
        &plan,
        |step| !matches!(step, Step::File { path, .. } if path.ends_with("EXAMPLE.md")),
    )
    .unwrap();
    assert_eq!(applied.written.len(), 4);
    assert!(!root.join("samples/EXAMPLE.md").exists());
    assert!(root.join("model/brew.py").is_file());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_file_where_a_directory_goes_writes_nothing() {
    let root = scratch("blocked");
    fs::write(root.join("model"), "a file").unwrap();
    let plan = project_setup::plan(&root, &EXAMPLE);
    assert!(
        plan.blocked
            .as_ref()
            .is_some_and(|path| path.ends_with("model"))
    );
    let error = project_setup::apply(&plan, |_| true).unwrap_err();
    assert!(matches!(error, SetupError::Blocked { .. }), "{error:?}");
    assert!(!root.join(".samplekitrc").exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn an_empty_project_is_its_structure_and_a_model_that_runs() {
    let root = scratch("empty-starter");
    let plan = project_setup::plan(&root, &EMPTY);
    let made: Vec<String> = plan
        .proposals
        .iter()
        .map(|proposal| match &proposal.step {
            Step::File { path, .. } | Step::Directory { path, .. } => {
                path.strip_prefix(&root).unwrap().display().to_string()
            }
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(made, [".samplekitrc", "model/main.py", "samples"]);
    project_setup::apply(&plan, |_| true).unwrap();
    assert!(root.join("samples").is_dir());
    assert_eq!(fs::read_dir(root.join("samples")).unwrap().count(), 0);
    let config = samplekit::config::project_config::load(&root.join(".samplekitrc")).unwrap();
    let template = samplekit::config::model_runtime::template_of(&config).unwrap();
    assert!(template.path().ends_with("model/main.py"));
    // It declares nothing: every declaration is a comment to fill in.
    assert!(samplekit::config::model_runtime::declared_properties(&config).is_empty());
    assert!(project_setup::plan(&root, &EMPTY).is_complete());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_project_without_a_model_is_its_configuration_and_samples() {
    let root = scratch("no-model");
    let answers = Answers {
        model: ModelAnswer::Without,
        ..EMPTY
    };
    let plan = project_setup::plan(&root, &answers);
    assert_eq!(plan.missing().count(), 2, "{plan:?}");
    project_setup::apply(&plan, |_| true).unwrap();
    assert!(!root.join("model").exists());
    let config = samplekit::config::project_config::load(&root.join(".samplekitrc")).unwrap();
    assert!(config.model().is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_questions_asked_follow_the_answers_and_the_folder() {
    let root = scratch("questions");
    let mut answers = Answers::default();
    assert_eq!(answers.starter, Starter::Empty);
    assert_eq!(
        project_setup::questions(&root, &answers),
        [Question::Starter, Question::Model, Question::Environment]
    );
    // The example asks nothing of a model.
    Question::Starter.choose(&mut answers, 1);
    assert_eq!(answers.starter, Starter::Example);
    assert_eq!(
        project_setup::questions(&root, &answers),
        [Question::Starter, Question::Environment]
    );
    // A project already set up is asked of its environment only.
    fs::write(root.join(".samplekitrc"), "schema_version = 1\n").unwrap();
    assert_eq!(
        project_setup::questions(&root, &answers),
        [Question::Environment]
    );
    // Nor given a model: the empty project's files are not completed.
    let plan = project_setup::plan(
        &root,
        &Answers {
            environment: false,
            ..EMPTY
        },
    );
    assert!(plan.is_complete(), "{plan:?}");
    // An environment holding samplekit leaves nothing to ask.
    // Laid out as the system lays an environment out.
    let (python, package) = if cfg!(windows) {
        (
            ".venv/Scripts/python.exe",
            ".venv/Lib/site-packages/samplekit",
        )
    } else {
        (
            ".venv/bin/python",
            ".venv/lib/python3.13/site-packages/samplekit",
        )
    };
    fs::create_dir_all(root.join(python).parent().unwrap()).unwrap();
    fs::write(root.join(python), "").unwrap();
    fs::create_dir_all(root.join(package)).unwrap();
    fs::write(root.join(package).join("__init__.py"), "").unwrap();
    assert!(project_setup::questions(&root, &answers).is_empty());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_file_written_since_the_plan_is_refused_and_kept() {
    // Planned missing, written meanwhile by another hand: looked for again as
    // it is written, refused as a sample written since is, and never
    // overwritten.
    let root = scratch("appeared");
    let plan = project_setup::plan(&root, &EMPTY);
    fs::write(root.join(".samplekitrc"), "mine\n").unwrap();
    let error = project_setup::apply(&plan, |_| true).unwrap_err();
    assert!(
        matches!(&error, SetupError::WrittenSince { path, written }
            if path.ends_with(".samplekitrc") && written.is_empty()),
        "{error:?}"
    );
    assert!(error.to_string().contains("another process"), "{error}");
    assert_eq!(
        fs::read_to_string(root.join(".samplekitrc")).unwrap(),
        "mine\n"
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn the_files_written_leave_out_the_directories() {
    // *3 files written* was said of two files and `samples/`.
    let root = scratch("counted");
    let applied = project_setup::apply(&project_setup::plan(&root, &EMPTY), |_| true).unwrap();
    assert_eq!(applied.written.len(), 3);
    assert_eq!(applied.files_written(), 2);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn an_environment_is_made_by_the_interpreter_each_system_has() {
    // `python3` is rarely on Windows, where the launcher is.
    let tried: Vec<&str> = project_setup::interpreters()
        .iter()
        .map(|(program, _)| *program)
        .collect();
    if cfg!(windows) {
        assert_eq!(tried, ["py", "python", "python3"]);
    } else {
        assert_eq!(tried, ["python3", "python"]);
    }
}

#[test]
#[cfg(unix)]
fn pip_said_in_another_encoding_is_still_said() {
    // A reason that does not decode as UTF-8 was dropped for *pip failed*.
    use std::os::unix::fs::PermissionsExt;
    let root = scratch("latin");
    fs::create_dir_all(root.join(".venv/bin")).unwrap();
    let python = root.join(".venv/bin/python");
    fs::write(
        &python,
        "#!/bin/sh\nprintf 'acc\\351s refus\\351\\n' >&2\nexit 1\n",
    )
    .unwrap();
    fs::set_permissions(&python, fs::Permissions::from_mode(0o755)).unwrap();
    let said =
        project_setup::make_environment(&root, &Step::Environment { create: false }).unwrap_err();
    assert!(said.contains("acc\u{fffd}s refus\u{fffd}"), "{said}");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn a_version_is_spelt_as_python_spells_it() {
    // Pip and a user installing the package write Python's spelling.
    for (build, python) in [
        ("1.0.0-rc.1", "1.0.0rc1"),
        ("2.1.0-alpha.3", "2.1.0a3"),
        ("2.1.0-beta.2", "2.1.0b2"),
        ("1.0.0", "1.0.0"),
    ] {
        assert_eq!(project_setup::python_spelling(build), python, "{build}");
    }
}

#[test]
fn a_model_already_there_is_named_where_it_is() {
    // *use a model file I already have* names it; nothing is copied.
    let root = scratch("own-model");
    fs::create_dir_all(root.join("brewery")).unwrap();
    fs::write(
        root.join("brewery/beam.py"),
        "import samplekit as sk\n\nclass Beam(sk.Sample):\n    pass\n",
    )
    .unwrap();
    fs::write(root.join("brewery/notes.txt"), "").unwrap();
    // What is typed is checked: nothing, a file not there, one not Python.
    assert!(project_setup::model_path(&root, "  ").is_err());
    let missing = project_setup::model_path(&root, "brewery/none.py").unwrap_err();
    assert!(missing.contains("no file at brewery/none.py"), "{missing}");
    let text = project_setup::model_path(&root, "brewery/notes.txt").unwrap_err();
    assert!(text.contains("not a Python file"), "{text}");
    // From the project's folder, or whole: said from the folder.
    assert_eq!(
        project_setup::model_path(&root, "./brewery/beam.py").unwrap(),
        PathBuf::from("brewery/beam.py")
    );
    let whole = root.join("brewery/beam.py").display().to_string();
    assert_eq!(
        project_setup::model_path(&root, &whole).unwrap(),
        PathBuf::from("brewery/beam.py")
    );
    // The plan names it, and writes no model of its own.
    let answers = Answers {
        model: ModelAnswer::Existing(PathBuf::from("brewery/beam.py")),
        ..EMPTY
    };
    assert!(answers.has_model());
    let plan = project_setup::plan(&root, &answers);
    assert_eq!(plan.missing().count(), 2, "{plan:?}");
    project_setup::apply(&plan, |_| true).unwrap();
    assert!(!root.join("model").exists());
    let config = samplekit::config::project_config::load(&root.join(".samplekitrc")).unwrap();
    let model = config.model().expect("the model named");
    assert!(model.path.ends_with("brewery/beam.py"), "{model:?}");
    assert!(model.class.is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn each_question_says_its_default_and_the_model_has_three_answers() {
    // Said to a newcomer, the default said so.
    let root = scratch("worded");
    for question in [Question::Starter, Question::Model, Question::Environment] {
        let choices = question.choices(&root);
        assert!(choices[0].0.contains("(default)"), "{choices:?}");
        assert!(
            choices[1..]
                .iter()
                .all(|(label, _)| !label.contains("default")),
            "{choices:?}"
        );
        assert_eq!(question.chosen(&Answers::default()), 0);
    }
    assert!(Question::Model.ask().contains("A model is a Python file"));
    let labels: Vec<&str> = Question::Model
        .choices(&root)
        .iter()
        .map(|(label, _)| *label)
        .collect();
    assert_eq!(
        labels,
        [
            "create a model to fill in (default)",
            "use a model file I already have",
            "no model for now"
        ]
    );
    // `choose` and `chosen` agree, and only the second asks a path.
    let mut answers = Answers::default();
    for choice in 0..3 {
        Question::Model.choose(&mut answers, choice);
        assert_eq!(Question::Model.chosen(&answers), choice);
        assert_eq!(Question::Model.asks_path(choice), choice == 1);
    }
    assert_eq!(answers.model, ModelAnswer::Without);
    assert!(!Question::Starter.asks_path(1));
    let _ = fs::remove_dir_all(root);
}
