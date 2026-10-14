//! Unit tests of the social state machine.

use super::*;
use crate::types::GroupsState;

/// Custom group G1 owned by A1 with members A1, A2, A3 and HUMAN.
fn group() -> Group {
    let mut s = GroupsState::new();
    let id = s.create(Group {
        name: "t".into(),
        owner_agent_id: "A1".into(),
        member_ids: ["A1", "A2", "A3", HUMAN].map(String::from).to_vec(),
        kind: GroupKind::Custom,
        ..Group::default()
    });
    s.find(&id).cloned().expect("created")
}

#[test]
fn parses_dsl_forms() {
    assert_eq!(parse_next("still_my_turn"), Ok(Next::StillMyTurn));
    assert_eq!(parse_next("GIVE_TALKING_STICK_TO {A2}"), Ok(Next::GiveTalkingStickTo("A2".into())));
    assert_eq!(parse_next("GATHER(A2, A3)"), Ok(Next::Gather(vec!["A2".into(), "A3".into()])));
    assert_eq!(parse_next("DISPATCH [A2 A3]"), Ok(Next::Dispatch(vec!["A2".into(), "A3".into()])));
    assert_eq!(
        parse_next("give_recurring_task(A2, 30 8 * * 1-5, check CI, then report)"),
        Ok(Next::GiveRecurringTask {
            agent_id: "A2".into(),
            cron: "30 8 * * 1-5".into(),
            message: "check CI, then report".into()
        })
    );
    let bad_cron = parse_next("GIVE_RECURRING_TASK(A2, 0 * *, m)").expect_err("4-field cron");
    assert!(bad_cron.contains("5-field"), "{bad_cron}");
}

#[test]
fn rejects_bad_dsl() {
    for bad in ["GATHER", "OWNER_DONE A2", "GIVE_TALKING_STICK_TO A2 A3", "NOPE"] {
        let err = parse_next(bad).expect_err(bad);
        assert!(err.contains("expected one of"), "{err}");
    }
}

#[test]
fn gather_resumes_origin_when_all_answered() {
    let mut g = group();
    apply_send(&mut g, "A1", Next::Gather(vec!["A2".into(), "A3".into()])).expect("ok");
    let gs = std::slice::from_ref(&g);
    assert!(runnable(gs, "A2") && runnable(gs, "A3") && !runnable(gs, "A1"));
    let err = apply_send(&mut g, "A2", Next::OwnerDone).expect_err("A2 not owner");
    assert!(err.contains("owner-only"), "{err}");
    apply_send(&mut g, "A2", Next::GiveTalkingStickTo("A3".into())).expect("ok");
    apply_send(&mut g, "A3", Next::GiveTalkingStickTo("A1".into())).expect("ok");
    assert_eq!(g.social, vec![Substate::SimpleTurn { agent_id: "A1".into() }]);
}

#[test]
fn human_stick_is_owner_only_and_not_runnable() {
    let mut g = group();
    let err = apply_send(&mut g, "A2", Next::GiveTalkingStickTo(HUMAN.into())).expect_err("non-owner");
    assert!(err.contains("only the owner"), "{err}");
    apply_send(&mut g, "A1", Next::StillMyTurn).expect("ok");
    apply_send(&mut g, "A1", Next::GiveTalkingStickTo(HUMAN.into())).expect("ok");
    assert!(g.social.is_empty());
    let outsider = apply_send(&mut g, "A9", Next::StillMyTurn).expect_err("non-member");
    assert!(outsider.contains("not a member"), "{outsider}");
}

#[test]
fn tick_fires_coming_back_then_dispatches_to_idle_non_runnable() {
    let mut g = group();
    g.social.push(Substate::ComingBack { agent_id: "A2".into(), at_ms: 5u64, message: "wake".into(), from: None });
    apply_send(&mut g, "A1", Next::Dispatch(vec!["A2".into(), "A3".into()])).expect("ok");
    let mut gs = vec![g];
    let fired = tick(&mut gs, (10u64, 0i32), &|_: &str| true);
    assert_eq!(fired.len(), 1);
    let g0 = gs.first().expect("group");
    assert!(g0.social.contains(&Substate::SimpleTurn { agent_id: "A2".into() }));
    assert!(g0.social.contains(&Substate::SimpleTurn { agent_id: "A3".into() }), "A2 busy, so A3");
}

/// `Recurrent` every minute for A2, as `coucou::schedule` stores it.
fn recurrent() -> Substate {
    Substate::Recurrent {
        id: "coucou_7".into(),
        agent_id: "A2".into(),
        creator_agent_id: Some("A1".into()),
        cron: "* * * * *".into(),
        message: "ping".into(),
    }
}

/// [`group`] holding [`recurrent`].
fn with_recurrent() -> Group {
    let mut g = group();
    g.social.push(recurrent());
    g
}

/// `at_ms` of every armed `ComingBack`.
fn armed(groups: &[Group]) -> Vec<u64> {
    groups
        .iter()
        .flat_map(|g| &g.social)
        .filter_map(|s| {
            cp_macros::deref_match!(s, {
                Substate::ComingBack { at_ms, .. } => Some(at_ms),
                Substate::SimpleTurn { .. }
                | Substate::Gather { .. }
                | Substate::Dispatch { .. }
                | Substate::Recurrent { .. } => None,
            })
        })
        .collect()
}

#[test]
fn recurrent_arms_once_then_rearms_after_firing() {
    let mut groups = vec![with_recurrent()];
    let busy = |_: &str| false;
    assert!(tick(&mut groups, (1_000u64, 0i32), &busy).is_empty());
    assert!(tick(&mut groups, (2_000u64, 0i32), &busy).is_empty(), "still armed");
    assert_eq!(armed(&groups), vec![60_000u64]);
    let fired = tick(&mut groups, (60_000u64, 0i32), &busy);
    assert_eq!(fired.first().and_then(|f| f.recurrent_id.clone()).as_deref(), Some("coucou_7"));
    assert_eq!(armed(&groups), vec![120_000u64], "re-armed at the next match");
}

#[test]
fn fired_recurrent_gives_turn_and_stays() {
    let mut groups = vec![with_recurrent()];
    let _armed = tick(&mut groups, (1_000u64, 0i32), &|_: &str| false);
    let _fired = tick(&mut groups, (60_000u64, 0i32), &|_: &str| false);
    let social = groups.first().map(|g| g.social.clone()).unwrap_or_default();
    assert!(social.contains(&Substate::SimpleTurn { agent_id: "A2".into() }));
    assert!(social.contains(&recurrent()), "the Recurrent itself stays");
}

#[test]
fn remove_recurrent_drops_its_coming_back() {
    let mut groups = vec![with_recurrent()];
    let _fired = tick(&mut groups, (1_000u64, 0i32), &|_: &str| true);
    let (gid, removed) = remove_recurrent(&mut groups, "coucou_7").expect("found");
    assert_eq!((gid.as_str(), removed), ("G1", recurrent()));
    assert!(groups.first().is_some_and(|g| g.social.is_empty()));
    assert!(remove_recurrent(&mut groups, "coucou_7").is_none());
}

#[test]
fn legacy_coming_back_without_from_still_loads() {
    let json = r#"{"kind":"coming_back","agent_id":"A2","at_ms":5,"message":"m"}"#;
    let sub: Substate = serde_json::from_str(json).expect("back-compat");
    assert_eq!(sub, Substate::ComingBack { agent_id: "A2".into(), at_ms: 5u64, message: "m".into(), from: None });
}
