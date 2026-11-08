use super::*;

fn report() -> Value {
    json!({"assessment":"observed","targets":[
        {"pane_id":"p1","recognition":"djinn_executable_family","associations":[{"group_key":"a","member_key":"one"},{"group_key":"b","member_key":"one"}]},
        {"pane_id":"p2","recognition":"unknown","associations":[{"group_key":"a","member_key":"two"}]},
        {"pane_id":"p3","recognition":"djinn_executable_family","associations":[]}],
        "target_count":3,"recognized_target_count":2,
        "declarations":{"unmatched":[{"group_key":"a","member_key":"lost"},{"group_key":"b","member_key":"lost"}],"other_endpoint_count":7,"outside_inspection_count":4},
        "authorization":"none","dispatchable":false})
}

#[test]
fn selection_matrix_preserves_full_aggregates_and_original_order() {
    for group in [None, Some("a"), Some("b"), Some("missing")] {
        for member in [None, Some("one"), Some("lost")] {
            if group.is_none() && member.is_some() {
                continue;
            }
            for recognized in [false, true] {
                let original = report();
                let selected = select_report(&original, group, member, recognized).unwrap();
                assert_eq!(original, report());
                assert_eq!(selected["declarations"]["other_endpoint_count"], 7);
                assert_eq!(selected["declarations"]["outside_inspection_count"], 4);
                let panes: Vec<_> = selected["targets"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| t["pane_id"].as_str().unwrap())
                    .collect();
                let mut sorted = panes.clone();
                sorted.sort();
                assert_eq!(panes, sorted);
                if group.is_none() && !recognized {
                    assert_eq!(selected, original);
                    assert!(selected.get("view").is_none());
                } else {
                    assert_eq!(selected["view"]["observed_target_count"], 3);
                }
                for t in selected["targets"].as_array().unwrap() {
                    if recognized {
                        assert_eq!(t["recognition"], "djinn_executable_family");
                    }
                    for a in t["associations"].as_array().unwrap() {
                        if let Some(g) = group {
                            assert_eq!(a["group_key"], g);
                        }
                        if let Some(m) = member {
                            assert_eq!(a["member_key"], m);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn invalid_selectors_and_failure_copy() {
    for bad in [
        "", "Upper", "a.b", "-a", "_a", "a b", "a\n", "a\0", "\u{e9}",
    ] {
        assert!(validate_selection(Some(bad), None).is_err());
    }
    assert!(validate_selection(Some(&"a".repeat(65)), None).is_err());
    assert!(validate_selection(None, Some("one")).is_err());
    assert!(validate_selection(Some(&"a".repeat(64)), Some("0_-")).is_ok());
    let original = json!({"assessment":"unavailable","targets":[],"target_count":null,"recognized_target_count":null,"declarations":{"assessment":"unavailable","unmatched":[]}});
    let mut selected = select_report(&original, Some("a"), None, true).unwrap();
    assert!(selected["view"]["observed_target_count"].is_null());
    selected.as_object_mut().unwrap().remove("view");
    assert_eq!(selected, original);
}
