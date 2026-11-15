use super::*;

fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    std::fs::write(temp.path().join("coven.json"), b"{}").unwrap();
    temp
}
fn selected() -> Entry {
    Entry {
        key: "one".into(),
        reference: Reference {
            source: "kitsune:djinn".into(),
            runtime: "djinn".into(),
            kind: Kind::Id,
            value: "saved".into(),
        },
        executable: "/absent/djinn".into(),
        pane_id: "w1:p1".into(),
        expected_terminal_id: "terminal-1".into(),
    }
}

#[test]
fn competing_locked_cas_writers_cannot_both_commit() {
    use std::sync::{Arc, Barrier};
    let temp = fixture();
    let token = load(temp.path()).unwrap().1;
    let barrier = Arc::new(Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let barrier = barrier.clone();
            let root = temp.path().to_owned();
            let expected = token.clone();
            std::thread::spawn(move || {
                assert_eq!(load(&root).unwrap().1, expected);
                barrier.wait();
                match cas(&root, &expected) {
                    Ok((guard, mut state)) => {
                        state.attempts.push(Attempt {
                            entry: selected(),
                            outcome: Outcome::SubmissionIntent,
                        });
                        save(&guard, &mut state, &SystemRecoveryIo).unwrap();
                        true
                    }
                    Err(error) => {
                        // Under parallel fsync load the bounded lock wait may
                        // expire before the loser can observe the new revision.
                        assert!(matches!(
                            error.code(),
                            "session_recovery_revision_conflict" | "workspace_lock_busy"
                        ));
                        false
                    }
                }
            })
        })
        .collect();
    let committed = handles
        .into_iter()
        .map(|handle| usize::from(handle.join().unwrap()))
        .sum::<usize>();
    assert_eq!(committed, 1);
    let (state, current) = load(temp.path()).unwrap();
    assert_ne!(current, token);
    assert_eq!(state.revision, 1);
    assert_eq!(state.attempts.len(), 1);
    assert!(state.attempts[0].outcome.blocks());
}

#[test]
fn retained_known_unsent_and_rejection_release_intent_but_uncertainty_never_does() {
    let temp = fixture();
    let (_, token) = load(temp.path()).unwrap();
    let (guard, mut state) = cas(temp.path(), &token).unwrap();
    state.attempts.push(Attempt {
        entry: selected(),
        outcome: Outcome::SubmissionIntent,
    });
    save(&guard, &mut state, &SystemRecoveryIo).unwrap();
    drop(guard);
    assert!(load(temp.path()).unwrap().0.attempts[0].outcome.blocks());
    for outcome in [
        Outcome::Unsent,
        Outcome::Rejected,
        Outcome::Deferred,
        Outcome::Queued,
        Outcome::Uncertain,
        Outcome::SubmissionIntent,
    ] {
        let (_, token) = load(temp.path()).unwrap();
        let (guard, mut state) = cas(temp.path(), &token).unwrap();
        state.attempts[0].outcome = outcome;
        save(&guard, &mut state, &SystemRecoveryIo).unwrap();
        drop(guard);
        let (persisted, _) = load(temp.path()).unwrap();
        assert_eq!(persisted.attempts.len(), 1);
        assert_eq!(
            persisted.attempts[0].outcome.blocks(),
            matches!(
                outcome,
                Outcome::Queued | Outcome::Uncertain | Outcome::SubmissionIntent
            )
        );
    }
}
