use super::*;

fn every_event() -> Vec<Event> {
    vec![
        Event::Reading,
        Event::Progress {
            done: 3,
            total: 9_000,
        },
        Event::Finished {
            effects: 375,
            programs: 11_000,
            translated: 10_000,
            refused: 900,
            skipped: 100,
            reused: 5,
            baked: 10_995,
        },
        Event::Identity {
            translator: "dxbc-spirv c7f06970 abcd".into(),
            legalizer: "0123abcd".into(),
            pack_version: 3,
        },
    ]
}

#[test]
fn every_event_is_one_line_that_reads_back_as_itself() {
    for event in every_event() {
        let line = event.line();
        assert!(!line.contains('\n'), "{line}");
        assert_eq!(Event::parse(&line), Some(event), "{line}");
    }
}

#[test]
fn an_event_is_named_by_its_event_field() {
    assert_eq!(Event::Reading.line(), r#"{"event":"reading"}"#);
    assert_eq!(
        Event::Progress { done: 1, total: 2 }.line(),
        r#"{"event":"progress","done":1,"total":2}"#
    );
}

#[test]
fn a_line_that_is_not_an_event_is_not_one() {
    for line in [
        "",
        "   ",
        "INFO baked 3 programs",
        "{}",
        r#"{"event":"nothing"}"#,
        r#"{"event":"progress","done":1}"#,
        "[1, 2]",
    ] {
        assert_eq!(Event::parse(line), None, "{line:?}");
    }
}

#[test]
fn a_line_is_read_with_the_whitespace_a_pipe_leaves() {
    assert_eq!(
        Event::parse("  {\"event\":\"reading\"}\r\n"),
        Some(Event::Reading)
    );
}

#[test]
fn progress_is_told_at_the_ends_and_about_every_hundredth_between() {
    let told = std::sync::Mutex::new(Vec::new());
    let progress = throttled(|event| told.lock().unwrap().push(event));
    for done in 0..=1_000 {
        progress(done, 1_000);
    }
    drop(progress);
    let told = told.into_inner().unwrap();
    let counts: Vec<usize> = told
        .iter()
        .map(|e| match e {
            Event::Progress { done, total } => {
                assert_eq!(*total, 1_000);
                *done
            }
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(counts.len(), 101);
    assert_eq!((counts[0], counts[100]), (0, 1_000));
    assert!(counts.windows(2).all(|w| w[1] == w[0] + 10 || w[0] == 0));
}

#[test]
fn a_short_bake_is_told_in_full_and_an_empty_one_once() {
    let told = std::sync::Mutex::new(Vec::new());
    let progress = throttled(|event| told.lock().unwrap().push(event));
    for done in 0..=3 {
        progress(done, 3);
    }
    assert_eq!(told.lock().unwrap().len(), 4);
    let none = std::sync::Mutex::new(Vec::new());
    let progress = throttled(|event| none.lock().unwrap().push(event));
    progress(0, 0);
    assert_eq!(
        *none.lock().unwrap(),
        [Event::Progress { done: 0, total: 0 }]
    );
}
