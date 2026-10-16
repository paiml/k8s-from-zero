use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Pod;
use kubelens::*;

fn pod(j: &str) -> Pod {
    serde_json::from_str(j).unwrap()
}
fn dep(j: &str) -> Deployment {
    serde_json::from_str(j).unwrap()
}
fn args(a: &[&str]) -> Vec<String> {
    a.iter().map(|s| s.to_string()).collect()
}

#[test]
fn pod_without_status_is_pending() {
    assert_eq!(
        pod_health(&pod(r#"{"metadata":{"name":"a"}}"#)),
        PodHealth::Pending
    );
}

#[test]
fn pod_status_without_phase_is_pending() {
    assert_eq!(pod_health(&pod(r#"{"status":{}}"#)), PodHealth::Pending);
}

#[test]
fn succeeded_and_failed_phases() {
    assert_eq!(
        pod_health(&pod(r#"{"status":{"phase":"Succeeded"}}"#)),
        PodHealth::Done
    );
    assert_eq!(
        pod_health(&pod(r#"{"status":{"phase":"Failed"}}"#)),
        PodHealth::Failing("Failed".into())
    );
}

#[test]
fn waiting_reason_fails_but_creating_does_not() {
    let w = |r: &str| {
        pod(&format!(
            r#"{{"status":{{"phase":"Pending","containerStatuses":[{{"name":"c","image":"i","imageID":"","ready":false,"restartCount":0,"state":{{"waiting":{{"reason":"{r}"}}}}}}]}}}}"#
        ))
    };
    assert_eq!(
        pod_health(&w("CrashLoopBackOff")),
        PodHealth::Failing("CrashLoopBackOff".into())
    );
    assert_eq!(pod_health(&w("ContainerCreating")), PodHealth::Pending);
}

#[test]
fn running_state_container_is_not_waiting_or_terminated() {
    let p = pod(
        r#"{"status":{"phase":"Running","containerStatuses":[{"name":"c","image":"i","imageID":"","ready":true,"restartCount":0,"state":{"running":{}}}],"conditions":[{"type":"Ready","status":"True"}]}}"#,
    );
    assert_eq!(pod_health(&p), PodHealth::Ready);
}

#[test]
fn terminated_nonzero_fails_zero_does_not() {
    let t = |code: i32, reason: &str| {
        pod(&format!(
            r#"{{"status":{{"phase":"Running","containerStatuses":[{{"name":"c","image":"i","imageID":"","ready":false,"restartCount":0,"state":{{"terminated":{{"exitCode":{code}{reason}}}}}}}]}}}}"#
        ))
    };
    assert_eq!(
        pod_health(&t(1, r#","reason":"Error""#)),
        PodHealth::Failing("Error".into())
    );
    assert_eq!(pod_health(&t(2, "")), PodHealth::Failing("Error".into()));
    assert_eq!(pod_health(&t(0, "")), PodHealth::Pending);
}

#[test]
fn running_and_ready_vs_running_not_ready() {
    let ready =
        pod(r#"{"status":{"phase":"Running","conditions":[{"type":"Ready","status":"True"}]}}"#);
    let not =
        pod(r#"{"status":{"phase":"Running","conditions":[{"type":"Ready","status":"False"}]}}"#);
    assert_eq!(pod_health(&ready), PodHealth::Ready);
    assert_eq!(pod_health(&not), PodHealth::Pending);
}

#[test]
fn summary_partitions() {
    let ps = vec![
        pod(r#"{"status":{"phase":"Succeeded"}}"#),
        pod(r#"{"status":{"phase":"Failed"}}"#),
        pod(r#"{}"#),
        pod(r#"{"status":{"phase":"Running","conditions":[{"type":"Ready","status":"True"}]}}"#),
    ];
    let s = summarize(&ps);
    assert_eq!(
        (s.ready, s.pending, s.done, s.failing, s.total),
        (1, 1, 1, 1, 4)
    );
    check_partition(&s);
}

#[test]
#[should_panic]
fn partition_contract_fires_when_broken() {
    check_partition(&Summary {
        ready: 1,
        total: 5,
        ..Summary::default()
    });
}

#[test]
fn rollout_states() {
    let base = |spec: &str, status: &str| dep(&format!(r#"{{"spec":{spec},"status":{status}}}"#));
    let sel = r#""selector":{},"template":{}"#;
    assert_eq!(
        rollout_status(&base(
            &format!(r#"{{"replicas":2,{sel}}}"#),
            r#"{"replicas":2,"updatedReplicas":2,"readyReplicas":2}"#
        )),
        Rollout::Complete
    );
    assert_eq!(
        rollout_status(&base(
            &format!(r#"{{{sel}}}"#),
            r#"{"replicas":1,"updatedReplicas":1,"readyReplicas":1}"#
        )),
        Rollout::Complete
    );
    assert_eq!(
        rollout_status(&base(
            &format!(r#"{{"replicas":3,{sel}}}"#),
            r#"{"replicas":2,"updatedReplicas":1}"#
        )),
        Rollout::InProgress {
            updated: 1,
            desired: 3
        }
    );
    assert_eq!(
        rollout_status(&base(
            &format!(r#"{{"replicas":1,{sel}}}"#),
            r#"{"conditions":[{"type":"Progressing","status":"False","reason":"ProgressDeadlineExceeded","lastTransitionTime":"2026-01-01T00:00:00Z","lastUpdateTime":"2026-01-01T00:00:00Z"}]}"#
        )),
        Rollout::Stalled
    );
}

#[test]
fn deployment_without_spec_or_status() {
    assert_eq!(
        rollout_status(&dep(r#"{}"#)),
        Rollout::InProgress {
            updated: 0,
            desired: 1
        }
    );
}

#[test]
fn run_pods_on_real_capture() {
    let (out, code) = run(&args(&["pods"]), include_str!("fixtures/pods.json"));
    assert_eq!(code, 1);
    assert!(out.contains("contract: pod-summary-partition OK"));
    assert!(out.contains("ready=3 pending=0 done=1 failing=3 total=7"));
}

#[test]
fn run_pods_healthy_exits_zero_and_nameless_pod() {
    let (out, code) = run(
        &args(&["pods"]),
        r#"{"items":[{"status":{"phase":"Succeeded"}}]}"#,
    );
    assert_eq!(code, 0);
    assert!(out.contains("\tDone"));
}

#[test]
fn run_deploys_on_real_capture() {
    let (out, code) = run(&args(&["deploys"]), include_str!("fixtures/deploys.json"));
    assert_eq!(code, 1);
    assert!(out.contains("big\tStalled") && out.contains("web\tComplete"));
}

#[test]
fn run_deploys_healthy_and_nameless() {
    let (out, code) = run(
        &args(&["deploys"]),
        r#"{"items":[{"spec":{"selector":{},"template":{}}}]}"#,
    );
    assert_eq!(code, 0);
    assert!(out.contains("InProgress"));
}

#[test]
fn run_errors_and_usage() {
    assert_eq!(run(&args(&["pods"]), "nope").1, 2);
    assert_eq!(run(&args(&["deploys"]), "nope").1, 2);
    assert_eq!(run(&args(&["pods"]), r#"{"items":[1]}"#).1, 2);
    assert_eq!(run(&args(&["deploys"]), r#"{"items":[1]}"#).1, 2);
    let (out, code) = run(&[], "");
    assert_eq!(code, 2);
    assert!(out.starts_with("usage"));
    assert_eq!(run(&args(&["pods"]), "{}").1, 0);
}

#[test]
#[ignore]
fn calls_nothing() {}

mod audit_tests {
    use kubelens::audit::*;
    use kubelens::run;
    use serde_json::json;

    fn role(rules: serde_json::Value) -> serde_json::Value {
        json!({"metadata": {"name": "r"}, "rules": rules})
    }

    #[test]
    fn clean_role_has_no_findings() {
        let r = role(json!([{"verbs": ["get"], "resources": ["pods"]}]));
        assert!(role_findings(&r).is_empty());
    }

    #[test]
    fn every_problem_is_reported() {
        let r = role(json!([
            {"verbs": ["*"], "resources": ["*"]},
            {"verbs": ["escalate", "get"], "resources": ["secrets"]}
        ]));
        let probs: Vec<_> = role_findings(&r).into_iter().map(|f| f.problem).collect();
        assert_eq!(
            probs,
            [
                "wildcard verb",
                "wildcard resource",
                "privilege escalation verb",
                "reads secrets"
            ]
        );
    }

    #[test]
    fn role_without_rules_or_name_is_clean() {
        assert!(role_findings(&json!({})).is_empty());
        let f = role_findings(&json!({"rules": [{"verbs": ["*"]}]}));
        assert_eq!(f[0].subject, "?");
    }

    #[test]
    fn rbac_list_flattens() {
        let l = json!({"items": [role(json!([{"verbs": ["*"], "resources": ["pods"]}])), role(json!([]))]});
        assert_eq!(rbac_findings(&l).len(), 1);
        assert!(rbac_findings(&json!({})).is_empty());
    }

    fn dep(image: &str, limits: bool) -> serde_json::Value {
        let mut c = json!({"image": image});
        if limits {
            c["resources"] = json!({"limits": {"cpu": "1"}});
        }
        json!({"kind": "Deployment", "metadata": {"name": "d"},
               "spec": {"template": {"spec": {"containers": [c]}}}})
    }

    #[test]
    fn manifest_checks() {
        let ok = json!({"items": [dep("nginx:1.27", true), {"kind": "Service"}]});
        assert!(manifest_findings(&ok).is_empty());
        let bad = json!({"items": [dep("nginx", false), dep("nginx:latest", true)]});
        let probs: Vec<_> = manifest_findings(&bad)
            .into_iter()
            .map(|f| f.problem)
            .collect();
        assert_eq!(
            probs,
            ["image not pinned", "no resource limits", "image not pinned"]
        );
        let noimg = json!({"items": [{"kind": "Deployment", "spec": {"template": {"spec": {"containers": [{}]}}}}]});
        assert_eq!(manifest_findings(&noimg).len(), 2);
        assert!(manifest_findings(&json!({"items": [{"kind": "Deployment"}]})).is_empty());
    }

    #[test]
    #[should_panic]
    fn contract_fires_when_a_wildcard_hides() {
        check_least_privilege(&json!({"verbs": ["*"]}), &[]);
    }

    #[test]
    fn run_subcommands() {
        let a = |s: &str| vec![s.to_string()];
        let (out, code) = run(
            &a("rbac"),
            r#"{"items":[{"metadata":{"name":"x"},"rules":[{"verbs":["*"],"resources":["pods"]}]}]}"#,
        );
        assert_eq!(code, 1);
        assert!(out.contains("x\twildcard verb") && out.contains("contract: least-privilege OK"));
        let (out, code) = run(&a("manifests"), r#"{"items":[]}"#);
        assert_eq!((code, out.contains("findings=0")), (0, true));
        assert_eq!(run(&a("rbac"), "nope").1, 2);
    }
}
